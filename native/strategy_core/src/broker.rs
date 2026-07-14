use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::models::OrderId;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Buy,
    Sell,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContractSide {
    Yes,
    No,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderType {
    Market,
    Limit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderStatus {
    Filled,
    Partial,
    Pending,
    Rejected,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderExecutionStyle {
    RestingLimit,
    Direct,
    Sweep,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderTimePolicy {
    GoodTillCanceled,
    ImmediateOrCancel,
    FillOrKill,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrokerUpdateStatus {
    Accepted,
    Rejected,
    Submitted,
    Resting,
    PartiallyFilled,
    Filled,
    CancelRequested,
    Cancelled,
    Expired,
    Closed,
    SubmissionUnknown,
    Reconciled,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub ticker: String,
    pub side: ContractSide,
    pub quantity: i64,
    pub avg_price: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PendingOrder {
    pub order_id: OrderId,
    pub sleeve_id: String,
    pub ticker: String,
    pub action: Action,
    pub contract_side: ContractSide,
    pub limit_price: f64,
    pub requested_quantity: i64,
    #[serde(default)]
    pub filled_quantity: i64,
    #[serde(default)]
    pub reserved_global: f64,
    #[serde(default)]
    pub reserved_sleeve: f64,
    #[serde(default)]
    pub fee_type: String,
    pub fee_multiplier: Option<f64>,
    #[serde(default)]
    pub fee_accumulator: f64,
    pub signal_type: Option<String>,
    pub signal_metadata: Option<String>,
    #[serde(default)]
    pub created_at: String,
    pub client_order_id: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OrderResult {
    pub order_id: OrderId,
    pub sleeve_id: String,
    pub status: OrderStatus,
    #[serde(default)]
    pub filled_quantity: i64,
    #[serde(default)]
    pub fill_price: f64,
    #[serde(default)]
    pub fee_cost: f64,
    #[serde(default)]
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OrderIntent {
    pub ticker: String,
    pub action: Action,
    pub contract_side: ContractSide,
    pub order_type: OrderType,
    pub quantity: i64,
    pub limit_price: Option<f64>,
    pub max_price: Option<f64>,
    pub max_cost: Option<f64>,
    pub execution_style: Option<OrderExecutionStyle>,
    pub time_policy: Option<OrderTimePolicy>,
    #[serde(default)]
    pub reduce_only: bool,
    #[serde(default)]
    pub post_only: bool,
    pub signal_type: Option<String>,
    pub signal_metadata: Option<String>,
    pub client_order_id: Option<String>,
    #[serde(default)]
    pub expires_after_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrokerOrderUpdate {
    pub order_id: OrderId,
    pub sleeve_id: String,
    pub ticker: String,
    pub status: BrokerUpdateStatus,
    pub action: Action,
    pub contract_side: ContractSide,
    pub requested_quantity: i64,
    #[serde(default)]
    pub filled_quantity: i64,
    #[serde(default)]
    pub remaining_quantity: i64,
    #[serde(default)]
    pub fill_price: f64,
    #[serde(default)]
    pub average_fill_price: f64,
    #[serde(default)]
    pub fee_cost: f64,
    #[serde(default)]
    pub reason: String,
    pub client_order_id: Option<String>,
    pub provider_order_id: Option<String>,
    pub provider_sequence: Option<String>,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub expires_at: Option<String>,
}

pub trait Broker {
    type Error;

    /// Place an order through the legacy convenience surface.
    ///
    /// The default normalizes these arguments into a complete [`OrderIntent`]
    /// and delegates to [`Broker::place_order_with_intent`].
    fn place_order(
        &mut self,
        ticker: &str,
        action: Action,
        contract_side: ContractSide,
        order_type: OrderType,
        quantity: i64,
        limit_price: Option<f64>,
        signal_type: Option<&str>,
        signal_metadata: Option<&str>,
        client_order_id: Option<&str>,
    ) -> Result<OrderResult, Self::Error> {
        self.place_order_with_intent(OrderIntent {
            ticker: ticker.to_string(),
            action,
            contract_side,
            order_type,
            quantity,
            limit_price,
            max_price: None,
            max_cost: None,
            execution_style: None,
            time_policy: None,
            reduce_only: false,
            post_only: false,
            signal_type: signal_type.map(str::to_string),
            signal_metadata: signal_metadata.map(str::to_string),
            client_order_id: client_order_id.map(str::to_string),
            expires_after_ms: None,
        })
    }

    /// Canonical full-fidelity order path implemented by every runtime broker.
    fn place_order_with_intent(&mut self, intent: OrderIntent) -> Result<OrderResult, Self::Error>;

    fn cancel_order(&mut self, order_id: &str) -> Result<bool, Self::Error>;
    fn cancel_all_orders(&mut self) -> Result<usize, Self::Error>;
    fn get_position(&self, ticker: &str, side: ContractSide) -> Option<&Position>;
    fn get_positions(&self) -> BTreeMap<String, &Position>;
    fn get_pending_orders(&self) -> Vec<&PendingOrder>;
    fn get_sleeve_buying_power(&self) -> f64;
}
