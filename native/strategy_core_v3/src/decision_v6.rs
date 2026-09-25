//! Decision V6: one run per event.
//!
//! A Strategy runs once per delivered event and returns every command it issued in that run.
//! V6 composes the immutable V4 owner projection with exact Strategy scope, side-aware Broker
//! state, the outcomes of recent commands that have no order record (`command_receipts`), the
//! deployment mode and the capabilities the host grants. The result carries the post-event
//! checkpoint (the kernel's private state plus the runner section that records what the
//! Strategy has seen of its orders), the decision's commands in issue order, and the command ids
//! whose final outcome the Strategy has now seen. Collections are canonically sorted before
//! encoding and validation rejects alternate orderings.
//!
//! See `docs/decision-v6.md` for the contract.

use std::collections::{BTreeMap, BTreeSet};

use bincode::{Decode, Encode};
use sha2::{Digest, Sha256};

use crate::current_v6::CurrentInputsV6;
use crate::decision_v4::{
    DecisionContextV4, DecisionV4Error, MAX_STATIONS, TriggerV4, decision_fence_v4_sha256,
};
use crate::supplied_v6::{ExtremeKindV6, SuppliedEventV6, SuppliedInputsV6};

pub const DECISION_CONTEXT_V6_MAGIC: &[u8; 8] = b"SDCTXV6A";
pub const DECISION_RESULT_V6_MAGIC: &[u8; 8] = b"SDRESV6A";
pub const MAX_DECISION_CONTEXT_V6_BYTES: usize = 20 * 1024 * 1024;
/// The kernel's 128 KiB state, the runner section, 64 commands, the derived order updates and
/// bounded telemetry.
pub const MAX_DECISION_RESULT_V6_BYTES: usize = 1024 * 1024;
/// The encoded size the runner admits commands and telemetry against: the result bound less
/// room for the decoder's accounting, which charges every decoded integer its full width.
/// The room is a heuristic, not a guarantee: the runner checks that each result decodes under
/// the result bound and sheds telemetry, then logs, until it does.
pub const RESULT_ENCODED_BUDGET_BYTES: usize = MAX_DECISION_RESULT_V6_BYTES - 128 * 1024;
pub const MAX_STRATEGY_PARAMETERS: usize = 256;
pub const MAX_BROKER_POSITIONS: usize = 256;
pub const MAX_BROKER_ORDERS: usize = 256;
/// Unacknowledged receipts per Sleeve; past this the Broker refuses new commands.
pub const MAX_COMMAND_RECEIPTS: usize = 256;
pub const MAX_STRATEGY_COMMANDS: usize = 64;
/// Every receipt and every terminal order in one context can be acknowledged at once.
pub const MAX_ACKNOWLEDGED_COMMANDS: usize = MAX_COMMAND_RECEIPTS + MAX_BROKER_ORDERS;
/// Live orders and commands the runner section may hold when a decision issues a Broker
/// command: a command that would take it to this many is a local error.
pub const MAX_RUNNER_ENTRIES: usize = 256;
/// Tombstones the runner section keeps besides its live entries; the oldest is evicted first.
pub const MAX_TOMBSTONES: usize = 32;
/// Entries (live and tombstones) one runner section holds. A tombstone whose order
/// reappears is live again and an adopted order is live from the start, so the live entries
/// of a derived section may pass `MAX_RUNNER_ENTRIES`, never this.
pub const MAX_RUNNER_SECTION_ENTRIES: usize = MAX_RUNNER_ENTRIES + MAX_TOMBSTONES;
/// Complete views above its vanish revision that must not show a tombstone before it expires.
pub const TOMBSTONE_EXPIRY_VIEWS: u16 = 16;
/// Times an order update is delivered to a kernel that returns an error for it before the
/// runner gives up and treats it as seen.
pub const MAX_DELIVERY_ATTEMPTS: u8 = 3;
/// Consecutive decisions an order update may wait for room in the decision (the room earlier
/// updates of the same decision took) before the runner gives up and treats it as seen.
pub const MAX_DELIVERY_DEFERRALS: u8 = 8;
pub const MAX_RESULT_EVIDENCE: usize = 64;
pub const MAX_RESULT_DIAGNOSTICS: usize = 64;
pub const MAX_RESULT_TELEMETRY: usize = 256;
pub const MAX_TELEMETRY_FIELDS: usize = 32;
pub const MAX_COMMAND_METADATA_BYTES: usize = 64 * 1024;
pub const MAX_EVIDENCE_PAYLOAD_BYTES: usize = 64 * 1024;
pub const MAX_TIMER_SEMANTICS_BYTES_V6: usize = 16 * 1024;
pub const MAX_RESULT_DIAGNOSTIC_BYTES: usize = 4 * 1024;
/// Maximum opaque private kernel state carried across one decision.
pub const MAX_KERNEL_CHECKPOINT_BYTES: usize = 128 * 1024;
pub const MAX_IDENTIFIER_BYTES: usize = 160;
/// The Broker's bound on a provider client order id.
pub const MAX_PROVIDER_CLIENT_ID_BYTES: usize = 128;
pub const MAX_SHORT_TEXT_BYTES: usize = 512;
pub const MAX_REASON_BYTES: usize = 4 * 1024;
/// The most of a refusal reason (a provider's rejection text or a receipt's reason) an order
/// update's evidence records; the kernel sees the whole reason (at most `MAX_REASON_BYTES`).
pub const MAX_REJECTION_REASON_BYTES: usize = 512;
pub const MAX_PRICE_MICROS: u64 = 1_000_000;
/// Request names the host may allow a Strategy (Phase 4); bounded now so the wire need not
/// change when they arrive.
pub const MAX_EXTERNAL_REQUEST_GRANTS: usize = 32;
/// Upper bound of one encoded runner entry: four bounded identifiers and fixed-width fields.
pub const MAX_ENCODED_RUNNER_ENTRY_BYTES: usize = 4 * (MAX_IDENTIFIER_BYTES + 3) + 80;
/// Open orders one Sleeve may hold in paper: open context orders plus the decision's own
/// places. The Sleeve's order view holds `MAX_BROKER_ORDERS`; the rest is room for terminal
/// orders whose outcome the Strategy has not yet acknowledged.
pub const MAX_OPEN_ORDERS: usize = 192;
/// Open orders one Sleeve may hold in live, where the Broker expands a cancel-all into one
/// cancel (3 plan rows) per order: a cancel-all over all of them still fits the plan with the
/// decision's 4 rows and 1 acknowledgement row: (512 - 5 - 3) / 3.
pub const MAX_OPEN_ORDERS_LIVE: usize =
    (MAX_DECISION_PLAN_ROWS - DECISION_PLAN_ROWS - ACKNOWLEDGEMENT_PLAN_ROWS - 3)
        / CANCEL_ORDER_PLAN_ROWS;
/// The Broker's refusal code of a Market sell outside paper (until Phase 5).
pub const MARKET_SELL_UNSUPPORTED_CODE: &str = "market_sell_unsupported";

/// The open orders a Sleeve may hold in `mode`, so a cancel-all over them always fits the
/// decision plan.
pub const fn max_open_orders(mode: DeploymentModeV6) -> usize {
    match mode {
        DeploymentModeV6::Paper => MAX_OPEN_ORDERS,
        DeploymentModeV6::Live => MAX_OPEN_ORDERS_LIVE,
    }
}
/// State, the runner section, three identifiers, the profile digest, and codec overhead.
pub const MAX_ENCODED_KERNEL_CHECKPOINT_BYTES: usize = MAX_KERNEL_CHECKPOINT_BYTES
    + MAX_RUNNER_SECTION_ENTRIES * MAX_ENCODED_RUNNER_ENTRY_BYTES
    + 3 * MAX_IDENTIFIER_BYTES
    + MAX_SHORT_TEXT_BYTES
    + 96;

/// The account plan row limit for one decision (traderv3 `MAX_DECISION_PLAN_ROWS`). The
/// open-order cap of each mode (`max_open_orders`) keeps a cancel-all over every open order
/// within it.
pub const MAX_DECISION_PLAN_ROWS: usize = 512;
/// Rows of one admitted place: acceptance, outbox, acceptance state, order, intent receipt.
pub const PLACE_ORDER_PLAN_ROWS: usize = 5;
/// Rows of one admitted cancel: cancellation, financial cancellation, intent receipt.
pub const CANCEL_ORDER_PLAN_ROWS: usize = 3;
/// Fixed rows of a cancel-all, before one row per order it cancels.
pub const CANCEL_ALL_ORDERS_PLAN_ROWS: usize = 3;
/// Extra rows, in live, of a place of the same decision that a cancel-all collapses.
pub const COLLAPSED_PLACE_PLAN_ROWS: usize = 1;
/// Rows of a refused command: its receipt.
pub const REFUSED_COMMAND_PLAN_ROWS: usize = 1;
/// Fixed rows of a decision with Broker commands: cash, shutdown, decision, checkpoint.
pub const DECISION_PLAN_ROWS: usize = 4;
/// Acknowledged receipts are removed with one statement.
pub const ACKNOWLEDGEMENT_PLAN_ROWS: usize = 1;

/// The capabilities a Decision V6 Strategy executable states in its handshake, sorted.
/// `broker-continuation` is gone with V5.
pub const HANDSHAKE_CAPABILITIES_V6: [&str; 3] = ["decision-v6", "kernel-checkpoint", "liveness"];

/// Evidence code of the order updates the runner derived for this decision.
pub const ORDER_UPDATES_EVIDENCE_CODE: &str = "order_updates";

const SLEEVE_ID_DOMAIN: &[u8] = b"trader-v3/sleeve-id/v1\0";
const DECISION_ID_DOMAIN: &[u8] = b"trader-v3/decision-id/v1\0";
const INTENT_ID_DOMAIN: &[u8] = b"trader-v3/intent-id/v1\0";
const CHECKPOINT_DIGEST_DOMAIN: &[u8] = b"strategy-core/decision-v6/checkpoint/v1\0";
const STATE_FENCE_DOMAIN: &[u8] = b"strategy-core/decision-v6/state-fence/v1\0";
const V5_CHECKPOINT_DIGEST_DOMAIN: &[u8] = b"strategy-core/decision-v5/checkpoint/v1\0";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecisionV6Error {
    Encode,
    Decode,
    BoundExceeded,
    TrailingBytes,
    InvalidContract,
    DuplicateIdentity,
    NonCanonicalOrder,
    V4(DecisionV4Error),
}

impl core::fmt::Display for DecisionV6Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for DecisionV6Error {}

#[derive(Clone, Copy, Debug, Encode, Decode, Eq, Ord, PartialEq, PartialOrd)]
pub enum ContractSideV6 {
    Yes,
    No,
}

#[derive(Clone, Copy, Debug, Encode, Decode, Eq, PartialEq)]
pub enum OrderActionV6 {
    Buy,
    Sell,
}

#[derive(Clone, Copy, Debug, Encode, Decode, Eq, PartialEq)]
pub enum OrderTypeV6 {
    Market,
    Limit,
}

#[derive(Clone, Copy, Debug, Encode, Decode, Eq, PartialEq)]
pub enum BrokerOrderStatusV6 {
    DurablyAccepted,
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

impl BrokerOrderStatusV6 {
    /// The order will not change again.
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Filled | Self::Cancelled | Self::Expired | Self::Rejected
        )
    }
}

#[derive(Clone, Copy, Debug, Encode, Decode, Eq, PartialEq)]
pub enum BrokerCommandKindV6 {
    PlaceOrder,
    CancelOrder,
    CancelAllOrders,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum StrategyParameterValueV6 {
    Null,
    Bool(bool),
    I64(i64),
    U64(u64),
    Decimal { coefficient: i64, scale: u8 },
    String(String),
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct StrategyScopeV6 {
    pub strategy_id: String,
    pub binding_id: String,
    pub profile: String,
    /// Sorted top-level values projected without loss into the JSON initializer.
    pub parameters: Vec<(String, StrategyParameterValueV6)>,
    /// The Sleeve's primary station. Every contributor station of the event is in
    /// `owner_state.opportunity.contributor_stations`.
    pub station_id: String,
    pub event_ticker: String,
    pub event_date: String,
    pub market_ids: Vec<String>,
    /// Digest attested by both the configured V4 state and the immutable Strategy release.
    pub profile_and_calculator_digest: String,
}

/// The deployment the decision runs in.
#[derive(Clone, Copy, Debug, Encode, Decode, Eq, PartialEq)]
pub enum DeploymentModeV6 {
    Paper,
    Live,
}

impl DeploymentModeV6 {
    /// The mode name traderv3 derives provider client ids with.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Live => "live",
        }
    }
}

