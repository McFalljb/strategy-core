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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContractQuantity(i64);

impl ContractQuantity {
    pub const ZERO: Self = Self(0);

    pub const fn from_hundredths(hundredths: i64) -> Self {
        Self(hundredths)
    }

    pub const fn checked_from_whole_contracts(whole_contracts: i64) -> Option<Self> {
        match whole_contracts.checked_mul(100) {
            Some(hundredths) => Some(Self(hundredths)),
            None => None,
        }
    }

    /// A contract count that is exactly representable in hundredths; anything finer is
    /// rejected rather than rounded.
    pub fn checked_from_contracts_f64(contracts: f64) -> Option<Self> {
        if !contracts.is_finite() {
            return None;
        }
        // Interpret the caller's round-trip decimal value, not a rounded binary
        // multiplication: an epsilon would admit off-grid quantities, and a float-to-int
        // cast could saturate at the range boundary.
        let value = crate::decimal::Decimal::parse(&contracts.to_string()).ok()?;
        let hundredths = match value.scale {
            0 => value.coefficient.checked_mul(100)?,
            1 => value.coefficient.checked_mul(10)?,
            2 => value.coefficient,
            _ => return None,
        };
        Some(Self(hundredths))
    }

    pub const fn hundredths(self) -> i64 {
        self.0
    }

    /// Whole contracts: the floor of the exact quantity.
    pub const fn whole_contracts(self) -> i64 {
        self.0.div_euclid(100)
    }

    pub const fn is_whole(self) -> bool {
        self.0 % 100 == 0
    }

    /// The whole-contract count when the quantity is whole; `None` for a fractional quantity.
    pub const fn checked_whole_contracts(self) -> Option<i64> {
        if self.is_whole() {
            Some(self.0 / 100)
        } else {
            None
        }
    }

    /// The largest whole-contract quantity not above this one.
    pub const fn floor_to_whole(self) -> Self {
        Self(self.0.div_euclid(100) * 100)
    }

    pub const fn is_positive(self) -> bool {
        self.0 > 0
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// The quantity as a floating-point contract count, for price arithmetic.
    pub fn contracts_f64(self) -> f64 {
        self.0 as f64 / 100.0
    }

    pub const fn abs(self) -> Self {
        Self(self.0.abs())
    }

    pub const fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    pub const fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }

    pub const fn checked_add(self, other: Self) -> Option<Self> {
        match self.0.checked_add(other.0) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    pub const fn checked_sub(self, other: Self) -> Option<Self> {
        match self.0.checked_sub(other.0) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    pub const fn min(self, other: Self) -> Self {
        if self.0 <= other.0 { self } else { other }
    }

    pub const fn max(self, other: Self) -> Self {
        if self.0 >= other.0 { self } else { other }
    }
}

impl core::ops::Add for ContractQuantity {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl core::ops::Sub for ContractQuantity {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

impl core::ops::AddAssign for ContractQuantity {
    fn add_assign(&mut self, other: Self) {
        self.0 += other.0;
    }
}

impl core::ops::SubAssign for ContractQuantity {
    fn sub_assign(&mut self, other: Self) {
        self.0 -= other.0;
    }
}

impl core::iter::Sum for ContractQuantity {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, |total, value| total + value)
    }
}

impl<'a> core::iter::Sum<&'a ContractQuantity> for ContractQuantity {
    fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, |total, value| total + *value)
    }
}

impl core::fmt::Display for ContractQuantity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.is_whole() {
            write!(f, "{}", self.whole_contracts())
        } else {
            let negative = self.0 < 0;
            let abs = self.0.unsigned_abs();
            write!(
                f,
                "{}{}.{:02}",
                if negative { "-" } else { "" },
                abs / 100,
                abs % 100
            )
        }
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
