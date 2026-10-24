use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header::CONTENT_TYPE},
};
use mongodb::{
    Client, Database,
    bson::{DateTime, doc, oid::ObjectId},
};
use orderrecon::{
    database,
    routes::{AppState, create_router},
};
use serde_json::{Value, json};
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

async fn test_app() -> Result<axum::Router, Box<dyn Error>> {
    Ok(create_router(AppState {
        database: test_database().await?,
    }))
}

async fn send_json(
    app: axum::Router,
    method: &str,
    uri: &str,
    payload: Value,
) -> Result<(StatusCode, Value), Box<dyn Error>> {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&payload)?))?;

    let response = app.oneshot(request).await?;
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let json = serde_json::from_slice(&body)?;

    Ok((status, json))
}

fn insert_discrepancy_document() -> mongodb::bson::Document {
    doc! {
        "run_id": ObjectId::new(),
        "discrepancy_type": "missing_payment",
        "order_ref": format!("TEST-ORDER-{}", ObjectId::new()),
        "payment_ids": [],
        "expected_amount_minor": 150000_i64,
        "actual_amount_minor": null,
        "expected_currency": "PHP",
        "actual_currency": null,
        "status": "open",
        "resolution_note": null,
        "created_at": DateTime::now(),
        "updated_at": DateTime::now()
    }
}

async fn insert_test_discrepancy(database: &Database) -> Result<ObjectId, Box<dyn Error>> {
    let collection = database.collection::<mongodb::bson::Document>("discrepancies");
    let result = collection.insert_one(insert_discrepancy_document()).await?;

    Ok(result.inserted_id.as_object_id().unwrap())
}

#[tokio::test]
async fn list_discrepancies_returns_paginated_results() -> Result<(), Box<dyn Error>> {
    let database = test_database().await?;
    insert_test_discrepancy(&database).await?;

    let app = create_router(AppState { database });
    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/discrepancies?status=open&limit=5")
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
async fn list_discrepancies_rejects_invalid_status() -> Result<(), Box<dyn Error>> {
    let app = test_app().await?;
    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/discrepancies?status=pending")
        .body(Body::empty())?;

    let response = app.oneshot(request).await?;
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let body: Value = serde_json::from_slice(&body)?;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "INVALID_STATUS");

    Ok(())
}

#[tokio::test]
async fn get_discrepancy_rejects_invalid_id() -> Result<(), Box<dyn Error>> {
    let app = test_app().await?;
    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/discrepancies/not-an-object-id")
        .body(Body::empty())?;

    let response = app.oneshot(request).await?;
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let body: Value = serde_json::from_slice(&body)?;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "INVALID_DISCREPANCY_ID");

    Ok(())
}

#[tokio::test]
async fn resolve_discrepancy_updates_status_and_note() -> Result<(), Box<dyn Error>> {
    let database = test_database().await?;
    let id = insert_test_discrepancy(&database).await?;
    let app = create_router(AppState { database });

    let (status, body) = send_json(
        app,
        "PATCH",
        &format!("/api/v1/discrepancies/{id}/resolution"),
        json!({
            "status": "resolved",
            "resolution_note": "Payment confirmed outside the system"
        }),
    )
    .await?;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "resolved");
    assert_eq!(
        body["resolution_note"],
        "Payment confirmed outside the system"
    );

    Ok(())
}

#[tokio::test]
async fn resolve_discrepancy_rejects_invalid_status() -> Result<(), Box<dyn Error>> {
    let database = test_database().await?;
    let id = insert_test_discrepancy(&database).await?;
    let app = create_router(AppState { database });

    let (status, body) = send_json(
        app,
        "PATCH",
        &format!("/api/v1/discrepancies/{id}/resolution"),
        json!({
            "status": "open",
            "resolution_note": "Test note"
        }),
    )
    .await?;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "INVALID_STATUS");

    Ok(())
}
