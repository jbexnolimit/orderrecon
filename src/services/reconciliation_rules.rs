use mongodb::bson::{DateTime, oid::ObjectId};

use crate::models::{
    discrepancy::{Discrepancy, DiscrepancyStatus, DiscrepancyType},
    order::Order,
    payment::Payment,
    reconciliation_run::ReconciliationSummary,
};

struct DiscrepancyDetails {
    discrepancy_type: DiscrepancyType,
    order_ref: Option<String>,
    payment_ids: Vec<ObjectId>,
    expected_amount_minor: Option<i64>,
    actual_amount_minor: Option<i64>,
    expected_currency: Option<String>,
    actual_currency: Option<String>,
}

fn new_discrepancy(run_id: ObjectId, details: DiscrepancyDetails) -> Discrepancy {
    let now = DateTime::now();

    Discrepancy {
        id: None,
        run_id,
        discrepancy_type: details.discrepancy_type,
        order_ref: details.order_ref,
        payment_ids: details.payment_ids,
        expected_amount_minor: details.expected_amount_minor,
        actual_amount_minor: details.actual_amount_minor,
        expected_currency: details.expected_currency,
        actual_currency: details.actual_currency,
        status: DiscrepancyStatus::Open,
        resolution_note: None,
        created_at: now,
        updated_at: now,
    }
}

pub fn reconcile_order(
    run_id: ObjectId,
    order: &Order,
    payments: &[&Payment],
    summary: &mut ReconciliationSummary,
) -> Option<Discrepancy> {
    let payment = match payments {
        [] => {
            summary.missing_payment += 1;

            return Some(new_discrepancy(
                run_id,
                DiscrepancyDetails {
                    discrepancy_type: DiscrepancyType::MissingPayment,
                    order_ref: Some(order.order_ref.clone()),
                    payment_ids: vec![],
                    expected_amount_minor: Some(order.amount_minor),
                    actual_amount_minor: None,
                    expected_currency: Some(order.currency.clone()),
                    actual_currency: None,
                },
            ));
        }
        [_, _, ..] => {
            summary.duplicate_payment += 1;

            let payment_ids = payments.iter().filter_map(|payment| payment.id).collect();
            let actual_amount = payments.iter().map(|payment| payment.amount_minor).sum();

            return Some(new_discrepancy(
                run_id,
                DiscrepancyDetails {
                    discrepancy_type: DiscrepancyType::DuplicatePayment,
                    order_ref: Some(order.order_ref.clone()),
                    payment_ids,
                    expected_amount_minor: Some(order.amount_minor),
                    actual_amount_minor: Some(actual_amount),
                    expected_currency: Some(order.currency.clone()),
                    actual_currency: None,
                },
            ));
        }
        [payment] => payment,
    };

    if payment.currency != order.currency {
        summary.currency_mismatch += 1;

        return Some(new_discrepancy(
            run_id,
            DiscrepancyDetails {
                discrepancy_type: DiscrepancyType::CurrencyMismatch,
                order_ref: Some(order.order_ref.clone()),
                payment_ids: payment.id.into_iter().collect(),
                expected_amount_minor: Some(order.amount_minor),
                actual_amount_minor: Some(payment.amount_minor),
                expected_currency: Some(order.currency.clone()),
                actual_currency: Some(payment.currency.clone()),
            },
        ));
    }

    if payment.amount_minor != order.amount_minor {
        summary.amount_mismatch += 1;

        return Some(new_discrepancy(
            run_id,
            DiscrepancyDetails {
                discrepancy_type: DiscrepancyType::AmountMismatch,
                order_ref: Some(order.order_ref.clone()),
                payment_ids: payment.id.into_iter().collect(),
                expected_amount_minor: Some(order.amount_minor),
                actual_amount_minor: Some(payment.amount_minor),
                expected_currency: Some(order.currency.clone()),
                actual_currency: Some(payment.currency.clone()),
            },
        ));
    }

    summary.matched_orders += 1;
    None
}

pub fn unmatched_payment_discrepancy(run_id: ObjectId, payment: &Payment) -> Discrepancy {
    new_discrepancy(
        run_id,
        DiscrepancyDetails {
            discrepancy_type: DiscrepancyType::UnmatchedPayment,
            order_ref: Some(payment.order_ref.clone()),
            payment_ids: payment.id.into_iter().collect(),
            expected_amount_minor: None,
            actual_amount_minor: Some(payment.amount_minor),
            expected_currency: None,
            actual_currency: Some(payment.currency.clone()),
        },
    )
}
