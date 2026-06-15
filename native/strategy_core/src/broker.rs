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

pub trait Broker {
    type Error;

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
    ) -> Result<OrderResult, Self::Error>;

    fn cancel_order(&mut self, order_id: &str) -> Result<bool, Self::Error>;
    fn cancel_all_orders(&mut self) -> Result<usize, Self::Error>;
    fn get_position(&self, ticker: &str, side: ContractSide) -> Option<&Position>;
    fn get_positions(&self) -> BTreeMap<String, &Position>;
    fn get_pending_orders(&self) -> Vec<&PendingOrder>;
    fn get_sleeve_buying_power(&self) -> f64;
}
