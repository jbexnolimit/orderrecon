use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header::CONTENT_TYPE},
};
use mongodb::{Client, Database, bson::doc};
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

fn unique_transaction_id() -> String {
    format!("TEST-TXN-{}", mongodb::bson::oid::ObjectId::new())
}

#[tokio::test]
async fn import_payments_imports_valid_payment_and_normalizes_fields() -> Result<(), Box<dyn Error>>
{
    let app = test_app().await?;
    let transaction_id = unique_transaction_id();

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/payments/import",
        json!({
            "source": "  TestGateway  ",
            "payments": [{
                "source_transaction_id": format!("  {transaction_id}  "),
                "order_ref": "  TEST-ORDER-001  ",
                "amount_minor": 150000,
                "currency": " php ",
                "status": "captured"
            }]
        }),
    )
    .await?;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["summary"]["total"], 1);
    assert_eq!(body["summary"]["imported"], 1);
    assert_eq!(body["summary"]["duplicates"], 0);
    assert_eq!(body["summary"]["rejected"], 0);
    assert_eq!(body["results"][0]["status"], "imported");
    assert_eq!(body["results"][0]["source_transaction_id"], transaction_id);

    Ok(())
}

#[tokio::test]
async fn import_payments_reports_duplicate_transaction() -> Result<(), Box<dyn Error>> {
    let app = test_app().await?;
    let transaction_id = unique_transaction_id();

    let payload = json!({
        "source": "TestGateway",
        "payments": [{
            "source_transaction_id": transaction_id,
            "order_ref": "TEST-ORDER-002",
            "amount_minor": 150000,
            "currency": "PHP",
            "status": "captured"
        }]
    });

    let (first_status, _) = send_json(
        app.clone(),
        "POST",
        "/api/v1/payments/import",
        payload.clone(),
    )
    .await?;

    let (second_status, body) = send_json(app, "POST", "/api/v1/payments/import", payload).await?;

    assert_eq!(first_status, StatusCode::OK);
    assert_eq!(second_status, StatusCode::OK);
    assert_eq!(body["summary"]["imported"], 0);
    assert_eq!(body["summary"]["duplicates"], 1);
    assert_eq!(body["summary"]["rejected"], 0);
    assert_eq!(body["results"][0]["status"], "duplicate");

    Ok(())
}

#[tokio::test]
async fn import_payments_reports_rejected_item_without_failing_batch() -> Result<(), Box<dyn Error>>
{
    let app = test_app().await?;

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/payments/import",
        json!({
            "source": "TestGateway",
            "payments": [
                {
                    "source_transaction_id": unique_transaction_id(),
                    "order_ref": "TEST-ORDER-003",
                    "amount_minor": 150000,
                    "currency": "PHP",
                    "status": "captured"
                },
                {
                    "source_transaction_id": unique_transaction_id(),
                    "order_ref": "TEST-ORDER-004",
                    "amount_minor": -1,
                    "currency": "PHP",
                    "status": "captured"
                }
            ]
        }),
    )
    .await?;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["summary"]["total"], 2);
    assert_eq!(body["summary"]["imported"], 1);
    assert_eq!(body["summary"]["duplicates"], 0);
    assert_eq!(body["summary"]["rejected"], 1);
    assert_eq!(body["results"][1]["status"], "rejected");

    Ok(())
}

#[tokio::test]
async fn import_payments_rejects_empty_batch() -> Result<(), Box<dyn Error>> {
    let app = test_app().await?;

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/payments/import",
        json!({
            "source": "TestGateway",
            "payments": []
        }),
    )
    .await?;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "VALIDATION_ERROR");

    Ok(())
}

#[tokio::test]
async fn list_payments_rejects_invalid_pagination() -> Result<(), Box<dyn Error>> {
    let app = test_app().await?;

    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/payments?page=0&limit=20")
        .body(Body::empty())?;

    let response = app.oneshot(request).await?;
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let body: Value = serde_json::from_slice(&body)?;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "INVALID_PAGINATION");

    Ok(())
}