/// What the host grants the Strategy for this decision.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct CapabilityGrantV6 {
    /// One-shot timers (`ScheduleTimer` / `CancelTimer`).
    pub timers: bool,
    /// Allowed external request names, strictly sorted. Empty until Phase 4 grants any.
    pub external_requests: Vec<String>,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct BrokerPositionV6 {
    pub market_id: String,
    pub side: ContractSideV6,
    /// Exact position quantity in hundredths of one contract.
    pub quantity_hundredths: u64,
    /// Entry cost excluding fees. The runner projects average price as cost / quantity.
    pub cost_basis_micros: u64,
    pub fees_micros: u64,
}

impl BrokerPositionV6 {
    pub fn average_entry_price(&self) -> f64 {
        self.cost_basis_micros as f64 * 100.0
            / self.quantity_hundredths as f64
            / MAX_PRICE_MICROS as f64
    }
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct BrokerOrderV6 {
    /// The command that placed the order ([`command_id_v6`] for V6 orders).
    pub command_id: String,
    pub intent_id: String,
    pub order_id: String,
    pub provider_order_id: Option<String>,
    pub provider_client_id: String,
    pub market_id: String,
    pub action: OrderActionV6,
    pub side: ContractSideV6,
    pub order_type: OrderTypeV6,
    /// Exact original order quantity in hundredths of one contract.
    pub quantity_hundredths: u64,
    /// Exact filled order quantity in hundredths of one contract.
    pub filled_quantity_hundredths: u64,
    /// Exact remaining order quantity in hundredths of one contract.
    pub remaining_quantity_hundredths: u64,
    pub limit_price_micros: Option<u64>,
    pub average_fill_price_micros: Option<u64>,
    pub reserved_principal_micros: u64,
    /// Unspent admitted budget above remaining principal, including retained price
    /// improvement available for fees. This is reserved cash, not charged fees.
    pub reserved_fee_micros: u64,
    /// Execution fees charged for this order's fills so far.
    pub fees_micros: u64,
    /// The provider's rejection text of a `Rejected` order, when it gave one (at most
    /// `MAX_REASON_BYTES`; empty is the same as none, and it is ignored on other statuses).
    /// The host writes it with the order's status, atomically. The runner hands kernels all
    /// of it (they classify transient rejections by it); evidence records at most
    /// `MAX_REJECTION_REASON_BYTES`.
    pub rejection_reason: Option<String>,
    pub created_at_unix_ms: Option<i64>,
    pub updated_at_unix_ms: Option<i64>,
    pub signal_type: Option<String>,
    pub signal_metadata: Option<String>,
    pub status: BrokerOrderStatusV6,
    pub revision: u64,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct BrokerDetailV6 {
    pub revision: u64,
    /// Exact Sleeve-local sum of active order principal and fee reservations.
    pub reserved_cash_micros: u64,
    /// Sorted by `(market_id, side)`.
    pub positions: Vec<BrokerPositionV6>,
    /// Sorted by `order_id`. Keeps every terminal order whose final status the runner has not
    /// acknowledged ahead of older acknowledged ones.
    pub orders: Vec<BrokerOrderV6>,
}

/// The outcome of a command that has no order record.
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum CommandOutcomeV6 {
    /// A cancel or cancel-all was admitted; its effect shows on the orders it targets.
    Accepted,
    /// Admission refused the command (see `strategy_ledger` refusal codes).
    Refused { code: String, reason: String },
}

/// The outcome of one recent command without an order record: a refused place, a cancel or a
/// cancel-all. It stays until a durable checkpoint acknowledges it.
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct CommandReceiptV6 {
    pub command_id: String,
    pub kind: BrokerCommandKindV6,
    pub outcome: CommandOutcomeV6,
}

/// An order's status as a Strategy sees it in an order update.
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum OrderUpdateStatusV6 {
    Accepted,
    Resting,
    PartiallyFilled,
    Filled,
    Cancelled,
    Expired,
    Refused { code: String, reason: String },
}

impl OrderUpdateStatusV6 {
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Filled | Self::Cancelled | Self::Expired | Self::Refused { .. }
        )
    }
}

/// What the Strategy has seen of one of its orders or commands.
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct RunnerEntryV6 {
    pub command_id: String,
    pub kind: BrokerCommandKindV6,
    /// A place's client order id; a cancel's target client order id.
    pub client_order_id: Option<String>,
    /// The order id once the Broker reports it; a cancel's target order id.
    pub order_id: Option<String>,
    /// The order's Market (a cancel's target Market).
    pub market_id: Option<String>,
    pub action: Option<OrderActionV6>,
    pub side: Option<ContractSideV6>,
    pub requested_quantity_hundredths: u64,
    /// The last status reported to the Strategy; `None` until the first. Never terminal: an
    /// entry is pruned once its terminal status is seen.
    pub last_status: Option<OrderUpdateStatusV6>,
    pub filled_quantity_hundredths: u64,
    /// The order revision last reported; an older record of the order is stale.
    pub order_revision: u64,
    /// The Broker revision of the context the command was issued (or adopted) in. A context
    /// at or below it may predate the command's admission, so its absence there is no news.
    pub issued_broker_revision: u64,
    /// A tombstone: a place whose order, or a cancel whose receipt, went missing from a
    /// complete, newer view (a vanished place was reported final once). If the order or the
    /// receipt reappears, its real update follows, a place's `newly_filled` counted from this
    /// entry. At most `MAX_TOMBSTONES` are kept; one expires after
    /// `TOMBSTONE_EXPIRY_VIEWS` complete views above `vanished_revision` that do not show it.
    pub vanished: bool,
    /// The Broker revision of the view the entry went missing from.
    pub vanished_revision: u64,
    /// Complete views above `vanished_revision` that did not show it since.
    pub absent_views: u16,
    /// Decisions whose kernel returned an error for this entry's pending update; the update
    /// is delivered again until `MAX_DELIVERY_ATTEMPTS`.
    pub delivery_failures: u8,
    /// Consecutive decisions that deferred this entry's pending update because earlier work
    /// in the decision took the room it needed; at `MAX_DELIVERY_DEFERRALS` it is abandoned.
    pub delivery_deferrals: u8,
}

impl RunnerEntryV6 {
    /// Counts toward `MAX_RUNNER_ENTRIES` when the decision issues a command (a tombstone
    /// counts toward `MAX_TOMBSTONES`).
    pub const fn is_live(&self) -> bool {
        !self.vanished
    }
}

/// The runner's record of the Strategy's orders and commands, in issue order.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct RunnerSectionV6 {
    /// False before the first decision (and after converting a V5 checkpoint): that
    /// decision records the Sleeve's open orders as seen, without updates.
    pub seeded: bool,
    /// The highest Broker revision of a view the section was compared with. An open order
    /// the section does not track is adopted only from a newer view: a view at or below it
    /// may be stale and show an order older than the Strategy was told of.
    pub newest_view_revision: u64,
    pub entries: Vec<RunnerEntryV6>,
}

/// Bounded, versioned private kernel state owned by one exact Strategy profile, plus the runner
/// section.
///
/// The host treats `state` as opaque bytes. The Strategy artifact owns the codec and must reject
/// unsupported versions. `state_sha256` binds the bytes, the runner section and all codec/scope
/// fields.
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct KernelCheckpointV6 {
    pub codec_profile: String,
    pub codec_version: u32,
    pub strategy_id: String,
    pub strategy_profile: String,
    pub profile_and_calculator_digest: String,
    pub sequence: u64,
    pub state: Vec<u8>,
    pub runner: RunnerSectionV6,
    pub state_sha256: [u8; 32],
}

impl KernelCheckpointV6 {
    /// Checks shape, byte bounds and integrity; callers still enforce scope/sequence authority.
    pub fn validate(&self) -> Result<(), DecisionV6Error> {
        validate_kernel_checkpoint_shape(self)
    }

    /// Recomputes `state_sha256` after the fields are set.
    pub fn seal(mut self) -> Self {
        self.state_sha256 = kernel_checkpoint_v6_sha256(&self);
        self
    }
}

