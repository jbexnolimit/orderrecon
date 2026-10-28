use mongodb::{
    Client, Database,
    bson::{DateTime, doc, oid::ObjectId},
};
use orderrecon::{
    database,
    models::{
        order::{Order, OrderStatus},
        payment::{Payment, PaymentStatus},
        reconciliation_run::{ReconciliationRun, RunStatus},
    },
    services::reconciliation,
};
use std::{env, error::Error};

async fn test_database() -> Result<Database, Box<dyn Error>> {
    dotenvy::dotenv().ok();

    let test_db_name =
        env::var("ORDERRECON_TEST_DATABASE").expect("Set ORDERRECON_TEST_DATABASE=orderrecon_test");

    assert_eq!(test_db_name, "orderrecon_test");

    if let Ok(app_db_name) = env::var("MONGODB_DATABASE") {
        assert_ne!(test_db_name, app_db_name);
    }

    let uri = env::var("MONGODB_URI").unwrap_or_else(|_| "mongodb://127.0.0.1:27017".to_string());

    let client = Client::with_uri_str(&uri).await?;
    let database = client.database(&test_db_name);

    database.run_command(doc! { "ping": 1 }).await?;
    database::ensure_indexes(&database).await?;

    Ok(database)
}

#[tokio::test]
async fn overflow_marks_reconciliation_run_as_failed() -> Result<(), Box<dyn Error>> {
    let database = test_database().await?;
    let order_ref = format!("ORD-OVERFLOW-{}", ObjectId::new());

    let order = Order {
        id: None,
        order_ref: order_ref.clone(),
        amount_minor: 100,
        currency: "PHP".to_string(),
        status: OrderStatus::Placed,
        created_at: DateTime::now(),
    };

    database
        .collection::<Order>("orders")
        .insert_one(order)
        .await?;

    let payments = [
        Payment {
            id: None,
            source: "test_gateway".to_string(),
            source_transaction_id: format!("TXN-{}", ObjectId::new()),
            order_ref: order_ref.clone(),
            amount_minor: i64::MAX,
            currency: "PHP".to_string(),
            status: PaymentStatus::Captured,
            imported_at: DateTime::now(),
        },
        Payment {
            id: None,
            source: "test_gateway".to_string(),
            source_transaction_id: format!("TXN-{}", ObjectId::new()),
            order_ref,
            amount_minor: 1,
            currency: "PHP".to_string(),
            status: PaymentStatus::Captured,
            imported_at: DateTime::now(),
        },
    ];

    database
        .collection::<Payment>("payments")
        .insert_many(payments)
        .await?;

    let started_at = DateTime::now();
    let result = reconciliation::execute(&database).await;

    assert!(result.is_err(), "Expected payment-total overflow to fail");

    let failed_run = database
        .collection::<ReconciliationRun>("reconciliation_runs")
        .find_one(doc! {
            "status": "failed",
            "started_at": { "$gte": started_at },
            "error": "Reconciliation processing failed."
        })
        .await?
        .expect("Expected the failed run to be saved");

    assert!(matches!(failed_run.status, RunStatus::Failed));
    assert!(failed_run.completed_at.is_some());
    assert_eq!(
        failed_run.error.as_deref(),
        Some("Reconciliation processing failed.")
    );

    Ok(())
}
