//! Decision Context V5: an exact, bounded transaction contract for stateful Strategies.
//!
//! V5 composes the immutable V4 owner projection. It adds exact Strategy scope, side-aware Broker
//! state, typed source and Broker triggers, and fenced commands. Collections are canonically sorted
//! before encoding and validation rejects alternate orderings.

use std::collections::BTreeSet;

use bincode::{Decode, Encode};
use sha2::{Digest, Sha256};

use crate::decision_v4::{DecisionContextV4, DecisionV4Error, TriggerV4, decision_fence_v4_sha256};
use crate::supplied_v5::{ExtremeKindV5, SuppliedEventV5, SuppliedInputsV5};
use crate::wire_supplied::FrozenSuppliedInputsV5;
mod replay_origin;
#[doc(hidden)]
pub use crate::wire_supplied::RetainedSuppliedEncodingV5;
pub(crate) use replay_origin::{replay_origin_digest, replay_origin_encoding};

/// Current V5 context encoding, including bounded host-owned Broker replay state.
pub const DECISION_CONTEXT_V5_MAGIC: &[u8; 8] = b"SDCTXV5E";
/// Frozen host-selected weather, forecast, oracle and captured-origin encoding.
pub const HOST_D_DECISION_CONTEXT_V5_MAGIC: &[u8; 8] = b"SDCTXV5D";
/// Frozen canonical supplied-inputs/2 encoding, without per-field host winners.
pub const CANONICAL_C_DECISION_CONTEXT_V5_MAGIC: &[u8; 8] = b"SDCTXV5C";
/// Durable V5 context encoding carrying the first supplied inputs shape (`supplied-inputs/1`).
pub const SUPPLIED_S_DECISION_CONTEXT_V5_MAGIC: &[u8; 8] = b"SDCTXV5S";
/// Durable V5 context encoding with explicit hundredths quantities and no supplied inputs.
pub const HUNDREDTHS_DECISION_CONTEXT_V5_MAGIC: &[u8; 8] = b"SDCTXV5H";
/// Durable legacy V5 context encoding whose Broker-state quantities are whole contracts.
pub const LEGACY_DECISION_CONTEXT_V5_MAGIC: &[u8; 8] = b"SDCTXV5\0";
/// Current V5 result encoding with explicit hundredths command and outcome quantities.
pub const DECISION_RESULT_V5_MAGIC: &[u8; 8] = b"SDRESV5H";
/// Durable legacy V5 result encoding whose command and outcome quantities are whole contracts.
pub const LEGACY_DECISION_RESULT_V5_MAGIC: &[u8; 8] = b"SDRESV5\0";
pub const MAX_DECISION_CONTEXT_V5_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_DECISION_RESULT_V5_BYTES: usize = 256 * 1024;
pub const MAX_STRATEGY_PARAMETERS: usize = 256;
pub const MAX_BROKER_POSITIONS: usize = 256;
pub const MAX_BROKER_ORDERS: usize = 256;
pub const MAX_STRATEGY_COMMANDS: usize = 64;
pub const MAX_RESULT_EVIDENCE: usize = 64;
pub const MAX_RESULT_DIAGNOSTICS: usize = 64;
pub const MAX_COMMAND_METADATA_BYTES: usize = 64 * 1024;
pub const MAX_TIMER_SEMANTICS_BYTES_V5: usize = 16 * 1024;
pub const MAX_RESULT_DIAGNOSTIC_BYTES: usize = 4 * 1024;
/// Maximum opaque private kernel state carried across one V5 transaction.
pub const MAX_KERNEL_CHECKPOINT_BYTES: usize = 128 * 1024;
pub const MAX_IDENTIFIER_BYTES: usize = 160;
pub const MAX_SHORT_TEXT_BYTES: usize = 512;
pub const MAX_REASON_BYTES: usize = 4 * 1024;
pub const MAX_PRICE_MICROS: u64 = 1_000_000;
const SLEEVE_ID_DOMAIN: &[u8] = b"trader-v3/sleeve-id/v1\0";
const COMMAND_DIGEST_DOMAIN: &[u8] = b"strategy-core/decision-v5/command/v1\0";
const CHECKPOINT_DIGEST_DOMAIN: &[u8] = b"strategy-core/decision-v5/checkpoint/v1\0";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecisionV5Error {
    Encode,
    Decode,
    BoundExceeded,
    TrailingBytes,
    InvalidContract,
    DuplicateIdentity,
    NonCanonicalOrder,
    V4(DecisionV4Error),
}

impl core::fmt::Display for DecisionV5Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for DecisionV5Error {}

#[derive(Clone, Copy, Debug, Encode, Decode, Eq, Ord, PartialEq, PartialOrd)]
pub enum ContractSideV5 {
    Yes,
    No,
}

#[derive(Clone, Copy, Debug, Encode, Decode, Eq, PartialEq)]
pub enum OrderActionV5 {
    Buy,
    Sell,
}

#[derive(Clone, Copy, Debug, Encode, Decode, Eq, PartialEq)]
pub enum OrderTypeV5 {
    Market,
    Limit,
}

