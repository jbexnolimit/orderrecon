use mongodb::{Client, Database, IndexModel, bson::doc, options::IndexOptions};

pub async fn connect(uri: &str, database_name: &str) -> Result<Database, mongodb::error::Error> {
    let client = Client::with_uri_str(uri).await?;
    let database = client.database(database_name);

    database.run_command(doc! { "ping": 1 }).await?;

    Ok(database)
}

pub async fn ensure_indexes(database: &Database) -> Result<(), mongodb::error::Error> {
    let orders = database.collection::<mongodb::bson::Document>("orders");
    let payments = database.collection::<mongodb::bson::Document>("payments");
    let runs = database.collection::<mongodb::bson::Document>("reconciliation_runs");
    let discrepancies = database.collection::<mongodb::bson::Document>("discrepancies");

    orders
        .create_index(
            IndexModel::builder()
                .keys(doc! { "order_ref": 1 })
                .options(
                    IndexOptions::builder()
                        .unique(true)
                        .name("uq_orders_order_ref".to_string())
                        .build(),
                )
                .build(),
        )
        .await?;

    payments
        .create_index(
            IndexModel::builder()
                .keys(doc! {
                    "source": 1,
                    "source_transaction_id": 1
                })
                .options(
                    IndexOptions::builder()
                        .unique(true)
                        .name("uq_payments_source_transaction".to_string())
                        .build(),
                )
                .build(),
        )
        .await?;

    payments
        .create_index(
            IndexModel::builder()
                .keys(doc! { "order_ref": 1, "status": 1 })
                .options(
                    IndexOptions::builder()
                        .name("ix_payments_order_status".to_string())
                        .build(),
                )
                .build(),
        )
        .await?;

    runs.create_index(
        IndexModel::builder()
            .keys(doc! { "started_at": -1 })
            .options(
                IndexOptions::builder()
                    .name("ix_runs_started_at".to_string())
                    .build(),
            )
            .build(),
    )
    .await?;

    discrepancies
        .create_index(
            IndexModel::builder()
                .keys(doc! {
                    "run_id": 1,
                    "discrepancy_type": 1,
                    "status": 1
                })
                .options(
                    IndexOptions::builder()
                        .name("ix_discrepancies_run_type_status".to_string())
                        .build(),
                )
                .build(),
        )
        .await?;

    discrepancies
        .create_index(
            IndexModel::builder()
                .keys(doc! { "order_ref": 1, "created_at": -1 })
                .options(
                    IndexOptions::builder()
                        .name("ix_discrepancies_order_created".to_string())
                        .build(),
                )
                .build(),
        )
        .await?;

    Ok(())
}
