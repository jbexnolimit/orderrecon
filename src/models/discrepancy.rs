use mongodb::bson::{DateTime, oid::ObjectId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiscrepancyType {
    MissingPayment,
    AmountMismatch,
    CurrencyMismatch,
    DuplicatePayment,
    UnmatchedPayment,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiscrepancyStatus {
    Open,
    Resolved,
    Ignored,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Discrepancy {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub run_id: ObjectId,
    pub discrepancy_type: DiscrepancyType,
    pub order_ref: Option<String>,
    pub payment_ids: Vec<ObjectId>,
    pub expected_amount_minor: Option<i64>,
    pub actual_amount_minor: Option<i64>,
    pub expected_currency: Option<String>,
    pub actual_currency: Option<String>,
    pub status: DiscrepancyStatus,
    pub resolution_note: Option<String>,
    pub created_at: DateTime,
    pub updated_at: DateTime,
}