#[derive(Clone, Copy, Debug, Encode, Decode, Eq, PartialEq)]
pub enum BrokerOrderStatusV5 {
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

#[derive(Clone, Copy, Debug, Encode, Decode, Eq, PartialEq)]
pub enum BrokerCommandKindV5 {
    PlaceOrder,
    CancelOrder,
    CancelAllOrders,
}

#[derive(Clone, Copy, Debug, Encode, Decode, Eq, PartialEq)]
pub enum BrokerOutcomeStatusV5 {
    Rejected,
    DurablyAccepted,
    Dispatched,
    Resting,
    PartiallyFilled,
    Filled,
    CancellationRequested,
    Cancelled,
    Expired,
    RecoveryRequired,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum StrategyParameterValueV5 {
    Null,
    Bool(bool),
    I64(i64),
    U64(u64),
    Decimal { coefficient: i64, scale: u8 },
    String(String),
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct StrategyScopeV5 {
    pub strategy_id: String,
    pub binding_id: String,
    pub profile: String,
    /// Sorted top-level values projected without loss into the frozen JSON initializer.
    pub parameters: Vec<(String, StrategyParameterValueV5)>,
    pub station_id: String,
    pub event_ticker: String,
    pub event_date: String,
    pub market_ids: Vec<String>,
    /// Digest attested by both the configured V4 state and the immutable Strategy release.
    pub profile_and_calculator_digest: String,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct BrokerPositionV5 {
    pub market_id: String,
    pub side: ContractSideV5,
    /// Exact position quantity in hundredths of one contract.
    pub quantity_hundredths: u64,
    /// Entry cost excluding fees. The adapter projects average price as cost / quantity.
    pub cost_basis_micros: u64,
    pub fees_micros: u64,
}

impl BrokerPositionV5 {
    pub fn average_entry_price(&self) -> f64 {
        self.cost_basis_micros as f64 * 100.0
            / self.quantity_hundredths as f64
            / MAX_PRICE_MICROS as f64
    }
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct BrokerOrderV5 {
    pub command_id: String,
    pub intent_id: String,
    pub order_id: String,
    pub provider_order_id: Option<String>,
    pub provider_client_id: String,
    pub market_id: String,
    pub action: OrderActionV5,
    pub side: ContractSideV5,
    pub order_type: OrderTypeV5,
    /// Exact original order quantity in hundredths of one contract.
    pub quantity_hundredths: u64,
    /// Exact filled order quantity in hundredths of one contract.
    pub filled_quantity_hundredths: u64,
    /// Exact remaining order quantity in hundredths of one contract.
    pub remaining_quantity_hundredths: u64,
    pub limit_price_micros: Option<u64>,
    pub average_fill_price_micros: Option<u64>,
    pub reserved_principal_micros: u64,
    pub reserved_fee_micros: u64,
    pub created_at_unix_ms: Option<i64>,
    pub updated_at_unix_ms: Option<i64>,
    pub signal_type: Option<String>,
    pub signal_metadata: Option<String>,
    pub status: BrokerOrderStatusV5,
    pub revision: u64,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct BrokerDetailV5 {
    pub revision: u64,
    /// Exact Sleeve-local sum of active order principal and fee reservations.
    pub reserved_cash_micros: u64,
    /// Sorted by `(market_id, side)`.
    pub positions: Vec<BrokerPositionV5>,
    /// Sorted by `order_id`; includes every bounded Strategy-owned order needed for reconciliation.
    pub orders: Vec<BrokerOrderV5>,
}

#[derive(Clone, Debug, Encode, Decode)]
struct LegacyBrokerPositionV5 {
    market_id: String,
    side: ContractSideV5,
    quantity: u64,
    cost_basis_micros: u64,
    fees_micros: u64,
}

#[derive(Clone, Debug, Encode, Decode)]
struct LegacyBrokerOrderV5 {
    command_id: String,
    intent_id: String,
    order_id: String,
    provider_order_id: Option<String>,
    provider_client_id: String,
    market_id: String,
    action: OrderActionV5,
    side: ContractSideV5,
    order_type: OrderTypeV5,
    quantity: u64,
    filled_quantity: u64,
    remaining_quantity: u64,
    limit_price_micros: Option<u64>,
    average_fill_price_micros: Option<u64>,
    reserved_principal_micros: u64,
    reserved_fee_micros: u64,
    created_at_unix_ms: Option<i64>,
    updated_at_unix_ms: Option<i64>,
    signal_type: Option<String>,
    signal_metadata: Option<String>,
    status: BrokerOrderStatusV5,
    revision: u64,
}

#[derive(Clone, Debug, Encode, Decode)]
struct LegacyBrokerDetailV5 {
    revision: u64,
    reserved_cash_micros: u64,
    positions: Vec<LegacyBrokerPositionV5>,
    orders: Vec<LegacyBrokerOrderV5>,
}

/// Bounded, versioned private kernel state owned by one exact Strategy profile.
///
/// The host treats `state` as opaque bytes. The Strategy artifact owns the codec and must reject
/// unsupported versions. `state_sha256` binds the bytes and all codec/scope fields.
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct KernelCheckpointV5 {
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
pub struct ContinuationCommitmentV5 {
    pub originating_delivery_id: String,
    pub sleeve_identity: String,
    pub sleeve_incarnation: u64,
    pub process_attempt: u64,
    pub route_epoch: u64,
    pub continuation_id: String,
    pub continuation_generation: u64,
    pub command_id: String,
    pub command_sha256: [u8; 32],
    pub expected_broker_revision: u64,
    /// Digest of the exact canonical Decision Context V5 persisted by Trader for replay.
    pub originating_context_sha256: [u8; 32],
    /// Exact pre-event state restored before replaying the awaited Broker return.
    pub pre_event_checkpoint: KernelCheckpointV5,
}

#[derive(Clone, Copy, Debug, Encode, Decode, Eq, PartialEq)]
pub enum KernelOrderStatusV5 {
    Filled,
    Partial,
    Pending,
    Rejected,
    Cancelled,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct KernelBrokerErrorV5 {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct KernelOrderResultV5 {
    pub order_id: String,
    pub status: KernelOrderStatusV5,
    pub filled_quantity_hundredths: u64,
    pub fill_price_micros: u64,
    pub fee_cost_micros: u64,
    pub reason: String,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum PlaceOrderReturnV5 {
    Ok(KernelOrderResultV5),
    Err(KernelBrokerErrorV5),
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum CancelOrderReturnV5 {
    Ok(bool),
    Err(KernelBrokerErrorV5),
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum CancelAllOrdersReturnV5 {
    Ok { cancelled_order_ids: Vec<String> },
    Err(KernelBrokerErrorV5),
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum BrokerCommandReturnV5 {
    PlaceOrder(PlaceOrderReturnV5),
    CancelOrder(CancelOrderReturnV5),
    CancelAllOrders(CancelAllOrdersReturnV5),
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct BrokerOutcomeV5 {
    pub outcome_id: String,
    pub continuation_id: String,
    pub continuation_generation: u64,
    pub command_id: String,
    pub command_kind: BrokerCommandKindV5,
    pub transition_sequence: u64,
    pub target_order_id: Option<String>,
    pub order_id: Option<String>,
    pub intent_id: Option<String>,
    pub provider_order_id: Option<String>,
    pub provider_client_id: Option<String>,
    pub status: BrokerOutcomeStatusV5,
    /// Exact value projected back into the frozen synchronous Broker capability.
    pub return_value: BrokerCommandReturnV5,
    pub requested_quantity_hundredths: u64,
    pub filled_quantity_hundredths: u64,
    pub remaining_quantity_hundredths: u64,
    pub average_fill_price_micros: Option<u64>,
    pub reason: Option<String>,
    pub updated_at_unix_ms: i64,
    pub broker_revision: u64,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum OwnerTriggerV5 {
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
    StationReport {
        station_id: String,
        report_id: String,
        report_type: String,
        report_revision: u64,
        provider: String,
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
    // Variants below are appended so durable encodings of the variants above keep their indices.
    NewLow {
        station_id: String,
        event_date: Option<String>,
        temperature_milli_c: Option<i32>,
        observed_at_unix_ms: i64,
        component_revision: u64,
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
}

impl OwnerTriggerV5 {
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
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum OriginatingTriggerV5 {
    Owner(OwnerTriggerV5),
    BrokerState { broker_revision: u64 },
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum TriggerV5 {
    Owner(OwnerTriggerV5),
    BrokerState {
        broker_revision: u64,
    },
    BrokerOutcome {
        outcome: Box<BrokerOutcomeV5>,
        originating_trigger: Box<OriginatingTriggerV5>,
    },
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct CommandFenceV5 {
    pub continuation_id: String,
    pub continuation_generation: u64,
    pub expected_broker_revision: u64,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct PlaceOrderV5 {
    pub command_id: String,
    pub fence: CommandFenceV5,
    pub market_id: String,
    pub action: OrderActionV5,
    pub side: ContractSideV5,
    pub order_type: OrderTypeV5,
    pub quantity_hundredths: u64,
    pub limit_price_micros: Option<u64>,
    /// Authoritative maximum per-contract execution price for a Market buy.
    pub market_price_cap_micros: Option<u64>,
    pub expires_after_ms: Option<i64>,
    pub reduce_only: bool,
    pub provider_client_id: String,
    pub signal_type: Option<String>,
    pub signal_metadata: Option<String>,
    pub metadata: Vec<u8>,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum StrategyCommandV5 {
    PlaceOrder(PlaceOrderV5),
    CancelOrder {
        command_id: String,
        fence: CommandFenceV5,
        order_id: String,
        expected_order_revision: u64,
    },
    CancelAllOrders {
        command_id: String,
        fence: CommandFenceV5,
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

impl StrategyCommandV5 {
    pub fn command_id(&self) -> &str {
        match self {
            Self::PlaceOrder(order) => &order.command_id,
            Self::CancelOrder { command_id, .. }
            | Self::CancelAllOrders { command_id, .. }
            | Self::ScheduleTimer { command_id, .. }
            | Self::CancelTimer { command_id, .. }
            | Self::Stop { command_id, .. } => command_id,
        }
    }

    pub fn broker_fence(&self) -> Option<&CommandFenceV5> {
        match self {
            Self::PlaceOrder(order) => Some(&order.fence),
            Self::CancelOrder { fence, .. } | Self::CancelAllOrders { fence, .. } => Some(fence),
            Self::ScheduleTimer { .. } | Self::CancelTimer { .. } | Self::Stop { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum DecisionDispositionV5 {
    Completed,
    AwaitingBrokerOutcome {
        continuation_id: String,
        continuation_generation: u64,
        awaited_command_id: String,
    },
    Rejected,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct ResultEvidenceV5 {
    pub code: String,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct ResultDiagnosticV5 {
    pub severity: String,
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct StationWeatherV5 {
    pub station_id: String,
    pub facts: strategy_core_kernel::WeatherFacts,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct StationForecastIssuanceV5 {
    pub station_id: String,
    pub models: Vec<strategy_core_kernel::forecast::ForecastIssuance>,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct DecisionContextV5 {
    pub owner_state: DecisionContextV4,
    pub strategy: StrategyScopeV5,
    pub broker: BrokerDetailV5,
    pub trigger: TriggerV5,
    /// Latest durable private kernel state. Absent only before the first successful invocation.
    pub kernel_checkpoint: Option<KernelCheckpointV5>,
    /// Present only for a Broker outcome delivery and loaded from the durable host ledger.
    pub continuation: Option<ContinuationCommitmentV5>,
    /// Authoritative wall clock supplied to the frozen kernel. No process-clock fallback is allowed.
    pub decision_time_unix_ms: i64,
    /// Provider inputs at their supplied precision plus the exact typed originating event.
    pub supplied: SuppliedInputsV5,
    /// One explicit weather winner set per owner station. Absent only for historical or
    /// controlled inputs whose host did not retain per-field acceptance evidence.
    pub current_weather: Option<Vec<StationWeatherV5>>,
    /// Accepted issuance for each delivered model; provider refresh originals remain separate.
    pub forecast_issuance: Option<Vec<StationForecastIssuanceV5>>,
    pub current_inputs: Option<crate::current_v5::CurrentInputsV5>,
    /// Private codec evidence preserved through durable outcome construction and cloning.
    #[doc(hidden)]
    pub retained_supplied_encoding: RetainedSuppliedEncodingV5,
    /// Host-owned completed calls and the current returned Broker state. Absent in historical
    /// deliveries and before the first Broker return; never part of private Strategy state.
    pub broker_replay: Option<crate::replay_v5::BrokerReplayV5>,
}

/// Frozen C wire container; ordinary facts always come from the active context.
#[derive(Clone, Debug, Encode, Decode)]
struct FrozenCDecisionContextV5 {
    owner_state: DecisionContextV4,
    strategy: StrategyScopeV5,
    broker: BrokerDetailV5,
    trigger: TriggerV5,
    kernel_checkpoint: Option<KernelCheckpointV5>,
    continuation: Option<ContinuationCommitmentV5>,
    decision_time_unix_ms: i64,
    supplied: FrozenSuppliedInputsV5,
}

impl DecisionContextV5 {
    fn has_current_only_fields(&self) -> bool {
        self.broker_replay.is_some()
            || self.current_weather.is_some()
            || self.forecast_issuance.is_some()
            || self.current_inputs.is_some()
            || crate::supplied_v5::has_current_only_fields(&self.supplied)
    }

    fn frozen_c(&self) -> FrozenCDecisionContextV5 {
        FrozenCDecisionContextV5 {
            owner_state: self.owner_state.clone(),
            strategy: self.strategy.clone(),
            broker: self.broker.clone(),
            trigger: self.trigger.clone(),
            kernel_checkpoint: self.kernel_checkpoint.clone(),
            continuation: self.continuation.clone(),
            decision_time_unix_ms: self.decision_time_unix_ms,
            supplied: FrozenSuppliedInputsV5::from_current(
                &self.supplied,
                &self.retained_supplied_encoding,
            ),
        }
    }
}

fn convert_frozen_c_context(
    context: FrozenCDecisionContextV5,
) -> Result<DecisionContextV5, DecisionV5Error> {
    let (supplied, mut retained_supplied_encoding) = context.supplied.into_current()?;
    retained_supplied_encoding.canonical_c = true;
    Ok(DecisionContextV5 {
        broker_replay: None,
        owner_state: context.owner_state,
        strategy: context.strategy,
        broker: context.broker,
        trigger: context.trigger,
        kernel_checkpoint: context.kernel_checkpoint,
        continuation: context.continuation,
        decision_time_unix_ms: context.decision_time_unix_ms,
        supplied,
        current_weather: None,
        forecast_issuance: None,
        current_inputs: None,
        retained_supplied_encoding,
    })
}

/// Frozen shape of the durable `SDCTXV5S` encoding: the first supplied inputs shape.
#[derive(Clone, Debug, Encode, Decode)]
struct SuppliedSDecisionContextV5 {
    owner_state: DecisionContextV4,
    strategy: StrategyScopeV5,
    broker: BrokerDetailV5,
    trigger: TriggerV5,
    kernel_checkpoint: Option<KernelCheckpointV5>,
    continuation: Option<ContinuationCommitmentV5>,
    decision_time_unix_ms: i64,
    supplied: crate::supplied_s::SuppliedInputsV5,
}

/// Frozen shape of the durable `SDCTXV5H` encoding: explicit hundredths, no supplied inputs.
#[derive(Clone, Debug, Encode, Decode)]
struct HundredthsDecisionContextV5 {
    owner_state: DecisionContextV4,
    strategy: StrategyScopeV5,
    broker: BrokerDetailV5,
    trigger: TriggerV5,
    kernel_checkpoint: Option<KernelCheckpointV5>,
    continuation: Option<ContinuationCommitmentV5>,
    decision_time_unix_ms: i64,
}

#[derive(Clone, Debug, Encode, Decode)]
struct LegacyKernelOrderResultV5 {
    order_id: String,
    status: KernelOrderStatusV5,
    filled_quantity: u64,
    fill_price_micros: u64,
    fee_cost_micros: u64,
    reason: String,
}

#[derive(Clone, Debug, Encode, Decode)]
enum LegacyPlaceOrderReturnV5 {
    Ok(LegacyKernelOrderResultV5),
    Err(KernelBrokerErrorV5),
}

#[derive(Clone, Debug, Encode, Decode)]
enum LegacyBrokerCommandReturnV5 {
    PlaceOrder(LegacyPlaceOrderReturnV5),
    CancelOrder(CancelOrderReturnV5),
    CancelAllOrders(CancelAllOrdersReturnV5),
}

#[derive(Clone, Debug, Encode, Decode)]
struct LegacyBrokerOutcomeV5 {
    outcome_id: String,
    continuation_id: String,
    continuation_generation: u64,
    command_id: String,
    command_kind: BrokerCommandKindV5,
    transition_sequence: u64,
    target_order_id: Option<String>,
    order_id: Option<String>,
    intent_id: Option<String>,
    provider_order_id: Option<String>,
    provider_client_id: Option<String>,
    status: BrokerOutcomeStatusV5,
    return_value: LegacyBrokerCommandReturnV5,
    requested_quantity: u64,
    filled_quantity: u64,
    remaining_quantity: u64,
    average_fill_price_micros: Option<u64>,
    reason: Option<String>,
    updated_at_unix_ms: i64,
    broker_revision: u64,
}

#[derive(Clone, Debug, Encode, Decode)]
enum LegacyTriggerV5 {
    Owner(OwnerTriggerV5),
    BrokerState {
        broker_revision: u64,
    },
    BrokerOutcome {
        outcome: Box<LegacyBrokerOutcomeV5>,
        originating_trigger: Box<OriginatingTriggerV5>,
    },
}

#[derive(Clone, Debug, Encode, Decode)]
struct LegacyPlaceOrderV5 {
    command_id: String,
    fence: CommandFenceV5,
    market_id: String,
    action: OrderActionV5,
    side: ContractSideV5,
    order_type: OrderTypeV5,
    quantity: u64,
    limit_price_micros: Option<u64>,
    market_price_cap_micros: Option<u64>,
    expires_after_ms: Option<i64>,
    reduce_only: bool,
    provider_client_id: String,
    signal_type: Option<String>,
    signal_metadata: Option<String>,
    metadata: Vec<u8>,
}

#[derive(Clone, Debug, Encode, Decode)]
enum LegacyStrategyCommandV5 {
    PlaceOrder(LegacyPlaceOrderV5),
    CancelOrder {
        command_id: String,
        fence: CommandFenceV5,
        order_id: String,
        expected_order_revision: u64,
    },
    CancelAllOrders {
        command_id: String,
        fence: CommandFenceV5,
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

#[derive(Clone, Debug, Encode, Decode)]
struct LegacyDecisionContextV5 {
    owner_state: DecisionContextV4,
    strategy: StrategyScopeV5,
    broker: LegacyBrokerDetailV5,
    trigger: LegacyTriggerV5,
    kernel_checkpoint: Option<KernelCheckpointV5>,
    continuation: Option<ContinuationCommitmentV5>,
    decision_time_unix_ms: i64,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct DecisionResultV5 {
    pub delivery_id: String,
    pub sleeve_identity: String,
    pub state_fence: String,
    pub expected_broker_revision: u64,
    pub disposition: DecisionDispositionV5,
    /// Completed results carry post-event state. Awaiting results carry the exact pre-event state.
    /// Rejected results preserve the input checkpoint unchanged.
    pub kernel_checkpoint: Option<KernelCheckpointV5>,
    pub commands: Vec<StrategyCommandV5>,
    pub evidence: Vec<ResultEvidenceV5>,
    pub diagnostics: Vec<ResultDiagnosticV5>,
}

#[derive(Clone, Debug, Encode, Decode)]
struct LegacyDecisionResultV5 {
    delivery_id: String,
    sleeve_identity: String,
    state_fence: String,
    expected_broker_revision: u64,
    disposition: DecisionDispositionV5,
    kernel_checkpoint: Option<KernelCheckpointV5>,
    commands: Vec<LegacyStrategyCommandV5>,
    evidence: Vec<ResultEvidenceV5>,
    diagnostics: Vec<ResultDiagnosticV5>,
}

impl DecisionContextV5 {
    pub fn validate(&self) -> Result<(), DecisionV5Error> {
        let max_points = if self.current_weather.is_some() {
            crate::supplied_v5::MAX_SUPPLIED_FORECAST_POINTS
        } else {
            crate::decision_v4::MAX_POINTS_PER_MODEL
        };
        self.owner_state
            .validate_with_forecast_point_bound(max_points)
            .map_err(DecisionV5Error::V4)?;
        validate_scope(self)?;
        validate_broker(self)?;
        crate::replay_v5::validate(self)?;
        if let Some(checkpoint) = &self.kernel_checkpoint {
            validate_kernel_checkpoint(&self.strategy, checkpoint)?;
        }
        crate::current_v5::validate(self)?;
        validate_trigger(self)?;
        if (self.retained_supplied_encoding.canonical_c && self.has_current_only_fields())
            || (self.retained_supplied_encoding.canonical_d && self.broker_replay.is_some())
            || (self.retained_supplied_encoding.canonical_c
                && self.retained_supplied_encoding.canonical_d)
        {
            return Err(DecisionV5Error::InvalidContract);
        }
        crate::supplied_v5::validate_supplied_inputs(&self.supplied)?;
        self.retained_supplied_encoding.validate(&self.supplied)?;
        validate_supplied(self)?;
        validate_current_weather(self)?;
        validate_forecast_issuance(self)?;
        Ok(())
    }

    /// Latest coherent Broker state for admission; Source and original invocation inputs stay
    /// frozen while the host-owned replay state advances between synchronous calls.
    pub fn admission_broker(&self) -> &BrokerDetailV5 {
        self.broker_replay
            .as_ref()
            .map_or(&self.broker, |replay| &replay.returned_state.broker)
    }

    /// The owner trigger that originated this transaction: the trigger itself, or the stored
    /// originating trigger of a Broker outcome replay.
    pub fn originating_owner_trigger(&self) -> Option<&OwnerTriggerV5> {
        match &self.trigger {
            TriggerV5::Owner(trigger) => Some(trigger),
            TriggerV5::BrokerState { .. } => None,
            TriggerV5::BrokerOutcome {
                originating_trigger,
                ..
            } => match originating_trigger.as_ref() {
                OriginatingTriggerV5::Owner(trigger) => Some(trigger),
                OriginatingTriggerV5::BrokerState { .. } => None,
            },
        }
    }
}

impl DecisionResultV5 {
    pub fn validate(&self) -> Result<(), DecisionV5Error> {
        if !valid_identifier(&self.delivery_id)
            || !valid_identifier(&self.sleeve_identity)
            || !valid_text(&self.state_fence, MAX_SHORT_TEXT_BYTES)
            || self.commands.len() > MAX_STRATEGY_COMMANDS
            || self.evidence.len() > MAX_RESULT_EVIDENCE
            || self.diagnostics.len() > MAX_RESULT_DIAGNOSTICS
        {
            return Err(DecisionV5Error::BoundExceeded);
        }
        if let Some(checkpoint) = &self.kernel_checkpoint {
            validate_kernel_checkpoint_shape(checkpoint)?;
        }
        if self.evidence.iter().any(|evidence| {
            !valid_identifier(&evidence.code) || evidence.payload.len() > MAX_COMMAND_METADATA_BYTES
        }) || self.diagnostics.iter().any(|diagnostic| {
            !valid_identifier(&diagnostic.severity)
                || !valid_identifier(&diagnostic.code)
                || !valid_text(&diagnostic.message, MAX_RESULT_DIAGNOSTIC_BYTES)
        }) {
            return Err(DecisionV5Error::BoundExceeded);
        }
        unique(self.commands.iter().map(StrategyCommandV5::command_id))?;
        let mut broker_command_ids = Vec::new();
        for command in &self.commands {
            validate_command(command)?;
            if let Some(fence) = command.broker_fence() {
                if fence.expected_broker_revision != self.expected_broker_revision {
                    return Err(DecisionV5Error::InvalidContract);
                }
                broker_command_ids.push(command.command_id());
            }
        }
        match &self.disposition {
            DecisionDispositionV5::Completed
                if !broker_command_ids.is_empty() || self.kernel_checkpoint.is_none() =>
            {
                Err(DecisionV5Error::InvalidContract)
            }
            DecisionDispositionV5::Completed => Ok(()),
            DecisionDispositionV5::AwaitingBrokerOutcome {
                continuation_id,
                continuation_generation,
                awaited_command_id,
            } => {
                if broker_command_ids != [awaited_command_id.as_str()]
                    || !valid_identifier(continuation_id)
                    || *continuation_generation == 0
                    || self.kernel_checkpoint.is_none()
                {
                    return Err(DecisionV5Error::InvalidContract);
                }
                let fence = self
                    .commands
                    .iter()
                    .find_map(StrategyCommandV5::broker_fence)
                    .ok_or(DecisionV5Error::InvalidContract)?;
                if fence.continuation_id != *continuation_id
                    || fence.continuation_generation != *continuation_generation
                {
                    return Err(DecisionV5Error::InvalidContract);
                }
                Ok(())
            }
            DecisionDispositionV5::Rejected if self.commands.is_empty() => Ok(()),
            DecisionDispositionV5::Rejected => Err(DecisionV5Error::InvalidContract),
        }
    }
}

pub fn validate_decision_result_v5(
    context: &DecisionContextV5,
    result: &DecisionResultV5,
) -> Result<(), DecisionV5Error> {
    context.validate()?;
    result.validate()?;
    if result.delivery_id != context.owner_state.delivery_id
        || result.sleeve_identity != context.owner_state.sleeve.sleeve_id
        || result.expected_broker_revision != context.admission_broker().revision
        || result.state_fence != hex_digest(&decision_fence_v5_sha256(context)?)
    {
        return Err(DecisionV5Error::InvalidContract);
    }
    validate_checkpoint_transition(context, result)?;
    for command in &result.commands {
        match command {
            StrategyCommandV5::PlaceOrder(order)
                if !context
                    .strategy
                    .market_ids
                    .iter()
                    .any(|market_id| market_id == &order.market_id) =>
            {
                return Err(DecisionV5Error::InvalidContract);
            }
            StrategyCommandV5::CancelOrder {
                order_id,
                expected_order_revision,
                ..
            } if !context.admission_broker().orders.iter().any(|order| {
                order.order_id == *order_id && order.revision == *expected_order_revision
            }) =>
            {
                return Err(DecisionV5Error::InvalidContract);
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn continuation_commitment_v5(
    context: &DecisionContextV5,
    result: &DecisionResultV5,
) -> Result<Option<ContinuationCommitmentV5>, DecisionV5Error> {
    validate_decision_result_v5(context, result)?;
    let DecisionDispositionV5::AwaitingBrokerOutcome {
        continuation_id,
        continuation_generation,
        awaited_command_id,
    } = &result.disposition
    else {
        return Ok(None);
    };
    let command = result
        .commands
        .iter()
        .find(|command| command.command_id() == awaited_command_id)
        .ok_or(DecisionV5Error::InvalidContract)?;
    Ok(Some(ContinuationCommitmentV5 {
        originating_delivery_id: result.delivery_id.clone(),
        sleeve_identity: result.sleeve_identity.clone(),
        sleeve_incarnation: context.owner_state.sleeve.incarnation,
        process_attempt: context.owner_state.sleeve.process_attempt,
        route_epoch: context.owner_state.sleeve.route_epoch,
        continuation_id: continuation_id.clone(),
        continuation_generation: *continuation_generation,
        command_id: awaited_command_id.clone(),
        command_sha256: strategy_command_v5_sha256(command)?,
        expected_broker_revision: result.expected_broker_revision,
        originating_context_sha256: originating_context_v5_sha256(context)?,
        pre_event_checkpoint: result
            .kernel_checkpoint
            .clone()
            .ok_or(DecisionV5Error::InvalidContract)?,
    }))
}

/// Decode and validate a retained invocation without changing its continuation's
/// original context or command encoding. The context is returned for host scope checks.
pub fn decode_continuation_v5(
    context_bytes: &[u8],
    result_bytes: &[u8],
) -> Result<(DecisionContextV5, Option<ContinuationCommitmentV5>), DecisionV5Error> {
    let context = decode_decision_context_v5(context_bytes)?;
    let result = decode_decision_result_v5(result_bytes)?;
    let Some(mut commitment) = continuation_commitment_v5(&context, &result)? else {
        return Ok((context, None));
    };
    if !matches!(context.trigger, TriggerV5::BrokerOutcome { .. }) {
        commitment.originating_context_sha256 =
            if context_bytes.starts_with(HUNDREDTHS_DECISION_CONTEXT_V5_MAGIC) {
                hundredths_decision_context_v5_sha256(&context)?
            } else if context_bytes.starts_with(SUPPLIED_S_DECISION_CONTEXT_V5_MAGIC) {
                supplied_s_decision_context_v5_sha256(&context)?
            } else if context_bytes.starts_with(LEGACY_DECISION_CONTEXT_V5_MAGIC) {
                legacy_decision_context_v5_sha256(&context)?
            } else {
                decision_context_v5_sha256(&context)?
            };
    }
    if result_bytes.starts_with(LEGACY_DECISION_RESULT_V5_MAGIC) {
        let command = result
            .commands
            .iter()
            .find(|command| command.command_id() == commitment.command_id)
            .ok_or(DecisionV5Error::InvalidContract)?;
        commitment.command_sha256 = command_digest(&legacy_command_from_current(command)?)?;
    }
    Ok((context, Some(commitment)))
}

pub fn strategy_command_v5_sha256(
    command: &StrategyCommandV5,
) -> Result<[u8; 32], DecisionV5Error> {
    validate_command(command)?;
    command_digest(command)
}

/// Matches a persisted command digest against the current explicit-hundredths encoding or,
/// when every changed quantity is an exact whole contract, the durable legacy encoding.
pub fn strategy_command_v5_digest_matches(
    command: &StrategyCommandV5,
    expected: [u8; 32],
) -> Result<bool, DecisionV5Error> {
    if strategy_command_v5_sha256(command)? == expected {
        return Ok(true);
    }
    let legacy = match legacy_command_from_current(command) {
        Ok(legacy) => legacy,
        Err(DecisionV5Error::InvalidContract) => return Ok(false),
        Err(error) => return Err(error),
    };
    Ok(command_digest(&legacy)? == expected)
}

fn command_digest<T: Encode>(command: &T) -> Result<[u8; 32], DecisionV5Error> {
    let config = bincode::config::standard()
        .with_big_endian()
        .with_variable_int_encoding();
    let bytes = bincode::encode_to_vec(command, config).map_err(|_| DecisionV5Error::Encode)?;
    let mut hasher = Sha256::new();
    hasher.update(COMMAND_DIGEST_DOMAIN);
    hasher.update(bytes);
    Ok(hasher.finalize().into())
}

pub fn kernel_checkpoint_v5_sha256(checkpoint: &KernelCheckpointV5) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(CHECKPOINT_DIGEST_DOMAIN);
    hash_checkpoint_component(&mut hasher, checkpoint.codec_profile.as_bytes());
    hasher.update(checkpoint.codec_version.to_be_bytes());
    hash_checkpoint_component(&mut hasher, checkpoint.strategy_id.as_bytes());
    hash_checkpoint_component(&mut hasher, checkpoint.strategy_profile.as_bytes());
    hash_checkpoint_component(
        &mut hasher,
        checkpoint.profile_and_calculator_digest.as_bytes(),
    );
    hasher.update(checkpoint.sequence.to_be_bytes());
    hash_checkpoint_component(&mut hasher, &checkpoint.state);
    hasher.finalize().into()
}

fn hash_checkpoint_component(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

pub fn encode_decision_context_v5(context: &DecisionContextV5) -> Result<Vec<u8>, DecisionV5Error> {
    context.validate()?;
    if context.retained_supplied_encoding.canonical_d {
        return encode_bounded(
            HOST_D_DECISION_CONTEXT_V5_MAGIC,
            &crate::wire_d::FrozenDDecisionContextV5::from_current(context),
            MAX_DECISION_CONTEXT_V5_BYTES,
        );
    }
    if context.retained_supplied_encoding.canonical_c {
        return encode_bounded(
            CANONICAL_C_DECISION_CONTEXT_V5_MAGIC,
            &context.frozen_c(),
            MAX_DECISION_CONTEXT_V5_BYTES,
        );
    }
    encode_bounded(
        DECISION_CONTEXT_V5_MAGIC,
        context,
        MAX_DECISION_CONTEXT_V5_BYTES,
    )
}

pub fn decode_decision_context_v5(bytes: &[u8]) -> Result<DecisionContextV5, DecisionV5Error> {
    let context = if bytes.starts_with(DECISION_CONTEXT_V5_MAGIC) {
        decode_bounded(
            DECISION_CONTEXT_V5_MAGIC,
            bytes,
            MAX_DECISION_CONTEXT_V5_BYTES,
        )?
    } else if bytes.starts_with(HOST_D_DECISION_CONTEXT_V5_MAGIC) {
        let frozen: crate::wire_d::FrozenDDecisionContextV5 = decode_bounded(
            HOST_D_DECISION_CONTEXT_V5_MAGIC,
            bytes,
            MAX_DECISION_CONTEXT_V5_BYTES,
        )?;
        frozen.into_current()
    } else if bytes.starts_with(CANONICAL_C_DECISION_CONTEXT_V5_MAGIC) {
        convert_frozen_c_context(decode_bounded(
            CANONICAL_C_DECISION_CONTEXT_V5_MAGIC,
            bytes,
            MAX_DECISION_CONTEXT_V5_BYTES,
        )?)?
    } else if bytes.starts_with(SUPPLIED_S_DECISION_CONTEXT_V5_MAGIC) {
        let frozen: SuppliedSDecisionContextV5 = decode_bounded(
            SUPPLIED_S_DECISION_CONTEXT_V5_MAGIC,
            bytes,
            MAX_DECISION_CONTEXT_V5_BYTES,
        )?;
        convert_supplied_s_context(frozen)?
    } else if bytes.starts_with(HUNDREDTHS_DECISION_CONTEXT_V5_MAGIC) {
        let hundredths: HundredthsDecisionContextV5 = decode_bounded(
            HUNDREDTHS_DECISION_CONTEXT_V5_MAGIC,
            bytes,
            MAX_DECISION_CONTEXT_V5_BYTES,
        )?;
        convert_hundredths_context(hundredths)
    } else if bytes.starts_with(LEGACY_DECISION_CONTEXT_V5_MAGIC) {
        let legacy: LegacyDecisionContextV5 = decode_bounded(
            LEGACY_DECISION_CONTEXT_V5_MAGIC,
            bytes,
            MAX_DECISION_CONTEXT_V5_BYTES,
        )?;
        convert_legacy_context(legacy)?
    } else {
        return Err(DecisionV5Error::Decode);
    };
    context.validate()?;
    Ok(context)
}

fn convert_supplied_s_context(
    context: SuppliedSDecisionContextV5,
) -> Result<DecisionContextV5, DecisionV5Error> {
    let retained_supplied_encoding = RetainedSuppliedEncodingV5::from_s(&context.supplied)?;
    Ok(DecisionContextV5 {
        broker_replay: None,
        owner_state: context.owner_state,
        strategy: context.strategy,
        broker: context.broker,
        trigger: context.trigger,
        kernel_checkpoint: context.kernel_checkpoint,
        continuation: context.continuation,
        decision_time_unix_ms: context.decision_time_unix_ms,
        supplied: crate::supplied_v5::from_frozen_s(context.supplied),
        current_weather: None,
        forecast_issuance: None,
        current_inputs: None,
        retained_supplied_encoding,
    })
}

/// The durable `SDCTXV5S` shape of a context. Only a context whose supplied block fits the
/// first supplied shape has one.
fn supplied_s_context_from_current(
    context: &DecisionContextV5,
) -> Result<SuppliedSDecisionContextV5, DecisionV5Error> {
    if context.has_current_only_fields() {
        return Err(DecisionV5Error::InvalidContract);
    }
    let mut supplied = crate::supplied_v5::to_frozen_s(&context.supplied)?;
    context.retained_supplied_encoding.restore_s(&mut supplied);
    Ok(SuppliedSDecisionContextV5 {
        owner_state: context.owner_state.clone(),
        strategy: context.strategy.clone(),
        broker: context.broker.clone(),
        trigger: context.trigger.clone(),
        kernel_checkpoint: context.kernel_checkpoint.clone(),
        continuation: context.continuation.clone(),
        decision_time_unix_ms: context.decision_time_unix_ms,
        supplied,
    })
}

fn supplied_s_decision_context_v5_sha256(
    context: &DecisionContextV5,
) -> Result<[u8; 32], DecisionV5Error> {
    let frozen = supplied_s_context_from_current(context)?;
    let encoded = encode_bounded(
        SUPPLIED_S_DECISION_CONTEXT_V5_MAGIC,
        &frozen,
        MAX_DECISION_CONTEXT_V5_BYTES,
    )?;
    Ok(Sha256::digest(encoded).into())
}

fn convert_hundredths_context(context: HundredthsDecisionContextV5) -> DecisionContextV5 {
    DecisionContextV5 {
        broker_replay: None,
        owner_state: context.owner_state,
        strategy: context.strategy,
        broker: context.broker,
        trigger: context.trigger,
        kernel_checkpoint: context.kernel_checkpoint,
        continuation: context.continuation,
        decision_time_unix_ms: context.decision_time_unix_ms,
        supplied: SuppliedInputsV5::default(),
        current_weather: None,
        forecast_issuance: None,
        current_inputs: None,
        retained_supplied_encoding: Default::default(),
    }
}

/// The durable hundredths shape of a context. Only a context without supplied inputs has one.
fn hundredths_context_from_current(
    context: &DecisionContextV5,
) -> Result<HundredthsDecisionContextV5, DecisionV5Error> {
    if !context.supplied.is_absent() || context.has_current_only_fields() {
        return Err(DecisionV5Error::InvalidContract);
    }
    Ok(HundredthsDecisionContextV5 {
        owner_state: context.owner_state.clone(),
        strategy: context.strategy.clone(),
        broker: context.broker.clone(),
        trigger: context.trigger.clone(),
        kernel_checkpoint: context.kernel_checkpoint.clone(),
        continuation: context.continuation.clone(),
        decision_time_unix_ms: context.decision_time_unix_ms,
    })
}

fn hundredths_decision_context_v5_sha256(
    context: &DecisionContextV5,
) -> Result<[u8; 32], DecisionV5Error> {
    let hundredths = hundredths_context_from_current(context)?;
    let encoded = encode_bounded(
        HUNDREDTHS_DECISION_CONTEXT_V5_MAGIC,
        &hundredths,
        MAX_DECISION_CONTEXT_V5_BYTES,
    )?;
    Ok(Sha256::digest(encoded).into())
}

fn convert_legacy_context(
    legacy: LegacyDecisionContextV5,
) -> Result<DecisionContextV5, DecisionV5Error> {
    let positions = legacy
        .broker
        .positions
        .into_iter()
        .map(|position| {
            Ok(BrokerPositionV5 {
                market_id: position.market_id,
                side: position.side,
                quantity_hundredths: whole_contracts_to_hundredths(position.quantity)?,
                cost_basis_micros: position.cost_basis_micros,
                fees_micros: position.fees_micros,
            })
        })
        .collect::<Result<Vec<_>, DecisionV5Error>>()?;
    let orders = legacy
        .broker
        .orders
        .into_iter()
        .map(|order| {
            Ok(BrokerOrderV5 {
                command_id: order.command_id,
                intent_id: order.intent_id,
                order_id: order.order_id,
                provider_order_id: order.provider_order_id,
                provider_client_id: order.provider_client_id,
                market_id: order.market_id,
                action: order.action,
                side: order.side,
                order_type: order.order_type,
                quantity_hundredths: whole_contracts_to_hundredths(order.quantity)?,
                filled_quantity_hundredths: whole_contracts_to_hundredths(order.filled_quantity)?,
                remaining_quantity_hundredths: whole_contracts_to_hundredths(
                    order.remaining_quantity,
                )?,
                limit_price_micros: order.limit_price_micros,
                average_fill_price_micros: order.average_fill_price_micros,
                reserved_principal_micros: order.reserved_principal_micros,
                reserved_fee_micros: order.reserved_fee_micros,
                created_at_unix_ms: order.created_at_unix_ms,
                updated_at_unix_ms: order.updated_at_unix_ms,
                signal_type: order.signal_type,
                signal_metadata: order.signal_metadata,
                status: order.status,
                revision: order.revision,
            })
        })
        .collect::<Result<Vec<_>, DecisionV5Error>>()?;
    Ok(DecisionContextV5 {
        owner_state: legacy.owner_state,
        strategy: legacy.strategy,
        broker: BrokerDetailV5 {
            revision: legacy.broker.revision,
            reserved_cash_micros: legacy.broker.reserved_cash_micros,
            positions,
            orders,
        },
        trigger: convert_legacy_trigger(legacy.trigger)?,
        broker_replay: None,
        kernel_checkpoint: legacy.kernel_checkpoint,
        continuation: legacy.continuation,
        decision_time_unix_ms: legacy.decision_time_unix_ms,
        supplied: SuppliedInputsV5::default(),
        current_weather: None,
        forecast_issuance: None,
        current_inputs: None,
        retained_supplied_encoding: Default::default(),
    })
}

fn whole_contracts_to_hundredths(quantity: u64) -> Result<u64, DecisionV5Error> {
    quantity
        .checked_mul(100)
        .ok_or(DecisionV5Error::InvalidContract)
}

fn hundredths_to_whole_contracts(quantity_hundredths: u64) -> Result<u64, DecisionV5Error> {
    (quantity_hundredths % 100 == 0)
        .then_some(quantity_hundredths / 100)
        .ok_or(DecisionV5Error::InvalidContract)
}

fn convert_legacy_order_result(
    result: LegacyKernelOrderResultV5,
) -> Result<KernelOrderResultV5, DecisionV5Error> {
    Ok(KernelOrderResultV5 {
        order_id: result.order_id,
        status: result.status,
        filled_quantity_hundredths: whole_contracts_to_hundredths(result.filled_quantity)?,
        fill_price_micros: result.fill_price_micros,
        fee_cost_micros: result.fee_cost_micros,
        reason: result.reason,
    })
}

fn convert_legacy_return(
    value: LegacyBrokerCommandReturnV5,
) -> Result<BrokerCommandReturnV5, DecisionV5Error> {
    Ok(match value {
        LegacyBrokerCommandReturnV5::PlaceOrder(LegacyPlaceOrderReturnV5::Ok(result)) => {
            BrokerCommandReturnV5::PlaceOrder(PlaceOrderReturnV5::Ok(convert_legacy_order_result(
                result,
            )?))
        }
        LegacyBrokerCommandReturnV5::PlaceOrder(LegacyPlaceOrderReturnV5::Err(error)) => {
            BrokerCommandReturnV5::PlaceOrder(PlaceOrderReturnV5::Err(error))
        }
        LegacyBrokerCommandReturnV5::CancelOrder(value) => {
            BrokerCommandReturnV5::CancelOrder(value)
        }
        LegacyBrokerCommandReturnV5::CancelAllOrders(value) => {
            BrokerCommandReturnV5::CancelAllOrders(value)
        }
    })
}

fn convert_legacy_outcome(
    outcome: LegacyBrokerOutcomeV5,
) -> Result<BrokerOutcomeV5, DecisionV5Error> {
    Ok(BrokerOutcomeV5 {
        outcome_id: outcome.outcome_id,
        continuation_id: outcome.continuation_id,
        continuation_generation: outcome.continuation_generation,
        command_id: outcome.command_id,
        command_kind: outcome.command_kind,
        transition_sequence: outcome.transition_sequence,
        target_order_id: outcome.target_order_id,
        order_id: outcome.order_id,
        intent_id: outcome.intent_id,
        provider_order_id: outcome.provider_order_id,
        provider_client_id: outcome.provider_client_id,
        status: outcome.status,
        return_value: convert_legacy_return(outcome.return_value)?,
        requested_quantity_hundredths: whole_contracts_to_hundredths(outcome.requested_quantity)?,
        filled_quantity_hundredths: whole_contracts_to_hundredths(outcome.filled_quantity)?,
        remaining_quantity_hundredths: whole_contracts_to_hundredths(outcome.remaining_quantity)?,
        average_fill_price_micros: outcome.average_fill_price_micros,
        reason: outcome.reason,
        updated_at_unix_ms: outcome.updated_at_unix_ms,
        broker_revision: outcome.broker_revision,
    })
}

fn convert_legacy_trigger(trigger: LegacyTriggerV5) -> Result<TriggerV5, DecisionV5Error> {
    Ok(match trigger {
        LegacyTriggerV5::Owner(trigger) => TriggerV5::Owner(trigger),
        LegacyTriggerV5::BrokerState { broker_revision } => {
            TriggerV5::BrokerState { broker_revision }
        }
        LegacyTriggerV5::BrokerOutcome {
            outcome,
            originating_trigger,
        } => TriggerV5::BrokerOutcome {
            outcome: Box::new(convert_legacy_outcome(*outcome)?),
            originating_trigger,
        },
    })
}

fn convert_legacy_command(
    command: LegacyStrategyCommandV5,
) -> Result<StrategyCommandV5, DecisionV5Error> {
    Ok(match command {
        LegacyStrategyCommandV5::PlaceOrder(order) => StrategyCommandV5::PlaceOrder(PlaceOrderV5 {
            command_id: order.command_id,
            fence: order.fence,
            market_id: order.market_id,
            action: order.action,
            side: order.side,
            order_type: order.order_type,
            quantity_hundredths: whole_contracts_to_hundredths(order.quantity)?,
            limit_price_micros: order.limit_price_micros,
            market_price_cap_micros: order.market_price_cap_micros,
            expires_after_ms: order.expires_after_ms,
            reduce_only: order.reduce_only,
            provider_client_id: order.provider_client_id,
            signal_type: order.signal_type,
            signal_metadata: order.signal_metadata,
            metadata: order.metadata,
        }),
        LegacyStrategyCommandV5::CancelOrder {
            command_id,
            fence,
            order_id,
            expected_order_revision,
        } => StrategyCommandV5::CancelOrder {
            command_id,
            fence,
            order_id,
            expected_order_revision,
        },
        LegacyStrategyCommandV5::CancelAllOrders { command_id, fence } => {
            StrategyCommandV5::CancelAllOrders { command_id, fence }
        }
        LegacyStrategyCommandV5::ScheduleTimer {
            command_id,
            key,
            scheduled_at_epoch_ns,
            generation,
            semantics,
        } => StrategyCommandV5::ScheduleTimer {
            command_id,
            key,
            scheduled_at_epoch_ns,
            generation,
            semantics,
        },
        LegacyStrategyCommandV5::CancelTimer {
            command_id,
            key,
            generation,
        } => StrategyCommandV5::CancelTimer {
            command_id,
            key,
            generation,
        },
        LegacyStrategyCommandV5::Stop { command_id, reason } => {
            StrategyCommandV5::Stop { command_id, reason }
        }
    })
}

fn legacy_return_from_current(
    value: &BrokerCommandReturnV5,
) -> Result<LegacyBrokerCommandReturnV5, DecisionV5Error> {
    Ok(match value {
        BrokerCommandReturnV5::PlaceOrder(PlaceOrderReturnV5::Ok(result)) => {
            LegacyBrokerCommandReturnV5::PlaceOrder(LegacyPlaceOrderReturnV5::Ok(
                LegacyKernelOrderResultV5 {
                    order_id: result.order_id.clone(),
                    status: result.status,
                    filled_quantity: hundredths_to_whole_contracts(
                        result.filled_quantity_hundredths,
                    )?,
                    fill_price_micros: result.fill_price_micros,
                    fee_cost_micros: result.fee_cost_micros,
                    reason: result.reason.clone(),
                },
            ))
        }
        BrokerCommandReturnV5::PlaceOrder(PlaceOrderReturnV5::Err(error)) => {
            LegacyBrokerCommandReturnV5::PlaceOrder(LegacyPlaceOrderReturnV5::Err(error.clone()))
        }
        BrokerCommandReturnV5::CancelOrder(value) => {
            LegacyBrokerCommandReturnV5::CancelOrder(value.clone())
        }
        BrokerCommandReturnV5::CancelAllOrders(value) => {
            LegacyBrokerCommandReturnV5::CancelAllOrders(value.clone())
        }
    })
}

fn legacy_outcome_from_current(
    outcome: &BrokerOutcomeV5,
) -> Result<LegacyBrokerOutcomeV5, DecisionV5Error> {
    Ok(LegacyBrokerOutcomeV5 {
        outcome_id: outcome.outcome_id.clone(),
        continuation_id: outcome.continuation_id.clone(),
        continuation_generation: outcome.continuation_generation,
        command_id: outcome.command_id.clone(),
        command_kind: outcome.command_kind,
        transition_sequence: outcome.transition_sequence,
        target_order_id: outcome.target_order_id.clone(),
        order_id: outcome.order_id.clone(),
        intent_id: outcome.intent_id.clone(),
        provider_order_id: outcome.provider_order_id.clone(),
        provider_client_id: outcome.provider_client_id.clone(),
        status: outcome.status,
        return_value: legacy_return_from_current(&outcome.return_value)?,
        requested_quantity: hundredths_to_whole_contracts(outcome.requested_quantity_hundredths)?,
        filled_quantity: hundredths_to_whole_contracts(outcome.filled_quantity_hundredths)?,
        remaining_quantity: hundredths_to_whole_contracts(outcome.remaining_quantity_hundredths)?,
        average_fill_price_micros: outcome.average_fill_price_micros,
        reason: outcome.reason.clone(),
        updated_at_unix_ms: outcome.updated_at_unix_ms,
        broker_revision: outcome.broker_revision,
    })
}

fn legacy_trigger_from_current(trigger: &TriggerV5) -> Result<LegacyTriggerV5, DecisionV5Error> {
    Ok(match trigger {
        TriggerV5::Owner(trigger) => LegacyTriggerV5::Owner(trigger.clone()),
        TriggerV5::BrokerState { broker_revision } => LegacyTriggerV5::BrokerState {
            broker_revision: *broker_revision,
        },
        TriggerV5::BrokerOutcome {
            outcome,
            originating_trigger,
        } => LegacyTriggerV5::BrokerOutcome {
            outcome: Box::new(legacy_outcome_from_current(outcome)?),
            originating_trigger: originating_trigger.clone(),
        },
    })
}

fn legacy_command_from_current(
    command: &StrategyCommandV5,
) -> Result<LegacyStrategyCommandV5, DecisionV5Error> {
    Ok(match command {
        StrategyCommandV5::PlaceOrder(order) => {
            LegacyStrategyCommandV5::PlaceOrder(LegacyPlaceOrderV5 {
                command_id: order.command_id.clone(),
                fence: order.fence.clone(),
                market_id: order.market_id.clone(),
                action: order.action,
                side: order.side,
                order_type: order.order_type,
                quantity: hundredths_to_whole_contracts(order.quantity_hundredths)?,
                limit_price_micros: order.limit_price_micros,
                market_price_cap_micros: order.market_price_cap_micros,
                expires_after_ms: order.expires_after_ms,
                reduce_only: order.reduce_only,
                provider_client_id: order.provider_client_id.clone(),
                signal_type: order.signal_type.clone(),
                signal_metadata: order.signal_metadata.clone(),
                metadata: order.metadata.clone(),
            })
        }
        StrategyCommandV5::CancelOrder {
            command_id,
            fence,
            order_id,
            expected_order_revision,
        } => LegacyStrategyCommandV5::CancelOrder {
            command_id: command_id.clone(),
            fence: fence.clone(),
            order_id: order_id.clone(),
            expected_order_revision: *expected_order_revision,
        },
        StrategyCommandV5::CancelAllOrders { command_id, fence } => {
            LegacyStrategyCommandV5::CancelAllOrders {
                command_id: command_id.clone(),
                fence: fence.clone(),
            }
        }
        StrategyCommandV5::ScheduleTimer {
            command_id,
            key,
            scheduled_at_epoch_ns,
            generation,
            semantics,
        } => LegacyStrategyCommandV5::ScheduleTimer {
            command_id: command_id.clone(),
            key: key.clone(),
            scheduled_at_epoch_ns: *scheduled_at_epoch_ns,
            generation: generation.clone(),
            semantics: semantics.clone(),
        },
        StrategyCommandV5::CancelTimer {
            command_id,
            key,
            generation,
        } => LegacyStrategyCommandV5::CancelTimer {
            command_id: command_id.clone(),
            key: key.clone(),
            generation: generation.clone(),
        },
        StrategyCommandV5::Stop { command_id, reason } => LegacyStrategyCommandV5::Stop {
            command_id: command_id.clone(),
            reason: reason.clone(),
        },
    })
}

fn legacy_context_from_current(
    context: &DecisionContextV5,
) -> Result<LegacyDecisionContextV5, DecisionV5Error> {
    if !context.supplied.is_absent() || context.has_current_only_fields() {
        return Err(DecisionV5Error::InvalidContract);
    }
    let whole_contracts = hundredths_to_whole_contracts;
    let positions = context
        .broker
        .positions
        .iter()
        .map(|position| {
            Ok(LegacyBrokerPositionV5 {
                market_id: position.market_id.clone(),
                side: position.side,
                quantity: whole_contracts(position.quantity_hundredths)?,
                cost_basis_micros: position.cost_basis_micros,
                fees_micros: position.fees_micros,
            })
        })
        .collect::<Result<Vec<_>, DecisionV5Error>>()?;
    let orders = context
        .broker
        .orders
        .iter()
        .map(|order| {
            Ok(LegacyBrokerOrderV5 {
                command_id: order.command_id.clone(),
                intent_id: order.intent_id.clone(),
                order_id: order.order_id.clone(),
                provider_order_id: order.provider_order_id.clone(),
                provider_client_id: order.provider_client_id.clone(),
                market_id: order.market_id.clone(),
                action: order.action,
                side: order.side,
                order_type: order.order_type,
                quantity: whole_contracts(order.quantity_hundredths)?,
                filled_quantity: whole_contracts(order.filled_quantity_hundredths)?,
                remaining_quantity: whole_contracts(order.remaining_quantity_hundredths)?,
                limit_price_micros: order.limit_price_micros,
                average_fill_price_micros: order.average_fill_price_micros,
                reserved_principal_micros: order.reserved_principal_micros,
                reserved_fee_micros: order.reserved_fee_micros,
                created_at_unix_ms: order.created_at_unix_ms,
                updated_at_unix_ms: order.updated_at_unix_ms,
                signal_type: order.signal_type.clone(),
                signal_metadata: order.signal_metadata.clone(),
                status: order.status,
                revision: order.revision,
            })
        })
        .collect::<Result<Vec<_>, DecisionV5Error>>()?;
    Ok(LegacyDecisionContextV5 {
        owner_state: context.owner_state.clone(),
        strategy: context.strategy.clone(),
        broker: LegacyBrokerDetailV5 {
            revision: context.broker.revision,
            reserved_cash_micros: context.broker.reserved_cash_micros,
            positions,
            orders,
        },
        trigger: legacy_trigger_from_current(&context.trigger)?,
        kernel_checkpoint: context.kernel_checkpoint.clone(),
        continuation: context.continuation.clone(),
        decision_time_unix_ms: context.decision_time_unix_ms,
    })
}

fn legacy_decision_context_v5_sha256(
    context: &DecisionContextV5,
) -> Result<[u8; 32], DecisionV5Error> {
    let legacy = legacy_context_from_current(context)?;
    let encoded = encode_bounded(
        LEGACY_DECISION_CONTEXT_V5_MAGIC,
        &legacy,
        MAX_DECISION_CONTEXT_V5_BYTES,
    )?;
    Ok(Sha256::digest(encoded).into())
}

pub fn encode_decision_result_v5(result: &DecisionResultV5) -> Result<Vec<u8>, DecisionV5Error> {
    result.validate()?;
    encode_bounded(
        DECISION_RESULT_V5_MAGIC,
        result,
        MAX_DECISION_RESULT_V5_BYTES,
    )
}

pub fn decode_decision_result_v5(bytes: &[u8]) -> Result<DecisionResultV5, DecisionV5Error> {
    let result = if bytes.starts_with(DECISION_RESULT_V5_MAGIC) {
        decode_bounded(
            DECISION_RESULT_V5_MAGIC,
            bytes,
            MAX_DECISION_RESULT_V5_BYTES,
        )?
    } else if bytes.starts_with(LEGACY_DECISION_RESULT_V5_MAGIC) {
        let legacy: LegacyDecisionResultV5 = decode_bounded(
            LEGACY_DECISION_RESULT_V5_MAGIC,
            bytes,
            MAX_DECISION_RESULT_V5_BYTES,
        )?;
        convert_legacy_result(legacy)?
    } else {
        return Err(DecisionV5Error::Decode);
    };
    result.validate()?;
    Ok(result)
}

fn convert_legacy_result(
    result: LegacyDecisionResultV5,
) -> Result<DecisionResultV5, DecisionV5Error> {
    Ok(DecisionResultV5 {
        delivery_id: result.delivery_id,
        sleeve_identity: result.sleeve_identity,
        state_fence: result.state_fence,
        expected_broker_revision: result.expected_broker_revision,
        disposition: result.disposition,
        kernel_checkpoint: result.kernel_checkpoint,
        commands: result
            .commands
            .into_iter()
            .map(convert_legacy_command)
            .collect::<Result<Vec<_>, _>>()?,
        evidence: result.evidence,
        diagnostics: result.diagnostics,
    })
}

#[cfg(test)]
fn legacy_result_from_current(
    result: &DecisionResultV5,
) -> Result<LegacyDecisionResultV5, DecisionV5Error> {
    Ok(LegacyDecisionResultV5 {
        delivery_id: result.delivery_id.clone(),
        sleeve_identity: result.sleeve_identity.clone(),
        state_fence: result.state_fence.clone(),
        expected_broker_revision: result.expected_broker_revision,
        disposition: result.disposition.clone(),
        kernel_checkpoint: result.kernel_checkpoint.clone(),
        commands: result
            .commands
            .iter()
            .map(legacy_command_from_current)
            .collect::<Result<Vec<_>, _>>()?,
        evidence: result.evidence.clone(),
        diagnostics: result.diagnostics.clone(),
    })
}

pub fn decision_context_v5_sha256(
    context: &DecisionContextV5,
) -> Result<[u8; 32], DecisionV5Error> {
    let encoded = encode_decision_context_v5(context)?;
    Ok(Sha256::digest(encoded).into())
}

pub fn decision_result_v5_sha256(result: &DecisionResultV5) -> Result<[u8; 32], DecisionV5Error> {
    let encoded = encode_decision_result_v5(result)?;
    Ok(Sha256::digest(encoded).into())
}

fn validate_forecast_issuance(context: &DecisionContextV5) -> Result<(), DecisionV5Error> {
    let Some(stations) = &context.forecast_issuance else {
        return Ok(());
    };
    if context.retained_supplied_encoding.canonical_c
        || stations.len() != context.owner_state.stations.len()
    {
        return Err(DecisionV5Error::InvalidContract);
    }
    for (accepted, station) in stations.iter().zip(&context.owner_state.stations) {
        if accepted.station_id != station.identity.station_id
            || accepted.models.len() != station.forecast.models.len()
        {
            return Err(DecisionV5Error::InvalidContract);
        }
        for (issued, model) in accepted.models.iter().zip(&station.forecast.models) {
            if !issued.is_valid()
                || issued.model_id != model.model_id
                || issued.version != model.version || match model.issued_at_unix_ms {
                Some(at) => at != issued.at_unix_ns.div_euclid(1_000_000),
                None => {
                    issued.basis
                        != strategy_core_kernel::forecast::ForecastIssuanceBasis::TimestampedVersion
                }
            }
                || issued.station_generation > station.provider_cursor.connection_generation
                || (issued.station_generation == station.provider_cursor.connection_generation
                    && (issued.station_revision > station.revision
                        || issued.forecast_generation > station.forecast_meta.generation))
            {
                return Err(DecisionV5Error::InvalidContract);
            }
        }
    }
    Ok(())
}

fn validate_current_weather(context: &DecisionContextV5) -> Result<(), DecisionV5Error> {
    let Some(stations) = &context.current_weather else {
        return Ok(());
    };
    if context.retained_supplied_encoding.canonical_c
        || stations.len() != context.owner_state.stations.len()
        || stations
            .iter()
            .zip(&context.owner_state.stations)
            .any(|(weather, station)| weather.station_id != station.identity.station_id)
    {
        return Err(DecisionV5Error::InvalidContract);
    }
    for (station, owner) in stations.iter().zip(&context.owner_state.stations) {
        if !station.facts.values_are_valid() {
            return Err(DecisionV5Error::InvalidContract);
        }
        if station
            .facts
            .text_values()
            .any(|value| value.len() > crate::supplied_v5::MAX_SUPPLIED_TEXT_BYTES)
        {
            return Err(DecisionV5Error::BoundExceeded);
        }
        for fact in station.facts.fields.values() {
            let provenance = &fact.provenance;
            if provenance.owner_generation == 0
                || provenance.owner_revision == 0
                || provenance.owner_generation > owner.provider_cursor.connection_generation
                || (provenance.owner_generation == owner.provider_cursor.connection_generation
                    && provenance.owner_revision > owner.revision)
            {
                return Err(DecisionV5Error::InvalidContract);
            }
            if let Some(envelope) = &provenance.envelope {
                crate::supplied_v5::validate_envelope(envelope)?;
            }
        }
    }
    Ok(())
}

fn validate_scope(context: &DecisionContextV5) -> Result<(), DecisionV5Error> {
    let scope = &context.strategy;
    if !strictly_sorted(scope.parameters.iter().map(|(key, _)| key.as_str()))
        || !strictly_sorted(scope.market_ids.iter().map(String::as_str))
    {
        return Err(DecisionV5Error::NonCanonicalOrder);
    }
    if !valid_identifier(&scope.strategy_id)
        || !valid_identifier(&scope.binding_id)
        || !valid_text(&scope.profile, MAX_SHORT_TEXT_BYTES)
        || scope.parameters.len() > MAX_STRATEGY_PARAMETERS
        || scope.parameters.iter().any(|(key, value)| {
            !valid_identifier(key)
                || matches!(value, StrategyParameterValueV5::Decimal { scale, .. } if *scale > 18)
                || matches!(value, StrategyParameterValueV5::String(value) if !valid_text(value, MAX_REASON_BYTES))
        })
        || !valid_identifier(&scope.station_id)
        || !valid_identifier(&scope.event_ticker)
        || !valid_text(&scope.event_date, MAX_SHORT_TEXT_BYTES)
        || !valid_text(&scope.profile_and_calculator_digest, MAX_SHORT_TEXT_BYTES)
        || scope.market_ids.is_empty()
    {
        return Err(DecisionV5Error::InvalidContract);
    }
    if scope.profile != context.owner_state.opportunity.match_profile
        || context.owner_state.sleeve.sleeve_id
            != derive_sleeve_identity_v5(
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
        return Err(DecisionV5Error::InvalidContract);
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
        return Err(DecisionV5Error::InvalidContract);
    }
    Ok(())
}

fn validate_broker(context: &DecisionContextV5) -> Result<(), DecisionV5Error> {
    if context.broker.revision != context.owner_state.broker.revision
        || context.broker.revision != context.owner_state.fence.broker_revision
    {
        return Err(DecisionV5Error::InvalidContract);
    }
    validate_broker_parts(
        &context.strategy.market_ids,
        &context.broker,
        context.owner_state.broker.locally_reserved_cash,
        context.owner_state.broker.current_commitment,
    )
}

pub(crate) fn validate_broker_parts(
    market_ids: &[String],
    broker: &BrokerDetailV5,
    locally_reserved_cash: u64,
    current_commitment: u64,
) -> Result<(), DecisionV5Error> {
    if !strictly_sorted(
        broker
            .positions
            .iter()
            .map(|position| (position.market_id.as_str(), position.side)),
    ) || !strictly_sorted(broker.orders.iter().map(|order| order.order_id.as_str()))
    {
        return Err(DecisionV5Error::NonCanonicalOrder);
    }
    let owner_market_ids = market_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if broker.positions.len() > MAX_BROKER_POSITIONS || broker.orders.len() > MAX_BROKER_ORDERS {
        return Err(DecisionV5Error::InvalidContract);
    }
    if broker.positions.iter().any(|position| {
        position.quantity_hundredths == 0
            || position.quantity_hundredths > i64::MAX as u64
            || u128::from(position.cost_basis_micros)
                > maximum_quantity_value(position.quantity_hundredths)
            || !owner_market_ids.contains(position.market_id.as_str())
    }) || broker.orders.iter().any(|order| {
        let accounted = order
            .filled_quantity_hundredths
            .checked_add(order.remaining_quantity_hundredths);
        let quantities_valid = accounted == Some(order.quantity_hundredths)
            || (order.status == BrokerOrderStatusV5::Cancelled
                && order.remaining_quantity_hundredths == 0
                && accounted.is_some_and(|quantity| quantity <= order.quantity_hundredths));
        !valid_identifier(&order.command_id)
            || !valid_identifier(&order.intent_id)
            || !valid_identifier(&order.order_id)
            || !valid_optional_identifier(&order.provider_order_id)
            || !valid_identifier(&order.provider_client_id)
            || !owner_market_ids.contains(order.market_id.as_str())
            || order.quantity_hundredths == 0
            || order.quantity_hundredths > i64::MAX as u64
            || !quantities_valid
            || order
                .limit_price_micros
                .is_some_and(|price| price > MAX_PRICE_MICROS)
            || order
                .average_fill_price_micros
                .is_some_and(|price| price > MAX_PRICE_MICROS)
            || matches!(order.order_type, OrderTypeV5::Limit) && order.limit_price_micros.is_none()
            || matches!(order.order_type, OrderTypeV5::Market) && order.limit_price_micros.is_some()
            || !valid_order_reservation(order)
            || !valid_optional_text(&order.signal_type, MAX_SHORT_TEXT_BYTES)
            || !valid_optional_text(&order.signal_metadata, MAX_COMMAND_METADATA_BYTES)
    }) {
        return Err(DecisionV5Error::InvalidContract);
    }
    let reserved_cash = broker.orders.iter().fold(0_u128, |total, order| {
        total + u128::from(order.reserved_principal_micros) + u128::from(order.reserved_fee_micros)
    });
    let position_commitment = broker.positions.iter().fold(0_u128, |total, position| {
        total + u128::from(position.cost_basis_micros) + u128::from(position.fees_micros)
    });
    if reserved_cash != u128::from(broker.reserved_cash_micros)
        || reserved_cash > u128::from(locally_reserved_cash)
        || position_commitment + reserved_cash != u128::from(current_commitment)
    {
        return Err(DecisionV5Error::InvalidContract);
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

fn valid_order_reservation(order: &BrokerOrderV5) -> bool {
    if matches!(order.action, OrderActionV5::Sell)
        || matches!(
            order.status,
            BrokerOrderStatusV5::Filled
                | BrokerOrderStatusV5::Cancelled
                | BrokerOrderStatusV5::Expired
                | BrokerOrderStatusV5::Rejected
        )
    {
        return order.reserved_principal_micros == 0 && order.reserved_fee_micros == 0;
    }
    let maximum_notional = maximum_quantity_value(order.remaining_quantity_hundredths);
    let principal_is_exact = match order.limit_price_micros {
        Some(price) => exact_quantity_value(order.remaining_quantity_hundredths, price)
            .is_some_and(|principal| principal == u128::from(order.reserved_principal_micros)),
        None => u128::from(order.reserved_principal_micros) <= maximum_notional,
    };
    principal_is_exact && u128::from(order.reserved_fee_micros) <= maximum_notional
}

fn maximum_quantity_value(quantity_hundredths: u64) -> u128 {
    u128::from(quantity_hundredths) * u128::from(MAX_PRICE_MICROS) / 100
}

fn exact_quantity_value(quantity_hundredths: u64, price_micros: u64) -> Option<u128> {
    let product = u128::from(quantity_hundredths) * u128::from(price_micros);
    (product % 100 == 0).then_some(product / 100)
}

fn validate_trigger(context: &DecisionContextV5) -> Result<(), DecisionV5Error> {
    if matches!(context.trigger, TriggerV5::BrokerOutcome { .. }) != context.continuation.is_some()
    {
        return Err(DecisionV5Error::InvalidContract);
    }
    match &context.trigger {
        TriggerV5::Owner(trigger) => validate_owner_trigger(context, trigger),
        TriggerV5::BrokerState { broker_revision }
            if *broker_revision == context.broker.revision
                && matches!(context.owner_state.trigger, TriggerV4::Recovery) =>
        {
            Ok(())
        }
        TriggerV5::BrokerOutcome {
            outcome,
            originating_trigger,
        } => {
            validate_originating_context(context, originating_trigger)?;
            validate_broker_outcome(context, outcome)
        }
        _ => Err(DecisionV5Error::InvalidContract),
    }
}

fn validate_originating_context(
    context: &DecisionContextV5,
    originating_trigger: &OriginatingTriggerV5,
) -> Result<(), DecisionV5Error> {
    let commitment = context
        .continuation
        .as_ref()
        .ok_or(DecisionV5Error::InvalidContract)?;
    let mut originating = originating_context_v5(context, originating_trigger);
    if commitment.originating_delivery_id != originating.owner_state.delivery_id
        || (context.broker_replay.is_none()
            && commitment.expected_broker_revision != originating.broker.revision)
    {
        return Err(DecisionV5Error::InvalidContract);
    }
    let digest_matches = |original: &DecisionContextV5| match &context.broker_replay {
        Some(replay) => replay_origin_digest(original, replay.origin_encoding)
            .map(|digest| digest == commitment.originating_context_sha256),
        None => originating_context_digest_matches(original, commitment.originating_context_sha256),
    };
    if digest_matches(&originating)? {
        return Ok(());
    }
    // On the first invocation the originating context has no checkpoint, while the awaiting
    // result creates sequence 1 as its exact pre-event checkpoint. The digest disambiguates this
    // one bootstrap shape without weakening any later checkpoint binding.
    if commitment.pre_event_checkpoint.sequence == 1 {
        originating.kernel_checkpoint = None;
        if digest_matches(&originating)? {
            return Ok(());
        }
    }
    Err(DecisionV5Error::InvalidContract)
}

fn originating_context_digest_matches(
    context: &DecisionContextV5,
    expected: [u8; 32],
) -> Result<bool, DecisionV5Error> {
    if decision_context_v5_sha256(context)? == expected {
        return Ok(true);
    }
    // Historical digests cannot attest newly attached winner data.
    if context.has_current_only_fields() {
        return Ok(false);
    }
    match supplied_s_decision_context_v5_sha256(context) {
        Ok(digest) if digest == expected => return Ok(true),
        Ok(_) | Err(DecisionV5Error::InvalidContract) => {}
        Err(error) => return Err(error),
    }
    match hundredths_decision_context_v5_sha256(context) {
        Ok(digest) if digest == expected => return Ok(true),
        Ok(_) | Err(DecisionV5Error::InvalidContract) => {}
        Err(error) => return Err(error),
    }
    match legacy_decision_context_v5_sha256(context) {
        Ok(digest) => Ok(digest == expected),
        Err(DecisionV5Error::InvalidContract) => Ok(false),
        Err(error) => Err(error),
    }
}

fn originating_context_v5_sha256(context: &DecisionContextV5) -> Result<[u8; 32], DecisionV5Error> {
    match &context.trigger {
        TriggerV5::BrokerOutcome { .. } => context
            .continuation
            .as_ref()
            .map(|commitment| commitment.originating_context_sha256)
            .ok_or(DecisionV5Error::InvalidContract),
        TriggerV5::Owner(_) | TriggerV5::BrokerState { .. } => decision_context_v5_sha256(context),
    }
}

fn originating_context_v5(
    replay: &DecisionContextV5,
    originating_trigger: &OriginatingTriggerV5,
) -> DecisionContextV5 {
    let mut originating = replay.clone();
    originating.trigger = match originating_trigger {
        OriginatingTriggerV5::Owner(trigger) => TriggerV5::Owner(trigger.clone()),
        OriginatingTriggerV5::BrokerState { broker_revision } => TriggerV5::BrokerState {
            broker_revision: *broker_revision,
        },
    };
    originating.continuation = None;
    originating.broker_replay = None;
    originating
}

fn validate_owner_trigger(
    context: &DecisionContextV5,
    trigger: &OwnerTriggerV5,
) -> Result<(), DecisionV5Error> {
    if matches!(trigger, OwnerTriggerV5::CapturedWeather { .. }) {
        return context
            .current_inputs
            .as_ref()
            .and_then(|inputs| inputs.originating.as_ref())
            .ok_or(DecisionV5Error::InvalidContract)?
            .validate(context);
    }
    let owner = &context.owner_state.trigger;
    let valid = match (trigger, owner) {
        (
            OwnerTriggerV5::Observation {
                station_id,
                observed_at_unix_ms,
                component_revision,
                source_generation,
                source_sequence,
            },
            TriggerV4::Weather {
                station_id: owner_station,
                source_generation: owner_generation,
                source_sequence: owner_sequence,
            },
        ) => context.owner_state.stations.iter().any(|station| {
            station_id == owner_station
                && source_generation == owner_generation
                && source_sequence == owner_sequence
                && station.identity.station_id == *station_id
                && station.observation_meta.revision == *component_revision
                && station.observation.observed_at_unix_ms == *observed_at_unix_ms
        }),
        (
            OwnerTriggerV5::ForecastUpdated {
                station_id,
                emitted_at_unix_ms,
                component_revision,
                source_generation,
                source_sequence,
            },
            TriggerV4::Weather {
                station_id: owner_station,
                source_generation: owner_generation,
                source_sequence: owner_sequence,
            },
        ) => context.owner_state.stations.iter().any(|station| {
            station_id == owner_station
                && source_generation == owner_generation
                && source_sequence == owner_sequence
                && station.identity.station_id == *station_id
                && station.forecast_meta.revision == *component_revision
                && station.forecast_meta.updated_at_unix_ms == Some(*emitted_at_unix_ms)
        }),
        (
            OwnerTriggerV5::OracleScoresUpdated {
                station_id,
                emitted_at_unix_ms,
                component_revision,
                source_generation,
                source_sequence,
            },
            TriggerV4::Weather {
                station_id: owner_station,
                source_generation: owner_generation,
                source_sequence: owner_sequence,
            },
        ) => context.owner_state.stations.iter().any(|station| {
            station_id == owner_station
                && source_generation == owner_generation
                && source_sequence == owner_sequence
                && station.identity.station_id == *station_id
                && station.oracle_meta.revision == *component_revision
                && station.oracle_meta.updated_at_unix_ms == Some(*emitted_at_unix_ms)
        }),
        (
            OwnerTriggerV5::NewHigh {
                station_id,
                event_date,
                temperature_milli_c,
                observed_at_unix_ms,
                component_revision,
                source_generation,
                source_sequence,
            },
            TriggerV4::Weather {
                station_id: owner_station,
                source_generation: owner_generation,
                source_sequence: owner_sequence,
            },
        ) => context.owner_state.stations.iter().any(|station| {
            let high = station.extrema.high.as_ref();
            station_id == owner_station
                && source_generation == owner_generation
                && source_sequence == owner_sequence
                && station.identity.station_id == *station_id
                && station.extrema_meta.revision == *component_revision
                && event_date
                    .as_ref()
                    .is_none_or(|date| date == &station.climate_event_date)
                && high.map(|value| value.value_milli_c) == *temperature_milli_c
                && high.and_then(|value| value.observed_at_unix_ms) == Some(*observed_at_unix_ms)
        }),
        (
            OwnerTriggerV5::NewLow {
                station_id,
                event_date,
                temperature_milli_c,
                observed_at_unix_ms,
                component_revision,
                source_generation,
                source_sequence,
            },
            TriggerV4::Weather {
                station_id: owner_station,
                source_generation: owner_generation,
                source_sequence: owner_sequence,
            },
        ) => context.owner_state.stations.iter().any(|station| {
            let low = station.extrema.low.as_ref();
            station_id == owner_station
                && source_generation == owner_generation
                && source_sequence == owner_sequence
                && station.identity.station_id == *station_id
                && station.extrema_meta.revision == *component_revision
                && event_date
                    .as_ref()
                    .is_none_or(|date| date == &station.climate_event_date)
                && low.map(|value| value.value_milli_c) == *temperature_milli_c
                && low.and_then(|value| value.observed_at_unix_ms) == Some(*observed_at_unix_ms)
        }),
        (
            OwnerTriggerV5::WeatherEvent {
                station_id,
                episode_id,
                state,
                component_revision,
                source_generation,
                source_sequence,
            },
            TriggerV4::Weather {
                station_id: owner_station,
                source_generation: owner_generation,
                source_sequence: owner_sequence,
            },
        ) => context.owner_state.stations.iter().any(|station| {
            // The captured transition is bound by validate_supplied, independently of
            // current membership: an ended episode may have left state before FIFO drain.
            station_id == owner_station
                && source_generation == owner_generation
                && source_sequence == owner_sequence
                && station.identity.station_id == *station_id
                && station.weather_events_meta.revision == *component_revision
                && !episode_id.is_empty()
                && !state.is_empty()
        }),
        (
            OwnerTriggerV5::StationReport {
                station_id,
                report_id,
                report_type,
                report_revision,
                provider,
                source_generation,
                source_sequence,
            },
            TriggerV4::StationReport {
                station_id: owner_station,
                report_id: owner_report,
                report_type: owner_type,
                report_revision: owner_revision,
                provider: owner_provider,
                source_generation: owner_generation,
                source_sequence: owner_sequence,
            },
        ) => {
            station_id == owner_station
                && report_id == owner_report
                && report_type == owner_type
                && report_revision == owner_revision
                && provider == owner_provider
                && source_generation == owner_generation
                && source_sequence == owner_sequence
                && context.owner_state.stations.iter().any(|station| {
                    station.identity.station_id == *station_id
                        && station.reports.iter().any(|report| {
                            report.report_id == *report_id && report.revision == *report_revision
                        })
                })
        }
        (
            OwnerTriggerV5::MarketPrice {
                market_id,
                price_revision,
                emitted_at_unix_ms,
            },
            TriggerV4::MarketPrice {
                market_id: owner_market,
                price_revision: owner_revision,
            },
        ) => {
            market_id == owner_market
                && price_revision == owner_revision
                && context.owner_state.markets.iter().any(|market| {
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
        (
            OwnerTriggerV5::Timer {
                key,
                scheduled_at_epoch_ns,
                generation,
            },
            TriggerV4::Timer { key: owner_key },
        ) => {
            key == owner_key
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
        (OwnerTriggerV5::Bootstrap, TriggerV4::Bootstrap)
        | (OwnerTriggerV5::Recovery, TriggerV4::Recovery) => true,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(DecisionV5Error::InvalidContract)
    }
}

/// Binds a present supplied block to the owner projection and the originating trigger.
fn validate_supplied(context: &DecisionContextV5) -> Result<(), DecisionV5Error> {
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
        return Err(DecisionV5Error::InvalidContract);
    }
    let trigger = context.originating_owner_trigger();
    let expects_event = trigger.is_some_and(OwnerTriggerV5::carries_supplied_event);
    let event = supplied.originating_event.as_ref();
    if expects_event != event.is_some() {
        return Err(DecisionV5Error::InvalidContract);
    }
    let (Some(trigger), Some(event)) = (trigger, event) else {
        return Ok(());
    };
    let bound = match (trigger, event) {
        (OwnerTriggerV5::Observation { station_id, .. }, SuppliedEventV5::Observation(event)) => {
            event.station_id == *station_id
        }
        (
            OwnerTriggerV5::StationReport {
                station_id,
                report_id,
                report_type,
                report_revision,
                ..
            },
            SuppliedEventV5::Report(event),
        ) => {
            event.station_id == *station_id
                && event.report_id == *report_id
                && event.report_type == *report_type
                && event.report_revision.unwrap_or(0) == *report_revision
        }
        (OwnerTriggerV5::NewHigh { station_id, .. }, SuppliedEventV5::Extreme(event)) => {
            event.station_id == *station_id && event.kind == ExtremeKindV5::High
        }
        (OwnerTriggerV5::NewLow { station_id, .. }, SuppliedEventV5::Extreme(event)) => {
            event.station_id == *station_id && event.kind == ExtremeKindV5::Low
        }
        (
            OwnerTriggerV5::WeatherEvent {
                station_id,
                episode_id,
                state,
                ..
            },
            SuppliedEventV5::WeatherEvent(event),
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
        Err(DecisionV5Error::InvalidContract)
    }
}

fn validate_broker_outcome(
    context: &DecisionContextV5,
    outcome: &BrokerOutcomeV5,
) -> Result<(), DecisionV5Error> {
    let commitment = context
        .continuation
        .as_ref()
        .ok_or(DecisionV5Error::InvalidContract)?;
    validate_continuation_commitment(context, commitment)?;
    if commitment.continuation_id != outcome.continuation_id
        || commitment.continuation_generation != outcome.continuation_generation
        || commitment.command_id != outcome.command_id
        || commitment.expected_broker_revision > outcome.broker_revision
    {
        return Err(DecisionV5Error::InvalidContract);
    }
    validate_broker_outcome_fields(context.admission_broker(), outcome)
}

pub(crate) fn cancelled_order_matches(broker: &BrokerDetailV5, outcome: &BrokerOutcomeV5) -> bool {
    outcome.command_kind == BrokerCommandKindV5::CancelOrder
        && outcome.status == BrokerOutcomeStatusV5::Cancelled
        && matches!(
            &outcome.return_value,
            BrokerCommandReturnV5::CancelOrder(CancelOrderReturnV5::Ok(true))
        )
        && outcome.target_order_id == outcome.order_id
        && broker.orders.iter().any(|order| {
            outcome.order_id.as_ref() == Some(&order.order_id)
                && order.status == BrokerOrderStatusV5::Cancelled
                && outcome.requested_quantity_hundredths == order.quantity_hundredths
                && outcome.filled_quantity_hundredths == order.filled_quantity_hundredths
                && outcome.remaining_quantity_hundredths == order.remaining_quantity_hundredths
                && outcome.average_fill_price_micros == order.average_fill_price_micros
        })
}

pub(crate) fn validate_broker_outcome_fields(
    broker: &BrokerDetailV5,
    outcome: &BrokerOutcomeV5,
) -> Result<(), DecisionV5Error> {
    let accounted_quantity = outcome
        .filled_quantity_hundredths
        .checked_add(outcome.remaining_quantity_hundredths);
    // A confirmed cancellation removes unfilled open quantity, not the original request.
    // Accept that residual only against the exact cancelled order in the returned state.
    let quantities_valid = accounted_quantity == Some(outcome.requested_quantity_hundredths)
        || (accounted_quantity
            .is_some_and(|quantity| quantity <= outcome.requested_quantity_hundredths)
            && cancelled_order_matches(broker, outcome));
    if !valid_identifier(&outcome.outcome_id)
        || !valid_identifier(&outcome.continuation_id)
        || outcome.continuation_generation == 0
        || !valid_identifier(&outcome.command_id)
        || outcome.transition_sequence == 0
        || !valid_optional_identifier(&outcome.target_order_id)
        || !valid_optional_identifier(&outcome.order_id)
        || !valid_optional_identifier(&outcome.intent_id)
        || !valid_optional_identifier(&outcome.provider_order_id)
        || !valid_optional_identifier(&outcome.provider_client_id)
        || outcome.requested_quantity_hundredths > i64::MAX as u64
        || !quantities_valid
        || outcome
            .average_fill_price_micros
            .is_some_and(|price| price > MAX_PRICE_MICROS)
        || !valid_optional_text(&outcome.reason, MAX_REASON_BYTES)
    {
        return Err(DecisionV5Error::InvalidContract);
    }
    let matching_order = outcome.order_id.as_ref().and_then(|order_id| {
        broker
            .orders
            .iter()
            .find(|order| order.order_id == *order_id)
    });
    match (&outcome.command_kind, &outcome.return_value) {
        (
            BrokerCommandKindV5::PlaceOrder,
            BrokerCommandReturnV5::PlaceOrder(PlaceOrderReturnV5::Ok(result)),
        ) => {
            if outcome.target_order_id.is_some()
                || !valid_identifier(&result.order_id)
                || !valid_text(&result.reason, MAX_REASON_BYTES)
                || result.filled_quantity_hundredths != outcome.filled_quantity_hundredths
                || result.fill_price_micros > MAX_PRICE_MICROS
                || u128::from(result.fee_cost_micros)
                    > maximum_quantity_value(result.filled_quantity_hundredths)
                || (result.filled_quantity_hundredths == 0
                    && (result.fill_price_micros != 0
                        || result.fee_cost_micros != 0
                        || outcome.average_fill_price_micros.is_some()))
                || (result.filled_quantity_hundredths > 0
                    && outcome.average_fill_price_micros != Some(result.fill_price_micros))
                || outcome.order_id.as_ref() != Some(&result.order_id)
                || matching_order.is_some_and(|order| order.command_id != outcome.command_id)
                || !kernel_order_status_matches_outcome(result.status, outcome.status)
            {
                return Err(DecisionV5Error::InvalidContract);
            }
        }
        (
            BrokerCommandKindV5::PlaceOrder,
            BrokerCommandReturnV5::PlaceOrder(PlaceOrderReturnV5::Err(error)),
        ) => {
            if outcome.target_order_id.is_some()
                || outcome.order_id.is_some()
                || !valid_kernel_error(error)
                || !matches!(
                    outcome.status,
                    BrokerOutcomeStatusV5::Rejected | BrokerOutcomeStatusV5::RecoveryRequired
                )
            {
                return Err(DecisionV5Error::InvalidContract);
            }
        }
        (
            BrokerCommandKindV5::CancelOrder,
            BrokerCommandReturnV5::CancelOrder(CancelOrderReturnV5::Ok(cancelled)),
        ) => {
            let identity_is_valid = if *cancelled {
                outcome.target_order_id == outcome.order_id && matching_order.is_some()
            } else if let Some(order) = matching_order {
                outcome.target_order_id.as_ref() == Some(&order.order_id)
                    && matches!(
                        order.status,
                        BrokerOrderStatusV5::Filled
                            | BrokerOrderStatusV5::Cancelled
                            | BrokerOrderStatusV5::Expired
                            | BrokerOrderStatusV5::Rejected
                    )
            } else {
                outcome.order_id.is_none()
                    && matches!(outcome.status, BrokerOutcomeStatusV5::Rejected)
            };
            if outcome.target_order_id.is_none() || !identity_is_valid {
                return Err(DecisionV5Error::InvalidContract);
            }
        }
        (
            BrokerCommandKindV5::CancelOrder,
            BrokerCommandReturnV5::CancelOrder(CancelOrderReturnV5::Err(error)),
        ) => {
            if outcome.target_order_id.is_none()
                || !valid_kernel_error(error)
                || !matches!(
                    outcome.status,
                    BrokerOutcomeStatusV5::Rejected | BrokerOutcomeStatusV5::RecoveryRequired
                )
            {
                return Err(DecisionV5Error::InvalidContract);
            }
        }
        (
            BrokerCommandKindV5::CancelAllOrders,
            BrokerCommandReturnV5::CancelAllOrders(CancelAllOrdersReturnV5::Ok {
                cancelled_order_ids,
            }),
        ) => {
            if outcome.target_order_id.is_some()
                || outcome.order_id.is_some()
                || cancelled_order_ids.len() > MAX_BROKER_ORDERS
                || !strictly_sorted(cancelled_order_ids.iter().map(String::as_str))
                || cancelled_order_ids.iter().any(|order_id| {
                    !broker
                        .orders
                        .iter()
                        .any(|order| order.order_id == *order_id)
                })
            {
                return Err(DecisionV5Error::InvalidContract);
            }
        }
        (
            BrokerCommandKindV5::CancelAllOrders,
            BrokerCommandReturnV5::CancelAllOrders(CancelAllOrdersReturnV5::Err(error)),
        ) => {
            if outcome.target_order_id.is_some()
                || outcome.order_id.is_some()
                || !valid_kernel_error(error)
                || !matches!(
                    outcome.status,
                    BrokerOutcomeStatusV5::Rejected | BrokerOutcomeStatusV5::RecoveryRequired
                )
            {
                return Err(DecisionV5Error::InvalidContract);
            }
        }
        _ => return Err(DecisionV5Error::InvalidContract),
    }
    if let Some(order) = matching_order {
        if outcome
            .intent_id
            .as_ref()
            .is_some_and(|id| id != &order.intent_id)
            || outcome
                .provider_client_id
                .as_ref()
                .is_some_and(|id| id != &order.provider_client_id)
            || outcome
                .provider_order_id
                .as_ref()
                .zip(order.provider_order_id.as_ref())
                .is_some_and(|(outcome_id, order_id)| outcome_id != order_id)
        {
            return Err(DecisionV5Error::InvalidContract);
        }
    }
    Ok(())
}

fn valid_kernel_error(error: &KernelBrokerErrorV5) -> bool {
    valid_identifier(&error.code) && valid_text(&error.message, MAX_REASON_BYTES)
}

fn kernel_order_status_matches_outcome(
    order: KernelOrderStatusV5,
    outcome: BrokerOutcomeStatusV5,
) -> bool {
    matches!(
        (order, outcome),
        (KernelOrderStatusV5::Filled, BrokerOutcomeStatusV5::Filled)
            | (
                KernelOrderStatusV5::Partial,
                BrokerOutcomeStatusV5::PartiallyFilled
            )
            | (
                KernelOrderStatusV5::Pending,
                BrokerOutcomeStatusV5::DurablyAccepted
                    | BrokerOutcomeStatusV5::Dispatched
                    | BrokerOutcomeStatusV5::Resting
            )
            | (
                KernelOrderStatusV5::Rejected,
                BrokerOutcomeStatusV5::Rejected
            )
            | (
                KernelOrderStatusV5::Cancelled,
                BrokerOutcomeStatusV5::Cancelled
            )
    )
}

fn validate_kernel_checkpoint_shape(
    checkpoint: &KernelCheckpointV5,
) -> Result<(), DecisionV5Error> {
    if checkpoint.state.len() > MAX_KERNEL_CHECKPOINT_BYTES {
        return Err(DecisionV5Error::BoundExceeded);
    }
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
        || checkpoint.state_sha256 != kernel_checkpoint_v5_sha256(checkpoint)
    {
        return Err(DecisionV5Error::InvalidContract);
    }
    Ok(())
}

fn validate_kernel_checkpoint(
    strategy: &StrategyScopeV5,
    checkpoint: &KernelCheckpointV5,
) -> Result<(), DecisionV5Error> {
    validate_kernel_checkpoint_shape(checkpoint)?;
    if checkpoint.strategy_id != strategy.strategy_id
        || checkpoint.strategy_profile != strategy.profile
        || checkpoint.profile_and_calculator_digest != strategy.profile_and_calculator_digest
    {
        return Err(DecisionV5Error::InvalidContract);
    }
    Ok(())
}

fn validate_checkpoint_transition(
    context: &DecisionContextV5,
    result: &DecisionResultV5,
) -> Result<(), DecisionV5Error> {
    let output = result.kernel_checkpoint.as_ref();
    if let Some(checkpoint) = output {
        validate_kernel_checkpoint(&context.strategy, checkpoint)?;
    }
    match &result.disposition {
        DecisionDispositionV5::Completed => {
            let checkpoint = output.ok_or(DecisionV5Error::InvalidContract)?;
            let expected_sequence = context
                .kernel_checkpoint
                .as_ref()
                .map_or(Some(1), |previous| previous.sequence.checked_add(1))
                .ok_or(DecisionV5Error::InvalidContract)?;
            if checkpoint.sequence != expected_sequence {
                return Err(DecisionV5Error::InvalidContract);
            }
        }
        DecisionDispositionV5::AwaitingBrokerOutcome { .. } => match &context.kernel_checkpoint {
            Some(previous) if output != Some(previous) => {
                return Err(DecisionV5Error::InvalidContract);
            }
            None if output.is_none_or(|checkpoint| checkpoint.sequence != 1) => {
                return Err(DecisionV5Error::InvalidContract);
            }
            _ => {}
        },
        DecisionDispositionV5::Rejected => {
            if output != context.kernel_checkpoint.as_ref() {
                return Err(DecisionV5Error::InvalidContract);
            }
        }
    }
    Ok(())
}

fn validate_continuation_commitment(
    context: &DecisionContextV5,
    commitment: &ContinuationCommitmentV5,
) -> Result<(), DecisionV5Error> {
    let sleeve = &context.owner_state.sleeve;
    if !valid_identifier(&commitment.originating_delivery_id)
        || !valid_identifier(&commitment.sleeve_identity)
        || commitment.sleeve_identity != sleeve.sleeve_id
        || commitment.sleeve_incarnation != sleeve.incarnation
        || commitment.process_attempt != sleeve.process_attempt
        || commitment.route_epoch != sleeve.route_epoch
        || !valid_identifier(&commitment.continuation_id)
        || commitment.continuation_generation == 0
        || !valid_identifier(&commitment.command_id)
        || commitment.command_sha256 == [0; 32]
        || commitment.originating_context_sha256 == [0; 32]
        || context.kernel_checkpoint.as_ref() != Some(&commitment.pre_event_checkpoint)
    {
        return Err(DecisionV5Error::InvalidContract);
    }
    validate_kernel_checkpoint(&context.strategy, &commitment.pre_event_checkpoint)
}

fn validate_command(command: &StrategyCommandV5) -> Result<(), DecisionV5Error> {
    if !valid_identifier(command.command_id()) {
        return Err(DecisionV5Error::InvalidContract);
    }
    match command {
        StrategyCommandV5::PlaceOrder(order) => {
            validate_fence(&order.fence)?;
            if !valid_identifier(&order.market_id)
                || order.quantity_hundredths == 0
                || order.quantity_hundredths > i64::MAX as u64
                || !valid_identifier(&order.provider_client_id)
                || order.metadata.len() > MAX_COMMAND_METADATA_BYTES
                || !valid_optional_text(&order.signal_type, MAX_SHORT_TEXT_BYTES)
                || !valid_optional_text(&order.signal_metadata, MAX_COMMAND_METADATA_BYTES)
                || order.expires_after_ms.is_some_and(|ttl| ttl <= 0)
                || !valid_place_order_prices(order)
            {
                return Err(DecisionV5Error::InvalidContract);
            }
        }
        StrategyCommandV5::CancelOrder {
            fence, order_id, ..
        } => {
            validate_fence(fence)?;
            if !valid_identifier(order_id) {
                return Err(DecisionV5Error::InvalidContract);
            }
        }
        StrategyCommandV5::CancelAllOrders { fence, .. } => validate_fence(fence)?,
        StrategyCommandV5::ScheduleTimer {
            key,
            generation,
            semantics,
            ..
        } => {
            if !valid_identifier(key)
                || !valid_identifier(generation)
                || semantics.len() > MAX_TIMER_SEMANTICS_BYTES_V5
            {
                return Err(DecisionV5Error::BoundExceeded);
            }
        }
        StrategyCommandV5::CancelTimer {
            key, generation, ..
        } => {
            if !valid_identifier(key) || !valid_identifier(generation) {
                return Err(DecisionV5Error::InvalidContract);
            }
        }
        StrategyCommandV5::Stop { reason, .. } if !valid_text(reason, MAX_REASON_BYTES) => {
            return Err(DecisionV5Error::BoundExceeded);
        }
        StrategyCommandV5::Stop { .. } => {}
    }
    Ok(())
}

fn valid_place_order_prices(order: &PlaceOrderV5) -> bool {
    match (order.action, order.order_type) {
        (OrderActionV5::Buy, OrderTypeV5::Market) => {
            order.limit_price_micros.is_none()
                && order
                    .market_price_cap_micros
                    .is_none_or(|price| (1..=MAX_PRICE_MICROS).contains(&price))
        }
        (OrderActionV5::Sell, OrderTypeV5::Market) => {
            order.limit_price_micros.is_none() && order.market_price_cap_micros.is_none()
        }
        (_, OrderTypeV5::Limit) => {
            order
                .limit_price_micros
                .is_some_and(|price| price <= MAX_PRICE_MICROS)
                && order.market_price_cap_micros.is_none()
        }
    }
}

fn validate_fence(fence: &CommandFenceV5) -> Result<(), DecisionV5Error> {
    if !valid_identifier(&fence.continuation_id) || fence.continuation_generation == 0 {
        return Err(DecisionV5Error::InvalidContract);
    }
    Ok(())
}

fn encode_bounded<T: Encode>(
    magic: &[u8; 8],
    value: &T,
    max_bytes: usize,
) -> Result<Vec<u8>, DecisionV5Error> {
    let config = bincode::config::standard()
        .with_big_endian()
        .with_variable_int_encoding();
    let payload = bincode::encode_to_vec(value, config).map_err(|_| DecisionV5Error::Encode)?;
    let total = magic
        .len()
        .checked_add(payload.len())
        .ok_or(DecisionV5Error::BoundExceeded)?;
    if total > max_bytes {
        return Err(DecisionV5Error::BoundExceeded);
    }
    let mut bytes = Vec::with_capacity(total);
    bytes.extend_from_slice(magic);
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

fn decode_bounded<T: Decode<()>>(
    magic: &[u8; 8],
    bytes: &[u8],
    max_bytes: usize,
) -> Result<T, DecisionV5Error> {
    if bytes.len() > max_bytes || !bytes.starts_with(magic) {
        return Err(DecisionV5Error::Decode);
    }
    let config = bincode::config::standard()
        .with_big_endian()
        .with_variable_int_encoding();
    let (value, consumed) = bincode::decode_from_slice(&bytes[magic.len()..], config)
        .map_err(|_| DecisionV5Error::Decode)?;
    if consumed != bytes.len() - magic.len() {
        return Err(DecisionV5Error::TrailingBytes);
    }
    Ok(value)
}

fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    output
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_IDENTIFIER_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
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

fn unique<T: Ord>(values: impl IntoIterator<Item = T>) -> Result<(), DecisionV5Error> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value) {
            return Err(DecisionV5Error::DuplicateIdentity);
        }
    }
    Ok(())
}

pub fn derive_sleeve_identity_v5(
    strategy_id: &str,
    binding_id: &str,
    venue_id: &str,
    opportunity_id: &str,
) -> String {
    let mut digest = Sha256::new();
    digest.update(SLEEVE_ID_DOMAIN);
    for component in [strategy_id, binding_id, venue_id, opportunity_id] {
        let length = u16::try_from(component.len()).expect("bounded V5 identity fits in u16");
        digest.update(length.to_be_bytes());
        digest.update(component.as_bytes());
    }
    hex_digest(&digest.finalize())
}

pub fn decision_fence_v5_sha256(context: &DecisionContextV5) -> Result<[u8; 32], DecisionV5Error> {
    context.validate()?;
    let mut hasher = Sha256::new();
    hasher.update(
        decision_fence_v4_sha256(&context.owner_state.fence)
            .map_err(DecisionV5Error::V4)?
            .as_bytes(),
    );
    hasher.update(context.strategy.strategy_id.as_bytes());
    hasher.update(context.strategy.binding_id.as_bytes());
    let config = bincode::config::standard()
        .with_big_endian()
        .with_variable_int_encoding();
    hasher.update(
        bincode::encode_to_vec(&context.strategy.parameters, config)
            .map_err(|_| DecisionV5Error::Encode)?,
    );
    hasher.update(context.broker.revision.to_be_bytes());
    match &context.kernel_checkpoint {
        Some(checkpoint) => {
            hasher.update([1]);
            hasher.update(checkpoint.state_sha256);
        }
        None => hasher.update([0]),
    }
    hasher.update(context.decision_time_unix_ms.to_be_bytes());
    Ok(hasher.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision_v4::{
        DecisionContextV4, FenceV4, MarketComparisonV4, MarketIdentityV4, MarketV4, OpportunityV4,
        StationIdentityV4, StationV4, SupervisorV4,
    };

    fn checkpoint(sequence: u64, state: &[u8]) -> KernelCheckpointV5 {
        let mut checkpoint = KernelCheckpointV5 {
            codec_profile: "dsm-reaction-v10-checkpoint".to_owned(),
            codec_version: 1,
            strategy_id: "dsm_reaction_v10".to_owned(),
            strategy_profile: "daily-high".to_owned(),
            profile_and_calculator_digest: "profile-calculator-digest".to_owned(),
            sequence,
            state: state.to_vec(),
            state_sha256: [0; 32],
        };
        checkpoint.state_sha256 = kernel_checkpoint_v5_sha256(&checkpoint);
        checkpoint
    }

    fn context() -> DecisionContextV5 {
        let market_id = "KXHIGHTSEA-26AUG30-T80".to_owned();
        let digest = "profile-calculator-digest".to_owned();
        let owner_state = DecisionContextV4 {
            delivery_id: "delivery.daily.1".to_owned(),
            sleeve: SupervisorV4 {
                sleeve_id: derive_sleeve_identity_v5(
                    "dsm_reaction_v10",
                    "binding.daily.v10",
                    "kalshi",
                    "KXHIGHTSEA-26AUG30",
                ),
                incarnation: 1,
                process_attempt: 1,
                route_epoch: 1,
            },
            trigger: TriggerV4::Recovery,
            fence: FenceV4 {
                profile_and_calculator_digest: digest.clone(),
                route_plan_sha256: [7; 32],
                broker_revision: 9,
                ..Default::default()
            },
            config: crate::decision_v4::ConfigV4 {
                profile_and_calculator_digest: digest.clone(),
                ..Default::default()
            },
            stations: vec![StationV4 {
                climate_event_date: "2026-08-30".to_owned(),
                climate_day_start_utc_unix_ms: 1,
                climate_day_end_utc_unix_ms: 2,
                identity: StationIdentityV4 {
                    station_id: "KSEA".to_owned(),
                    logical_location: "KSEA".to_owned(),
                    timezone: "America/Los_Angeles".to_owned(),
                    ..Default::default()
                },
                ..Default::default()
            }],
            opportunity: OpportunityV4 {
                opportunity_id: "KXHIGHTSEA-26AUG30".to_owned(),
                venue_id: "kalshi".to_owned(),
                match_profile: "daily-high".to_owned(),
                market_ids: vec![market_id.clone()],
                contributor_stations: vec!["KSEA".to_owned()],
                ..Default::default()
            },
            markets: vec![MarketV4 {
                identity: MarketIdentityV4 {
                    market_id: market_id.clone(),
                    opportunity_id: "KXHIGHTSEA-26AUG30".to_owned(),
                    event_ticker: "KXHIGHTSEA-26AUG30".to_owned(),
                    ..Default::default()
                },
                revision: 3,
                minutetemp_comparison: Some(MarketComparisonV4 {
                    event_date: "2026-08-30".to_owned(),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            broker: crate::decision_v4::BrokerV4 {
                revision: 9,
                locally_reserved_cash: 600_000,
                current_commitment: 1_800_000,
                ..Default::default()
            },
            delivered_at_monotonic_ns: 1,
            hard_expires_at_monotonic_ns: 2,
            ..Default::default()
        };
        DecisionContextV5 {
            owner_state,
            trigger: TriggerV5::Owner(OwnerTriggerV5::Recovery),
            kernel_checkpoint: Some(checkpoint(1, b"durable-kernel-state")),
            continuation: None,
            strategy: StrategyScopeV5 {
                strategy_id: "dsm_reaction_v10".to_owned(),
                binding_id: "binding.daily.v10".to_owned(),
                profile: "daily-high".to_owned(),
                station_id: "KSEA".to_owned(),
                event_ticker: "KXHIGHTSEA-26AUG30".to_owned(),
                event_date: "2026-08-30".to_owned(),
                market_ids: vec![market_id.clone()],
                parameters: vec![],
                profile_and_calculator_digest: digest,
            },
            broker: BrokerDetailV5 {
                revision: 9,
                reserved_cash_micros: 600_000,
                positions: vec![BrokerPositionV5 {
                    market_id: market_id.clone(),
                    side: ContractSideV5::Yes,
                    quantity_hundredths: 200,
                    cost_basis_micros: 1_200_000,
                    fees_micros: 0,
                }],
                orders: vec![BrokerOrderV5 {
                    command_id: "command.daily.1".to_owned(),
                    intent_id: "intent.daily.1".to_owned(),
                    order_id: "order.daily.1".to_owned(),
                    provider_order_id: Some("paper-order-1".to_owned()),
                    provider_client_id: "dsm-v10-ksea-20260830".to_owned(),
                    market_id,
                    action: OrderActionV5::Buy,
                    side: ContractSideV5::Yes,
                    order_type: OrderTypeV5::Limit,
                    quantity_hundredths: 300,
                    filled_quantity_hundredths: 200,
                    remaining_quantity_hundredths: 100,
                    limit_price_micros: Some(600_000),
                    average_fill_price_micros: Some(590_000),
                    reserved_principal_micros: 600_000,
                    reserved_fee_micros: 0,
                    created_at_unix_ms: Some(1),
                    updated_at_unix_ms: Some(2),
                    signal_type: Some("dsm_reaction_v10".to_owned()),
                    signal_metadata: Some("{}".to_owned()),
                    status: BrokerOrderStatusV5::PartiallyFilled,
                    revision: 2,
                }],
            },
            decision_time_unix_ms: 1_788_062_400_000,
            supplied: SuppliedInputsV5::default(),
            current_weather: None,
            forecast_issuance: None,
            current_inputs: None,
            retained_supplied_encoding: Default::default(),
            broker_replay: None,
        }
    }

    fn awaiting_result() -> DecisionResultV5 {
        DecisionResultV5 {
            delivery_id: "delivery.daily.1".to_owned(),
            sleeve_identity: derive_sleeve_identity_v5(
                "dsm_reaction_v10",
                "binding.daily.v10",
                "kalshi",
                "KXHIGHTSEA-26AUG30",
            ),
            state_fence: "fence".to_owned(),
            expected_broker_revision: 9,
            disposition: DecisionDispositionV5::AwaitingBrokerOutcome {
                continuation_id: "continuation.daily.1".to_owned(),
                continuation_generation: 1,
                awaited_command_id: "command.daily.2".to_owned(),
            },
            kernel_checkpoint: Some(checkpoint(1, b"durable-kernel-state")),
            commands: vec![StrategyCommandV5::PlaceOrder(PlaceOrderV5 {
                command_id: "command.daily.2".to_owned(),
                fence: CommandFenceV5 {
                    continuation_id: "continuation.daily.1".to_owned(),
                    continuation_generation: 1,
                    expected_broker_revision: 9,
                },
                market_id: "KXHIGHTSEA-26AUG30-T80".to_owned(),
                action: OrderActionV5::Buy,
                side: ContractSideV5::No,
                order_type: OrderTypeV5::Limit,
                quantity_hundredths: 300,
                limit_price_micros: Some(400_000),
                market_price_cap_micros: None,
                expires_after_ms: Some(30_000),
                reduce_only: false,
                provider_client_id: "dsm-v10-ksea-20260830-2".to_owned(),
                signal_type: Some("dsm_reaction_v10".to_owned()),
                signal_metadata: Some("{}".to_owned()),
                metadata: vec![],
            })],
            evidence: vec![],
            diagnostics: vec![],
        }
    }

    #[test]
    fn v5_round_trip_preserves_exact_side_aware_broker_snapshot() {
        let context = context();
        let encoded = encode_decision_context_v5(&context).unwrap();
        assert!(encoded.starts_with(DECISION_CONTEXT_V5_MAGIC));
        assert!(!encoded.starts_with(LEGACY_DECISION_CONTEXT_V5_MAGIC));
        assert_eq!(decode_decision_context_v5(&encoded).unwrap(), context);
    }

    fn encode_legacy_context_for_test(context: &LegacyDecisionContextV5) -> Vec<u8> {
        encode_bounded(
            LEGACY_DECISION_CONTEXT_V5_MAGIC,
            context,
            MAX_DECISION_CONTEXT_V5_BYTES,
        )
        .unwrap()
    }

    #[test]
    fn v5_decodes_durable_legacy_whole_contract_contexts_into_hundredths() {
        let context = context();
        let legacy = legacy_context_from_current(&context).unwrap();
        let encoded = encode_legacy_context_for_test(&legacy);

        let decoded = decode_decision_context_v5(&encoded).unwrap();
        assert_eq!(decoded, context);
        assert_eq!(decoded.broker.positions[0].quantity_hundredths, 200);
        assert_eq!(decoded.broker.orders[0].quantity_hundredths, 300);
        assert_eq!(decoded.broker.orders[0].filled_quantity_hundredths, 200);
        assert_eq!(decoded.broker.orders[0].remaining_quantity_hundredths, 100);
    }

    #[test]
    fn v5_legacy_decoder_rejects_unrepresentable_hundredths_quantities() {
        let legacy = legacy_context_from_current(&context()).unwrap();
        let unrepresentable_whole_quantity = u64::MAX / 100 + 1;

        let mut position_overflow = legacy.clone();
        position_overflow.broker.positions[0].quantity = unrepresentable_whole_quantity;
        assert_eq!(
            decode_decision_context_v5(&encode_legacy_context_for_test(&position_overflow)),
            Err(DecisionV5Error::InvalidContract)
        );

        let mut original_overflow = legacy.clone();
        original_overflow.broker.orders[0].quantity = unrepresentable_whole_quantity;
        assert_eq!(
            decode_decision_context_v5(&encode_legacy_context_for_test(&original_overflow)),
            Err(DecisionV5Error::InvalidContract)
        );

        let mut filled_overflow = legacy.clone();
        filled_overflow.broker.orders[0].filled_quantity = unrepresentable_whole_quantity;
        assert_eq!(
            decode_decision_context_v5(&encode_legacy_context_for_test(&filled_overflow)),
            Err(DecisionV5Error::InvalidContract)
        );

        let mut remaining_overflow = legacy;
        remaining_overflow.broker.orders[0].remaining_quantity = unrepresentable_whole_quantity;
        assert_eq!(
            decode_decision_context_v5(&encode_legacy_context_for_test(&remaining_overflow)),
            Err(DecisionV5Error::InvalidContract)
        );
    }

    fn fractional_context() -> DecisionContextV5 {
        let mut context = context();
        context.broker.positions[0].quantity_hundredths = 1;
        context.broker.positions[0].cost_basis_micros = 5_000;
        let order = &mut context.broker.orders[0];
        order.quantity_hundredths = 250;
        order.filled_quantity_hundredths = 125;
        order.remaining_quantity_hundredths = 125;
        order.limit_price_micros = Some(400_000);
        order.reserved_principal_micros = 500_000;
        context.broker.reserved_cash_micros = 500_000;
        context.owner_state.broker.locally_reserved_cash = 500_000;
        context.owner_state.broker.current_commitment = 505_000;
        context
    }

    #[test]
    fn v5_accepts_exact_fractional_broker_state_and_explicit_hundredths_commands() {
        let mut context = fractional_context();
        context.validate().unwrap();
        let encoded = encode_decision_context_v5(&context).unwrap();
        let decoded = decode_decision_context_v5(&encoded).unwrap();
        assert_eq!(decoded.broker.positions[0].quantity_hundredths, 1);
        assert_eq!(decoded.broker.orders[0].filled_quantity_hundredths, 125);
        assert_eq!(decoded.broker.orders[0].quantity_hundredths, 250);
        assert_eq!(decoded.broker.positions[0].average_entry_price(), 0.5);
        let StrategyCommandV5::PlaceOrder(command) = &awaiting_result().commands[0] else {
            panic!("expected place order");
        };
        assert_eq!(command.quantity_hundredths, 300);

        context.broker.orders[0].limit_price_micros = Some(400_001);
        assert_eq!(context.validate(), Err(DecisionV5Error::InvalidContract));
    }

    #[test]
    fn v5_outcome_replay_accepts_a_durable_legacy_originating_context_digest() {
        let replay_context = exact_replay_context();
        let legacy_replay = legacy_context_from_current(&replay_context).unwrap();
        let legacy_bytes = encode_legacy_context_for_test(&legacy_replay);
        assert_eq!(
            decode_decision_context_v5(&legacy_bytes).unwrap(),
            replay_context
        );

        let mut replay = exact_replay_context();
        let originating_trigger = match &replay.trigger {
            TriggerV5::BrokerOutcome {
                originating_trigger,
                ..
            } => originating_trigger.clone(),
            _ => panic!("expected broker outcome"),
        };
        let originating = originating_context_v5(&replay, &originating_trigger);
        replay
            .continuation
            .as_mut()
            .unwrap()
            .originating_context_sha256 = legacy_decision_context_v5_sha256(&originating).unwrap();

        replay.validate().unwrap();
    }

    fn awaiting_result_for(context: &DecisionContextV5) -> DecisionResultV5 {
        let mut result = awaiting_result();
        result.delivery_id = context.owner_state.delivery_id.clone();
        result.sleeve_identity = context.owner_state.sleeve.sleeve_id.clone();
        result.expected_broker_revision = context.broker.revision;
        result.state_fence = hex_digest(&decision_fence_v5_sha256(context).unwrap());
        result
    }

    fn capped_market_result() -> DecisionResultV5 {
        let mut result = awaiting_result();
        let StrategyCommandV5::PlaceOrder(order) = &mut result.commands[0] else {
            panic!("fixture command must be place order");
        };
        order.order_type = OrderTypeV5::Market;
        order.limit_price_micros = None;
        order.market_price_cap_micros = Some(990_000);
        order.expires_after_ms = None;
        result
    }

    #[test]
    fn v5_result_round_trip_preserves_fenced_continuation() {
        let result = awaiting_result();
        let encoded = encode_decision_result_v5(&result).unwrap();
        assert!(encoded.starts_with(DECISION_RESULT_V5_MAGIC));
        assert!(!encoded.starts_with(LEGACY_DECISION_RESULT_V5_MAGIC));
        assert_eq!(decode_decision_result_v5(&encoded).unwrap(), result);
    }

    #[test]
    fn v5_decodes_durable_legacy_whole_contract_results_and_matches_command_digest() {
        let result = awaiting_result();
        let legacy = legacy_result_from_current(&result).unwrap();
        let encoded = encode_bounded(
            LEGACY_DECISION_RESULT_V5_MAGIC,
            &legacy,
            MAX_DECISION_RESULT_V5_BYTES,
        )
        .unwrap();
        let decoded = decode_decision_result_v5(&encoded).unwrap();
        assert_eq!(decoded, result);

        let legacy_command = legacy_command_from_current(&result.commands[0]).unwrap();
        let legacy_digest = command_digest(&legacy_command).unwrap();
        assert_ne!(
            strategy_command_v5_sha256(&decoded.commands[0]).unwrap(),
            legacy_digest
        );
        assert!(strategy_command_v5_digest_matches(&decoded.commands[0], legacy_digest).unwrap());

        let mut overflowing = legacy_result_from_current(&result).unwrap();
        let LegacyStrategyCommandV5::PlaceOrder(order) = &mut overflowing.commands[0] else {
            unreachable!();
        };
        order.quantity = u64::MAX / 100 + 1;
        let bytes = encode_bounded(
            LEGACY_DECISION_RESULT_V5_MAGIC,
            &overflowing,
            MAX_DECISION_RESULT_V5_BYTES,
        )
        .unwrap();
        assert_eq!(
            decode_decision_result_v5(&bytes),
            Err(DecisionV5Error::InvalidContract)
        );
    }

    #[test]
    fn v5_commands_and_outcomes_round_trip_fractional_hundredths() {
        for quantity_hundredths in [1, 125, 250] {
            let mut result = awaiting_result();
            let StrategyCommandV5::PlaceOrder(order) = &mut result.commands[0] else {
                unreachable!();
            };
            order.quantity_hundredths = quantity_hundredths;
            let decoded =
                decode_decision_result_v5(&encode_decision_result_v5(&result).unwrap()).unwrap();
            let StrategyCommandV5::PlaceOrder(order) = &decoded.commands[0] else {
                unreachable!();
            };
            assert_eq!(order.quantity_hundredths, quantity_hundredths);
        }

        let mut context = exact_replay_context();
        let TriggerV5::BrokerOutcome { outcome, .. } = &mut context.trigger else {
            unreachable!();
        };
        outcome.requested_quantity_hundredths = 250;
        outcome.filled_quantity_hundredths = 125;
        outcome.remaining_quantity_hundredths = 125;
        outcome.status = BrokerOutcomeStatusV5::PartiallyFilled;
        outcome.average_fill_price_micros = Some(590_000);
        let BrokerCommandReturnV5::PlaceOrder(PlaceOrderReturnV5::Ok(result)) =
            &mut outcome.return_value
        else {
            unreachable!();
        };
        result.status = KernelOrderStatusV5::Partial;
        result.filled_quantity_hundredths = 125;
        result.fill_price_micros = 590_000;
        context.validate().unwrap();
        let decoded =
            decode_decision_context_v5(&encode_decision_context_v5(&context).unwrap()).unwrap();
        let TriggerV5::BrokerOutcome { outcome, .. } = decoded.trigger else {
            unreachable!();
        };
        assert_eq!(outcome.requested_quantity_hundredths, 250);
        assert_eq!(outcome.filled_quantity_hundredths, 125);
        assert_eq!(outcome.remaining_quantity_hundredths, 125);
    }

    #[test]
    fn v5_context_aware_validation_builds_exact_continuation_commitment() {
        let context = context();
        let result = awaiting_result_for(&context);
        validate_decision_result_v5(&context, &result).unwrap();
        let commitment = continuation_commitment_v5(&context, &result)
            .unwrap()
            .unwrap();
        assert_eq!(commitment.command_id, "command.daily.2");
        assert_eq!(
            commitment.sleeve_identity,
            context.owner_state.sleeve.sleeve_id
        );
        assert_ne!(commitment.command_sha256, [0; 32]);
        assert_eq!(
            commitment.originating_context_sha256,
            decision_context_v5_sha256(&context).unwrap()
        );
        assert_eq!(
            commitment.pre_event_checkpoint,
            context.kernel_checkpoint.clone().unwrap()
        );
    }

    #[test]
    fn v5_first_invocation_outcome_replays_origin_without_an_input_checkpoint() {
        let mut origin = context();
        origin.kernel_checkpoint = None;
        let result = awaiting_result_for(&origin);
        validate_decision_result_v5(&origin, &result).unwrap();
        let commitment = continuation_commitment_v5(&origin, &result)
            .unwrap()
            .unwrap();
        let originating_trigger = match &origin.trigger {
            TriggerV5::Owner(trigger) => OriginatingTriggerV5::Owner(trigger.clone()),
            _ => unreachable!(),
        };
        let mut replay = origin.clone();
        replay.kernel_checkpoint = result.kernel_checkpoint.clone();
        replay.continuation = Some(commitment);
        replay.trigger = TriggerV5::BrokerOutcome {
            outcome: Box::new(resting_outcome_for_awaited_command()),
            originating_trigger: Box::new(originating_trigger),
        };
        replay.validate().unwrap();
    }

    #[test]
    fn v5_awaiting_result_must_preserve_the_exact_pre_event_checkpoint() {
        let context = context();
        let mut result = awaiting_result_for(&context);
        result.kernel_checkpoint = Some(checkpoint(2, b"mutated-before-outcome"));
        result.state_fence = hex_digest(&decision_fence_v5_sha256(&context).unwrap());
        assert_eq!(
            validate_decision_result_v5(&context, &result),
            Err(DecisionV5Error::InvalidContract)
        );
    }

    #[test]
    fn v5_completed_result_advances_checkpoint_sequence_once() {
        let context = context();
        let mut result = awaiting_result_for(&context);
        result.commands.clear();
        result.disposition = DecisionDispositionV5::Completed;
        result.kernel_checkpoint = Some(checkpoint(2, b"post-event-state"));
        validate_decision_result_v5(&context, &result).unwrap();
        result.kernel_checkpoint = Some(checkpoint(3, b"skipped-sequence"));
        assert_eq!(
            validate_decision_result_v5(&context, &result),
            Err(DecisionV5Error::InvalidContract)
        );
    }

    #[test]
    fn v5_checkpoint_is_bounded_and_digest_protected() {
        let mut result = awaiting_result();
        let mut oversized = checkpoint(1, &[1]);
        oversized.state = vec![1; MAX_KERNEL_CHECKPOINT_BYTES + 1];
        oversized.state_sha256 = kernel_checkpoint_v5_sha256(&oversized);
        result.kernel_checkpoint = Some(oversized);
        assert_eq!(
            encode_decision_result_v5(&result),
            Err(DecisionV5Error::BoundExceeded)
        );

        let mut context = context();
        context.kernel_checkpoint.as_mut().unwrap().state.push(0);
        assert_eq!(context.validate(), Err(DecisionV5Error::InvalidContract));
    }

    #[test]
    fn v5_context_aware_validation_rejects_market_outside_exact_scope() {
        let context = context();
        let mut result = awaiting_result_for(&context);
        let StrategyCommandV5::PlaceOrder(order) = &mut result.commands[0] else {
            panic!("fixture command must be place order");
        };
        order.market_id = "KXHIGHTDEN-26AUG30-T80".to_owned();
        assert_eq!(
            validate_decision_result_v5(&context, &result),
            Err(DecisionV5Error::InvalidContract)
        );
    }

    #[test]
    fn v5_capped_market_buy_round_trip_and_hash_bind_the_authoritative_cap() {
        let result = capped_market_result();
        let bytes = encode_decision_result_v5(&result).unwrap();
        assert_eq!(decode_decision_result_v5(&bytes).unwrap(), result);

        let capped_command = &result.commands[0];
        let capped_hash = strategy_command_v5_sha256(capped_command).unwrap();
        let mut changed = capped_command.clone();
        let StrategyCommandV5::PlaceOrder(order) = &mut changed else {
            panic!("fixture command must be place order");
        };
        order.market_price_cap_micros = Some(980_000);
        assert_ne!(strategy_command_v5_sha256(&changed).unwrap(), capped_hash);
    }

    #[test]
    fn v5_rejects_malformed_market_price_caps() {
        let invalid_shapes = [
            (OrderActionV5::Buy, OrderTypeV5::Market, None, Some(0)),
            (
                OrderActionV5::Buy,
                OrderTypeV5::Market,
                None,
                Some(MAX_PRICE_MICROS + 1),
            ),
            (
                OrderActionV5::Buy,
                OrderTypeV5::Limit,
                Some(400_000),
                Some(400_000),
            ),
            (
                OrderActionV5::Sell,
                OrderTypeV5::Market,
                None,
                Some(400_000),
            ),
        ];

        for (action, order_type, limit_price_micros, market_price_cap_micros) in invalid_shapes {
            let mut result = awaiting_result();
            let StrategyCommandV5::PlaceOrder(order) = &mut result.commands[0] else {
                panic!("fixture command must be place order");
            };
            order.action = action;
            order.order_type = order_type;
            order.limit_price_micros = limit_price_micros;
            order.market_price_cap_micros = market_price_cap_micros;
            assert_eq!(
                encode_decision_result_v5(&result),
                Err(DecisionV5Error::InvalidContract)
            );
        }
    }

    #[test]
    fn v5_result_rejects_duplicate_command_identity() {
        let mut result = awaiting_result();
        result.commands.push(result.commands[0].clone());
        assert_eq!(
            encode_decision_result_v5(&result),
            Err(DecisionV5Error::DuplicateIdentity)
        );
    }

    #[test]
    fn v5_result_rejects_unawaited_economic_command() {
        let mut result = awaiting_result();
        result.disposition = DecisionDispositionV5::Completed;
        assert_eq!(
            encode_decision_result_v5(&result),
            Err(DecisionV5Error::InvalidContract)
        );
    }

    #[test]
    fn v5_rejects_noncanonical_broker_ordering() {
        let mut context = context();
        let mut second = context.broker.orders[0].clone();
        second.command_id = "command.daily.0".to_owned();
        second.intent_id = "intent.daily.0".to_owned();
        second.order_id = "order.daily.0".to_owned();
        second.provider_order_id = Some("paper-order-0".to_owned());
        second.provider_client_id = "dsm-v10-ksea-20260830-0".to_owned();
        context.broker.orders.push(second);
        assert_eq!(context.validate(), Err(DecisionV5Error::NonCanonicalOrder));
    }

    #[test]
    fn v5_rejects_strategy_scope_not_bound_to_owner_projection() {
        let mut context = context();
        context.strategy.station_id = "KDEN".to_owned();
        assert_eq!(context.validate(), Err(DecisionV5Error::InvalidContract));
    }

    fn partial_fill_outcome() -> BrokerOutcomeV5 {
        BrokerOutcomeV5 {
            outcome_id: "outcome.daily.1".to_owned(),
            continuation_id: "continuation.daily.1".to_owned(),
            continuation_generation: 1,
            command_id: "command.daily.1".to_owned(),
            command_kind: BrokerCommandKindV5::PlaceOrder,
            transition_sequence: 1,
            target_order_id: None,
            order_id: Some("order.daily.1".to_owned()),
            intent_id: Some("intent.daily.1".to_owned()),
            provider_order_id: Some("paper-order-1".to_owned()),
            provider_client_id: Some("dsm-v10-ksea-20260830".to_owned()),
            status: BrokerOutcomeStatusV5::PartiallyFilled,
            return_value: BrokerCommandReturnV5::PlaceOrder(PlaceOrderReturnV5::Ok(
                KernelOrderResultV5 {
                    order_id: "order.daily.1".to_owned(),
                    status: KernelOrderStatusV5::Partial,
                    filled_quantity_hundredths: 200,
                    fill_price_micros: 590_000,
                    fee_cost_micros: 10_000,
                    reason: "partial fill".to_owned(),
                },
            )),
            requested_quantity_hundredths: 300,
            filled_quantity_hundredths: 200,
            remaining_quantity_hundredths: 100,
            average_fill_price_micros: Some(590_000),
            reason: None,
            updated_at_unix_ms: 2,
            broker_revision: 9,
        }
    }

    fn resting_outcome_for_awaited_command() -> BrokerOutcomeV5 {
        BrokerOutcomeV5 {
            outcome_id: "outcome.daily.2".to_owned(),
            continuation_id: "continuation.daily.1".to_owned(),
            continuation_generation: 1,
            command_id: "command.daily.2".to_owned(),
            command_kind: BrokerCommandKindV5::PlaceOrder,
            transition_sequence: 1,
            target_order_id: None,
            order_id: Some("order.daily.2".to_owned()),
            intent_id: Some("intent.daily.2".to_owned()),
            provider_order_id: Some("paper-order-2".to_owned()),
            provider_client_id: Some("dsm-v10-ksea-20260830-2".to_owned()),
            status: BrokerOutcomeStatusV5::Resting,
            return_value: BrokerCommandReturnV5::PlaceOrder(PlaceOrderReturnV5::Ok(
                KernelOrderResultV5 {
                    order_id: "order.daily.2".to_owned(),
                    status: KernelOrderStatusV5::Pending,
                    filled_quantity_hundredths: 0,
                    fill_price_micros: 0,
                    fee_cost_micros: 0,
                    reason: "resting".to_owned(),
                },
            )),
            requested_quantity_hundredths: 300,
            filled_quantity_hundredths: 0,
            remaining_quantity_hundredths: 300,
            average_fill_price_micros: None,
            reason: None,
            updated_at_unix_ms: 3,
            broker_revision: 10,
        }
    }

    fn install_outcome(context: &mut DecisionContextV5, outcome: BrokerOutcomeV5) {
        let originating_context_sha256 = decision_context_v5_sha256(context).unwrap();
        let originating_trigger = match &context.trigger {
            TriggerV5::Owner(trigger) => OriginatingTriggerV5::Owner(trigger.clone()),
            TriggerV5::BrokerState { broker_revision } => OriginatingTriggerV5::BrokerState {
                broker_revision: *broker_revision,
            },
            TriggerV5::BrokerOutcome { .. } => panic!("test context already has an outcome"),
        };
        context.continuation = Some(ContinuationCommitmentV5 {
            originating_delivery_id: context.owner_state.delivery_id.clone(),
            sleeve_identity: context.owner_state.sleeve.sleeve_id.clone(),
            sleeve_incarnation: context.owner_state.sleeve.incarnation,
            process_attempt: context.owner_state.sleeve.process_attempt,
            route_epoch: context.owner_state.sleeve.route_epoch,
            continuation_id: outcome.continuation_id.clone(),
            continuation_generation: outcome.continuation_generation,
            command_id: outcome.command_id.clone(),
            command_sha256: [1; 32],
            expected_broker_revision: context.broker.revision,
            originating_context_sha256,
            pre_event_checkpoint: context.kernel_checkpoint.clone().unwrap(),
        });
        context.trigger = TriggerV5::BrokerOutcome {
            outcome: Box::new(outcome),
            originating_trigger: Box::new(originating_trigger),
        };
    }

    fn exact_replay_context() -> DecisionContextV5 {
        let original = context();
        let result = awaiting_result_for(&original);
        let commitment = continuation_commitment_v5(&original, &result)
            .unwrap()
            .unwrap();
        let mut replay = original;
        replay.continuation = Some(commitment);
        replay.trigger = TriggerV5::BrokerOutcome {
            outcome: Box::new(resting_outcome_for_awaited_command()),
            originating_trigger: Box::new(OriginatingTriggerV5::Owner(OwnerTriggerV5::Recovery)),
        };
        replay
    }

    #[test]
    fn v5_outcome_replay_binds_the_exact_canonical_originating_context() {
        let original = context();
        let replay = exact_replay_context();
        let commitment = replay.continuation.clone().unwrap();
        assert_eq!(
            commitment.originating_context_sha256,
            decision_context_v5_sha256(&original).unwrap()
        );
        replay.validate().unwrap();
        let encoded = encode_decision_context_v5(&replay).unwrap();
        assert_eq!(decode_decision_context_v5(&encoded).unwrap(), replay);
        assert_eq!(replay.owner_state, original.owner_state);
        assert_eq!(replay.strategy, original.strategy);
        assert_eq!(replay.broker, original.broker);
        assert_eq!(replay.decision_time_unix_ms, original.decision_time_unix_ms);

        let mut chained_result = awaiting_result_for(&replay);
        chained_result.state_fence = hex_digest(&decision_fence_v5_sha256(&replay).unwrap());
        let chained = continuation_commitment_v5(&replay, &chained_result)
            .unwrap()
            .unwrap();
        assert_eq!(
            chained.originating_context_sha256,
            commitment.originating_context_sha256
        );
    }

    #[test]
    fn v5_outcome_replay_rejects_changed_originating_trigger() {
        let original = context();
        let result = awaiting_result_for(&original);
        let commitment = continuation_commitment_v5(&original, &result)
            .unwrap()
            .unwrap();
        let mut replay = original;
        replay.continuation = Some(commitment);
        replay.trigger = TriggerV5::BrokerOutcome {
            outcome: Box::new(resting_outcome_for_awaited_command()),
            originating_trigger: Box::new(OriginatingTriggerV5::Owner(OwnerTriggerV5::Bootstrap)),
        };
        assert_eq!(replay.validate(), Err(DecisionV5Error::InvalidContract));
    }

    #[test]
    fn v5_outcome_replay_rejects_changed_originating_context_hash() {
        let mut replay = exact_replay_context();
        replay
            .continuation
            .as_mut()
            .unwrap()
            .originating_context_sha256 = [9; 32];
        assert_eq!(replay.validate(), Err(DecisionV5Error::InvalidContract));
    }

    #[test]
    fn v5_outcome_replay_rejects_changed_originating_delivery_or_broker_revision() {
        let mut changed_delivery = exact_replay_context();
        changed_delivery
            .continuation
            .as_mut()
            .unwrap()
            .originating_delivery_id = "delivery.unrelated".to_owned();
        assert_eq!(
            changed_delivery.validate(),
            Err(DecisionV5Error::InvalidContract)
        );

        let mut changed_revision = exact_replay_context();
        changed_revision
            .continuation
            .as_mut()
            .unwrap()
            .expected_broker_revision += 1;
        assert_eq!(
            changed_revision.validate(),
            Err(DecisionV5Error::InvalidContract)
        );
    }

    #[test]
    fn v5_round_trip_preserves_exact_broker_transition_identity() {
        let mut context = context();
        install_outcome(&mut context, partial_fill_outcome());
        let encoded = encode_decision_context_v5(&context).unwrap();
        assert_eq!(decode_decision_context_v5(&encoded).unwrap(), context);
    }

    #[test]
    fn v5_broker_outcome_restores_checkpoint_from_exact_originating_context() {
        let mut context = context();
        install_outcome(&mut context, partial_fill_outcome());
        context.validate().unwrap();
        assert_eq!(
            context.kernel_checkpoint.as_ref(),
            context
                .continuation
                .as_ref()
                .map(|commitment| &commitment.pre_event_checkpoint)
        );

        context.owner_state.sleeve.process_attempt += 1;
        assert_eq!(context.validate(), Err(DecisionV5Error::InvalidContract));
    }

    #[test]
    fn v5_broker_outcome_requires_exact_quantity_conservation() {
        let mut context = context();
        let mut outcome = partial_fill_outcome();
        outcome.requested_quantity_hundredths = 4;
        install_outcome(&mut context, outcome);
        assert_eq!(context.validate(), Err(DecisionV5Error::InvalidContract));
    }

    #[test]
    fn v5_broker_outcome_cannot_claim_an_unrelated_place_command() {
        let mut context = context();
        let mut outcome = partial_fill_outcome();
        outcome.command_id = "command.daily.unrelated".to_owned();
        install_outcome(&mut context, outcome);
        assert_eq!(context.validate(), Err(DecisionV5Error::InvalidContract));
    }

    #[test]
    fn v5_parameters_must_be_canonically_ordered() {
        let mut context = context();
        context.strategy.parameters = vec![
            ("z".to_owned(), StrategyParameterValueV5::U64(1)),
            ("a".to_owned(), StrategyParameterValueV5::U64(2)),
        ];
        assert_eq!(context.validate(), Err(DecisionV5Error::NonCanonicalOrder));
    }

    #[test]
    fn v5_accepts_exact_market_position_and_order_bounds() {
        let mut context = context();
        context.owner_state.markets.clear();
        context.owner_state.opportunity.market_ids.clear();
        context.strategy.market_ids.clear();
        context.broker.positions.clear();
        context.broker.orders.clear();
        for index in 0..crate::decision_v4::MAX_MARKETS {
            let market_id = format!("KXHIGHTSEA-26AUG30-T{index:03}");
            context
                .owner_state
                .opportunity
                .market_ids
                .push(market_id.clone());
            context.strategy.market_ids.push(market_id.clone());
            context.owner_state.markets.push(MarketV4 {
                identity: MarketIdentityV4 {
                    market_id: market_id.clone(),
                    opportunity_id: "KXHIGHTSEA-26AUG30".to_owned(),
                    event_ticker: "KXHIGHTSEA-26AUG30".to_owned(),
                    ..Default::default()
                },
                minutetemp_comparison: Some(MarketComparisonV4 {
                    event_date: "2026-08-30".to_owned(),
                    ..Default::default()
                }),
                ..Default::default()
            });
            for side in [ContractSideV5::Yes, ContractSideV5::No] {
                context.broker.positions.push(BrokerPositionV5 {
                    market_id: market_id.clone(),
                    side,
                    quantity_hundredths: 100,
                    cost_basis_micros: 500_000,
                    fees_micros: 1_000,
                });
            }
        }
        for index in 0..MAX_BROKER_ORDERS {
            let market_id = context.strategy.market_ids[index / 2].clone();
            context.broker.orders.push(BrokerOrderV5 {
                command_id: format!("command.daily.{index:03}"),
                intent_id: format!("intent.daily.{index:03}"),
                order_id: format!("order.daily.{index:03}"),
                provider_order_id: Some(format!("paper-order-{index:03}")),
                provider_client_id: format!("dsm-v10-ksea-{index:03}"),
                market_id,
                action: OrderActionV5::Buy,
                side: ContractSideV5::Yes,
                order_type: OrderTypeV5::Limit,
                quantity_hundredths: 100,
                filled_quantity_hundredths: 0,
                remaining_quantity_hundredths: 100,
                limit_price_micros: Some(500_000),
                average_fill_price_micros: None,
                reserved_principal_micros: 500_000,
                reserved_fee_micros: 0,
                created_at_unix_ms: Some(1),
                updated_at_unix_ms: Some(2),
                signal_type: None,
                signal_metadata: None,
                status: BrokerOrderStatusV5::Resting,
                revision: 1,
            });
        }
        context.broker.reserved_cash_micros = 128_000_000;
        context.owner_state.broker.locally_reserved_cash = 128_000_000;
        context.owner_state.broker.current_commitment = 256_256_000;
        context.validate().unwrap();
        assert_eq!(context.broker.positions.len(), MAX_BROKER_POSITIONS);
        assert_eq!(context.broker.orders.len(), MAX_BROKER_ORDERS);
    }

    #[test]
    fn v5_decoders_reject_trailing_bytes() {
        let mut context_bytes = encode_decision_context_v5(&context()).unwrap();
        context_bytes.push(0);
        assert_eq!(
            decode_decision_context_v5(&context_bytes),
            Err(DecisionV5Error::TrailingBytes)
        );

        let mut result_bytes = encode_decision_result_v5(&awaiting_result()).unwrap();
        result_bytes.push(0);
        assert_eq!(
            decode_decision_result_v5(&result_bytes),
            Err(DecisionV5Error::TrailingBytes)
        );
    }

    fn decimal(text: &str) -> Option<crate::supplied_v5::DecimalV5> {
        Some(crate::supplied_v5::DecimalV5::parse(text).unwrap())
    }

    fn supplied_envelope(event_id: &str, sequence: u64) -> crate::supplied_v5::EventEnvelopeV5 {
        crate::supplied_v5::EventEnvelopeV5 {
            event_id: event_id.to_owned(),
            sequence,
            city_sequence: Some(9),
            slug: Some("sea".to_owned()),
            emitted_at_unix_ns: 1_788_062_345_123_456_789,
            event_key: Some("KSEA|metar|2026-08-30T20:05:00Z".to_owned()),
            source_timestamp_unix_ns: Some(1_788_062_340_000_000_000),
            wmo_emit_time_unix_ns: None,
            producer_received_at_unix_ns: Some(1_788_062_344_500_000_000),
            live_published_at_unix_ns: Some(1_788_062_344_900_000_000),
            persistence_status: Some("committed".to_owned()),
            producer_sequence: Some(1_001),
            received_at_unix_ns: 1_788_062_345_200_000_000,
        }
    }

    fn supplied_observation() -> crate::supplied_v5::SuppliedObservationV5 {
        crate::supplied_v5::SuppliedObservationV5 {
            envelope: Some(supplied_envelope("evt-obs-44", 44)),
            source: "minutetemp.websocket.v1".to_owned(),
            station_id: "KSEA".to_owned(),
            observed_at_unix_ns: 1_788_062_340_000_000_000,
            lag_seconds: Some(45),
            preliminary: false,
            temperature_c: decimal("22.77777777777778"),
            temperature_f: decimal("73"),
            temp_min_c: decimal("22.5"),
            temp_max_c: decimal("23.5"),
            temp_min_f: decimal("72.5"),
            temp_max_f: decimal("74.3"),
            dewpoint: decimal("12.8"),
            relative_humidity: decimal("53.4"),
            wind_speed: decimal("4.1"),
            wind_direction: decimal("230"),
            text_description: Some("Partly Cloudy".to_owned()),
            temperature_day_mode: Some("nws_climate_day".to_owned()),
            temperature_day_date: Some("2026-08-30".to_owned()),
            ..Default::default()
        }
    }

    fn supplied_report() -> crate::supplied_v5::SuppliedReportV5 {
        crate::supplied_v5::SuppliedReportV5 {
            envelope: Some(supplied_envelope("evt-dsm-40", 40)),
            source: "minutetemp.websocket.v1".to_owned(),
            station_id: "KSEA".to_owned(),
            report_id: "report.dsm.1".to_owned(),
            report_fingerprint: Some("fp-1".to_owned()),
            report_revision: Some(2),
            report_updated_at_unix_ns: Some(1_788_062_300_000_000_000),
            report_type: "dsm".to_owned(),
            report_date: "2026-08-30".to_owned(),
            issuance_time_unix_ns: Some(1_788_062_280_000_000_000),
            fetched_at_unix_ns: Some(1_788_062_290_000_000_000),
            source_url: Some("https://weather.example/dsm".to_owned()),
            max_temp_f: decimal("80"),
            max_temp_c: decimal("26.7"),
            max_temp_time_unix_ns: Some(1_788_051_000_000_000_000),
            min_temp_f: decimal("58"),
            min_temp_c: decimal("14.4"),
            min_temp_time_unix_ns: Some(1_788_020_000_000_000_000),
            temp_f: None,
            temp_c: None,
            provider: Some("dsm".to_owned()),
        }
    }

    fn supplied_station() -> crate::supplied_v5::SuppliedStationV5 {
        use crate::supplied_v5::*;
        SuppliedStationV5 {
            station_id: "KSEA".to_owned(),
            observation: Some(supplied_observation()),
            daily_extremes: Some(SuppliedDailyExtremesV5 {
                source: "minutetemp.rest.latest".to_owned(),
                received_at_unix_ns: 1_788_000_000_000_000_000,
                daily_high_f: decimal("80"),
                daily_low_f: decimal("58"),
                daily_high_c: decimal("26.7"),
                daily_low_c: decimal("14.4"),
                asos_daily_high_f: decimal("79.5"),
                asos_daily_low_f: decimal("58.1"),
                temperature_day_mode: Some("nws_climate_day".to_owned()),
                temperature_day_date: Some("2026-08-30".to_owned()),
                temperature_unit: Some("F".to_owned()),
                uses_nws_climate_day: Some(true),
                ..Default::default()
            }),
            reports: vec![supplied_report()],
            extreme_high: Some(SuppliedExtremeV5 {
                envelope: Some(supplied_envelope("evt-high-41", 41)),
                source: "minutetemp.websocket.v1".to_owned(),
                kind: ExtremeKindV5::High,
                station_id: "KSEA".to_owned(),
                value_f: decimal("80"),
                value_c: decimal("26.67"),
                prev_value_f: decimal("79.5"),
                observed_at_unix_ns: Some(1_788_051_000_000_000_000),
                temperature_day_mode: Some("nws_climate_day".to_owned()),
                temperature_day_date: Some("2026-08-30".to_owned()),
                is_from_report: true,
                report_type: Some("dsm".to_owned()),
                source_report_id: Some("report.dsm.1".to_owned()),
            }),
            extreme_low: None,
            weather_events: vec![SuppliedWeatherEventV5 {
                envelope: Some(supplied_envelope("evt-wx-42", 42)),
                source: "minutetemp.websocket.v1".to_owned(),
                station_id: "KSEA".to_owned(),
                episode_id: "01a03d6a-f462-7153-9133-dbd2a26af5b4".to_owned(),
                event_type: "thunderstorm".to_owned(),
                tier: "tier1".to_owned(),
                state: "active".to_owned(),
                name: "Thunderstorm".to_owned(),
                badge: Some("TS".to_owned()),
                detail: Some("TS in vicinity".to_owned()),
                summary: Some("Thunderstorm near KSEA".to_owned()),
                started_at_unix_ns: Some(1_788_060_000_000_000_000),
                last_confirmed_at_unix_ns: Some(1_788_062_000_000_000_000),
                ended_at_unix_ns: None,
                source_snapshot: Some(SuppliedWeatherEventSourceV5 {
                    metar_type: Some("METAR".to_owned()),
                    wx_string: Some("VCTS".to_owned()),
                    wind_speed_kt: decimal("12"),
                    visibility_mi: decimal("6.21"),
                    ..Default::default()
                }),
            }],
            forecast: Some(SuppliedForecastV5 {
                source: "minutetemp.rest.forecast".to_owned(),
                received_at_unix_ns: 1_788_000_000_000_000_000,
                advertised_versions: vec![(
                    "ncep_hrrr_conus".to_owned(),
                    "2026-08-30T18:00:00Z".to_owned(),
                )],
                models: vec![SuppliedForecastModelV5 {
                    model_id: "ncep_hrrr_conus".to_owned(),
                    run_id: Some("run-1".to_owned()),
                    version: Some("2026-08-30T18:00:00Z".to_owned()),
                    fetched_at: Some("2026-08-30T18:00:00Z".to_owned()),
                    fetched_at_unix_ns: Some(1_788_055_200_000_000_000),
                    issued_at: None,
                    issued_at_unix_ns: None,
                    timezone: Some("America/Los_Angeles".to_owned()),
                    utc_offset_seconds: Some(-25_200),
                    hourly: vec![
                        SuppliedForecastPointV5 {
                            time: "2026-08-30T19:00:00Z".to_owned(),
                            time_unix_ns: 1_788_058_800_000_000_000,
                            temperature_2m_f: decimal("78.6"),
                            temperature_2m_c: decimal("25.88888888888889"),
                            apparent_temperature_f: decimal("77.9"),
                            ..Default::default()
                        },
                        SuppliedForecastPointV5 {
                            time: "2026-08-30T20:00:00Z".to_owned(),
                            time_unix_ns: 1_788_062_400_000_000_000,
                            temperature_2m_f: decimal("80.1"),
                            temperature_2m_c: decimal("26.72222222222222"),
                            ..Default::default()
                        },
                    ],
                }],
            }),
            oracle_tables: vec![SuppliedOracleTableV5 {
                updated_at_unix_ns: None,
                source: "minutetemp.rest.oracle".to_owned(),
                received_at_unix_ns: 1_788_000_000_000_000_000,
                station_id: "KSEA".to_owned(),
                range_start: "2026-08-23".to_owned(),
                range_end: "2026-08-29".to_owned(),
                days_requested: Some(7),
                all_time: None,
                score_mode: Some("day_of".to_owned()),
                rank_by: Some("high".to_owned()),
                notification_modes: vec!["day_of".to_owned()],
                notification_updated_at_unix_ns: Some(1_788_000_000_000_000_000),
                scores: vec![SuppliedOracleScoreV5 {
                    rank: None,
                    model_id: "ncep_hrrr_conus".to_owned(),
                    model_name: "HRRR CONUS".to_owned(),
                    is_public: Some(false),
                    high_mae: decimal("1.4"),
                    low_mae: decimal("2.1"),
                    high_bias: decimal("0.8"),
                    low_bias: decimal("-0.6"),
                    combined_mae: decimal("1.75"),
                    day_count: Some(7),
                }],
            }],
        }
    }

    fn supplied_observation_context() -> DecisionContextV5 {
        use crate::supplied_v5::*;
        let mut context = context();
        context.owner_state.trigger = TriggerV4::Weather {
            station_id: "KSEA".to_owned(),
            source_generation: 3,
            source_sequence: 44,
        };
        let station = &mut context.owner_state.stations[0];
        station.observation_meta.revision = 7;
        station.observation.observed_at_unix_ms = 1_788_062_340_000;
        station.observation.temperature_milli_c = Some(22_778);
        station.weather_events_meta.revision = 5;
        station
            .weather_events
            .push(crate::decision_v4::WeatherEventV4 {
                event_id: "01a03d6a-f462-7153-9133-dbd2a26af5b4".to_owned(),
                event_type: "thunderstorm".to_owned(),
                state: "active".to_owned(),
                ..Default::default()
            });
        context.trigger = TriggerV5::Owner(OwnerTriggerV5::Observation {
            station_id: "KSEA".to_owned(),
            observed_at_unix_ms: 1_788_062_340_000,
            component_revision: 7,
            source_generation: 3,
            source_sequence: 44,
        });
        context.supplied = SuppliedInputsV5 {
            contract_version: SUPPLIED_INPUTS_CONTRACT_VERSION.to_owned(),
            stations: vec![supplied_station()],
            originating_event: Some(SuppliedEventV5::Observation(supplied_observation())),
        };
        context
    }

    fn supplied_ended_episode_context() -> DecisionContextV5 {
        use crate::supplied_v5::*;
        let mut context = supplied_observation_context();
        context.owner_state.trigger = TriggerV4::Weather {
            station_id: "KSEA".to_owned(),
            source_generation: 3,
            source_sequence: 45,
        };
        let station = &mut context.owner_state.stations[0];
        station.weather_events.clear();
        station.weather_events_meta.revision = 6;
        let episode = "01a03d6a-f462-7153-9133-dbd2a26af5b4".to_owned();
        context.trigger = TriggerV5::Owner(OwnerTriggerV5::WeatherEvent {
            station_id: "KSEA".to_owned(),
            episode_id: episode.clone(),
            state: "ended".to_owned(),
            component_revision: 6,
            source_generation: 3,
            source_sequence: 45,
        });
        let mut ended = supplied_station().weather_events.remove(0);
        ended.envelope = Some(supplied_envelope("evt-wx-45", 45));
        ended.state = "ended".to_owned();
        ended.ended_at_unix_ns = Some(1_788_062_400_000_000_000);
        context.supplied.stations[0].weather_events.clear();
        context.supplied.originating_event = Some(SuppliedEventV5::WeatherEvent(ended));
        context
    }

    /// A replay whose commitment was persisted by the hundredths era: its originating digest
    /// binds the `SDCTXV5H` bytes of the originating context.
    fn hundredths_era_replay_context() -> DecisionContextV5 {
        let mut replay = exact_replay_context();
        let originating = match &replay.trigger {
            TriggerV5::BrokerOutcome {
                originating_trigger,
                ..
            } => originating_context_v5(&replay, originating_trigger),
            _ => unreachable!(),
        };
        replay
            .continuation
            .as_mut()
            .unwrap()
            .originating_context_sha256 =
            hundredths_decision_context_v5_sha256(&originating).unwrap();
        replay.validate().unwrap();
        replay
    }

    fn encode_hundredths_context_for_test(context: &DecisionContextV5) -> Vec<u8> {
        encode_bounded(
            HUNDREDTHS_DECISION_CONTEXT_V5_MAGIC,
            &hundredths_context_from_current(context).unwrap(),
            MAX_DECISION_CONTEXT_V5_BYTES,
        )
        .unwrap()
    }

    #[test]
    fn current_packet_retains_new_fields_without_historical_downgrade() {
        let mut context = supplied_observation_context();
        let historical_digest = supplied_s_decision_context_v5_sha256(&context).unwrap();
        let table = &mut context.supplied.stations[0].oracle_tables[0];
        table.updated_at_unix_ns = Some(1_788_000_000_000_000_123);
        table.scores[0].rank = Some(1);
        let bytes = encode_decision_context_v5(&context).unwrap();
        assert_eq!(
            decode_decision_context_v5(&bytes).unwrap(),
            context,
            "new packet fields must not disappear through a historical prefix"
        );
        assert!(!originating_context_digest_matches(&context, historical_digest).unwrap());
        assert!(supplied_s_context_from_current(&context).is_err());
        context.retained_supplied_encoding.canonical_c = true;
        assert!(
            encode_decision_context_v5(&context).is_err(),
            "new fields cannot be silently encoded as C"
        );
        context.retained_supplied_encoding.canonical_c = false;
        context.supplied.stations[0].oracle_tables[0].scores[0].rank = Some(0);
        assert!(
            context.validate().is_err(),
            "a present rank must match provider row order"
        );
        let mut context = supplied_observation_context();
        context.forecast_issuance = Some(
            context
                .owner_state
                .stations
                .iter()
                .map(|station| StationForecastIssuanceV5 {
                    station_id: station.identity.station_id.clone(),
                    models: vec![],
                })
                .collect(),
        );
        context.validate().unwrap();
        assert_eq!(
            decode_decision_context_v5(&encode_decision_context_v5(&context).unwrap()).unwrap(),
            context
        );
        assert!(!originating_context_digest_matches(&context, historical_digest).unwrap());
        assert!(
            supplied_s_context_from_current(&context).is_err(),
            "S must not discard an attached issuance set, even present-empty"
        );
        context.retained_supplied_encoding.canonical_c = true;
        assert!(encode_decision_context_v5(&context).is_err());

        let context = current_packet_corpus_context();
        assert_eq!(
            decode_decision_context_v5(&encode_decision_context_v5(&context).unwrap()).unwrap(),
            context
        );
        for result in [
            supplied_s_context_from_current(&context).map(|_| ()),
            hundredths_context_from_current(&context).map(|_| ()),
            legacy_context_from_current(&context).map(|_| ()),
        ] {
            assert!(result.is_err());
        }
        let invalid = [
            (
                "current-duplicate-oracle-query",
                (|context: &mut DecisionContextV5| {
                    let inputs = &mut context.current_inputs.as_mut().unwrap().stations[0].oracles;
                    inputs.push(inputs[0].clone());
                }) as fn(&mut DecisionContextV5),
            ),
            ("current-oracle-query-label-conflict", |context| {
                context.current_inputs.as_mut().unwrap().stations[0].oracles[0]
                    .supplied
                    .as_mut()
                    .unwrap()
                    .rank_by = Some("low".to_owned());
            }),
            ("current-future-weather-acceptance", |context| {
                context.current_weather.as_mut().unwrap()[0]
                    .facts
                    .fields
                    .values_mut()
                    .next()
                    .unwrap()
                    .provenance
                    .owner_revision = 8;
            }),
            ("current-future-oracle-revision", |context| {
                context.current_inputs.as_mut().unwrap().stations[0].oracles[0]
                    .meta
                    .revision = 8;
                context.owner_state.stations[0].oracle_meta.revision = 8;
            }),
            ("current-origin-supplied-mismatch", |context| {
                let Some(SuppliedEventV5::Observation(value)) = &mut context
                    .current_inputs
                    .as_mut()
                    .unwrap()
                    .originating
                    .as_mut()
                    .unwrap()
                    .supplied
                else {
                    unreachable!()
                };
                value.observed_at_unix_ns += 1_000_000;
            }),
            ("current-origin-text-bound", |context| {
                let crate::current_v5::WeatherDataV5::Observation(value) = &mut context
                    .current_inputs
                    .as_mut()
                    .unwrap()
                    .originating
                    .as_mut()
                    .unwrap()
                    .data
                else {
                    unreachable!()
                };
                value.text_description = Some("x".repeat(2049));
            }),
        ];
        for (name, mutate) in invalid {
            let mut invalid = context.clone();
            mutate(&mut invalid);
            assert!(invalid.validate().is_err(), "{name}");
        }
    }

    #[test]
    fn v5_supplied_inputs_round_trip_with_the_supplied_magic() {
        for context in [
            supplied_observation_context(),
            supplied_ended_episode_context(),
        ] {
            context.validate().unwrap();
            let encoded = encode_decision_context_v5(&context).unwrap();
            assert!(encoded.starts_with(DECISION_CONTEXT_V5_MAGIC));
            let decoded = decode_decision_context_v5(&encoded).unwrap();
            assert_eq!(decoded, context);
            assert_eq!(
                decoded.supplied.stations[0]
                    .observation
                    .as_ref()
                    .unwrap()
                    .temperature_c
                    .unwrap()
                    .to_string(),
                "22.77777777777778"
            );
            assert_eq!(
                hundredths_context_from_current(&context).err(),
                Some(DecisionV5Error::InvalidContract),
                "a context with supplied inputs has no hundredths shape"
            );
            assert_eq!(
                legacy_context_from_current(&context).err(),
                Some(DecisionV5Error::InvalidContract)
            );
        }
    }

    #[test]
    fn v5_decodes_durable_supplied_s_contexts_and_replays_their_digests() {
        let context = supplied_observation_context();
        let frozen = supplied_s_context_from_current(&context).unwrap();
        let bytes = encode_bounded(
            SUPPLIED_S_DECISION_CONTEXT_V5_MAGIC,
            &frozen,
            MAX_DECISION_CONTEXT_V5_BYTES,
        )
        .unwrap();
        let decoded = decode_decision_context_v5(&bytes).unwrap();
        assert_eq!(decoded, context);
        assert_eq!(
            decoded.supplied.contract_version,
            crate::supplied_v5::SUPPLIED_INPUTS_CONTRACT_VERSION
        );
        assert!(
            encode_decision_context_v5(&decoded)
                .unwrap()
                .starts_with(DECISION_CONTEXT_V5_MAGIC)
        );
        let result = encode_decision_result_v5(&awaiting_result_for(&context)).unwrap();
        let (_, commitment) = decode_continuation_v5(&bytes, &result).unwrap();
        let commitment = commitment.unwrap();
        assert_eq!(
            commitment.originating_context_sha256,
            supplied_s_decision_context_v5_sha256(&context).unwrap()
        );

        // Reopening the owner invocation is insufficient: a persisted Broker outcome must
        // validate against that same S-origin commitment, including after C re-encoding.
        let TriggerV5::Owner(originating_trigger) = context.trigger.clone() else {
            unreachable!();
        };
        let mut replay = context;
        replay.kernel_checkpoint = Some(commitment.pre_event_checkpoint.clone());
        replay.continuation = Some(commitment.clone());
        replay.trigger = TriggerV5::BrokerOutcome {
            outcome: Box::new(resting_outcome_for_awaited_command()),
            originating_trigger: Box::new(OriginatingTriggerV5::Owner(originating_trigger)),
        };
        replay.validate().unwrap();
        let replay_bytes = encode_decision_context_v5(&replay).unwrap();
        let reopened = decode_decision_context_v5(&replay_bytes).unwrap();
        assert_eq!(reopened, replay);
        let next = encode_decision_result_v5(&awaiting_result_for(&reopened)).unwrap();
        let (_, chained) = decode_continuation_v5(&replay_bytes, &next).unwrap();
        assert_eq!(
            chained.unwrap().originating_context_sha256,
            commitment.originating_context_sha256,
        );
        let mut changed = reopened;
        changed.supplied.stations[0]
            .observation
            .as_mut()
            .unwrap()
            .temperature_f = Some(crate::supplied_v5::DecimalV5::parse("73.1").unwrap());
        assert_eq!(changed.validate(), Err(DecisionV5Error::InvalidContract));
    }

    #[test]
    fn v5_decodes_durable_hundredths_contexts_and_replays_their_digests() {
        let context = context();
        let bytes = encode_hundredths_context_for_test(&context);
        assert!(bytes.starts_with(HUNDREDTHS_DECISION_CONTEXT_V5_MAGIC));
        let decoded = decode_decision_context_v5(&bytes).unwrap();
        assert_eq!(decoded, context);
        assert!(decoded.supplied.is_absent());
        assert_ne!(
            encode_decision_context_v5(&decoded).unwrap(),
            bytes,
            "re-encoding uses the current magic; stored bytes are never rewritten"
        );

        // A commitment persisted by the hundredths era binds the hundredths digest.
        let mut replay = exact_replay_context();
        let originating = match &replay.trigger {
            TriggerV5::BrokerOutcome {
                originating_trigger,
                ..
            } => originating_context_v5(&replay, originating_trigger),
            _ => unreachable!(),
        };
        replay
            .continuation
            .as_mut()
            .unwrap()
            .originating_context_sha256 =
            hundredths_decision_context_v5_sha256(&originating).unwrap();
        replay.validate().unwrap();
        let expected: [u8; 32] =
            Sha256::digest(encode_hundredths_context_for_test(&originating)).into();
        assert_eq!(
            hundredths_decision_context_v5_sha256(&originating).unwrap(),
            expected
        );
    }

    #[test]
    fn v5_new_low_and_weather_event_triggers_bind_the_owner_projection() {
        let mut context = context();
        context.owner_state.trigger = TriggerV4::Weather {
            station_id: "KSEA".to_owned(),
            source_generation: 3,
            source_sequence: 50,
        };
        let station = &mut context.owner_state.stations[0];
        station.extrema_meta.revision = 4;
        station.extrema.low = Some(crate::decision_v4::ExtremeV4 {
            value_milli_c: 14_444,
            observed_at_unix_ms: Some(1_788_020_000_000),
            ..Default::default()
        });
        context.trigger = TriggerV5::Owner(OwnerTriggerV5::NewLow {
            station_id: "KSEA".to_owned(),
            event_date: Some("2026-08-30".to_owned()),
            temperature_milli_c: Some(14_444),
            observed_at_unix_ms: 1_788_020_000_000,
            component_revision: 4,
            source_generation: 3,
            source_sequence: 50,
        });
        context.validate().unwrap();
        let mut changed = context.clone();
        changed.trigger = TriggerV5::Owner(OwnerTriggerV5::NewLow {
            station_id: "KSEA".to_owned(),
            event_date: None,
            temperature_milli_c: Some(14_400),
            observed_at_unix_ms: 1_788_020_000_000,
            component_revision: 4,
            source_generation: 3,
            source_sequence: 50,
        });
        assert_eq!(changed.validate(), Err(DecisionV5Error::InvalidContract));

        let ended = supplied_ended_episode_context();
        ended.validate().unwrap();
        let mut resurrected = ended.clone();
        resurrected.owner_state.stations[0].weather_events.push(
            crate::decision_v4::WeatherEventV4 {
                event_id: "01a03d6a-f462-7153-9133-dbd2a26af5b4".to_owned(),
                state: "active".to_owned(),
                ..Default::default()
            },
        );
        // Queued transitions compose against fresh state: an `ended` transition delivered
        // while the owner's current state still lists the episode is bound by its supplied
        // event, not by current membership.
        resurrected.validate().unwrap();
        let mut unbound = ended.clone();
        unbound.supplied.originating_event = None;
        assert_eq!(
            unbound.validate(),
            Err(DecisionV5Error::InvalidContract),
            "a weather trigger without its supplied event is unbound"
        );
    }

    #[test]
    fn v5_supplied_inputs_must_bind_owner_stations_and_the_originating_trigger() {
        use crate::supplied_v5::*;
        let mut unbound_station = supplied_observation_context();
        unbound_station.supplied.stations[0].station_id = "KDEN".to_owned();
        assert_eq!(
            unbound_station.validate(),
            Err(DecisionV5Error::InvalidContract)
        );

        let mut kind_mismatch = supplied_observation_context();
        kind_mismatch.supplied.originating_event = Some(SuppliedEventV5::Report(supplied_report()));
        assert_eq!(
            kind_mismatch.validate(),
            Err(DecisionV5Error::InvalidContract)
        );

        let mut missing_event = supplied_observation_context();
        missing_event.supplied.originating_event = None;
        assert_eq!(
            missing_event.validate(),
            Err(DecisionV5Error::InvalidContract)
        );

        let mut noncanonical = supplied_observation_context();
        noncanonical.supplied.stations[0]
            .observation
            .as_mut()
            .unwrap()
            .temperature_f = Some(DecimalV5 {
            coefficient: 730,
            scale: 1,
        });
        assert_eq!(
            noncanonical.validate(),
            Err(DecisionV5Error::InvalidContract)
        );

        let mut ended_in_state = supplied_observation_context();
        let event = &mut ended_in_state.supplied.stations[0].weather_events[0];
        event.state = "ended".to_owned();
        event.ended_at_unix_ns = Some(1);
        assert_eq!(
            ended_in_state.validate(),
            Err(DecisionV5Error::InvalidContract)
        );

        // An event-carrying trigger that recovers from a hundredths context without a supplied
        // block stays valid: absence is the documented pre-supplied form.
        let mut absent = supplied_observation_context();
        absent.supplied = SuppliedInputsV5::default();
        absent.validate().unwrap();
    }

    #[test]
    fn v5_corpus_measurements_and_v4_fixture_remain_stable() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../conformance/v5/decision-transactions.json");
        let corpus: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(corpus["schema"], "strategy-core-decision-v5-corpus/7");

        let vectors = corpus["valid"].as_array().unwrap();
        let measured = corpus_measurements();
        assert_eq!(vectors.len(), measured.len());
        let valid_inventory = vectors
            .iter()
            .map(|vector| {
                (
                    vector["id"].as_str().unwrap(),
                    vector["kind"].as_str().unwrap(),
                )
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            valid_inventory,
            measured
                .iter()
                .map(|(id, kind, _)| (*id, *kind))
                .collect::<BTreeSet<_>>()
        );

        let invalid = corpus["invalid"].as_array().unwrap();
        assert_eq!(invalid.len(), 36);
        let invalid_inventory = invalid
            .iter()
            .map(|vector| {
                (
                    vector["id"].as_str().unwrap(),
                    vector["category"].as_str().unwrap(),
                )
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            invalid_inventory,
            BTreeSet::from([
                ("duplicate-command-identity", "duplicate_identity"),
                ("unawaited-economic-command", "invalid_contract"),
                ("noncanonical-broker-ordering", "noncanonical_order"),
                ("unbound-strategy-scope", "invalid_contract"),
                ("nonconserving-broker-outcome", "invalid_contract"),
                ("noncanonical-parameters", "invalid_contract"),
                ("changed-checkpoint-without-digest", "invalid_contract"),
                ("oversized-kernel-checkpoint", "bound_exceeded"),
                (
                    "awaiting-result-mutates-pre-event-checkpoint",
                    "invalid_contract",
                ),
                ("changed-originating-context-hash", "invalid_contract"),
                ("changed-originating-trigger", "invalid_contract"),
                ("zero-market-price-cap", "invalid_contract"),
                ("market-price-cap-above-payout", "invalid_contract"),
                ("limit-order-with-market-price-cap", "invalid_contract"),
                ("market-sell-with-market-price-cap", "invalid_contract"),
                ("non-exact-fractional-order-reservation", "invalid_contract",),
                (
                    "legacy-whole-quantity-unrepresentable-as-u64-hundredths",
                    "invalid_contract",
                ),
                (
                    "legacy-result-whole-quantity-unrepresentable-as-u64-hundredths",
                    "invalid_contract",
                ),
                ("supplied-stations-not-bound-to-owner", "invalid_contract"),
                ("supplied-event-kind-mismatch", "invalid_contract"),
                ("supplied-noncanonical-decimal", "invalid_contract"),
                (
                    "supplied-ended-episode-in-current-state",
                    "invalid_contract"
                ),
                ("new-low-trigger-not-bound-to-extrema", "invalid_contract"),
                ("current-duplicate-oracle-query", "invalid_contract"),
                ("current-oracle-query-label-conflict", "invalid_contract"),
                ("current-future-weather-acceptance", "invalid_contract"),
                ("current-future-oracle-revision", "invalid_contract"),
                ("current-origin-supplied-mismatch", "invalid_contract"),
                ("current-origin-text-bound", "bound_exceeded"),
                ("replay-missing-predecessor", "invalid_contract"),
                ("replay-out-of-order-generation", "invalid_contract"),
                ("replay-stale-call-fence", "invalid_contract"),
                ("replay-incoherent-publication-revision", "invalid_contract"),
                ("replay-incoherent-returned-finances", "invalid_contract"),
                ("replay-cannot-downgrade-to-d", "invalid_contract"),
                ("replay-call-bound-exceeded", "bound_exceeded"),
            ])
        );

        for (id, kind, bytes) in &measured {
            let vector = vectors.iter().find(|vector| vector["id"] == *id).unwrap();
            assert_measurement(vector, bytes);
            if matches!(
                *kind,
                "decision_context_v5_current" | "decision_context_v5_replay_e"
            ) {
                let restored = decode_decision_context_v5(bytes).unwrap();
                assert_eq!(
                    encode_decision_context_v5(&restored).unwrap(),
                    *bytes,
                    "D and E payloads must preserve their bytes and originating commitments"
                );
            }
        }
    }

    /// Every corpus vector's exact bytes. Durable earlier encodings are measured through their
    /// frozen shapes so recorded digests stay verifiable after the current encoding moves on.
    fn encode_c_corpus_context(mut context: DecisionContextV5) -> Vec<u8> {
        context.retained_supplied_encoding.canonical_c = true;
        if let TriggerV5::BrokerOutcome {
            originating_trigger,
            ..
        } = &context.trigger
        {
            let original = originating_context_v5(&context, originating_trigger);
            context
                .continuation
                .as_mut()
                .unwrap()
                .originating_context_sha256 = decision_context_v5_sha256(&original).unwrap();
        }
        encode_decision_context_v5(&context).unwrap()
    }

    fn current_packet_corpus_context() -> DecisionContextV5 {
        use crate::{current_v5::*, decision_v4::*};
        use strategy_core_kernel::{
            WeatherFact, WeatherFactProvenance, WeatherFacts, WeatherField, WeatherValue,
        };
        let mut context = supplied_observation_context();
        let original = context.supplied.originating_event.take().unwrap();
        let SuppliedEventV5::Observation(observation) = &original else {
            unreachable!()
        };
        let envelope = observation.envelope.as_ref().unwrap();
        let station = &mut context.owner_state.stations[0];
        station.revision = 7;
        station.observation.station_id = station.identity.station_id.clone();
        station.provider_cursor = CursorV4 {
            connection_generation: 3,
            sequence: envelope.sequence,
            event_id: envelope.event_id.clone(),
            city_sequence: envelope.city_sequence,
            emitted_at_unix_ms: envelope.emitted_at_unix_ns.div_euclid(1_000_000),
            received_at_unix_ms: envelope.received_at_unix_ns.div_euclid(1_000_000),
            snapshot_complete: true,
        };
        let meta = ComponentMetaV4 {
            authority: AuthorityV4::Current,
            revision: 7,
            generation: 44,
            provenance: vec![ProvenanceV4 {
                provider: "minutetemp".to_owned(),
                source: observation.source.clone(),
                event_id: Some(envelope.event_id.clone()),
                connection_epoch: Some(3),
                sequence: Some(envelope.sequence),
                provider_at_unix_ms: Some(envelope.emitted_at_unix_ns.div_euclid(1_000_000)),
                received_at_unix_ms: envelope.received_at_unix_ns.div_euclid(1_000_000),
                ..Default::default()
            }],
            ..Default::default()
        };
        station.observation_meta = meta.clone();
        station.observation.provenance = meta.provenance[0].clone();
        let mut facts = WeatherFacts::default();
        facts.fields.insert(
            WeatherField::Temperature,
            WeatherFact {
                value: WeatherValue::Temperature {
                    c: observation.temperature_c,
                    f: observation.temperature_f,
                },
                provenance: WeatherFactProvenance {
                    source: observation.source.clone(),
                    owner_generation: 3,
                    owner_revision: 7,
                    supplied: true,
                    envelope: observation.envelope.clone(),
                    ..Default::default()
                },
            },
        );
        context.current_weather = Some(vec![StationWeatherV5 {
            station_id: "KSEA".to_owned(),
            facts: facts.clone(),
        }]);
        let mut oracle = context.supplied.stations[0].oracle_tables.remove(0);
        oracle.updated_at_unix_ns = Some(1_788_062_340_000_000_123);
        oracle.scores[0].rank = Some(1);
        let micros = |value: Option<crate::supplied_v5::DecimalV5>| {
            value.map(|value| (value.to_f64() * 1_000_000.0).round() as i64)
        };
        station.oracle = OracleTableV4 {
            query: OracleQueryV4 {
                station_id: "KSEA".to_owned(),
                mode: "day_of".to_owned(),
                rank_by: RankByV4::High,
                days: 7,
            },
            range_start: oracle.range_start.clone(),
            range_end: oracle.range_end.clone(),
            updated_at_unix_ms: oracle.updated_at_unix_ns.map(|at| at.div_euclid(1_000_000)),
            rows: oracle
                .scores
                .iter()
                .enumerate()
                .map(|(index, row)| OracleRowV4 {
                    rank: index as u8 + 1,
                    model_id: row.model_id.clone(),
                    model_name: row.model_name.clone(),
                    is_public: row.is_public,
                    high_mae_millionths: micros(row.high_mae),
                    low_mae_millionths: micros(row.low_mae),
                    combined_mae_millionths: micros(row.combined_mae),
                    high_bias_millionths: micros(row.high_bias),
                    low_bias_millionths: micros(row.low_bias),
                    day_count: row.day_count.map(|value| value as u16),
                })
                .collect(),
            provenance: meta.provenance[0].clone(),
        };
        station.oracle_meta = meta.clone();
        let forecast = context.supplied.stations[0].forecast.as_mut().unwrap();
        // The old corpus's millisecond-era fixture had inconsistent text/epoch pairs.
        // Preserve those C rows verbatim; the separate current vector uses the actual instants.
        let issued = 1_788_112_800_000_000_000_i64; // 2026-08-30T18:00:00Z
        forecast.models[0].fetched_at_unix_ns = Some(issued);
        for (index, point) in forecast.models[0].hourly.iter_mut().enumerate() {
            point.time_unix_ns = issued + (index as i64 + 1) * 3_600_000_000_000;
        }
        let model = &forecast.models[0];
        let version = model.version.clone().unwrap();
        station.climate_day_start_utc_unix_ms = issued.div_euclid(1_000_000) - 11 * 60 * 60 * 1000; // 07:00 UTC, midnight PDT.
        station.climate_day_end_utc_unix_ms =
            station.climate_day_start_utc_unix_ms + 24 * 60 * 60 * 1000;
        station.forecast.models = vec![ForecastModelV4 {
            model_id: model.model_id.clone(),
            version: version.clone(),
            hourly: model
                .hourly
                .iter()
                .map(|point| ForecastPointV4 {
                    at_unix_ms: point.time_unix_ns.div_euclid(1_000_000),
                    temperature_milli_c: point
                        .temperature_2m_c
                        .map(|value| (value.to_f64() * 1000.0).round() as i32),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }];
        station.forecast_meta = meta.clone();
        context.forecast_issuance = Some(vec![StationForecastIssuanceV5 {
            station_id: "KSEA".to_owned(),
            models: vec![strategy_core_kernel::forecast::ForecastIssuance {
                model_id: model.model_id.clone(),
                version: version.clone(),
                at_unix_ns: issued,
                basis: strategy_core_kernel::forecast::ForecastIssuanceBasis::TimestampedVersion,
                original_text: Some(version),
                source: forecast.source.clone(),
                received_at_unix_ns: forecast.received_at_unix_ns,
                station_generation: 3,
                station_revision: 7,
                forecast_generation: 44,
            }],
        }]);
        context.current_inputs = Some(CurrentInputsV5 {
            stations: vec![StationInputsV5 {
                station_id: "KSEA".to_owned(),
                oracles: vec![OracleInputV5 {
                    meta: meta.clone(),
                    table: station.oracle.clone(),
                    supplied: Some(oracle),
                }],
            }],
            originating: Some(OriginatingWeatherV5 {
                station_id: "KSEA".to_owned(),
                source_generation: 3,
                source_sequence: 44,
                station_revision: 7,
                meta,
                cursor: station.provider_cursor.clone(),
                data: WeatherDataV5::Observation(station.observation.clone()),
                facts,
                supplied: Some(original),
            }),
        });
        context.trigger = TriggerV5::Owner(OwnerTriggerV5::CapturedWeather {
            station_id: "KSEA".to_owned(),
            source_generation: 3,
            source_sequence: 44,
        });
        context
    }

    include!("decision_v5/replay_tests.rs");

    fn corpus_measurements() -> Vec<(&'static str, &'static str, Vec<u8>)> {
        let context = context();
        let mut measured = Vec::new();
        measured.push((
            "daily-high-side-aware-broker-context-hundredths",
            "decision_context_v5_hundredths",
            encode_hundredths_context_for_test(&context),
        ));
        measured.push((
            "daily-high-side-aware-broker-context-legacy-whole",
            "decision_context_v5_legacy_whole",
            encode_legacy_context_for_test(&legacy_context_from_current(&context).unwrap()),
        ));
        measured.push((
            "daily-high-fractional-broker-context-hundredths",
            "decision_context_v5_hundredths",
            encode_hundredths_context_for_test(&fractional_context()),
        ));
        let result = awaiting_result();
        measured.push((
            "daily-high-fenced-place-continuation-legacy-whole",
            "decision_result_v5_legacy_whole",
            encode_bounded(
                LEGACY_DECISION_RESULT_V5_MAGIC,
                &legacy_result_from_current(&result).unwrap(),
                MAX_DECISION_RESULT_V5_BYTES,
            )
            .unwrap(),
        ));
        measured.push((
            "daily-high-fenced-place-continuation",
            "decision_result_v5_hundredths",
            encode_decision_result_v5(&result).unwrap(),
        ));
        measured.push((
            "daily-high-capped-market-place-continuation",
            "decision_result_v5_hundredths",
            encode_decision_result_v5(&capped_market_result()).unwrap(),
        ));
        measured.push((
            "daily-high-exact-origin-broker-outcome-replay",
            "decision_context_v5_hundredths",
            encode_hundredths_context_for_test(&hundredths_era_replay_context()),
        ));
        measured.push((
            "unchanged-v4-owner-projection",
            "decision_context_v4",
            crate::decision_v4::encode_decision_context_v4(&context.owner_state).unwrap(),
        ));
        measured.push((
            "daily-high-side-aware-broker-context-supplied-absent",
            "decision_context_v5_supplied",
            encode_c_corpus_context(context.clone()),
        ));
        measured.push((
            "daily-high-exact-origin-broker-outcome-replay-supplied-absent",
            "decision_context_v5_supplied",
            encode_c_corpus_context(exact_replay_context()),
        ));
        measured.push((
            "daily-high-observation-with-supplied-inputs",
            "decision_context_v5_supplied",
            encode_c_corpus_context(supplied_observation_context()),
        ));
        measured.push((
            "daily-high-ended-episode-with-supplied-inputs",
            "decision_context_v5_supplied",
            encode_c_corpus_context(supplied_ended_episode_context()),
        ));
        // Schema 6's named D vectors are immutable historical evidence, not E fixtures.
        let mut current = current_packet_corpus_context();
        current.retained_supplied_encoding.canonical_d = true;
        measured.push((
            "current-complete-station-and-origin",
            "decision_context_v5_current",
            encode_decision_context_v5(&current).unwrap(),
        ));
        let result = awaiting_result_for(&current);
        let commitment = continuation_commitment_v5(&current, &result)
            .unwrap()
            .unwrap();
        let mut replay = current.clone();
        replay.continuation = Some(commitment);
        let TriggerV5::Owner(trigger) = current.trigger else {
            unreachable!()
        };
        replay.trigger = TriggerV5::BrokerOutcome {
            outcome: Box::new(resting_outcome_for_awaited_command()),
            originating_trigger: Box::new(OriginatingTriggerV5::Owner(trigger)),
        };
        measured.push((
            "current-complete-origin-outcome-replay",
            "decision_context_v5_current",
            encode_decision_context_v5(&replay).unwrap(),
        ));
        let current_e = current_packet_corpus_context();
        measured.push((
            "current-complete-station-and-origin-e",
            "decision_context_v5_replay_e",
            encode_decision_context_v5(&current_e).unwrap(),
        ));
        measured.push((
            "current-bounded-broker-replay-e",
            "decision_context_v5_replay_e",
            encode_decision_context_v5(&replay_corpus_context(current_e, 2).unwrap()).unwrap(),
        ));
        measured
    }

    /// Prints the `valid` corpus entries for `conformance/v5/decision-transactions.json`.
    /// Run with `cargo test -- --ignored --nocapture print_v5_corpus_measurements`.
    #[test]
    #[ignore]
    fn print_v5_corpus_measurements() {
        let entries = corpus_measurements()
            .into_iter()
            .map(|(id, kind, bytes)| {
                serde_json::json!({
                    "id": id,
                    "kind": kind,
                    "byte_count": bytes.len(),
                    "sha256": hex_digest(&Sha256::digest(&bytes)),
                })
            })
            .collect::<Vec<_>>();
        println!("{}", serde_json::to_string_pretty(&entries).unwrap());
    }

    fn assert_measurement(vector: &serde_json::Value, bytes: &[u8]) {
        assert_eq!(vector["byte_count"].as_u64().unwrap(), bytes.len() as u64);
        assert_eq!(
            vector["sha256"].as_str().unwrap(),
            hex_digest(&Sha256::digest(bytes))
        );
    }
}
