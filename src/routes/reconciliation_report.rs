use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use futures_util::TryStreamExt;
use mongodb::bson::{doc, oid::ObjectId};
use serde::Deserialize;
use serde_json::{Value, json};

use super::AppState;
use crate::models::discrepancy::{Discrepancy, DiscrepancyStatus, DiscrepancyType};
use crate::models::reconciliation_run::{ReconciliationRun, RunStatus};

#[derive(Debug, Deserialize)]
pub struct ReconciliationReportQuery {
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

pub async fn get_reconciliation_report(
    State(state): State<AppState>,
    Query(query): Query<ReconciliationReportQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let runs = state
        .database
        .collection::<ReconciliationRun>("reconciliation_runs");

    let run = if let Some(run_id) = query.run_id {
        let object_id = ObjectId::parse_str(&run_id).map_err(|_| {
            error_response(
                StatusCode::BAD_REQUEST,
                "INVALID_RUN_ID",
                "run_id must be a valid MongoDB ObjectId.",
            )
        })?;

        runs.find_one(doc! { "_id": object_id })
            .await
            .map_err(|error| {
                tracing::error!("Failed to retrieve reconciliation run: {error}");
                error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "The reconciliation report could not be retrieved.",
                )
            })?
    } else {
        runs.find_one(doc! { "status": "completed" })
            .sort(doc! { "started_at": -1, "_id": -1 })
            .await
            .map_err(|error| {
                tracing::error!("Failed to retrieve latest reconciliation run: {error}");
                error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "The reconciliation report could not be retrieved.",
                )
            })?
    };

    let run = match run {
        Some(run) => run,
        None => {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                "RUN_NOT_FOUND",
                "No matching reconciliation run was found.",
            ));
        }
    };

    if !matches!(&run.status, RunStatus::Completed) {
        return Err(error_response(
            StatusCode::CONFLICT,
            "RUN_NOT_COMPLETED",
            "A report is only available for completed reconciliation runs.",
        ));
    }

    let run_id = run.id.ok_or_else(|| {
        error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INVALID_RUN_RECORD",
            "The reconciliation run has no ID.",
        )
    })?;

    let discrepancies = state.database.collection::<Discrepancy>("discrepancies");

    let cursor = discrepancies
        .find(doc! { "run_id": run_id })
        .await
        .map_err(|error| {
            tracing::error!("Failed to query report discrepancies: {error}");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "The reconciliation report could not be retrieved.",
            )
        })?;

    let records: Vec<Discrepancy> = cursor.try_collect().await.map_err(|error| {
        tracing::error!("Failed to read report discrepancies: {error}");
        error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "The reconciliation report could not be retrieved.",
        )
    })?;

    let mut by_type = json!({
        "missing_payment": 0,
        "amount_mismatch": 0,
        "currency_mismatch": 0,
        "duplicate_payment": 0,
        "unmatched_payment": 0
    });

    let mut by_status = json!({
        "open": 0,
        "resolved": 0,
        "ignored": 0
    });

    for discrepancy in &records {
        let type_key = match &discrepancy.discrepancy_type {
            DiscrepancyType::MissingPayment => "missing_payment",
            DiscrepancyType::AmountMismatch => "amount_mismatch",
            DiscrepancyType::CurrencyMismatch => "currency_mismatch",
            DiscrepancyType::DuplicatePayment => "duplicate_payment",
            DiscrepancyType::UnmatchedPayment => "unmatched_payment",
        };

        let status_key = match &discrepancy.status {
            DiscrepancyStatus::Open => "open",
            DiscrepancyStatus::Resolved => "resolved",
            DiscrepancyStatus::Ignored => "ignored",
        };

        by_type[type_key] = json!(by_type[type_key].as_u64().unwrap_or(0) + 1);
        by_status[status_key] = json!(by_status[status_key].as_u64().unwrap_or(0) + 1);
    }

    let summary = serde_json::to_value(&run.summary).unwrap_or_else(|_| json!({}));

    Ok(Json(json!({
        "run_id": run_id,
        "status": "completed",
        "started_at": run.started_at,
        "completed_at": run.completed_at,
        "summary": summary,
        "discrepancies": {
            "total": records.len(),
            "by_type": by_type,
            "by_status": by_status
        }
    })))
}