/// A Decision V5 kernel checkpoint, in the V5 field order, as a host stored it before the V6
/// cutover. Decode stored bytes into it with the codec that wrote them, then convert it with
/// [`convert_v5_kernel_checkpoint`].
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct KernelCheckpointV5Layout {
    pub codec_profile: String,
    pub codec_version: u32,
    pub strategy_id: String,
    pub strategy_profile: String,
    pub profile_and_calculator_digest: String,
    pub sequence: u64,
    pub state: Vec<u8>,
    pub state_sha256: [u8; 32],
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum OwnerTriggerV6 {
    Observation {
        station_id: String,
        observed_at_unix_ms: i64,
        component_revision: u64,
        source_generation: u64,
        source_sequence: u64,
    },
    ForecastUpdated {
        station_id: String,
        emitted_at_unix_ms: i64,
        component_revision: u64,
        source_generation: u64,
        source_sequence: u64,
    },
    OracleScoresUpdated {
        station_id: String,
        emitted_at_unix_ms: i64,
        component_revision: u64,
        source_generation: u64,
        source_sequence: u64,
    },
    NewHigh {
        station_id: String,
        event_date: Option<String>,
        temperature_milli_c: Option<i32>,
        observed_at_unix_ms: i64,
        component_revision: u64,
        source_generation: u64,
        source_sequence: u64,
    },
    NewLow {
        station_id: String,
        event_date: Option<String>,
        temperature_milli_c: Option<i32>,
        observed_at_unix_ms: i64,
        component_revision: u64,
        source_generation: u64,
        source_sequence: u64,
    },
    StationReport {
        station_id: String,
        report_id: String,
        report_type: String,
        report_revision: u64,
        provider: String,
        source_generation: u64,
        source_sequence: u64,
    },
    WeatherEvent {
        station_id: String,
        episode_id: String,
        state: String,
        component_revision: u64,
        source_generation: u64,
        source_sequence: u64,
    },
    /// Current packet event; complete accepted data is separate from current station state.
    CapturedWeather {
        station_id: String,
        source_generation: u64,
        source_sequence: u64,
    },
    MarketPrice {
        market_id: String,
        price_revision: u64,
        emitted_at_unix_ms: i64,
    },
    Timer {
        key: String,
        scheduled_at_epoch_ns: u64,
        generation: String,
    },
    Bootstrap,
    Recovery,
}

impl OwnerTriggerV6 {
    /// True for the weather event families whose exact supplied event accompanies a delivery.
    pub fn carries_supplied_event(&self) -> bool {
        matches!(
            self,
            Self::Observation { .. }
                | Self::StationReport { .. }
                | Self::NewHigh { .. }
                | Self::NewLow { .. }
                | Self::WeatherEvent { .. }
        )
    }

    /// The station a weather, forecast or oracle trigger is about.
    pub fn station_id(&self) -> Option<&str> {
        match self {
            Self::Observation { station_id, .. }
            | Self::ForecastUpdated { station_id, .. }
            | Self::OracleScoresUpdated { station_id, .. }
            | Self::NewHigh { station_id, .. }
            | Self::NewLow { station_id, .. }
            | Self::StationReport { station_id, .. }
            | Self::WeatherEvent { station_id, .. }
            | Self::CapturedWeather { station_id, .. } => Some(station_id),
            Self::MarketPrice { .. } | Self::Timer { .. } | Self::Bootstrap | Self::Recovery => {
                None
            }
        }
    }
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum TriggerV6 {
    Owner(OwnerTriggerV6),
    /// One of the Sleeve's orders or command receipts changed. Valid on its own.
    BrokerState {
        broker_revision: u64,
    },
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct PlaceOrderV6 {
    pub command_id: String,
    pub market_id: String,
    pub action: OrderActionV6,
    pub side: ContractSideV6,
    pub order_type: OrderTypeV6,
    pub quantity_hundredths: u64,
    pub limit_price_micros: Option<u64>,
    /// Authoritative maximum per-contract execution price for a Market buy.
    pub market_price_cap_micros: Option<u64>,
    pub expires_after_ms: Option<i64>,
    pub reduce_only: bool,
    /// The kernel's `client_order_id`, else [`derive_provider_client_id_v6`].
    pub provider_client_id: String,
    pub signal_type: Option<String>,
    pub signal_metadata: Option<String>,
    pub metadata: Vec<u8>,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum CancelTargetV6 {
    /// An order in the context's Broker state, at the revision the decision saw.
    Order {
        order_id: String,
        expected_order_revision: u64,
    },
    /// An order placed earlier in the same decision, which has no order id yet.
    SameDecision { provider_client_id: String },
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum StrategyCommandV6 {
    PlaceOrder(PlaceOrderV6),
    CancelOrder {
        command_id: String,
        target: CancelTargetV6,
    },
    CancelAllOrders {
        command_id: String,
    },
    ScheduleTimer {
        command_id: String,
        key: String,
        scheduled_at_epoch_ns: u64,
        generation: String,
        semantics: Vec<u8>,
    },
    CancelTimer {
        command_id: String,
        key: String,
        generation: String,
    },
    Stop {
        command_id: String,
        reason: String,
    },
}

impl StrategyCommandV6 {
    pub fn command_id(&self) -> &str {
        match self {
            Self::PlaceOrder(order) => &order.command_id,
            Self::CancelOrder { command_id, .. }
            | Self::CancelAllOrders { command_id }
            | Self::ScheduleTimer { command_id, .. }
            | Self::CancelTimer { command_id, .. }
            | Self::Stop { command_id, .. } => command_id,
        }
    }

    /// The Broker command kind, or `None` for timer and stop commands.
    pub fn broker_kind(&self) -> Option<BrokerCommandKindV6> {
        match self {
            Self::PlaceOrder(_) => Some(BrokerCommandKindV6::PlaceOrder),
            Self::CancelOrder { .. } => Some(BrokerCommandKindV6::CancelOrder),
            Self::CancelAllOrders { .. } => Some(BrokerCommandKindV6::CancelAllOrders),
            Self::ScheduleTimer { .. } | Self::CancelTimer { .. } | Self::Stop { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum DecisionDispositionV6 {
    /// The kernel handled the event; the checkpoint advances by exactly one.
    Completed,
    /// The kernel returned an error; the checkpoint is unchanged and there are no commands.
    Rejected,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct ResultEvidenceV6 {
    pub code: String,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct ResultDiagnosticV6 {
    pub severity: String,
    pub code: String,
    pub message: String,
}

/// The value of one annotation. Floats keep their exact bits.
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum AnnotationValueV6 {
    Text(String),
    Integer(i64),
    FloatBits(u64),
    Bool(bool),
    Null,
}

/// One kernel telemetry entry, in the order the kernel recorded it. Floats keep their exact
/// bits.
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum TelemetryEntryV6 {
    Counter {
        name: String,
        value_bits: u64,
        fields: Vec<(String, String)>,
    },
    Gauge {
        name: String,
        value_bits: u64,
        fields: Vec<(String, String)>,
    },
    Annotation {
        name: String,
        value: AnnotationValueV6,
        fields: Vec<(String, String)>,
    },
}

impl TelemetryEntryV6 {
    fn parts(&self) -> (&str, &[(String, String)]) {
        match self {
            Self::Counter { name, fields, .. }
            | Self::Gauge { name, fields, .. }
            | Self::Annotation { name, fields, .. } => (name, fields),
        }
    }

    /// Whether the entry fits the result bounds.
    pub fn is_valid(&self) -> bool {
        let (name, fields) = self.parts();
        valid_text(name, MAX_SHORT_TEXT_BYTES)
            && fields.len() <= MAX_TELEMETRY_FIELDS
            && fields.iter().all(|(key, value)| {
                valid_text(key, MAX_SHORT_TEXT_BYTES) && value.len() <= MAX_SHORT_TEXT_BYTES
            })
            && match self {
                Self::Annotation {
                    value: AnnotationValueV6::Text(text),
                    ..
                } => text.len() <= MAX_REASON_BYTES,
                _ => true,
            }
    }
}

/// One order update the runner derived and delivered, as recorded in the result evidence.
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct OrderUpdateRecordV6 {
    pub command_id: String,
    pub kind: BrokerCommandKindV6,
    pub client_order_id: Option<String>,
    pub order_id: Option<String>,
    pub market_id: Option<String>,
    pub action: Option<OrderActionV6>,
    pub side: Option<ContractSideV6>,
    pub status: OrderUpdateStatusV6,
    pub requested_quantity_hundredths: u64,
    pub filled_quantity_hundredths: u64,
    pub remaining_quantity_hundredths: u64,
    pub newly_filled_quantity_hundredths: u64,
    pub average_fill_price_micros: Option<u64>,
    pub fees_micros: u64,
    pub is_final: bool,
    /// The Broker no longer reports the order: `status` is the last one seen and nothing
    /// remains. Updates follow if the order reappears.
    pub vanished: bool,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct StationWeatherV6 {
    pub station_id: String,
    pub facts: strategy_core_kernel::WeatherFacts,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct StationForecastIssuanceV6 {
    pub station_id: String,
    pub models: Vec<strategy_core_kernel::forecast::ForecastIssuance>,
}

/// Exact native cap for one scoped Market. The V4 identity carries its native Fahrenheit
/// floor; its Celsius slots remain historical/derived evidence.
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct MarketStrikesV6 {
    pub market_id: String,
    pub cap_strike_milli_f: Option<i64>,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct DecisionContextV6 {
    pub owner_state: DecisionContextV4,
    pub strategy: StrategyScopeV6,
    pub deployment_mode: DeploymentModeV6,
    pub capabilities: CapabilityGrantV6,
    pub broker: BrokerDetailV6,
    /// True when `broker.orders` holds every order of the Sleeve the host keeps; false when the
    /// host had to truncate the view. A truncated view never reports an order as vanished and
    /// allows no cancel-all.
    pub orders_complete: bool,
    /// Outcomes of recent commands without an order record, strictly sorted by command id.
    pub command_receipts: Vec<CommandReceiptV6>,
    pub trigger: TriggerV6,
    /// Latest private kernel state. Absent only before the first successful decision.
    pub kernel_checkpoint: Option<KernelCheckpointV6>,
    /// Authoritative wall clock supplied to the kernel. No process-clock fallback is allowed.
    pub decision_time_unix_ms: i64,
    /// Provider inputs at their supplied precision plus the exact typed originating event.
    pub supplied: SuppliedInputsV6,
    /// One explicit weather winner set per owner station, when the host retained per-field
    /// acceptance evidence.
    pub current_weather: Option<Vec<StationWeatherV6>>,
    /// Accepted issuance for each delivered model; provider refresh originals remain separate.
    pub forecast_issuance: Option<Vec<StationForecastIssuanceV6>>,
    pub current_inputs: Option<CurrentInputsV6>,
    /// Complete scoped Market order when present.
    pub market_strikes: Option<Vec<MarketStrikesV6>>,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct DecisionResultV6 {
    pub delivery_id: String,
    pub sleeve_identity: String,
    pub state_fence: String,
    /// The decision's fence: the Broker revision of its context.
    pub expected_broker_revision: u64,
    pub disposition: DecisionDispositionV6,
    /// Completed results carry the post-event checkpoint; Rejected results the input one.
    pub kernel_checkpoint: Option<KernelCheckpointV6>,
    /// In issue order; the command at index `n` is `command_id_v6(.., n)`, `command.` and the
    /// first 32 hex digits of its IntentId.
    pub commands: Vec<StrategyCommandV6>,
    /// Receipts and terminal orders in the context whose final outcome the Strategy has seen.
    pub acknowledged_command_ids: Vec<String>,
    pub evidence: Vec<ResultEvidenceV6>,
    pub diagnostics: Vec<ResultDiagnosticV6>,
    pub telemetry: Vec<TelemetryEntryV6>,
}

// ---------------------------------------------------------------------------------------------
// Context validation
// ---------------------------------------------------------------------------------------------

impl DecisionContextV6 {
    pub fn validate(&self) -> Result<(), DecisionV6Error> {
        let max_points = if self.current_weather.is_some() {
            crate::supplied_v6::MAX_SUPPLIED_FORECAST_POINTS
        } else {
            crate::decision_v4::MAX_POINTS_PER_MODEL
        };
        self.owner_state
            .validate_with_forecast_point_bound(max_points)
            .map_err(DecisionV6Error::V4)?;
        validate_scope(self)?;
        validate_contributor_stations(self)?;
        validate_capabilities(&self.capabilities)?;
        validate_broker(self)?;
        validate_receipts(self)?;
        if let Some(checkpoint) = &self.kernel_checkpoint {
            validate_kernel_checkpoint(&self.strategy, checkpoint)?;
        }
        crate::current_v6::validate(self)?;
        validate_trigger(self)?;
        crate::supplied_v6::validate_supplied_inputs(&self.supplied)?;
        validate_supplied(self)?;
        validate_current_weather(self)?;
        validate_forecast_issuance(self)?;
        validate_market_strikes(self)?;
        Ok(())
    }

    /// The owner trigger of this decision; `None` for a Broker-state trigger.
    pub fn owner_trigger(&self) -> Option<&OwnerTriggerV6> {
        match &self.trigger {
            TriggerV6::Owner(trigger) => Some(trigger),
            TriggerV6::BrokerState { .. } => None,
        }
    }

    /// Every station whose data settles the Sleeve's event, as the owner projection lists them.
    pub fn contributor_stations(&self) -> &[String] {
        &self.owner_state.opportunity.contributor_stations
    }

    /// The generation every timer scheduled in this decision carries.
    pub fn timer_generation(&self) -> String {
        format!("timer.{}", self.owner_state.delivery_id)
    }

    /// The id of the decision's command at `ordinal` ([`command_id_v6`]).
    pub fn command_id(&self, ordinal: usize) -> String {
        let sleeve = &self.owner_state.sleeve;
        u32::try_from(ordinal)
            .ok()
            .and_then(|ordinal| {
                command_id_v6(
                    &sleeve.sleeve_id,
                    sleeve.incarnation,
                    &self.owner_state.delivery_id,
                    ordinal,
                )
            })
            .expect("a validated context has a digest Sleeve id and a bounded ordinal")
    }
}

/// The IntentId of a decision's command at `ordinal`, as traderv3 derives it:
/// `sha256(INTENT_DOMAIN, DecisionId, ordinal)` with `DecisionId = sha256(DECISION_DOMAIN,
/// Sleeve id, incarnation, delivery id)`. The ordinal is the command's index in the result,
/// timer and stop commands included. `None` when the Sleeve id is not a 64-hex digest.
pub fn intent_id_v6(
    sleeve_id: &str,
    incarnation: u64,
    delivery_id: &str,
    ordinal: u32,
) -> Option<[u8; 32]> {
    let sleeve = parse_hex_digest(sleeve_id)?;
    let length = u16::try_from(delivery_id.len()).ok()?;
    let mut decision = Sha256::new();
    decision.update(DECISION_ID_DOMAIN);
    decision.update(sleeve);
    decision.update(incarnation.to_be_bytes());
    decision.update(length.to_be_bytes());
    decision.update(delivery_id.as_bytes());
    let mut intent = Sha256::new();
    intent.update(INTENT_ID_DOMAIN);
    intent.update(decision.finalize());
    intent.update(ordinal.to_be_bytes());
    Some(intent.finalize().into())
}

/// `command.<first 32 hex digits of the command's IntentId>`: deterministic, and unique across
/// Sleeves, incarnations and deliveries because the IntentId binds all three.
pub fn command_id_v6(
    sleeve_id: &str,
    incarnation: u64,
    delivery_id: &str,
    ordinal: u32,
) -> Option<String> {
    let intent = intent_id_v6(sleeve_id, incarnation, delivery_id, ordinal)?;
    Some(format!("command.{}", &hex_digest(&intent)[..32]))
}

fn validate_market_strikes(context: &DecisionContextV6) -> Result<(), DecisionV6Error> {
    let Some(strikes) = &context.market_strikes else {
        return Ok(());
    };
    if strikes.len() != context.owner_state.markets.len() {
        return Err(DecisionV6Error::InvalidContract);
    }
    for (strike, market) in strikes.iter().zip(&context.owner_state.markets) {
        let identity = &market.identity;
        if strike.market_id != identity.market_id
            || (strike.cap_strike_milli_f.is_some() && identity.cap_strike_milli_c.is_some())
            || identity
                .floor_strike_milli_f
                .zip(strike.cap_strike_milli_f)
                .is_some_and(|(floor, cap)| floor > cap)
        {
            return Err(DecisionV6Error::InvalidContract);
        }
    }
    Ok(())
}

fn validate_forecast_issuance(context: &DecisionContextV6) -> Result<(), DecisionV6Error> {
    let Some(stations) = &context.forecast_issuance else {
        return Ok(());
    };
    if stations.len() != context.owner_state.stations.len() {
        return Err(DecisionV6Error::InvalidContract);
    }
    for (accepted, station) in stations.iter().zip(&context.owner_state.stations) {
        if accepted.station_id != station.identity.station_id
            || accepted.models.len() != station.forecast.models.len()
        {
            return Err(DecisionV6Error::InvalidContract);
        }
        for (issued, model) in accepted.models.iter().zip(&station.forecast.models) {
            let issued_at_matches = match model.issued_at_unix_ms {
                Some(at) => at == issued.at_unix_ns.div_euclid(1_000_000),
                None => {
                    issued.basis
                        == strategy_core_kernel::forecast::ForecastIssuanceBasis::TimestampedVersion
                }
            };
            if !issued.is_valid()
                || issued.model_id != model.model_id
                || issued.version != model.version
                || !issued_at_matches
                || issued.station_generation > station.provider_cursor.connection_generation
                || (issued.station_generation == station.provider_cursor.connection_generation
                    && (issued.station_revision > station.revision
                        || issued.forecast_generation > station.forecast_meta.generation))
            {
                return Err(DecisionV6Error::InvalidContract);
            }
        }
    }
    Ok(())
}

fn validate_current_weather(context: &DecisionContextV6) -> Result<(), DecisionV6Error> {
    let Some(stations) = &context.current_weather else {
        return Ok(());
    };
    if stations.len() != context.owner_state.stations.len()
        || stations
            .iter()
            .zip(&context.owner_state.stations)
            .any(|(weather, station)| weather.station_id != station.identity.station_id)
    {
        return Err(DecisionV6Error::InvalidContract);
    }
    for (station, owner) in stations.iter().zip(&context.owner_state.stations) {
        if !station.facts.values_are_valid() {
            return Err(DecisionV6Error::InvalidContract);
        }
        if station
            .facts
            .text_values()
            .any(|value| value.len() > crate::supplied_v6::MAX_SUPPLIED_TEXT_BYTES)
        {
            return Err(DecisionV6Error::BoundExceeded);
        }
        for fact in station.facts.fields.values() {
            let provenance = &fact.provenance;
            if provenance.owner_generation == 0
                || provenance.owner_revision == 0
                || provenance.owner_generation > owner.provider_cursor.connection_generation
                || (provenance.owner_generation == owner.provider_cursor.connection_generation
                    && provenance.owner_revision > owner.revision)
            {
                return Err(DecisionV6Error::InvalidContract);
            }
            if let Some(envelope) = &provenance.envelope {
                crate::supplied_v6::validate_envelope(envelope)?;
            }
        }
    }
    Ok(())
}

fn validate_scope(context: &DecisionContextV6) -> Result<(), DecisionV6Error> {
    let scope = &context.strategy;
    if !strictly_sorted(scope.parameters.iter().map(|(key, _)| key.as_str()))
        || !strictly_sorted(scope.market_ids.iter().map(String::as_str))
    {
        return Err(DecisionV6Error::NonCanonicalOrder);
    }
    if !valid_identifier(&scope.strategy_id)
        || !valid_identifier(&scope.binding_id)
        || !valid_text(&scope.profile, MAX_SHORT_TEXT_BYTES)
        || scope.parameters.len() > MAX_STRATEGY_PARAMETERS
        || scope.parameters.iter().any(|(key, value)| {
            !valid_identifier(key)
                || matches!(value, StrategyParameterValueV6::Decimal { scale, .. } if *scale > 18)
                || matches!(value, StrategyParameterValueV6::String(value) if !valid_text(value, MAX_REASON_BYTES))
        })
        || !valid_identifier(&scope.station_id)
        || !valid_identifier(&scope.event_ticker)
        || !valid_text(&scope.event_date, MAX_SHORT_TEXT_BYTES)
        || !valid_text(&scope.profile_and_calculator_digest, MAX_SHORT_TEXT_BYTES)
        || scope.market_ids.is_empty()
    {
        return Err(DecisionV6Error::InvalidContract);
    }
    if scope.profile != context.owner_state.opportunity.match_profile
        || context.owner_state.sleeve.sleeve_id
            != derive_sleeve_identity_v6(
                &scope.strategy_id,
                &scope.binding_id,
                &context.owner_state.opportunity.venue_id,
                &context.owner_state.opportunity.opportunity_id,
            )
        || !context
            .owner_state
            .opportunity
            .contributor_stations
            .iter()
            .any(|station| station == &scope.station_id)
        || !context.owner_state.stations.iter().any(|station| {
            station.identity.station_id == scope.station_id
                && station.climate_event_date == scope.event_date
        })
        || context.owner_state.config.profile_and_calculator_digest
            != scope.profile_and_calculator_digest
        || context.owner_state.fence.profile_and_calculator_digest
            != scope.profile_and_calculator_digest
    {
        return Err(DecisionV6Error::InvalidContract);
    }
    let owner_market_ids = context
        .owner_state
        .markets
        .iter()
        .map(|market| market.identity.market_id.as_str())
        .collect::<BTreeSet<_>>();
    let scope_market_ids = scope
        .market_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if owner_market_ids != scope_market_ids
        || context.owner_state.markets.iter().any(|market| {
            market.identity.opportunity_id != context.owner_state.opportunity.opportunity_id
                || market.identity.event_ticker != scope.event_ticker
        })
    {
        return Err(DecisionV6Error::InvalidContract);
    }
    Ok(())
}

/// The event's contributor stations are exactly the owner projection's stations.
fn validate_contributor_stations(context: &DecisionContextV6) -> Result<(), DecisionV6Error> {
    let contributors = context.contributor_stations();
    if contributors.len() > MAX_STATIONS {
        return Err(DecisionV6Error::BoundExceeded);
    }
    unique(contributors.iter().map(String::as_str))?;
    let owner = context
        .owner_state
        .stations
        .iter()
        .map(|station| station.identity.station_id.as_str())
        .collect::<BTreeSet<_>>();
    if contributors
        .iter()
        .any(|station| !valid_identifier(station))
        || contributors
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            != owner
    {
        return Err(DecisionV6Error::InvalidContract);
    }
    Ok(())
}

fn validate_capabilities(capabilities: &CapabilityGrantV6) -> Result<(), DecisionV6Error> {
    if capabilities.external_requests.len() > MAX_EXTERNAL_REQUEST_GRANTS {
        return Err(DecisionV6Error::BoundExceeded);
    }
    if !strictly_sorted(capabilities.external_requests.iter()) {
        return Err(DecisionV6Error::NonCanonicalOrder);
    }
    if capabilities
        .external_requests
        .iter()
        .any(|name| !valid_identifier(name))
    {
        return Err(DecisionV6Error::InvalidContract);
    }
    Ok(())
}

fn validate_broker(context: &DecisionContextV6) -> Result<(), DecisionV6Error> {
    if context.broker.revision != context.owner_state.broker.revision
        || context.broker.revision != context.owner_state.fence.broker_revision
    {
        return Err(DecisionV6Error::InvalidContract);
    }
    let broker = &context.broker;
    if !strictly_sorted(
        broker
            .positions
            .iter()
            .map(|position| (position.market_id.as_str(), position.side)),
    ) || !strictly_sorted(broker.orders.iter().map(|order| order.order_id.as_str()))
    {
        return Err(DecisionV6Error::NonCanonicalOrder);
    }
    let owner_market_ids = context
        .strategy
        .market_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if broker.positions.len() > MAX_BROKER_POSITIONS || broker.orders.len() > MAX_BROKER_ORDERS {
        return Err(DecisionV6Error::InvalidContract);
    }
    if broker.positions.iter().any(|position| {
        position.quantity_hundredths == 0
            || position.quantity_hundredths > i64::MAX as u64
            || u128::from(position.cost_basis_micros)
                > maximum_quantity_value(position.quantity_hundredths)
            || !owner_market_ids.contains(position.market_id.as_str())
    }) || broker
        .orders
        .iter()
        .any(|order| validate_broker_order_v6(order, &context.strategy.market_ids).is_err())
    {
        return Err(DecisionV6Error::InvalidContract);
    }
    let reserved_cash = broker.orders.iter().fold(0_u128, |total, order| {
        total + u128::from(order.reserved_principal_micros) + u128::from(order.reserved_fee_micros)
    });
    let position_commitment = broker.positions.iter().fold(0_u128, |total, position| {
        total + u128::from(position.cost_basis_micros) + u128::from(position.fees_micros)
    });
    if reserved_cash != u128::from(broker.reserved_cash_micros)
        || reserved_cash > u128::from(context.owner_state.broker.locally_reserved_cash)
        || position_commitment + reserved_cash
            != u128::from(context.owner_state.broker.current_commitment)
    {
        return Err(DecisionV6Error::InvalidContract);
    }
    unique(broker.orders.iter().map(|order| order.command_id.as_str()))?;
    unique(broker.orders.iter().map(|order| order.intent_id.as_str()))?;
    unique(
        broker
            .orders
            .iter()
            .map(|order| order.provider_client_id.as_str()),
    )?;
    unique(
        broker
            .orders
            .iter()
            .filter_map(|order| order.provider_order_id.as_deref()),
    )?;
    Ok(())
}

/// Checks one Broker order record on its own: identities, quantities, prices, reservations
/// and text bounds, and that its Market is one of `market_ids`. A host can run it over its
/// order records before building a context, to quarantine a record that would make every
/// context of the Sleeve invalid.
///
/// A terminal order that stopped early (`Cancelled`, `Expired`, `Rejected`) may report no
/// remaining quantity with less than its whole quantity filled.
pub fn validate_broker_order_v6(
    order: &BrokerOrderV6,
    market_ids: &[String],
) -> Result<(), DecisionV6Error> {
    let accounted = order
        .filled_quantity_hundredths
        .checked_add(order.remaining_quantity_hundredths);
    let stopped_early = matches!(
        order.status,
        BrokerOrderStatusV6::Cancelled
            | BrokerOrderStatusV6::Expired
            | BrokerOrderStatusV6::Rejected
    );
    let quantities_valid = accounted == Some(order.quantity_hundredths)
        || (stopped_early
            && order.remaining_quantity_hundredths == 0
            && accounted.is_some_and(|quantity| quantity <= order.quantity_hundredths));
    if !valid_identifier(&order.command_id)
        || !valid_identifier(&order.intent_id)
        || !valid_identifier(&order.order_id)
        || !valid_optional_identifier(&order.provider_order_id)
        || !valid_identifier(&order.provider_client_id)
        || !market_ids.iter().any(|market| *market == order.market_id)
        || order.quantity_hundredths == 0
        || order.quantity_hundredths > i64::MAX as u64
        || !quantities_valid
        || order
            .limit_price_micros
            .is_some_and(|price| price > MAX_PRICE_MICROS)
        || order
            .average_fill_price_micros
            .is_some_and(|price| price > MAX_PRICE_MICROS)
        || matches!(order.order_type, OrderTypeV6::Limit) && order.limit_price_micros.is_none()
        || matches!(order.order_type, OrderTypeV6::Market) && order.limit_price_micros.is_some()
        || !valid_order_reservation(order)
        || !valid_optional_text(&order.signal_type, MAX_SHORT_TEXT_BYTES)
        || !valid_optional_text(&order.signal_metadata, MAX_COMMAND_METADATA_BYTES)
        || order
            .rejection_reason
            .as_ref()
            .is_some_and(|reason| reason.len() > MAX_REASON_BYTES)
    {
        return Err(DecisionV6Error::InvalidContract);
    }
    Ok(())
}

fn valid_order_reservation(order: &BrokerOrderV6) -> bool {
    if matches!(order.action, OrderActionV6::Sell) || order.status.is_terminal() {
        return order.reserved_principal_micros == 0 && order.reserved_fee_micros == 0;
    }
    let maximum_notional = maximum_quantity_value(order.remaining_quantity_hundredths);
    let principal_is_exact = match order.limit_price_micros {
        Some(price) => exact_quantity_value(order.remaining_quantity_hundredths, price)
            .is_some_and(|principal| principal == u128::from(order.reserved_principal_micros)),
        None => u128::from(order.reserved_principal_micros) <= maximum_notional,
    };
    // Price improvement may remain reserved until completion. Conservation is
    // checked against the owner cash and commitment, not the unfilled payout.
    principal_is_exact
        && (order.remaining_quantity_hundredths != 0 || order.reserved_fee_micros == 0)
}

fn maximum_quantity_value(quantity_hundredths: u64) -> u128 {
    u128::from(quantity_hundredths) * u128::from(MAX_PRICE_MICROS) / 100
}

fn exact_quantity_value(quantity_hundredths: u64, price_micros: u64) -> Option<u128> {
    let product = u128::from(quantity_hundredths) * u128::from(price_micros);
    (product % 100 == 0).then_some(product / 100)
}

fn validate_receipts(context: &DecisionContextV6) -> Result<(), DecisionV6Error> {
    let receipts = &context.command_receipts;
    if receipts.len() > MAX_COMMAND_RECEIPTS {
        return Err(DecisionV6Error::BoundExceeded);
    }
    if !strictly_sorted(receipts.iter().map(|receipt| receipt.command_id.as_str())) {
        return Err(DecisionV6Error::NonCanonicalOrder);
    }
    let order_commands = context
        .broker
        .orders
        .iter()
        .map(|order| order.command_id.as_str())
        .collect::<BTreeSet<_>>();
    for receipt in receipts {
        let refused = match &receipt.outcome {
            CommandOutcomeV6::Accepted => false,
            CommandOutcomeV6::Refused { code, reason } => {
                if !valid_identifier(code) || !valid_text(reason, MAX_REASON_BYTES) {
                    return Err(DecisionV6Error::InvalidContract);
                }
                true
            }
        };
        // An admitted place has an order record, not a receipt.
        if !valid_identifier(&receipt.command_id)
            || order_commands.contains(receipt.command_id.as_str())
            || (receipt.kind == BrokerCommandKindV6::PlaceOrder && !refused)
        {
            return Err(DecisionV6Error::InvalidContract);
        }
    }
    Ok(())
}

fn validate_trigger(context: &DecisionContextV6) -> Result<(), DecisionV6Error> {
    match &context.trigger {
        TriggerV6::Owner(trigger) => validate_owner_trigger(context, trigger),
        TriggerV6::BrokerState { broker_revision }
            if *broker_revision == context.broker.revision =>
        {
            Ok(())
        }
        TriggerV6::BrokerState { .. } => Err(DecisionV6Error::InvalidContract),
    }
}

fn validate_owner_trigger(
    context: &DecisionContextV6,
    trigger: &OwnerTriggerV6,
) -> Result<(), DecisionV6Error> {
    if matches!(trigger, OwnerTriggerV6::CapturedWeather { .. }) {
        return context
            .current_inputs
            .as_ref()
            .and_then(|inputs| inputs.originating.as_ref())
            .ok_or(DecisionV6Error::InvalidContract)?
            .validate(context);
    }
    let owner = &context.owner_state.trigger;
    let stations = &context.owner_state.stations;
    let weather = |station_id: &String,
                   source_generation: &u64,
                   source_sequence: &u64,
                   matches: &dyn Fn(&crate::decision_v4::StationV4) -> bool| {
        let TriggerV4::Weather {
            station_id: owner_station,
            source_generation: owner_generation,
            source_sequence: owner_sequence,
        } = owner
        else {
            return false;
        };
        station_id == owner_station
            && source_generation == owner_generation
            && source_sequence == owner_sequence
            && stations
                .iter()
                .any(|station| station.identity.station_id == *station_id && matches(station))
    };
    let valid = match trigger {
        OwnerTriggerV6::Observation {
            station_id,
            observed_at_unix_ms,
            component_revision,
            source_generation,
            source_sequence,
        } => weather(station_id, source_generation, source_sequence, &|station| {
            station.observation_meta.revision == *component_revision
                && station.observation.observed_at_unix_ms == *observed_at_unix_ms
        }),
        OwnerTriggerV6::ForecastUpdated {
            station_id,
            emitted_at_unix_ms,
            component_revision,
            source_generation,
            source_sequence,
        } => weather(station_id, source_generation, source_sequence, &|station| {
            station.forecast_meta.revision == *component_revision
                && station.forecast_meta.updated_at_unix_ms == Some(*emitted_at_unix_ms)
        }),
        OwnerTriggerV6::OracleScoresUpdated {
            station_id,
            emitted_at_unix_ms,
            component_revision,
            source_generation,
            source_sequence,
        } => weather(station_id, source_generation, source_sequence, &|station| {
            station.oracle_meta.revision == *component_revision
                && station.oracle_meta.updated_at_unix_ms == Some(*emitted_at_unix_ms)
        }),
        OwnerTriggerV6::NewHigh {
            station_id,
            event_date,
            temperature_milli_c,
            observed_at_unix_ms,
            component_revision,
            source_generation,
            source_sequence,
        }
        | OwnerTriggerV6::NewLow {
            station_id,
            event_date,
            temperature_milli_c,
            observed_at_unix_ms,
            component_revision,
            source_generation,
            source_sequence,
        } => {
            let high = matches!(trigger, OwnerTriggerV6::NewHigh { .. });
            weather(station_id, source_generation, source_sequence, &|station| {
                let extreme = if high {
                    station.extrema.high.as_ref()
                } else {
                    station.extrema.low.as_ref()
                };
                station.extrema_meta.revision == *component_revision
                    && event_date
                        .as_ref()
                        .is_none_or(|date| date == &station.climate_event_date)
                    && extreme.map(|value| value.value_milli_c) == *temperature_milli_c
                    && extreme.and_then(|value| value.observed_at_unix_ms)
                        == Some(*observed_at_unix_ms)
            })
        }
        OwnerTriggerV6::WeatherEvent {
            station_id,
            episode_id,
            state,
            component_revision,
            source_generation,
            source_sequence,
        } => weather(station_id, source_generation, source_sequence, &|station| {
            // The captured transition is bound by validate_supplied, independently of
            // current membership: an ended episode may have left state before FIFO drain.
            station.weather_events_meta.revision == *component_revision
                && !episode_id.is_empty()
                && !state.is_empty()
        }),
        OwnerTriggerV6::StationReport {
            station_id,
            report_id,
            report_type,
            report_revision,
            provider,
            source_generation,
            source_sequence,
        } => {
            matches!(
                owner,
                TriggerV4::StationReport {
                    station_id: owner_station,
                    report_id: owner_report,
                    report_type: owner_type,
                    report_revision: owner_revision,
                    provider: owner_provider,
                    source_generation: owner_generation,
                    source_sequence: owner_sequence,
                } if station_id == owner_station
                    && report_id == owner_report
                    && report_type == owner_type
                    && report_revision == owner_revision
                    && provider == owner_provider
                    && source_generation == owner_generation
                    && source_sequence == owner_sequence
            ) && stations.iter().any(|station| {
                station.identity.station_id == *station_id
                    && station.reports.iter().any(|report| {
                        report.report_id == *report_id && report.revision == *report_revision
                    })
            })
        }
        OwnerTriggerV6::MarketPrice {
            market_id,
            price_revision,
            emitted_at_unix_ms,
        } => {
            matches!(
                owner,
                TriggerV4::MarketPrice {
                    market_id: owner_market,
                    price_revision: owner_revision,
                } if market_id == owner_market && price_revision == owner_revision
            ) && context.owner_state.markets.iter().any(|market| {
                market.identity.market_id == *market_id
                    && market.revision == *price_revision
                    && [
                        market.ticker_meta.updated_at_unix_ms,
                        market.book_meta.updated_at_unix_ms,
                        market.last_trade_meta.updated_at_unix_ms,
                    ]
                    .into_iter()
                    .flatten()
                    .any(|updated_at| updated_at == *emitted_at_unix_ms)
            })
        }
        OwnerTriggerV6::Timer {
            key,
            scheduled_at_epoch_ns,
            generation,
        } => {
            matches!(owner, TriggerV4::Timer { key: owner_key } if key == owner_key)
                && context
                    .owner_state
                    .timer_recovery
                    .as_ref()
                    .is_some_and(|timers| {
                        timers.iter().any(|timer| {
                            timer.key == *key
                                && timer.scheduled_at == *scheduled_at_epoch_ns
                                && timer.generation == *generation
                        })
                    })
        }
        OwnerTriggerV6::Bootstrap => matches!(owner, TriggerV4::Bootstrap),
        OwnerTriggerV6::Recovery => matches!(owner, TriggerV4::Recovery),
        OwnerTriggerV6::CapturedWeather { .. } => false,
    };
    if valid {
        Ok(())
    } else {
        Err(DecisionV6Error::InvalidContract)
    }
}

/// Binds a present supplied block to the owner projection and the owner trigger.
fn validate_supplied(context: &DecisionContextV6) -> Result<(), DecisionV6Error> {
    let supplied = &context.supplied;
    if supplied.is_absent() {
        return Ok(());
    }
    let owner_stations = context
        .owner_state
        .stations
        .iter()
        .map(|station| station.identity.station_id.as_str())
        .collect::<BTreeSet<_>>();
    let supplied_stations = supplied
        .stations
        .iter()
        .map(|station| station.station_id.as_str())
        .collect::<BTreeSet<_>>();
    if owner_stations != supplied_stations {
        return Err(DecisionV6Error::InvalidContract);
    }
    let trigger = context.owner_trigger();
    let expects_event = trigger.is_some_and(OwnerTriggerV6::carries_supplied_event);
    let event = supplied.originating_event.as_ref();
    if expects_event != event.is_some() {
        return Err(DecisionV6Error::InvalidContract);
    }
    let (Some(trigger), Some(event)) = (trigger, event) else {
        return Ok(());
    };
    let bound = match (trigger, event) {
        (OwnerTriggerV6::Observation { station_id, .. }, SuppliedEventV6::Observation(event)) => {
            event.station_id == *station_id
        }
        (
            OwnerTriggerV6::StationReport {
                station_id,
                report_id,
                report_type,
                report_revision,
                ..
            },
            SuppliedEventV6::Report(event),
        ) => {
            event.station_id == *station_id
                && event.report_id == *report_id
                && event.report_type == *report_type
                && event.report_revision.unwrap_or(0) == *report_revision
        }
        (OwnerTriggerV6::NewHigh { station_id, .. }, SuppliedEventV6::Extreme(event)) => {
            event.station_id == *station_id && event.kind == ExtremeKindV6::High
        }
        (OwnerTriggerV6::NewLow { station_id, .. }, SuppliedEventV6::Extreme(event)) => {
            event.station_id == *station_id && event.kind == ExtremeKindV6::Low
        }
        (
            OwnerTriggerV6::WeatherEvent {
                station_id,
                episode_id,
                state,
                ..
            },
            SuppliedEventV6::WeatherEvent(event),
        ) => {
            event.station_id == *station_id
                && event.episode_id == *episode_id
                && event.state == *state
        }
        _ => false,
    };
    if bound {
        Ok(())
    } else {
        Err(DecisionV6Error::InvalidContract)
    }
}

fn validate_kernel_checkpoint_shape(
    checkpoint: &KernelCheckpointV6,
) -> Result<(), DecisionV6Error> {
    if checkpoint.state.len() > MAX_KERNEL_CHECKPOINT_BYTES {
        return Err(DecisionV6Error::BoundExceeded);
    }
    validate_runner_section(&checkpoint.runner)?;
    if !valid_identifier(&checkpoint.codec_profile)
        || checkpoint.codec_version == 0
        || !valid_identifier(&checkpoint.strategy_id)
        || !valid_identifier(&checkpoint.strategy_profile)
        || !valid_text(
            &checkpoint.profile_and_calculator_digest,
            MAX_SHORT_TEXT_BYTES,
        )
        || checkpoint.sequence == 0
        || checkpoint.state.is_empty()
        || checkpoint.state_sha256 == [0; 32]
        || checkpoint.state_sha256 != kernel_checkpoint_v6_sha256(checkpoint)
    {
        return Err(DecisionV6Error::InvalidContract);
    }
    Ok(())
}

fn validate_runner_section(runner: &RunnerSectionV6) -> Result<(), DecisionV6Error> {
    let tombstones = runner.entries.iter().filter(|entry| entry.vanished).count();
    if runner.entries.len() > MAX_RUNNER_SECTION_ENTRIES || tombstones > MAX_TOMBSTONES {
        return Err(DecisionV6Error::BoundExceeded);
    }
    if runner.entries.iter().any(|entry| {
        entry.delivery_failures >= MAX_DELIVERY_ATTEMPTS
            || entry.delivery_deferrals >= MAX_DELIVERY_DEFERRALS
            || (!entry.vanished && (entry.vanished_revision != 0 || entry.absent_views != 0))
            || entry.absent_views >= TOMBSTONE_EXPIRY_VIEWS
    }) {
        return Err(DecisionV6Error::InvalidContract);
    }
    unique(runner.entries.iter().map(|entry| entry.command_id.as_str()))?;
    unique(runner.entries.iter().filter_map(|entry| {
        (entry.kind == BrokerCommandKindV6::PlaceOrder)
            .then_some(entry.client_order_id.as_deref())
            .flatten()
    }))?;
    for entry in &runner.entries {
        let order_fields_valid = valid_optional_identifier(&entry.client_order_id)
            && valid_optional_identifier(&entry.order_id)
            && valid_optional_identifier(&entry.market_id)
            && entry.filled_quantity_hundredths <= entry.requested_quantity_hundredths
            && entry.requested_quantity_hundredths <= i64::MAX as u64;
        let shape = match entry.kind {
            BrokerCommandKindV6::PlaceOrder => {
                entry.client_order_id.is_some()
                    && entry.market_id.is_some()
                    && entry.action.is_some()
                    && entry.side.is_some()
                    && entry.requested_quantity_hundredths > 0
                    && entry
                        .last_status
                        .as_ref()
                        .is_none_or(|status| !status.is_terminal())
            }
            BrokerCommandKindV6::CancelOrder => entry.last_status.is_none(),
            BrokerCommandKindV6::CancelAllOrders => {
                entry.last_status.is_none()
                    && entry.client_order_id.is_none()
                    && entry.order_id.is_none()
                    && entry.market_id.is_none()
                    && entry.action.is_none()
                    && entry.side.is_none()
                    && entry.requested_quantity_hundredths == 0
                    && entry.filled_quantity_hundredths == 0
            }
        };
        if !valid_identifier(&entry.command_id) || !order_fields_valid || !shape {
            return Err(DecisionV6Error::InvalidContract);
        }
    }
    Ok(())
}

fn validate_kernel_checkpoint(
    strategy: &StrategyScopeV6,
    checkpoint: &KernelCheckpointV6,
) -> Result<(), DecisionV6Error> {
    validate_kernel_checkpoint_shape(checkpoint)?;
    if checkpoint.strategy_id != strategy.strategy_id
        || checkpoint.strategy_profile != strategy.profile
        || checkpoint.profile_and_calculator_digest != strategy.profile_and_calculator_digest
    {
        return Err(DecisionV6Error::InvalidContract);
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Result validation
// ---------------------------------------------------------------------------------------------

impl DecisionResultV6 {
    /// Shape and bounds that need no context.
    pub fn validate(&self) -> Result<(), DecisionV6Error> {
        if !valid_identifier(&self.delivery_id)
            || !valid_identifier(&self.sleeve_identity)
            || !valid_text(&self.state_fence, MAX_SHORT_TEXT_BYTES)
            || self.commands.len() > MAX_STRATEGY_COMMANDS
            || self.acknowledged_command_ids.len() > MAX_ACKNOWLEDGED_COMMANDS
            || self.evidence.len() > MAX_RESULT_EVIDENCE
            || self.diagnostics.len() > MAX_RESULT_DIAGNOSTICS
            || self.telemetry.len() > MAX_RESULT_TELEMETRY
        {
            return Err(DecisionV6Error::BoundExceeded);
        }
        if let Some(checkpoint) = &self.kernel_checkpoint {
            validate_kernel_checkpoint_shape(checkpoint)?;
        }
        if self.evidence.iter().any(|evidence| {
            !valid_identifier(&evidence.code) || evidence.payload.len() > MAX_EVIDENCE_PAYLOAD_BYTES
        }) || self.diagnostics.iter().any(|diagnostic| {
            !valid_identifier(&diagnostic.severity)
                || !valid_identifier(&diagnostic.code)
                || !valid_text(&diagnostic.message, MAX_RESULT_DIAGNOSTIC_BYTES)
        }) || !self.telemetry.iter().all(TelemetryEntryV6::is_valid)
        {
            return Err(DecisionV6Error::BoundExceeded);
        }
        for evidence in &self.evidence {
            if evidence.code == ORDER_UPDATES_EVIDENCE_CODE {
                decode_order_update_evidence(evidence)?;
            }
        }
        if self
            .acknowledged_command_ids
            .iter()
            .any(|id| !valid_identifier(id))
        {
            return Err(DecisionV6Error::InvalidContract);
        }
        unique(self.acknowledged_command_ids.iter().map(String::as_str))?;
        unique(self.commands.iter().map(StrategyCommandV6::command_id))?;
        let generation = format!("timer.{}", self.delivery_id);
        let mut client_ids = BTreeSet::new();
        let mut timer_keys = BTreeSet::new();
        for command in &self.commands {
            validate_command(command)?;
            match command {
                StrategyCommandV6::PlaceOrder(order) => {
                    if !client_ids.insert(order.provider_client_id.as_str()) {
                        return Err(DecisionV6Error::DuplicateIdentity);
                    }
                }
                StrategyCommandV6::CancelOrder {
                    target: CancelTargetV6::SameDecision { provider_client_id },
                    ..
                } if !client_ids.contains(provider_client_id.as_str()) => {
                    return Err(DecisionV6Error::InvalidContract);
                }
                StrategyCommandV6::ScheduleTimer {
                    key,
                    generation: scheduled,
                    ..
                } => {
                    if *scheduled != generation {
                        return Err(DecisionV6Error::InvalidContract);
                    }
                    if !timer_keys.insert(key.as_str()) {
                        return Err(DecisionV6Error::DuplicateIdentity);
                    }
                }
                StrategyCommandV6::CancelTimer { key, .. } => {
                    if !timer_keys.insert(key.as_str()) {
                        return Err(DecisionV6Error::DuplicateIdentity);
                    }
                }
                _ => {}
            }
        }
        match &self.disposition {
            DecisionDispositionV6::Completed if self.kernel_checkpoint.is_none() => {
                Err(DecisionV6Error::InvalidContract)
            }
            DecisionDispositionV6::Completed => {
                let runner = &self.kernel_checkpoint.as_ref().expect("checked").runner;
                let tracked = runner
                    .entries
                    .iter()
                    .map(|entry| entry.command_id.as_str())
                    .collect::<BTreeSet<_>>();
                // Every Broker command is tracked until its outcome is seen, and nothing
                // tracked is acknowledged.
                if self.commands.iter().any(|command| {
                    command.broker_kind().is_some() && !tracked.contains(command.command_id())
                }) || self
                    .acknowledged_command_ids
                    .iter()
                    .any(|id| tracked.contains(id.as_str()))
                {
                    return Err(DecisionV6Error::InvalidContract);
                }
                Ok(())
            }
            DecisionDispositionV6::Rejected
                if self.commands.is_empty() && self.acknowledged_command_ids.is_empty() =>
            {
                Ok(())
            }
            DecisionDispositionV6::Rejected => Err(DecisionV6Error::InvalidContract),
        }
    }
}

/// Validates a result against the context it answers.
pub fn validate_decision_result_v6(
    context: &DecisionContextV6,
    result: &DecisionResultV6,
) -> Result<(), DecisionV6Error> {
    context.validate()?;
    result.validate()?;
    if result.delivery_id != context.owner_state.delivery_id
        || result.sleeve_identity != context.owner_state.sleeve.sleeve_id
        || result.expected_broker_revision != context.broker.revision
        || result.state_fence != hex_digest(&decision_fence_v6_sha256(context)?)
    {
        return Err(DecisionV6Error::InvalidContract);
    }
    validate_checkpoint_transition(context, result)?;
    for (ordinal, command) in result.commands.iter().enumerate() {
        if command.command_id() != context.command_id(ordinal) {
            return Err(DecisionV6Error::InvalidContract);
        }
    }
    for command in &result.commands {
        match command {
            StrategyCommandV6::PlaceOrder(order)
                if !context
                    .strategy
                    .market_ids
                    .iter()
                    .any(|market_id| market_id == &order.market_id) =>
            {
                return Err(DecisionV6Error::InvalidContract);
            }
            StrategyCommandV6::CancelOrder {
                target:
                    CancelTargetV6::Order {
                        order_id,
                        expected_order_revision,
                    },
                ..
            } if !context.broker.orders.iter().any(|order| {
                order.order_id == *order_id && order.revision == *expected_order_revision
            }) =>
            {
                return Err(DecisionV6Error::InvalidContract);
            }
            StrategyCommandV6::ScheduleTimer { .. } | StrategyCommandV6::CancelTimer { .. }
                if !context.capabilities.timers =>
            {
                return Err(DecisionV6Error::InvalidContract);
            }
            StrategyCommandV6::CancelAllOrders { .. } if !context.orders_complete => {
                return Err(DecisionV6Error::InvalidContract);
            }
            _ => {}
        }
    }
    let acknowledgeable = context
        .command_receipts
        .iter()
        .map(|receipt| receipt.command_id.as_str())
        .chain(
            context
                .broker
                .orders
                .iter()
                .filter(|order| order.status.is_terminal())
                .map(|order| order.command_id.as_str()),
        )
        .collect::<BTreeSet<_>>();
    if result
        .acknowledged_command_ids
        .iter()
        .any(|id| !acknowledgeable.contains(id.as_str()))
    {
        return Err(DecisionV6Error::InvalidContract);
    }
    if decision_plan_rows_v6(context, result) > MAX_DECISION_PLAN_ROWS {
        return Err(DecisionV6Error::BoundExceeded);
    }
    let places = result
        .commands
        .iter()
        .filter(|command| matches!(command, StrategyCommandV6::PlaceOrder(_)))
        .count();
    if places > 0
        && open_orders(&context.broker) + places > max_open_orders(context.deployment_mode)
    {
        return Err(DecisionV6Error::BoundExceeded);
    }
    Ok(())
}

fn validate_checkpoint_transition(
    context: &DecisionContextV6,
    result: &DecisionResultV6,
) -> Result<(), DecisionV6Error> {
    let output = result.kernel_checkpoint.as_ref();
    if let Some(checkpoint) = output {
        validate_kernel_checkpoint(&context.strategy, checkpoint)?;
    }
    match &result.disposition {
        DecisionDispositionV6::Completed => {
            let checkpoint = output.ok_or(DecisionV6Error::InvalidContract)?;
            let expected_sequence = context
                .kernel_checkpoint
                .as_ref()
                .map_or(Some(1), |previous| previous.sequence.checked_add(1))
                .ok_or(DecisionV6Error::InvalidContract)?;
            if checkpoint.sequence != expected_sequence {
                return Err(DecisionV6Error::InvalidContract);
            }
        }
        DecisionDispositionV6::Rejected => {
            if output != context.kernel_checkpoint.as_ref() {
                return Err(DecisionV6Error::InvalidContract);
            }
        }
    }
    Ok(())
}

/// Bounds of one command on its own, as result validation checks them.
pub fn validate_command_v6(command: &StrategyCommandV6) -> Result<(), DecisionV6Error> {
    validate_command(command)
}

fn validate_command(command: &StrategyCommandV6) -> Result<(), DecisionV6Error> {
    if !valid_identifier(command.command_id()) {
        return Err(DecisionV6Error::InvalidContract);
    }
    match command {
        StrategyCommandV6::PlaceOrder(order) => {
            if !valid_identifier(&order.market_id)
                || order.quantity_hundredths == 0
                || order.quantity_hundredths > i64::MAX as u64
                || !valid_provider_client_id(&order.provider_client_id)
                || order.metadata.len() > MAX_COMMAND_METADATA_BYTES
                || !valid_optional_text(&order.signal_type, MAX_SHORT_TEXT_BYTES)
                || !valid_optional_text(&order.signal_metadata, MAX_COMMAND_METADATA_BYTES)
                || order.expires_after_ms.is_some_and(|ttl| ttl <= 0)
                || !valid_place_order_prices(order)
            {
                return Err(DecisionV6Error::InvalidContract);
            }
        }
        StrategyCommandV6::CancelOrder { target, .. } => {
            let valid = match target {
                CancelTargetV6::Order { order_id, .. } => valid_identifier(order_id),
                CancelTargetV6::SameDecision { provider_client_id } => {
                    valid_provider_client_id(provider_client_id)
                }
            };
            if !valid {
                return Err(DecisionV6Error::InvalidContract);
            }
        }
        StrategyCommandV6::CancelAllOrders { .. } => {}
        StrategyCommandV6::ScheduleTimer {
            key,
            generation,
            semantics,
            ..
        } => {
            if !valid_identifier(key)
                || !valid_identifier(generation)
                || semantics.len() > MAX_TIMER_SEMANTICS_BYTES_V6
            {
                return Err(DecisionV6Error::BoundExceeded);
            }
        }
        StrategyCommandV6::CancelTimer {
            key, generation, ..
        } => {
            if !valid_identifier(key) || !valid_identifier(generation) {
                return Err(DecisionV6Error::InvalidContract);
            }
        }
        StrategyCommandV6::Stop { reason, .. } if !valid_text(reason, MAX_REASON_BYTES) => {
            return Err(DecisionV6Error::BoundExceeded);
        }
        StrategyCommandV6::Stop { .. } => {}
    }
    Ok(())
}

fn valid_place_order_prices(order: &PlaceOrderV6) -> bool {
    match (order.action, order.order_type) {
        (OrderActionV6::Buy, OrderTypeV6::Market) => {
            order.limit_price_micros.is_none()
                && order
                    .market_price_cap_micros
                    .is_none_or(|price| (1..=MAX_PRICE_MICROS).contains(&price))
        }
        (OrderActionV6::Sell, OrderTypeV6::Market) => {
            order.limit_price_micros.is_none() && order.market_price_cap_micros.is_none()
        }
        (_, OrderTypeV6::Limit) => {
            order
                .limit_price_micros
                .is_some_and(|price| price <= MAX_PRICE_MICROS)
                && order.market_price_cap_micros.is_none()
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Plan rows
// ---------------------------------------------------------------------------------------------

/// Counts the account plan rows a decision's Broker commands need, as the runner does while
/// the kernel issues them, in issue order:
///
/// - 4 per decision with a Broker command, plus 1 when the result acknowledges anything
///   (acknowledged receipts and orders are removed with one statement);
/// - 5 per place;
/// - 3 per cancel, or 1 when its target is already final in the context (the Broker refuses
///   it);
/// - in paper, 3 per cancel-all plus 1 per order open at that point: every non-terminal
///   context order and every place issued before it in the decision (a cancel-all includes the
///   decision's own earlier orders);
/// - in live, where the Broker expands a cancel-all into one cancel per order on the priority
///   lane, 3 per open context order it cancels plus 1 per own place it collapses (at least 1,
///   for its receipt).
///
/// The orders a cancel-all covers are then being cancelled, so a later cancel-all counts only
/// places issued after the earlier one.
///
/// The count is an upper bound of the Broker's: a command the Broker refuses costs
/// [`REFUSED_COMMAND_PLAN_ROWS`] instead, and an order already being cancelled is still
/// counted. traderv3's parity test checks the runner's count is at least the owner's.
#[derive(Clone, Debug)]
pub struct DecisionPlanRows {
    rows: usize,
    broker_commands: usize,
    acknowledgements: bool,
    mode: DeploymentModeV6,
    /// Orders a cancel-all would cancel: non-terminal context orders and this decision's own.
    active_orders: BTreeSet<String>,
}

impl DecisionPlanRows {
    /// Starts from the context's orders; `acknowledgements` when the result acknowledges any.
    pub fn new(broker: &BrokerDetailV6, acknowledgements: bool, mode: DeploymentModeV6) -> Self {
        Self {
            rows: 0,
            broker_commands: 0,
            acknowledgements,
            mode,
            active_orders: broker
                .orders
                .iter()
                .filter(|order| !order.status.is_terminal())
                .map(|order| format!("order:{}", order.order_id))
                .collect(),
        }
    }

    /// The rows of the decision so far; zero while it has no Broker command.
    pub fn total(&self) -> usize {
        if self.broker_commands == 0 {
            return 0;
        }
        DECISION_PLAN_ROWS
            + self.rows
            + if self.acknowledgements {
                ACKNOWLEDGEMENT_PLAN_ROWS
            } else {
                0
            }
    }

    /// The total if `command` were added next.
    pub fn with(&self, command: &StrategyCommandV6, broker: &BrokerDetailV6) -> Self {
        let mut next = self.clone();
        next.add(command, broker);
        next
    }

    pub fn add(&mut self, command: &StrategyCommandV6, broker: &BrokerDetailV6) {
        let rows = match command {
            StrategyCommandV6::PlaceOrder(order) => {
                self.active_orders
                    .insert(format!("client:{}", order.provider_client_id));
                // In live the Broker refuses a Market sell with a receipt (a row of its own).
                let refused_market_sell = self.mode == DeploymentModeV6::Live
                    && order.action == OrderActionV6::Sell
                    && order.order_type == OrderTypeV6::Market;
                PLACE_ORDER_PLAN_ROWS
                    + if refused_market_sell {
                        REFUSED_COMMAND_PLAN_ROWS
                    } else {
                        0
                    }
            }
            StrategyCommandV6::CancelOrder {
                target: CancelTargetV6::Order { order_id, .. },
                ..
            } => {
                let final_target = broker
                    .orders
                    .iter()
                    .any(|order| order.order_id == *order_id && order.status.is_terminal());
                if final_target {
                    REFUSED_COMMAND_PLAN_ROWS
                } else {
                    CANCEL_ORDER_PLAN_ROWS
                }
            }
            StrategyCommandV6::CancelOrder { .. } => CANCEL_ORDER_PLAN_ROWS,
            StrategyCommandV6::CancelAllOrders { .. } => {
                let rows = match self.mode {
                    DeploymentModeV6::Paper => {
                        CANCEL_ALL_ORDERS_PLAN_ROWS + self.active_orders.len()
                    }
                    DeploymentModeV6::Live => {
                        let context_orders = self
                            .active_orders
                            .iter()
                            .filter(|key| key.starts_with("order:"))
                            .count();
                        let own_places = self.active_orders.len() - context_orders;
                        (CANCEL_ORDER_PLAN_ROWS * context_orders
                            + COLLAPSED_PLACE_PLAN_ROWS * own_places)
                            .max(REFUSED_COMMAND_PLAN_ROWS)
                    }
                };
                self.active_orders.clear();
                rows
            }
            StrategyCommandV6::ScheduleTimer { .. }
            | StrategyCommandV6::CancelTimer { .. }
            | StrategyCommandV6::Stop { .. } => return,
        };
        self.broker_commands += 1;
        self.rows += rows;
    }
}

/// The context's open (non-terminal) orders.
pub fn open_orders(broker: &BrokerDetailV6) -> usize {
    broker
        .orders
        .iter()
        .filter(|order| !order.status.is_terminal())
        .count()
}

/// The account plan rows of a result's Broker commands (zero without one).
pub fn decision_plan_rows_v6(context: &DecisionContextV6, result: &DecisionResultV6) -> usize {
    let mut rows = DecisionPlanRows::new(
        &context.broker,
        !result.acknowledged_command_ids.is_empty(),
        context.deployment_mode,
    );
    for command in &result.commands {
        rows.add(command, &context.broker);
    }
    rows.total()
}

// ---------------------------------------------------------------------------------------------
// Identities
// ---------------------------------------------------------------------------------------------

pub fn derive_sleeve_identity_v6(
    strategy_id: &str,
    binding_id: &str,
    venue_id: &str,
    opportunity_id: &str,
) -> String {
    let mut digest = Sha256::new();
    digest.update(SLEEVE_ID_DOMAIN);
    for component in [strategy_id, binding_id, venue_id, opportunity_id] {
        let length = u16::try_from(component.len()).expect("bounded V6 identity fits in u16");
        digest.update(length.to_be_bytes());
        digest.update(component.as_bytes());
    }
    hex_digest(&digest.finalize())
}

/// The provider client id traderv3 derives for a place without a kernel `client_order_id`:
/// `tv3<mode>_<first 24 hex of IntentId>`, where `IntentId = sha256(DecisionId, ordinal)`
/// and `DecisionId` binds the Sleeve, its incarnation and the delivery. `None` when the
/// Sleeve id is not a 64-hex digest.
pub fn derive_provider_client_id_v6(
    mode: DeploymentModeV6,
    sleeve_id: &str,
    incarnation: u64,
    delivery_id: &str,
    ordinal: u32,
) -> Option<String> {
    let intent = intent_id_v6(sleeve_id, incarnation, delivery_id, ordinal)?;
    Some(format!(
        "tv3{}_{}",
        mode.as_str(),
        &hex_digest(&intent)[..24]
    ))
}

fn parse_hex_digest(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut bytes = [0; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let text = std::str::from_utf8(pair).ok()?;
        if !text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            return None;
        }
        bytes[index] = u8::from_str_radix(text, 16).ok()?;
    }
    Some(bytes)
}

// ---------------------------------------------------------------------------------------------
// Digests
// ---------------------------------------------------------------------------------------------

pub fn kernel_checkpoint_v6_sha256(checkpoint: &KernelCheckpointV6) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(CHECKPOINT_DIGEST_DOMAIN);
    hash_component(&mut hasher, checkpoint.codec_profile.as_bytes());
    hasher.update(checkpoint.codec_version.to_be_bytes());
    hash_component(&mut hasher, checkpoint.strategy_id.as_bytes());
    hash_component(&mut hasher, checkpoint.strategy_profile.as_bytes());
    hash_component(
        &mut hasher,
        checkpoint.profile_and_calculator_digest.as_bytes(),
    );
    hasher.update(checkpoint.sequence.to_be_bytes());
    hash_component(&mut hasher, &checkpoint.state);
    hash_component(
        &mut hasher,
        &bincode::encode_to_vec(&checkpoint.runner, wire_config())
            .expect("the runner section encodes"),
    );
    hasher.finalize().into()
}

fn hash_component(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

/// The fence a host recomputes from the context it sent and compares with the result's
/// `state_fence`.
pub fn decision_fence_v6_sha256(context: &DecisionContextV6) -> Result<[u8; 32], DecisionV6Error> {
    context.validate()?;
    let mut hasher = Sha256::new();
    hasher.update(STATE_FENCE_DOMAIN);
    hash_component(
        &mut hasher,
        decision_fence_v4_sha256(&context.owner_state.fence)
            .map_err(DecisionV6Error::V4)?
            .as_bytes(),
    );
    hash_component(&mut hasher, context.strategy.strategy_id.as_bytes());
    hash_component(&mut hasher, context.strategy.binding_id.as_bytes());
    hash_component(&mut hasher, &encode(&context.strategy.parameters)?);
    hasher.update(context.broker.revision.to_be_bytes());
    match &context.kernel_checkpoint {
        Some(checkpoint) => {
            hasher.update([1]);
            hasher.update(checkpoint.state_sha256);
        }
        None => hasher.update([0]),
    }
    hasher.update(context.decision_time_unix_ms.to_be_bytes());
    hash_component(&mut hasher, &encode(&context.market_strikes)?);
    hash_component(&mut hasher, &encode(&context.deployment_mode)?);
    hash_component(&mut hasher, &encode(&context.capabilities)?);
    hash_component(&mut hasher, &encode(&context.command_receipts)?);
    Ok(hasher.finalize().into())
}

/// The wire-encoded length of a value (without a magic).
pub fn encoded_len<T: Encode>(value: &T) -> usize {
    bincode::encode_to_vec(value, wire_config()).map_or(usize::MAX, |bytes| bytes.len())
}

fn encode<T: Encode>(value: &T) -> Result<Vec<u8>, DecisionV6Error> {
    bincode::encode_to_vec(value, wire_config()).map_err(|_| DecisionV6Error::Encode)
}

pub fn decision_context_v6_sha256(
    context: &DecisionContextV6,
) -> Result<[u8; 32], DecisionV6Error> {
    Ok(Sha256::digest(encode_decision_context_v6(context)?).into())
}

pub fn decision_result_v6_sha256(result: &DecisionResultV6) -> Result<[u8; 32], DecisionV6Error> {
    Ok(Sha256::digest(encode_decision_result_v6(result)?).into())
}

// ---------------------------------------------------------------------------------------------
// Codecs
// ---------------------------------------------------------------------------------------------

/// Encodes a valid context. The bytes are checked to decode within the decoder's allocation
/// limit, so a context the host builds is one every reader accepts.
pub fn encode_decision_context_v6(context: &DecisionContextV6) -> Result<Vec<u8>, DecisionV6Error> {
    context.validate()?;
    let bytes = encode_bounded(
        DECISION_CONTEXT_V6_MAGIC,
        context,
        MAX_DECISION_CONTEXT_V6_BYTES,
    )?;
    decode_bounded::<DecisionContextV6, MAX_DECISION_CONTEXT_V6_BYTES>(
        DECISION_CONTEXT_V6_MAGIC,
        &bytes,
    )
    .map_err(|_| DecisionV6Error::BoundExceeded)?;
    Ok(bytes)
}

/// Decodes a context. The decoder claims every length prefix against the context bound before
/// allocating, so a corrupt prefix is a `Decode` error, never an allocation abort.
pub fn decode_decision_context_v6(bytes: &[u8]) -> Result<DecisionContextV6, DecisionV6Error> {
    let context: DecisionContextV6 =
        decode_bounded::<_, MAX_DECISION_CONTEXT_V6_BYTES>(DECISION_CONTEXT_V6_MAGIC, bytes)?;
    context.validate()?;
    Ok(context)
}

/// Encodes a valid result, checked to decode within the decoder's allocation limit.
pub fn encode_decision_result_v6(result: &DecisionResultV6) -> Result<Vec<u8>, DecisionV6Error> {
    result.validate()?;
    let bytes = encode_bounded(
        DECISION_RESULT_V6_MAGIC,
        result,
        MAX_DECISION_RESULT_V6_BYTES,
    )?;
    decode_bounded::<DecisionResultV6, MAX_DECISION_RESULT_V6_BYTES>(
        DECISION_RESULT_V6_MAGIC,
        &bytes,
    )
    .map_err(|_| DecisionV6Error::BoundExceeded)?;
    Ok(bytes)
}

/// Decodes a result with the same allocation limit as a context.
pub fn decode_decision_result_v6(bytes: &[u8]) -> Result<DecisionResultV6, DecisionV6Error> {
    let result: DecisionResultV6 =
        decode_bounded::<_, MAX_DECISION_RESULT_V6_BYTES>(DECISION_RESULT_V6_MAGIC, bytes)?;
    result.validate()?;
    Ok(result)
}

/// Encodes order update records as evidence entries in order, each within the payload bound.
pub fn order_update_evidence(
    records: &[OrderUpdateRecordV6],
) -> Result<Vec<ResultEvidenceV6>, DecisionV6Error> {
    let mut evidence = Vec::new();
    let mut chunk: Vec<OrderUpdateRecordV6> = Vec::new();
    let mut chunk_bytes = 0;
    for record in records {
        let bytes = bincode::encode_to_vec(record, wire_config())
            .map_err(|_| DecisionV6Error::Encode)?
            .len();
        // The list length prefix takes at most nine bytes.
        if !chunk.is_empty() && chunk_bytes + bytes + 9 > MAX_EVIDENCE_PAYLOAD_BYTES {
            evidence.push(order_update_chunk(&chunk)?);
            chunk.clear();
            chunk_bytes = 0;
        }
        chunk.push(record.clone());
        chunk_bytes += bytes;
    }
    if !chunk.is_empty() {
        evidence.push(order_update_chunk(&chunk)?);
    }
    Ok(evidence)
}

fn order_update_chunk(
    records: &[OrderUpdateRecordV6],
) -> Result<ResultEvidenceV6, DecisionV6Error> {
    let payload =
        bincode::encode_to_vec(records, wire_config()).map_err(|_| DecisionV6Error::Encode)?;
    if payload.len() > MAX_EVIDENCE_PAYLOAD_BYTES {
        return Err(DecisionV6Error::BoundExceeded);
    }
    Ok(ResultEvidenceV6 {
        code: ORDER_UPDATES_EVIDENCE_CODE.to_owned(),
        payload,
    })
}

/// Decodes one `order_updates` evidence entry.
pub fn decode_order_update_evidence(
    evidence: &ResultEvidenceV6,
) -> Result<Vec<OrderUpdateRecordV6>, DecisionV6Error> {
    if evidence.code != ORDER_UPDATES_EVIDENCE_CODE {
        return Err(DecisionV6Error::InvalidContract);
    }
    let (records, consumed): (Vec<OrderUpdateRecordV6>, usize) = bincode::decode_from_slice(
        &evidence.payload,
        wire_config().with_limit::<MAX_EVIDENCE_PAYLOAD_BYTES>(),
    )
    .map_err(|_| DecisionV6Error::Decode)?;
    if consumed != evidence.payload.len() {
        return Err(DecisionV6Error::TrailingBytes);
    }
    if records.iter().any(|record| {
        !valid_identifier(&record.command_id)
            || !valid_optional_identifier(&record.client_order_id)
            || !valid_optional_identifier(&record.order_id)
            || !valid_optional_identifier(&record.market_id)
    }) {
        return Err(DecisionV6Error::InvalidContract);
    }
    Ok(records)
}

/// Converts a checkpoint saved under Decision V5. The kernel's state bytes carry over unchanged
/// (each kernel's codec owns its versions) and the runner section starts empty, so the first
/// decision records the current Broker state as seen without emitting updates.
pub fn convert_v5_kernel_checkpoint(
    checkpoint: KernelCheckpointV5Layout,
) -> Result<KernelCheckpointV6, DecisionV6Error> {
    let mut hasher = Sha256::new();
    hasher.update(V5_CHECKPOINT_DIGEST_DOMAIN);
    hash_component(&mut hasher, checkpoint.codec_profile.as_bytes());
    hasher.update(checkpoint.codec_version.to_be_bytes());
    hash_component(&mut hasher, checkpoint.strategy_id.as_bytes());
    hash_component(&mut hasher, checkpoint.strategy_profile.as_bytes());
    hash_component(
        &mut hasher,
        checkpoint.profile_and_calculator_digest.as_bytes(),
    );
    hasher.update(checkpoint.sequence.to_be_bytes());
    hash_component(&mut hasher, &checkpoint.state);
    let v5_digest: [u8; 32] = hasher.finalize().into();
    if checkpoint.state_sha256 != v5_digest {
        return Err(DecisionV6Error::InvalidContract);
    }
    let converted = KernelCheckpointV6 {
        codec_profile: checkpoint.codec_profile,
        codec_version: checkpoint.codec_version,
        strategy_id: checkpoint.strategy_id,
        strategy_profile: checkpoint.strategy_profile,
        profile_and_calculator_digest: checkpoint.profile_and_calculator_digest,
        sequence: checkpoint.sequence,
        state: checkpoint.state,
        runner: RunnerSectionV6::default(),
        state_sha256: [0; 32],
    }
    .seal();
    converted.validate()?;
    Ok(converted)
}

pub(crate) fn wire_config() -> bincode::config::Configuration<
    bincode::config::BigEndian,
    bincode::config::Varint,
    bincode::config::NoLimit,
> {
    bincode::config::standard()
        .with_big_endian()
        .with_variable_int_encoding()
}

fn encode_bounded<T: Encode>(
    magic: &[u8; 8],
    value: &T,
    max_bytes: usize,
) -> Result<Vec<u8>, DecisionV6Error> {
    let payload =
        bincode::encode_to_vec(value, wire_config()).map_err(|_| DecisionV6Error::Encode)?;
    let total = magic
        .len()
        .checked_add(payload.len())
        .ok_or(DecisionV6Error::BoundExceeded)?;
    if total > max_bytes {
        return Err(DecisionV6Error::BoundExceeded);
    }
    let mut bytes = Vec::with_capacity(total);
    bytes.extend_from_slice(magic);
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

/// Decodes under an allocation limit of `LIMIT` bytes: bincode claims every length prefix
/// (and every decoded primitive) against it before allocating.
fn decode_bounded<T: Decode<()>, const LIMIT: usize>(
    magic: &[u8; 8],
    bytes: &[u8],
) -> Result<T, DecisionV6Error> {
    if bytes.len() > LIMIT || !bytes.starts_with(magic) {
        return Err(DecisionV6Error::Decode);
    }
    let (value, consumed) =
        bincode::decode_from_slice(&bytes[magic.len()..], wire_config().with_limit::<LIMIT>())
            .map_err(|_| DecisionV6Error::Decode)?;
    if consumed != bytes.len() - magic.len() {
        return Err(DecisionV6Error::TrailingBytes);
    }
    Ok(value)
}

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

pub(crate) fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    output
}

pub fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_IDENTIFIER_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

/// An identifier within the Broker's client order id bound.
pub fn valid_provider_client_id(value: &str) -> bool {
    valid_identifier(value) && value.len() <= MAX_PROVIDER_CLIENT_ID_BYTES
}

fn valid_optional_identifier(value: &Option<String>) -> bool {
    value.as_ref().is_none_or(|value| valid_identifier(value))
}

fn valid_text(value: &str, max_bytes: usize) -> bool {
    !value.is_empty() && value.len() <= max_bytes
}

fn valid_optional_text(value: &Option<String>, max_bytes: usize) -> bool {
    value
        .as_ref()
        .is_none_or(|value| valid_text(value, max_bytes))
}

fn strictly_sorted<T: Ord>(values: impl IntoIterator<Item = T>) -> bool {
    let mut previous = None;
    for value in values {
        if previous.as_ref().is_some_and(|previous| previous >= &value) {
            return false;
        }
        previous = Some(value);
    }
    true
}

fn unique<T: Ord>(values: impl IntoIterator<Item = T>) -> Result<(), DecisionV6Error> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value) {
            return Err(DecisionV6Error::DuplicateIdentity);
        }
    }
    Ok(())
}

/// Receipts by command id, for callers that look them up.
pub fn receipts_by_command(receipts: &[CommandReceiptV6]) -> BTreeMap<&str, &CommandReceiptV6> {
    receipts
        .iter()
        .map(|receipt| (receipt.command_id.as_str(), receipt))
        .collect()
}

#[cfg(test)]
#[path = "decision_v6/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "decision_v6/corpus_tests.rs"]
mod corpus_tests;
