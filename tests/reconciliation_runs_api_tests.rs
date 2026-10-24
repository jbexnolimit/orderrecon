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

fn completed_run_document() -> mongodb::bson::Document {
    doc! {
        "status": "completed",
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
async fn list_reconciliation_runs_returns_paginated_results() -> Result<(), Box<dyn Error>> {
    let database = test_database().await?;
    let collection = database.collection::<mongodb::bson::Document>("reconciliation_runs");

    collection.insert_one(completed_run_document()).await?;

    let app = create_router(AppState { database });
    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/reconciliation-runs?limit=5")
        .body(Body::empty())?;

    let response = app.oneshot(request).await?;
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let body: Value = serde_json::from_slice(&body)?;

    assert_eq!(status, StatusCode::OK);
    assert!(body["data"].is_array());
    assert_eq!(body["pagination"]["page"], 1);
    assert_eq!(body["pagination"]["limit"], 5);
    assert!(body["pagination"]["returned"].as_u64().unwrap() >= 1);

    Ok(())
}

#[tokio::test]
async fn list_reconciliation_runs_rejects_invalid_pagination() -> Result<(), Box<dyn Error>> {
    let database = test_database().await?;
    let app = create_router(AppState { database });

    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/reconciliation-runs?page=0")
        .body(Body::empty())?;

    let response = app.oneshot(request).await?;
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let body: Value = serde_json::from_slice(&body)?;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "INVALID_PAGINATION");

    Ok(())
}

#[tokio::test]
async fn get_reconciliation_run_rejects_invalid_id() -> Result<(), Box<dyn Error>> {
    let database = test_database().await?;
    let app = create_router(AppState { database });

    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/reconciliation-runs/not-an-object-id")
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
async fn get_reconciliation_run_returns_404_when_missing() -> Result<(), Box<dyn Error>> {
    let database = test_database().await?;
    let app = create_router(AppState { database });
    let unknown_id = ObjectId::new();

    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/reconciliation-runs/{unknown_id}"))
        .body(Body::empty())?;

    let response = app.oneshot(request).await?;
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let body: Value = serde_json::from_slice(&body)?;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "RUN_NOT_FOUND");

    Ok(())
}
