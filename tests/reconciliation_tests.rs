use mongodb::bson::{DateTime, oid::ObjectId};
use orderrecon::{
    models::{
        discrepancy::{DiscrepancyStatus, DiscrepancyType},
        order::{Order, OrderStatus},
        payment::{Payment, PaymentStatus},
        reconciliation_run::ReconciliationSummary,
    },
    services::reconciliation_rules::{reconcile_order, unmatched_payment_discrepancy},
};

fn make_order() -> Order {
    Order {
        id: Some(ObjectId::new()),
        order_ref: "ORD-1001".to_string(),
        amount_minor: 150_000,
        currency: "PHP".to_string(),
        status: OrderStatus::Placed,
        created_at: DateTime::now(),
    }
}

fn make_payment(order_ref: &str, amount_minor: i64, currency: &str) -> Payment {
    Payment {
        id: Some(ObjectId::new()),
        source: "test_gateway".to_string(),
        source_transaction_id: format!("TXN-{}", ObjectId::new()),
        order_ref: order_ref.to_string(),
        amount_minor,
        currency: currency.to_string(),
        status: PaymentStatus::Captured,
        imported_at: DateTime::now(),
    }
}

#[test]
fn matching_payment_has_no_discrepancy() {
    let order = make_order();
    let payment = make_payment("ORD-1001", 150_000, "PHP");
    let run_id = ObjectId::new();
    let mut summary = ReconciliationSummary::default();

    let result = reconcile_order(run_id, &order, &[&payment], &mut summary);

    assert!(result.is_none());
    assert_eq!(summary.matched_orders, 1);
    assert_eq!(summary.missing_payment, 0);
    assert_eq!(summary.amount_mismatch, 0);
    assert_eq!(summary.currency_mismatch, 0);
    assert_eq!(summary.duplicate_payment, 0);
}

#[test]
fn missing_payment_creates_discrepancy() {
    let order = make_order();
    let run_id = ObjectId::new();
    let mut summary = ReconciliationSummary::default();

    let result = reconcile_order(run_id, &order, &[], &mut summary);

    let discrepancy = result.expect("Expected a missing-payment discrepancy");

    assert_eq!(
        discrepancy.discrepancy_type,
        DiscrepancyType::MissingPayment
    );
    assert_eq!(discrepancy.order_ref.as_deref(), Some("ORD-1001"));
    assert_eq!(discrepancy.expected_amount_minor, Some(150_000));
    assert_eq!(discrepancy.actual_amount_minor, None);
    assert_eq!(discrepancy.status, DiscrepancyStatus::Open);
    assert_eq!(summary.missing_payment, 1);
}

#[test]
fn incorrect_amount_creates_amount_mismatch() {
    let order = make_order();
    let payment = make_payment("ORD-1001", 140_000, "PHP");
    let run_id = ObjectId::new();
    let mut summary = ReconciliationSummary::default();

    let result = reconcile_order(run_id, &order, &[&payment], &mut summary);

    let discrepancy = result.expect("Expected an amount-mismatch discrepancy");

    assert_eq!(
        discrepancy.discrepancy_type,
        DiscrepancyType::AmountMismatch
    );
    assert_eq!(discrepancy.expected_amount_minor, Some(150_000));
    assert_eq!(discrepancy.actual_amount_minor, Some(140_000));
    assert_eq!(summary.amount_mismatch, 1);
}

#[test]
fn incorrect_currency_creates_currency_mismatch() {
    let order = make_order();
    let payment = make_payment("ORD-1001", 150_000, "USD");
    let run_id = ObjectId::new();
    let mut summary = ReconciliationSummary::default();

    let result = reconcile_order(run_id, &order, &[&payment], &mut summary);

    let discrepancy = result.expect("Expected a currency-mismatch discrepancy");

    assert_eq!(
        discrepancy.discrepancy_type,
        DiscrepancyType::CurrencyMismatch
    );
    assert_eq!(discrepancy.expected_currency.as_deref(), Some("PHP"));
    assert_eq!(discrepancy.actual_currency.as_deref(), Some("USD"));
    assert_eq!(summary.currency_mismatch, 1);
    assert_eq!(summary.amount_mismatch, 0);
}

#[test]
fn duplicate_payments_take_priority_over_amount_mismatch() {
    let order = make_order();
    let payment_one = make_payment("ORD-1001", 100_000, "PHP");
    let payment_two = make_payment("ORD-1001", 60_000, "PHP");
    let run_id = ObjectId::new();
    let mut summary = ReconciliationSummary::default();

    let result = reconcile_order(run_id, &order, &[&payment_one, &payment_two], &mut summary);

    let discrepancy = result.expect("Expected a duplicate-payment discrepancy");

    assert_eq!(
        discrepancy.discrepancy_type,
        DiscrepancyType::DuplicatePayment
    );
    assert_eq!(discrepancy.payment_ids.len(), 2);
    assert_eq!(discrepancy.expected_amount_minor, Some(150_000));
    assert_eq!(discrepancy.actual_amount_minor, Some(160_000));
    assert_eq!(summary.duplicate_payment, 1);
    assert_eq!(summary.amount_mismatch, 0);
}

#[test]
fn unmatched_payment_references_unknown_order() {
    let payment = make_payment("ORD-UNKNOWN", 75_000, "PHP");
    let run_id = ObjectId::new();

    let discrepancy = unmatched_payment_discrepancy(run_id, &payment);

    assert_eq!(
        discrepancy.discrepancy_type,
        DiscrepancyType::UnmatchedPayment
    );
    assert_eq!(discrepancy.order_ref.as_deref(), Some("ORD-UNKNOWN"));
    assert_eq!(discrepancy.actual_amount_minor, Some(75_000));
    assert_eq!(discrepancy.expected_amount_minor, None);
    assert_eq!(discrepancy.status, DiscrepancyStatus::Open);
}
