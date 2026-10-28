use std::collections::{HashMap, HashSet};

use futures_util::TryStreamExt;
use mongodb::{
    Database,
    bson::{DateTime, doc},
};

use crate::{
    models::{
        discrepancy::Discrepancy,
        order::Order,
        payment::Payment,
        reconciliation_run::{ReconciliationRun, ReconciliationSummary, RunStatus},
    },
    services::reconciliation_rules::{reconcile_order, unmatched_payment_discrepancy},
};

#[derive(Debug, thiserror::Error)]
pub enum ReconciliationError {
    #[error(transparent)]
    Database(#[from] mongodb::error::Error),

    #[error(transparent)]
    Rule(#[from] crate::services::reconciliation_rules::ReconciliationRuleError),
}

pub async fn execute(database: &Database) -> Result<ReconciliationRun, ReconciliationError> {
    let orders_collection = database.collection::<Order>("orders");
    let payments_collection = database.collection::<Payment>("payments");
    let runs_collection = database.collection::<ReconciliationRun>("reconciliation_runs");
    let discrepancies_collection = database.collection::<Discrepancy>("discrepancies");

    let started_at = DateTime::now();

    let initial_run = ReconciliationRun {
        id: None,
        status: RunStatus::Running,
        started_at,
        completed_at: None,
        summary: ReconciliationSummary::default(),
        error: None,
    };

    let insert_result = runs_collection.insert_one(initial_run).await?;

    let run_id = insert_result
        .inserted_id
        .as_object_id()
        .expect("MongoDB should generate an ObjectId for a new run");

    let processing_result: Result<(ReconciliationSummary, DateTime), ReconciliationError> = async {
        let orders: Vec<Order> = orders_collection.find(doc! {}).await?.try_collect().await?;

        let captured_payments: Vec<Payment> = payments_collection
            .find(doc! { "status": "captured" })
            .await?
            .try_collect()
            .await?;

        let existing_order_refs: HashSet<&str> = orders
            .iter()
            .map(|order| order.order_ref.as_str())
            .collect();

        let mut payments_by_order: HashMap<&str, Vec<&Payment>> = HashMap::new();

        for payment in &captured_payments {
            payments_by_order
                .entry(payment.order_ref.as_str())
                .or_default()
                .push(payment);
        }

        let mut summary = ReconciliationSummary {
            total_orders: orders.len() as u64,
            ..Default::default()
        };

        let mut discrepancies = Vec::new();

        for order in &orders {
            let payments = payments_by_order
                .get(order.order_ref.as_str())
                .map(Vec::as_slice)
                .unwrap_or(&[]);

            if let Some(discrepancy) = reconcile_order(run_id, order, payments, &mut summary)? {
                discrepancies.push(discrepancy);
            }
        }

        for payment in &captured_payments {
            if !existing_order_refs.contains(payment.order_ref.as_str()) {
                summary.unmatched_payments += 1;
                discrepancies.push(unmatched_payment_discrepancy(run_id, payment));
            }
        }

        if !discrepancies.is_empty() {
            discrepancies_collection.insert_many(discrepancies).await?;
        }

        let completed_at = DateTime::now();

        runs_collection
            .update_one(
                doc! { "_id": run_id },
                doc! {
                    "$set": {
                        "status": "completed",
                        "completed_at": completed_at,
                        "summary": {
                            "total_orders": summary.total_orders as i64,
                            "matched_orders": summary.matched_orders as i64,
                            "missing_payment": summary.missing_payment as i64,
                            "amount_mismatch": summary.amount_mismatch as i64,
                            "currency_mismatch": summary.currency_mismatch as i64,
                            "duplicate_payment": summary.duplicate_payment as i64,
                            "unmatched_payments": summary.unmatched_payments as i64
                        },
                        "error": mongodb::bson::Bson::Null
                    }
                },
            )
            .await?;

        Ok((summary, completed_at))
    }
    .await;

    match processing_result {
        Ok((summary, completed_at)) => Ok(ReconciliationRun {
            id: Some(run_id),
            status: RunStatus::Completed,
            started_at,
            completed_at: Some(completed_at),
            summary,
            error: None,
        }),
        Err(error) => {
            if let Err(update_error) = runs_collection
                .update_one(
                    doc! { "_id": run_id },
                    doc! {
                        "$set": {
                            "status": "failed",
                            "completed_at": DateTime::now(),
                            "error": "Reconciliation processing failed."
                        }
                    },
                )
                .await
            {
                tracing::error!(
                    run_id = %run_id,
                    error = %update_error,
                    "Could not mark reconciliation run as failed"
                );
            }

            Err(error)
        }
    }
}
