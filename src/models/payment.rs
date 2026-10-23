use mongodb::bson::{DateTime, oid::ObjectId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaymentStatus {
    Captured,
    Pending,
    Failed,
    Refunded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Payment {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub source: String,
    pub source_transaction_id: String,
    pub order_ref: String,
    pub amount_minor: i64,
    pub currency: String,
    pub status: PaymentStatus,
    pub imported_at: DateTime,
}
