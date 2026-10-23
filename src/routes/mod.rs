pub mod discrepancies;
pub mod orders;
pub mod payments;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use mongodb::{Database, bson::doc};
use serde_json::{Value, json};

#[derive(Clone)]
pub struct AppState {
    pub database: Database,
}

async fn health(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    match state.database.run_command(doc! { "ping": 1 }).await {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "service": "orderrecon-api",
                "database": "connected"
            })),
        ),
        Err(error) => {
            tracing::error!("MongoDB health check failed: {error}");

            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "status": "error",
                    "service": "orderrecon-api",
                    "database": "unavailable"
                })),
            )
        }
    }
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route(
            "/api/v1/orders",
            get(orders::list_orders).post(orders::create_order),
        )
        .route("/api/v1/orders/{order_ref}", get(orders::get_order))
        .route("/api/v1/payments", get(payments::list_payments))
        .route(
            "/api/v1/payments/import",
            axum::routing::post(payments::import_payments),
        )
        .route(
            "/api/v1/reconciliation-runs",
            post(create_reconciliation_run),
        )
        .route(
            "/api/v1/discrepancies",
            get(discrepancies::list_discrepancies),
        )
        .route(
            "/api/v1/discrepancies/{id}",
            get(discrepancies::get_discrepancy),
        )
        .route(
            "/api/v1/discrepancies/{id}/resolution",
            axum::routing::patch(discrepancies::resolve_discrepancy),
        )
        .with_state(state)
}

async fn create_reconciliation_run(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    match crate::services::reconciliation::execute(&state.database).await {
        Ok(run) => {
            let body = serde_json::to_value(run).unwrap_or_else(|_| json!({}));

            Ok((StatusCode::CREATED, Json(body)))
        }
        Err(error) => {
            tracing::error!("Reconciliation failed: {error}");

            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": {
                        "code": "RECONCILIATION_FAILED",
                        "message": "The reconciliation run could not be completed."
                    }
                })),
            ))
        }
    }
}
