use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use mongodb::{Database, bson::doc};
use serde_json::{Value, json};

#[derive(Clone)]
pub struct AppState {
    pub database: Database,
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .with_state(state)
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
