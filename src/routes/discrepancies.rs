use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use futures_util::TryStreamExt;
use mongodb::bson::{DateTime, Document, doc, oid::ObjectId};
use serde::Deserialize;
use serde_json::{Value, json};

use super::AppState;
use crate::models::discrepancy::{Discrepancy, DiscrepancyStatus};

#[derive(Debug, Deserialize)]
pub struct ListDiscrepanciesQuery {
    pub page: Option<i64>,
    pub limit: Option<i64>,
    pub status: Option<String>,
    pub discrepancy_type: Option<String>,
    pub run_id: Option<String>,
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

pub async fn list_discrepancies(
    State(state): State<AppState>,
    Query(query): Query<ListDiscrepanciesQuery>,
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

    let mut filter = Document::new();

    if let Some(status) = query.status {
        let valid = matches!(status.as_str(), "open" | "resolved" | "ignored");

        if !valid {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "INVALID_STATUS",
                "status must be open, resolved, or ignored.",
            ));
        }

        filter.insert("status", status);
    }

    if let Some(discrepancy_type) = query.discrepancy_type {
        let valid = matches!(
            discrepancy_type.as_str(),
            "missing_payment"
                | "amount_mismatch"
                | "currency_mismatch"
                | "duplicate_payment"
                | "unmatched_payment"
        );

        if !valid {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "INVALID_DISCREPANCY_TYPE",
                "The supplied discrepancy_type is not supported.",
            ));
        }

        filter.insert("discrepancy_type", discrepancy_type);
    }

    if let Some(run_id) = query.run_id {
        let object_id = ObjectId::parse_str(&run_id).map_err(|_| {
            error_response(
                StatusCode::BAD_REQUEST,
                "INVALID_RUN_ID",
                "run_id must be a valid MongoDB ObjectId.",
            )
        })?;

        filter.insert("run_id", object_id);
    }

    let collection = state.database.collection::<Discrepancy>("discrepancies");

    let total = collection
        .count_documents(filter.clone())
        .await
        .map_err(|error| {
            tracing::error!("Failed to count discrepancies: {error}");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "Discrepancies could not be retrieved.",
            )
        })?;

    let cursor = collection
        .find(filter)
        .sort(doc! { "created_at": -1, "_id": -1 })
        .skip(skip as u64)
        .limit(limit)
        .await
        .map_err(|error| {
            tracing::error!("Failed to list discrepancies: {error}");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "Discrepancies could not be retrieved.",
            )
        })?;

    let discrepancies: Vec<Discrepancy> = cursor.try_collect().await.map_err(|error| {
        tracing::error!("Failed to read discrepancies: {error}");
        error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "Discrepancies could not be retrieved.",
        )
    })?;

    let data = serde_json::to_value(discrepancies).unwrap_or_else(|_| json!([]));

    Ok(Json(json!({
        "data": data,
        "pagination": {
            "page": page,
            "limit": limit,
            "total": total,
            "returned": data.as_array().map_or(0, Vec::len)
        }
    })))
}

pub async fn get_discrepancy(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let object_id = ObjectId::parse_str(&id).map_err(|_| {
        error_response(
            StatusCode::BAD_REQUEST,
            "INVALID_DISCREPANCY_ID",
            "The discrepancy ID must be a valid MongoDB ObjectId.",
        )
    })?;

    let collection = state.database.collection::<Discrepancy>("discrepancies");

    let discrepancy = collection
        .find_one(doc! { "_id": object_id })
        .await
        .map_err(|error| {
            tracing::error!("Failed to retrieve discrepancy: {error}");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "The discrepancy could not be retrieved.",
            )
        })?;

    match discrepancy {
        Some(discrepancy) => {
            let body = serde_json::to_value(discrepancy).unwrap_or_else(|_| json!({}));
            Ok(Json(body))
        }
        None => Err(error_response(
            StatusCode::NOT_FOUND,
            "DISCREPANCY_NOT_FOUND",
            "No discrepancy exists with this ID.",
        )),
    }
}

#[derive(Debug, Deserialize)]
pub struct ResolveDiscrepancyRequest {
    pub status: String,
    pub resolution_note: String,
}

pub async fn resolve_discrepancy(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ResolveDiscrepancyRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let object_id = ObjectId::parse_str(&id).map_err(|_| {
        error_response(
            StatusCode::BAD_REQUEST,
            "INVALID_DISCREPANCY_ID",
            "The discrepancy ID must be a valid MongoDB ObjectId.",
        )
    })?;

    let status = match input.status.as_str() {
        "resolved" => DiscrepancyStatus::Resolved,
        "ignored" => DiscrepancyStatus::Ignored,
        _ => {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "INVALID_STATUS",
                "status must be resolved or ignored.",
            ));
        }
    };

    let resolution_note = input.resolution_note.trim().to_string();

    if resolution_note.is_empty() || resolution_note.len() > 1000 {
        return Err(error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "VALIDATION_ERROR",
            "resolution_note must contain between 1 and 1000 characters.",
        ));
    }

    let status_value = match status {
        DiscrepancyStatus::Resolved => "resolved",
        DiscrepancyStatus::Ignored => "ignored",
        DiscrepancyStatus::Open => unreachable!(),
    };

    let collection = state.database.collection::<Discrepancy>("discrepancies");

    let result = collection
        .update_one(
            doc! { "_id": object_id },
            doc! {
                "$set": {
                    "status": status_value,
                    "resolution_note": &resolution_note,
                    "updated_at": DateTime::now()
                }
            },
        )
        .await
        .map_err(|error| {
            tracing::error!("Failed to resolve discrepancy: {error}");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "The discrepancy could not be updated.",
            )
        })?;

    if result.matched_count == 0 {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "DISCREPANCY_NOT_FOUND",
            "No discrepancy exists with this ID.",
        ));
    }

    let discrepancy = collection
        .find_one(doc! { "_id": object_id })
        .await
        .map_err(|error| {
            tracing::error!("Failed to retrieve updated discrepancy: {error}");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "The updated discrepancy could not be retrieved.",
            )
        })?;

    match discrepancy {
        Some(discrepancy) => {
            let body = serde_json::to_value(discrepancy).unwrap_or_else(|_| json!({}));
            Ok(Json(body))
        }
        None => Err(error_response(
            StatusCode::NOT_FOUND,
            "DISCREPANCY_NOT_FOUND",
            "The discrepancy could not be found after updating.",
        )),
    }
}
