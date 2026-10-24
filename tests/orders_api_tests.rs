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
        "Refusing to run integration tests against any database except orderrecon_test"
    );

    if let Ok(app_db_name) = env::var("MONGODB_DATABASE") {
        assert_ne!(
            test_db_name, app_db_name,
            "Refusing to run integration tests against the application database"
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
    let database = test_database().await?;

    Ok(create_router(AppState { database }))
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
    let json: Value = serde_json::from_slice(&body)?;

    Ok((status, json))
}

fn unique_order_ref() -> String {
    format!("TEST-{}", mongodb::bson::oid::ObjectId::new())
}

#[tokio::test]
async fn create_order_returns_201_and_normalizes_input() -> Result<(), Box<dyn Error>> {
    let app = test_app().await?;
    let order_ref = unique_order_ref();

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/orders",
        json!({
            "order_ref": format!("  {order_ref}  "),
            "amount_minor": 150000,
            "currency": " php "
        }),
    )
    .await?;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["order_ref"], order_ref);
    assert_eq!(body["amount_minor"], 150000);
    assert_eq!(body["currency"], "PHP");
    assert_eq!(body["status"], "placed");

    Ok(())
}

#[tokio::test]
async fn create_order_rejects_non_positive_amount() -> Result<(), Box<dyn Error>> {
    let app = test_app().await?;

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/orders",
        json!({
            "order_ref": unique_order_ref(),
            "amount_minor": 0,
            "currency": "PHP"
        }),
    )
    .await?;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "VALIDATION_ERROR");

    Ok(())
}

#[tokio::test]
async fn create_order_rejects_invalid_currency() -> Result<(), Box<dyn Error>> {
    let app = test_app().await?;

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/orders",
        json!({
            "order_ref": unique_order_ref(),
            "amount_minor": 150000,
            "currency": "PESO"
        }),
    )
    .await?;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "VALIDATION_ERROR");

    Ok(())
}

#[tokio::test]
async fn create_order_rejects_duplicate_order_ref() -> Result<(), Box<dyn Error>> {
    let app = test_app().await?;
    let order_ref = unique_order_ref();

    let payload = json!({
        "order_ref": order_ref,
        "amount_minor": 150000,
        "currency": "PHP"
    });

    let (first_status, _) =
        send_json(app.clone(), "POST", "/api/v1/orders", payload.clone()).await?;

    let (second_status, body) = send_json(app, "POST", "/api/v1/orders", payload).await?;

    assert_eq!(first_status, StatusCode::CREATED);
    assert_eq!(second_status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "DUPLICATE_ORDER");

    Ok(())
}

#[tokio::test]
async fn get_order_returns_404_when_missing() -> Result<(), Box<dyn Error>> {
    let app = test_app().await?;
    let order_ref = unique_order_ref();

    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/orders/{order_ref}"))
        .body(Body::empty())?;

    let response = app.oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    Ok(())
}
