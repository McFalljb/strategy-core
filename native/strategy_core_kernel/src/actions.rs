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

/// The order a cancel names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelTarget {
    /// An order the Broker reports, by its order id.
    OrderId(String),
    /// An order by its client order id, including one placed earlier in the same decision,
    /// which has no order id yet.
    ClientOrderId(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelOrderRequest {
    pub target: CancelTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelAllOrdersRequest {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PendingOrderView<'a> {
    /// Empty for an order placed earlier in this decision.
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

/// An order's status as the Broker (and, within a decision, the runner's provisional view)
/// reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrokerOrderStatus {
    /// Placed earlier in this decision; the Broker has not seen it yet.
    Submitted,
    Accepted,
    Dispatched,
    Resting,
    PartiallyFilled,
    Filled,
    CancellationRequested,
    Cancelled,
    Expired,
    Rejected,
    RecoveryRequired,
}

impl BrokerOrderStatus {
    /// The status text `PendingOrderView::status` carries.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Submitted => "submitted",
            Self::Accepted => "accepted",
            Self::Dispatched => "dispatched",
            Self::Resting => "pending",
            Self::PartiallyFilled => "partial",
            Self::Filled => "filled",
            Self::CancellationRequested => "cancellation_requested",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
            Self::Rejected => "rejected",
            Self::RecoveryRequired => "recovery_required",
        }
    }

    /// The order will not change again.
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Filled | Self::Cancelled | Self::Expired | Self::Rejected
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderStatusView<'a> {
    /// Empty for an order placed earlier in this decision.
    pub order_id: &'a str,
    pub client_order_id: &'a str,
    pub status: BrokerOrderStatus,
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

/// What `place_order` returns at once: the order's command and client order id. What happens
/// to the order arrives later as [`OrderUpdate`] events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderTicket {
    pub command_id: String,
    pub client_order_id: String,
}

/// What `cancel_order` and `cancel_all_orders` return at once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandTicket {
    pub command_id: String,
}

/// The kind of Broker command an [`OrderUpdate`] reports on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrokerCommandKind {
    PlaceOrder,
    CancelOrder,
    CancelAllOrders,
}

/// An order's status in an [`OrderUpdate`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderUpdateStatus {
    Accepted,
    Resting,
    PartiallyFilled,
    Filled,
    Cancelled,
    Expired,
    /// Admission refused the command (price moved, allowance, balance, shutdown, retired, live
    /// not armed, cancel target final, ...) or the provider rejected the order.
    Refused {
        code: String,
        reason: String,
    },
}

impl OrderUpdateStatus {
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Filled | Self::Cancelled | Self::Expired | Self::Refused { .. }
        )
    }
}

/// What happened to one of the Strategy's orders or commands since the last update it saw.
///
/// A place's updates report its order. A cancel's update is its refusal (`command_kind` is
/// `CancelOrder`, the order fields describe the target, whose own status is unchanged); an
/// admitted cancel shows as the target order's `Cancelled` update. A cancel-all's update is its
/// refusal, with no order: empty `client_order_id` and `ticker`, no `action` or `contract_side`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderUpdate {
    pub command_kind: BrokerCommandKind,
    pub command_id: String,
    pub client_order_id: String,
    pub order_id: Option<String>,
    pub ticker: String,
    pub action: Option<OrderAction>,
    pub contract_side: Option<ContractSide>,
    pub status: OrderUpdateStatus,
    pub requested: ContractQuantity,
    pub filled: ContractQuantity,
    pub remaining: ContractQuantity,
    /// Filled since the last update the Strategy saw for this order.
    pub newly_filled: ContractQuantity,
    pub average_fill_price: Option<f64>,
    /// Execution fees charged so far, in dollars.
    pub fee_cost: f64,
    /// No further update follows for this command: its status is terminal, or the order
    /// vanished.
    pub is_final: bool,
    /// The Broker no longer reports the order: `status` is the last one seen, `remaining` is
    /// zero and `is_final` is true. If the order reappears, its updates continue (the next one
    /// reports what was filled meanwhile), so a vanished update is final only as far as the
    /// runner knows.
    pub vanished: bool,
}

/// The HTTP method of an [`HttpRequest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HttpMethod {
    Get,
    Post,
}

/// An HTTP call the host makes for the Strategy after the decision is saved.
///
/// `endpoint` is a name from the Strategy's allowlist, never a URL: the host owns the base
/// URL and any credential. `path` (starting with `/`, query included) is appended to the
/// endpoint's base URL. The answer arrives later as a [`StrategyEvent::ExternalResponse`].
///
/// [`StrategyEvent::ExternalResponse`]: crate::StrategyEvent::ExternalResponse
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpRequest {
    pub endpoint: String,
    pub method: HttpMethod,
    pub path: String,
    pub body: Vec<u8>,
    /// At most two minutes; the host gives up and answers `Timeout` after it.
    pub timeout_ms: u32,
}

/// A command the host runs for the Strategy after the decision is saved.
///
/// `command` is a name from the Strategy's allowlist, never a path: the host owns the program,
/// its leading arguments and its environment. `args` follow the configured ones; `stdin` is
/// written to the program's standard input. Its standard output is the response body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandRequest {
    pub command: String,
    pub args: Vec<String>,
    pub stdin: Vec<u8>,
    /// At most two minutes; the host kills the program and answers `Timeout` after it.
    pub timeout_ms: u32,
}

/// What `request_http` and `request_command` return at once. The answer arrives later as an
/// [`ExternalResponse`] with the same `request_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestTicket {
    pub request_id: String,
}

/// Why an external request has no answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalErrorKind {
    /// The host did not send it (not allowed, too many outstanding, shutting down).
    Refused,
    /// No answer within the request's timeout; a command was killed.
    Timeout,
    /// The call could not be made (connection, TLS, I/O, the program could not start).
    Transport,
    /// The HTTP status was not 2xx; `message` carries the start of the body.
    Status(u16),
    /// The response body (or a command's output) was larger than the host accepts.
    TooLarge,
    /// The response was not a valid HTTP response.
    Malformed,
    /// The command exited unsuccessfully; `None` when a signal ended it.
    Exit(Option<i32>),
    /// traderd restarted while the request was in flight; it may or may not have been
    /// performed. Ask again if it still matters.
    Abandoned,
}

/// The answer to one external request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalOutcome {
    /// A 2xx HTTP status and its body, or a command's exit status 0 and its standard output.
    Ok { status: u16, body: Vec<u8> },
    Err {
        kind: ExternalErrorKind,
        message: String,
    },
}

/// The answer to an [`HttpRequest`] or [`CommandRequest`] the Strategy issued in an earlier
/// decision, delivered as its own event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalResponse {
    pub request_id: String,
    pub outcome: ExternalOutcome,
}
