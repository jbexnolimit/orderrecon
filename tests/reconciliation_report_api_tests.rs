use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use mongodb::{
    Client, Database,
    bson::{DateTime, doc, oid::ObjectId},
};
use orderrecon::{
    database,
    routes::{AppState, create_router},
};
use serde_json::Value;
use std::{env, error::Error};
use tower::ServiceExt;

async fn test_database() -> Result<Database, Box<dyn Error>> {
    dotenvy::dotenv().ok();

    let test_db_name = env::var("ORDERRECON_TEST_DATABASE")
        .expect("Set ORDERRECON_TEST_DATABASE=orderrecon_test before running tests");

    assert_eq!(
        test_db_name, "orderrecon_test",
        "Refusing to run tests against any database except orderrecon_test"
    );

    if let Ok(app_db_name) = env::var("MONGODB_DATABASE") {
        assert_ne!(
            test_db_name, app_db_name,
            "Refusing to run tests against the application database"
        );
    }

    let uri = env::var("MONGODB_URI").unwrap_or_else(|_| "mongodb://127.0.0.1:27017".to_string());

    let client = Client::with_uri_str(&uri).await?;
    let database = client.database(&test_db_name);

    database.run_command(doc! { "ping": 1 }).await?;
    database::ensure_indexes(&database).await?;

    Ok(database)
}

fn run_document(status: &str) -> mongodb::bson::Document {
    doc! {
        "status": status,
        "started_at": DateTime::now(),
        "completed_at": DateTime::now(),
        "summary": {
            "total_orders": 3,
            "matched_orders": 1,
            "missing_payment": 1,
            "amount_mismatch": 1,
            "currency_mismatch": 0,
            "duplicate_payment": 0,
            "unmatched_payments": 0
        },
        "error": null
    }
}

#[tokio::test]
async fn report_returns_400_for_invalid_run_id() -> Result<(), Box<dyn Error>> {
    let database = test_database().await?;
    let app = create_router(AppState { database });

    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/reports/reconciliation?run_id=invalid-id")
        .body(Body::empty())?;

    let response = app.oneshot(request).await?;
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let body: Value = serde_json::from_slice(&body)?;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "INVALID_RUN_ID");

    Ok(())
}

#[tokio::test]
async fn report_returns_404_for_missing_run() -> Result<(), Box<dyn Error>> {
    let database = test_database().await?;
    let app = create_router(AppState { database });
    let unknown_id = ObjectId::new();

    let request = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/v1/reports/reconciliation?run_id={unknown_id}"
        ))
        .body(Body::empty())?;

    let response = app.oneshot(request).await?;
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let body: Value = serde_json::from_slice(&body)?;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "RUN_NOT_FOUND");

    Ok(())
}

#[tokio::test]
async fn report_rejects_incomplete_run() -> Result<(), Box<dyn Error>> {
    let database = test_database().await?;
    let runs = database.collection::<mongodb::bson::Document>("reconciliation_runs");

    let result = runs.insert_one(run_document("running")).await?;

    let run_id = result.inserted_id.as_object_id().unwrap();
    let app = create_router(AppState { database });

    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/reports/reconciliation?run_id={run_id}"))
        .body(Body::empty())?;

    let response = app.oneshot(request).await?;
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let body: Value = serde_json::from_slice(&body)?;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "RUN_NOT_COMPLETED");

    Ok(())
}

#[tokio::test]
async fn report_returns_summary_for_completed_run() -> Result<(), Box<dyn Error>> {
    let database = test_database().await?;
    let runs = database.collection::<mongodb::bson::Document>("reconciliation_runs");

    let result = runs.insert_one(run_document("completed")).await?;

    let run_id = result.inserted_id.as_object_id().unwrap();
    let app = create_router(AppState { database });

    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/reports/reconciliation?run_id={run_id}"))
        .body(Body::empty())?;

    let response = app.oneshot(request).await?;
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let body: Value = serde_json::from_slice(&body)?;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["run_id"]["$oid"], run_id.to_hex());
    assert_eq!(body["status"], "completed");
    assert_eq!(body["summary"]["total_orders"], 3);
    assert_eq!(body["summary"]["matched_orders"], 1);
    assert_eq!(body["discrepancies"]["total"], 0);
    assert_eq!(body["discrepancies"]["by_type"]["missing_payment"], 0);
    assert_eq!(body["discrepancies"]["by_status"]["open"], 0);

    Ok(())
}
