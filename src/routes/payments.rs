use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use futures_util::TryStreamExt;
use mongodb::bson::{DateTime, doc};
use serde::Deserialize;
use serde_json::{Value, json};

use super::AppState;
use crate::models::payment::{Payment, PaymentStatus};

#[derive(Debug, Deserialize)]
pub struct ImportPaymentsRequest {
    pub source: String,
    pub payments: Vec<PaymentImportItem>,
}

#[derive(Debug, Deserialize)]
pub struct PaymentImportItem {
    pub source_transaction_id: String,
    pub order_ref: String,
    pub amount_minor: i64,
    pub currency: String,
    pub status: PaymentStatus,
}

#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    pub page: Option<i64>,
    pub limit: Option<i64>,
}

fn error_response(status: StatusCode, code: &str, message: &str) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(json!({
            "error": {
                "code": code,
                "message": message
            }
        })),
    )
}

pub async fn import_payments(
    State(state): State<AppState>,
    Json(input): Json<ImportPaymentsRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let source = input.source.trim().to_string();

    if source.is_empty() || source.len() > 100 {
        return Err(error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "VALIDATION_ERROR",
            "source must contain between 1 and 100 characters.",
        ));
    }

    if input.payments.is_empty() || input.payments.len() > 500 {
        return Err(error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "VALIDATION_ERROR",
            "Provide between 1 and 500 payments per import.",
        ));
    }

    let collection = state.database.collection::<Payment>("payments");

    let mut imported = 0usize;
    let mut duplicates = 0usize;
    let mut rejected = 0usize;
    let mut results: Vec<Value> = Vec::new();

    for item in input.payments {
        let transaction_id = item.source_transaction_id.trim().to_string();
        let order_ref = item.order_ref.trim().to_string();
        let currency = item.currency.trim().to_ascii_uppercase();

        let validation_error = if transaction_id.is_empty() || transaction_id.len() > 150 {
            Some("source_transaction_id must contain between 1 and 150 characters.")
        } else if order_ref.is_empty() || order_ref.len() > 100 {
            Some("order_ref must contain between 1 and 100 characters.")
        } else if item.amount_minor <= 0 {
            Some("amount_minor must be a positive integer.")
        } else if currency.len() != 3 || !currency.bytes().all(|b| b.is_ascii_uppercase()) {
            Some("currency must be a three-letter code, such as PHP or USD.")
        } else {
            None
        };

        if let Some(message) = validation_error {
            rejected += 1;
            results.push(json!({
                "source_transaction_id": transaction_id,
                "status": "rejected",
                "message": message
            }));
            continue;
        }

        let payment = Payment {
            id: None,
            source: source.clone(),
            source_transaction_id: transaction_id.clone(),
            order_ref,
            amount_minor: item.amount_minor,
            currency,
            status: item.status,
            imported_at: DateTime::now(),
        };

        match collection.insert_one(&payment).await {
            Ok(result) => {
                imported += 1;

                results.push(json!({
                    "source_transaction_id": transaction_id,
                    "status": "imported",
                    "id": result.inserted_id.as_object_id().map(|id| id.to_hex())
                }));
            }
            Err(error) if error.to_string().contains("E11000") => {
                duplicates += 1;

                results.push(json!({
                    "source_transaction_id": transaction_id,
                    "status": "duplicate",
                    "message": "This source transaction has already been imported."
                }));
            }
            Err(error) => {
                tracing::error!("Failed to import payment {}: {}", transaction_id, error);

                rejected += 1;
                results.push(json!({
                    "source_transaction_id": transaction_id,
                    "status": "rejected",
                    "message": "The payment could not be saved."
                }));
            }
        }
    }

    Ok((
        StatusCode::OK,
        Json(json!({
            "summary": {
                "total": imported + duplicates + rejected,
                "imported": imported,
                "duplicates": duplicates,
                "rejected": rejected
            },
            "results": results
        })),
    ))
}

pub async fn list_payments(
    State(state): State<AppState>,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let page = query.page.unwrap_or(1);
    let limit = query.limit.unwrap_or(20);

    if page < 1 || !(1..=100).contains(&limit) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "INVALID_PAGINATION",
            "page must be at least 1 and limit must be between 1 and 100.",
        ));
    }

    let skip = match (page - 1).checked_mul(limit) {
        Some(value) => value,
        None => {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "INVALID_PAGINATION",
                "The requested page is too large.",
            ));
        }
    };

    let collection = state.database.collection::<Payment>("payments");

    let cursor = collection
        .find(doc! {})
        .sort(doc! { "imported_at": -1, "_id": -1 })
        .skip(skip as u64)
        .limit(limit)
        .await
        .map_err(|error| {
            tracing::error!("Failed to list payments: {error}");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "Payments could not be retrieved.",
            )
        })?;

    let payments: Vec<Payment> = cursor.try_collect().await.map_err(|error| {
        tracing::error!("Failed to read payments: {error}");
        error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "Payments could not be retrieved.",
        )
    })?;

    let data = serde_json::to_value(payments).unwrap_or_else(|_| json!([]));
    let returned = data.as_array().map_or(0, Vec::len);

    Ok(Json(json!({
        "data": data,
        "pagination": {
            "page": page,
            "limit": limit,
            "returned": returned
        }
    })))
}
