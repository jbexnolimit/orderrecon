use mongodb::bson::{DateTime, oid::ObjectId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReconciliationSummary {
    pub total_orders: u64,
    pub matched_orders: u64,
    pub missing_payment: u64,
    pub amount_mismatch: u64,
    pub currency_mismatch: u64,
    pub duplicate_payment: u64,
    pub unmatched_payments: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconciliationRun {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub status: RunStatus,
    pub started_at: DateTime,
    pub completed_at: Option<DateTime>,
    pub summary: ReconciliationSummary,
    pub error: Option<String>,
}
