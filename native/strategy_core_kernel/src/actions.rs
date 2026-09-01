use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderAction {
    Buy,
    Sell,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractSide {
    Yes,
    No,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderType {
    Market,
    Limit,
}

/// Exact contract quantity represented in hundredths of one contract.
///
/// Callers must choose the scale explicitly; no value-shape or sentinel inference is supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContractQuantity(i64);

impl ContractQuantity {
    pub const fn from_hundredths(hundredths: i64) -> Self {
        Self(hundredths)
    }

    pub const fn checked_from_whole_contracts(whole_contracts: i64) -> Option<Self> {
        match whole_contracts.checked_mul(100) {
            Some(hundredths) => Some(Self(hundredths)),
            None => None,
        }
    }

    pub const fn hundredths(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatus {
    Filled,
    Partial,
    Pending,
    Rejected,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaceOrderRequest {
    pub ticker: String,
    pub action: OrderAction,
    pub contract_side: ContractSide,
    pub order_type: OrderType,
    pub quantity: ContractQuantity,
    pub limit_price: Option<f64>,
    #[serde(default)]
    pub expires_after_ms: Option<i64>,
    #[serde(default)]
    pub reduce_only: bool,
    pub signal_type: Option<String>,
    pub signal_metadata: Option<String>,
    pub client_order_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelOrderRequest {
    pub order_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelAllOrdersRequest {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PendingOrderView<'a> {
    pub order_id: &'a str,
    pub ticker: &'a str,
    pub status: &'a str,
    pub action: &'a str,
    pub contract_side: &'a str,
    pub limit_price: Option<f64>,
    pub requested_quantity: ContractQuantity,
    pub filled_quantity: ContractQuantity,
    pub remaining_quantity: ContractQuantity,
    pub reserved_cost: f64,
    pub client_order_id: Option<&'a str>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderStatusView<'a> {
    pub order_id: &'a str,
    pub client_order_id: &'a str,
    pub status: OrderStatus,
    pub requested_quantity: ContractQuantity,
    pub filled_quantity: ContractQuantity,
    pub remaining_quantity: ContractQuantity,
    pub reason: &'a str,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WakeAtRequest {
    pub when: DateTime<Utc>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TelemetryAction {
    pub name: String,
    pub value: f64,
    pub fields: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogAction {
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopAction {
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum KernelAction {
    PlaceOrder(PlaceOrderRequest),
    CancelOrder(CancelOrderRequest),
    CancelAllOrders(CancelAllOrdersRequest),
    WakeAt(WakeAtRequest),
    Telemetry(TelemetryAction),
    Log(LogAction),
    Stop(StopAction),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderResult {
    pub order_id: String,
    pub sleeve_id: String,
    pub status: OrderStatus,
    pub filled_quantity: ContractQuantity,
    pub fill_price: f64,
    pub fee_cost: f64,
    pub reason: String,
}
