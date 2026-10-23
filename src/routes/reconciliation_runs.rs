use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use futures_util::TryStreamExt;
use mongodb::bson::{doc, oid::ObjectId};
use serde::Deserialize;
use serde_json::{Value, json};

use super::AppState;
use crate::models::reconciliation_run::ReconciliationRun;

#[derive(Debug, Deserialize)]
pub struct ListReconciliationRunsQuery {
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

pub async fn list_reconciliation_runs(
    State(state): State<AppState>,
    Query(query): Query<ListReconciliationRunsQuery>,
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

    let skip = (page - 1).checked_mul(limit).ok_or_else(|| {
        error_response(
            StatusCode::BAD_REQUEST,
            "INVALID_PAGINATION",
            "The requested page is too large.",
        )
    })?;

    let collection = state
        .database
        .collection::<ReconciliationRun>("reconciliation_runs");

    let total = collection.count_documents(doc! {}).await.map_err(|error| {
        tracing::error!("Failed to count reconciliation runs: {error}");
        error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "Reconciliation runs could not be retrieved.",
        )
    })?;

    let cursor = collection
        .find(doc! {})
        .sort(doc! { "started_at": -1, "_id": -1 })
        .skip(skip as u64)
        .limit(limit)
        .await
        .map_err(|error| {
            tracing::error!("Failed to list reconciliation runs: {error}");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "Reconciliation runs could not be retrieved.",
            )
        })?;

    let runs: Vec<ReconciliationRun> = cursor.try_collect().await.map_err(|error| {
        tracing::error!("Failed to read reconciliation runs: {error}");
        error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "Reconciliation runs could not be retrieved.",
        )
    })?;

    let data = serde_json::to_value(runs).unwrap_or_else(|_| json!([]));
    let returned = data.as_array().map_or(0, Vec::len);

    Ok(Json(json!({
        "data": data,
        "pagination": {
            "page": page,
            "limit": limit,
            "total": total,
            "returned": returned
        }
    })))
}

pub async fn get_reconciliation_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let object_id = ObjectId::parse_str(&id).map_err(|_| {
        error_response(
            StatusCode::BAD_REQUEST,
            "INVALID_RUN_ID",
            "The reconciliation run ID must be a valid MongoDB ObjectId.",
        )
    })?;

    let collection = state
        .database
        .collection::<ReconciliationRun>("reconciliation_runs");

    let run = collection
        .find_one(doc! { "_id": object_id })
        .await
        .map_err(|error| {
            tracing::error!("Failed to retrieve reconciliation run: {error}");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "The reconciliation run could not be retrieved.",
            )
        })?;

    match run {
        Some(run) => {
            let body = serde_json::to_value(run).unwrap_or_else(|_| json!({}));
            Ok(Json(body))
        }
        None => Err(error_response(
            StatusCode::NOT_FOUND,
            "RUN_NOT_FOUND",
            "No reconciliation run exists with this ID.",
        )),
    }
}
