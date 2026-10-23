use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use futures_util::TryStreamExt;
use mongodb::bson::{DateTime, doc};
use serde::Deserialize;
use serde_json::{Value, json};

use super::AppState;
use crate::models::order::{Order, OrderStatus};

#[derive(Debug, Deserialize)]
pub struct CreateOrderRequest {
    pub order_ref: String,
    pub amount_minor: i64,
    pub currency: String,
    pub status: Option<OrderStatus>,
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

pub async fn create_order(
    State(state): State<AppState>,
    Json(input): Json<CreateOrderRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let order_ref = input.order_ref.trim().to_string();
    let currency = input.currency.trim().to_ascii_uppercase();

    if order_ref.is_empty() || order_ref.len() > 100 {
        return Err(error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "VALIDATION_ERROR",
            "order_ref must contain between 1 and 100 characters.",
        ));
    }

    if input.amount_minor <= 0 {
        return Err(error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "VALIDATION_ERROR",
            "amount_minor must be a positive integer.",
        ));
    }

    if currency.len() != 3 || !currency.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "VALIDATION_ERROR",
            "currency must be a three-letter code, such as PHP or USD.",
        ));
    }

    let order = Order {
        id: None,
        order_ref,
        amount_minor: input.amount_minor,
        currency,
        status: input.status.unwrap_or(OrderStatus::Placed),
        created_at: DateTime::now(),
    };

    let collection = state.database.collection::<Order>("orders");

    match collection.insert_one(&order).await {
        Ok(result) => {
            let mut created = order;
            created.id = result.inserted_id.as_object_id();

            let body = serde_json::to_value(created).unwrap_or_else(|_| json!({}));

            Ok((StatusCode::CREATED, Json(body)))
        }
        Err(error) if error.to_string().contains("E11000") => Err(error_response(
            StatusCode::CONFLICT,
            "DUPLICATE_ORDER",
            "An order with this order_ref already exists.",
        )),
        Err(error) => {
            tracing::error!("Failed to create order: {error}");

            Err(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "The order could not be created.",
            ))
        }
    }
}

pub async fn list_orders(
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

    let collection = state.database.collection::<Order>("orders");

    let cursor = collection
        .find(doc! {})
        .sort(doc! { "created_at": -1, "_id": -1 })
        .skip(skip as u64)
        .limit(limit)
        .await
        .map_err(|error| {
            tracing::error!("Failed to list orders: {error}");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "Orders could not be retrieved.",
            )
        })?;

    let orders: Vec<Order> = cursor.try_collect().await.map_err(|error| {
        tracing::error!("Failed to read orders: {error}");
        error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "Orders could not be retrieved.",
        )
    })?;

    let data = serde_json::to_value(orders).unwrap_or_else(|_| json!([]));

    Ok(Json(json!({
        "data": data,
        "pagination": {
            "page": page,
            "limit": limit,
            "returned": data.as_array().map_or(0, Vec::len)
        }
    })))
}

pub async fn get_order(
    State(state): State<AppState>,
    Path(order_ref): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let collection = state.database.collection::<Order>("orders");

    let order = collection
        .find_one(doc! { "order_ref": &order_ref })
        .await
        .map_err(|error| {
            tracing::error!("Failed to retrieve order: {error}");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "The order could not be retrieved.",
            )
        })?;

    match order {
        Some(order) => {
            let body = serde_json::to_value(order).unwrap_or_else(|_| json!({}));
            Ok(Json(body))
        }
        None => Err(error_response(
            StatusCode::NOT_FOUND,
            "ORDER_NOT_FOUND",
            "No order exists with this order_ref.",
        )),
    }
}
