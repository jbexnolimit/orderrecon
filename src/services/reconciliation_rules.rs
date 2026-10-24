use mongodb::bson::{DateTime, oid::ObjectId};

use crate::models::{
    discrepancy::{Discrepancy, DiscrepancyStatus, DiscrepancyType},
    order::Order,
    payment::Payment,
    reconciliation_run::ReconciliationSummary,
};

fn new_discrepancy(
    run_id: ObjectId,
    discrepancy_type: DiscrepancyType,
    order_ref: Option<String>,
    payment_ids: Vec<ObjectId>,
    expected_amount_minor: Option<i64>,
    actual_amount_minor: Option<i64>,
    expected_currency: Option<String>,
    actual_currency: Option<String>,
) -> Discrepancy {
    let now = DateTime::now();

    Discrepancy {
        id: None,
        run_id,
        discrepancy_type,
        order_ref,
        payment_ids,
        expected_amount_minor,
        actual_amount_minor,
        expected_currency,
        actual_currency,
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
                DiscrepancyType::MissingPayment,
                Some(order.order_ref.clone()),
                vec![],
                Some(order.amount_minor),
                None,
                Some(order.currency.clone()),
                None,
            ));
        }
        [_, _, ..] => {
            summary.duplicate_payment += 1;

            let payment_ids = payments.iter().filter_map(|payment| payment.id).collect();

            let actual_amount = payments.iter().map(|payment| payment.amount_minor).sum();

            return Some(new_discrepancy(
                run_id,
                DiscrepancyType::DuplicatePayment,
                Some(order.order_ref.clone()),
                payment_ids,
                Some(order.amount_minor),
                Some(actual_amount),
                Some(order.currency.clone()),
                None,
            ));
        }
        [payment] => payment,
    };

    if payment.currency != order.currency {
        summary.currency_mismatch += 1;

        return Some(new_discrepancy(
            run_id,
            DiscrepancyType::CurrencyMismatch,
            Some(order.order_ref.clone()),
            payment.id.into_iter().collect(),
            Some(order.amount_minor),
            Some(payment.amount_minor),
            Some(order.currency.clone()),
            Some(payment.currency.clone()),
        ));
    }

    if payment.amount_minor != order.amount_minor {
        summary.amount_mismatch += 1;

        return Some(new_discrepancy(
            run_id,
            DiscrepancyType::AmountMismatch,
            Some(order.order_ref.clone()),
            payment.id.into_iter().collect(),
            Some(order.amount_minor),
            Some(payment.amount_minor),
            Some(order.currency.clone()),
            Some(payment.currency.clone()),
        ));
    }

    summary.matched_orders += 1;
    None
}

pub fn unmatched_payment_discrepancy(run_id: ObjectId, payment: &Payment) -> Discrepancy {
    new_discrepancy(
        run_id,
        DiscrepancyType::UnmatchedPayment,
        Some(payment.order_ref.clone()),
        payment.id.into_iter().collect(),
        None,
        Some(payment.amount_minor),
        None,
        Some(payment.currency.clone()),
    )
}
