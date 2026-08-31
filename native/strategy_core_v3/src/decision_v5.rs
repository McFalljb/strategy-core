//! Decision Context V5: an exact, bounded transaction contract for stateful Strategies.
//!
//! V5 composes the immutable V4 owner projection. It adds exact Strategy scope, side-aware Broker
//! state, typed source and Broker triggers, and fenced commands. Collections are canonically sorted
//! before encoding and validation rejects alternate orderings.

use std::collections::BTreeSet;

use bincode::{Decode, Encode};
use sha2::{Digest, Sha256};

use crate::decision_v4::{DecisionContextV4, DecisionV4Error, TriggerV4, decision_fence_v4_sha256};

pub const DECISION_CONTEXT_V5_MAGIC: &[u8; 8] = b"SDCTXV5\0";
pub const DECISION_RESULT_V5_MAGIC: &[u8; 8] = b"SDRESV5\0";
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
    pub quantity: u64,
    /// Entry cost excluding fees. The adapter projects average price as cost / quantity.
    pub cost_basis_micros: u64,
    pub fees_micros: u64,
}

impl BrokerPositionV5 {
    pub fn average_entry_price(&self) -> f64 {
        self.cost_basis_micros as f64 / self.quantity as f64 / MAX_PRICE_MICROS as f64
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
    pub quantity: u64,
    pub filled_quantity: u64,
    pub remaining_quantity: u64,
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
    pub filled_quantity: u64,
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
    pub requested_quantity: u64,
    pub filled_quantity: u64,
    pub remaining_quantity: u64,
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
    pub quantity: u64,
    pub limit_price_micros: Option<u64>,
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

impl DecisionContextV5 {
    pub fn validate(&self) -> Result<(), DecisionV5Error> {
        self.owner_state.validate().map_err(DecisionV5Error::V4)?;
        validate_scope(self)?;
        validate_broker(self)?;
        if let Some(checkpoint) = &self.kernel_checkpoint {
            validate_kernel_checkpoint(&self.strategy, checkpoint)?;
        }
        validate_trigger(self)?;
        Ok(())
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
        || result.expected_broker_revision != context.broker.revision
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
            } if !context.broker.orders.iter().any(|order| {
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
        originating_context_sha256: decision_context_v5_sha256(context)?,
        pre_event_checkpoint: result
            .kernel_checkpoint
            .clone()
            .ok_or(DecisionV5Error::InvalidContract)?,
    }))
}

pub fn strategy_command_v5_sha256(
    command: &StrategyCommandV5,
) -> Result<[u8; 32], DecisionV5Error> {
    validate_command(command)?;
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
    encode_bounded(
        DECISION_CONTEXT_V5_MAGIC,
        context,
        MAX_DECISION_CONTEXT_V5_BYTES,
    )
}

pub fn decode_decision_context_v5(bytes: &[u8]) -> Result<DecisionContextV5, DecisionV5Error> {
    let context: DecisionContextV5 = decode_bounded(
        DECISION_CONTEXT_V5_MAGIC,
        bytes,
        MAX_DECISION_CONTEXT_V5_BYTES,
    )?;
    context.validate()?;
    Ok(context)
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
    let result: DecisionResultV5 = decode_bounded(
        DECISION_RESULT_V5_MAGIC,
        bytes,
        MAX_DECISION_RESULT_V5_BYTES,
    )?;
    result.validate()?;
    Ok(result)
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
    let broker = &context.broker;
    if !strictly_sorted(
        broker
            .positions
            .iter()
            .map(|position| (position.market_id.as_str(), position.side)),
    ) || !strictly_sorted(broker.orders.iter().map(|order| order.order_id.as_str()))
    {
        return Err(DecisionV5Error::NonCanonicalOrder);
    }
    let owner_market_ids = context
        .strategy
        .market_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if broker.revision != context.owner_state.broker.revision
        || broker.revision != context.owner_state.fence.broker_revision
        || broker.positions.len() > MAX_BROKER_POSITIONS
        || broker.orders.len() > MAX_BROKER_ORDERS
    {
        return Err(DecisionV5Error::InvalidContract);
    }
    if broker.positions.iter().any(|position| {
        position.quantity == 0
            || position.quantity > i64::MAX as u64
            || u128::from(position.cost_basis_micros)
                > u128::from(position.quantity) * u128::from(MAX_PRICE_MICROS)
            || !owner_market_ids.contains(position.market_id.as_str())
    }) || broker.orders.iter().any(|order| {
        !valid_identifier(&order.command_id)
            || !valid_identifier(&order.intent_id)
            || !valid_identifier(&order.order_id)
            || !valid_optional_identifier(&order.provider_order_id)
            || !valid_identifier(&order.provider_client_id)
            || !owner_market_ids.contains(order.market_id.as_str())
            || order.quantity == 0
            || order.quantity > i64::MAX as u64
            || order.filled_quantity.checked_add(order.remaining_quantity) != Some(order.quantity)
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
        || reserved_cash > u128::from(context.owner_state.broker.locally_reserved_cash)
        || position_commitment + reserved_cash
            != u128::from(context.owner_state.broker.current_commitment)
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
    let maximum_notional = u128::from(order.remaining_quantity) * u128::from(MAX_PRICE_MICROS);
    let principal_is_exact = match order.limit_price_micros {
        Some(price) => {
            u128::from(order.reserved_principal_micros)
                == u128::from(order.remaining_quantity) * u128::from(price)
        }
        None => u128::from(order.reserved_principal_micros) <= maximum_notional,
    };
    principal_is_exact && u128::from(order.reserved_fee_micros) <= maximum_notional
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
    let mut originating = context.clone();
    originating.trigger = match originating_trigger {
        OriginatingTriggerV5::Owner(trigger) => TriggerV5::Owner(trigger.clone()),
        OriginatingTriggerV5::BrokerState { broker_revision } => TriggerV5::BrokerState {
            broker_revision: *broker_revision,
        },
    };
    originating.continuation = None;
    // Delivery may occur after a process restart. The ledger preserves every originating
    // economic input while the current process attempt is used only for runtime routing.
    originating.owner_state.sleeve.process_attempt = commitment.process_attempt;
    if decision_context_v5_sha256(&originating)? != commitment.originating_context_sha256 {
        return Err(DecisionV5Error::InvalidContract);
    }
    Ok(())
}

fn validate_owner_trigger(
    context: &DecisionContextV5,
    trigger: &OwnerTriggerV5,
) -> Result<(), DecisionV5Error> {
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
        || outcome.requested_quantity > i64::MAX as u64
        || outcome
            .filled_quantity
            .checked_add(outcome.remaining_quantity)
            != Some(outcome.requested_quantity)
        || outcome
            .average_fill_price_micros
            .is_some_and(|price| price > MAX_PRICE_MICROS)
        || !valid_optional_text(&outcome.reason, MAX_REASON_BYTES)
        || outcome.broker_revision != context.broker.revision
    {
        return Err(DecisionV5Error::InvalidContract);
    }
    let matching_order = outcome.order_id.as_ref().and_then(|order_id| {
        context
            .broker
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
                || result.filled_quantity != outcome.filled_quantity
                || result.fill_price_micros > MAX_PRICE_MICROS
                || u128::from(result.fee_cost_micros)
                    > u128::from(result.filled_quantity) * u128::from(MAX_PRICE_MICROS)
                || (result.filled_quantity == 0
                    && (result.fill_price_micros != 0
                        || result.fee_cost_micros != 0
                        || outcome.average_fill_price_micros.is_some()))
                || (result.filled_quantity > 0
                    && outcome.average_fill_price_micros != Some(result.fill_price_micros))
                || outcome.order_id.as_ref() != Some(&result.order_id)
                || (!matches!(outcome.status, BrokerOutcomeStatusV5::Rejected)
                    && matching_order.is_none())
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
                    !context.broker.orders.iter().any(|order| {
                        order.order_id == *order_id
                            && matches!(
                                order.status,
                                BrokerOrderStatusV5::CancellationRequested
                                    | BrokerOrderStatusV5::Cancelled
                            )
                    })
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
            || !order_status_matches_outcome(order.status, outcome.status)
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

fn order_status_matches_outcome(
    order: BrokerOrderStatusV5,
    outcome: BrokerOutcomeStatusV5,
) -> bool {
    matches!(
        (order, outcome),
        (
            BrokerOrderStatusV5::DurablyAccepted,
            BrokerOutcomeStatusV5::DurablyAccepted
        ) | (
            BrokerOrderStatusV5::Dispatched,
            BrokerOutcomeStatusV5::Dispatched
        ) | (BrokerOrderStatusV5::Resting, BrokerOutcomeStatusV5::Resting)
            | (
                BrokerOrderStatusV5::PartiallyFilled,
                BrokerOutcomeStatusV5::PartiallyFilled
            )
            | (BrokerOrderStatusV5::Filled, BrokerOutcomeStatusV5::Filled)
            | (
                BrokerOrderStatusV5::CancellationRequested,
                BrokerOutcomeStatusV5::CancellationRequested
            )
            | (
                BrokerOrderStatusV5::Cancelled,
                BrokerOutcomeStatusV5::Cancelled
            )
            | (BrokerOrderStatusV5::Expired, BrokerOutcomeStatusV5::Expired)
            | (
                BrokerOrderStatusV5::Rejected,
                BrokerOutcomeStatusV5::Rejected
            )
            | (
                BrokerOrderStatusV5::RecoveryRequired,
                BrokerOutcomeStatusV5::RecoveryRequired
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
        || commitment.process_attempt > sleeve.process_attempt
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
                || order.quantity == 0
                || order.quantity > i64::MAX as u64
                || !valid_identifier(&order.provider_client_id)
                || order.metadata.len() > MAX_COMMAND_METADATA_BYTES
                || !valid_optional_text(&order.signal_type, MAX_SHORT_TEXT_BYTES)
                || !valid_optional_text(&order.signal_metadata, MAX_COMMAND_METADATA_BYTES)
                || order.expires_after_ms.is_some_and(|ttl| ttl <= 0)
                || matches!(order.order_type, OrderTypeV5::Limit)
                    && order.limit_price_micros.is_none()
                || matches!(order.order_type, OrderTypeV5::Market)
                    && order.limit_price_micros.is_some()
                || order
                    .limit_price_micros
                    .is_some_and(|price| price > MAX_PRICE_MICROS)
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
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
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
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
                    quantity: 2,
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
                    quantity: 3,
                    filled_quantity: 2,
                    remaining_quantity: 1,
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
                quantity: 3,
                limit_price_micros: Some(400_000),
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
        assert_eq!(decode_decision_context_v5(&encoded).unwrap(), context);
    }

    fn awaiting_result_for(context: &DecisionContextV5) -> DecisionResultV5 {
        let mut result = awaiting_result();
        result.delivery_id = context.owner_state.delivery_id.clone();
        result.sleeve_identity = context.owner_state.sleeve.sleeve_id.clone();
        result.expected_broker_revision = context.broker.revision;
        result.state_fence = hex_digest(&decision_fence_v5_sha256(context).unwrap());
        result
    }

    #[test]
    fn v5_result_round_trip_preserves_fenced_continuation() {
        let result = awaiting_result();
        let encoded = encode_decision_result_v5(&result).unwrap();
        assert_eq!(decode_decision_result_v5(&encoded).unwrap(), result);
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
            commitment.pre_event_checkpoint,
            context.kernel_checkpoint.clone().unwrap()
        );
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
                    filled_quantity: 2,
                    fill_price_micros: 590_000,
                    fee_cost_micros: 10_000,
                    reason: "partial fill".to_owned(),
                },
            )),
            requested_quantity: 3,
            filled_quantity: 2,
            remaining_quantity: 1,
            average_fill_price_micros: Some(590_000),
            reason: None,
            updated_at_unix_ms: 2,
            broker_revision: 9,
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
            originating_delivery_id: "delivery.daily.0".to_owned(),
            sleeve_identity: context.owner_state.sleeve.sleeve_id.clone(),
            sleeve_incarnation: context.owner_state.sleeve.incarnation,
            process_attempt: context.owner_state.sleeve.process_attempt,
            route_epoch: context.owner_state.sleeve.route_epoch,
            continuation_id: outcome.continuation_id.clone(),
            continuation_generation: outcome.continuation_generation,
            command_id: outcome.command_id.clone(),
            command_sha256: [1; 32],
            expected_broker_revision: 8,
            originating_context_sha256,
            pre_event_checkpoint: context.kernel_checkpoint.clone().unwrap(),
        });
        context.trigger = TriggerV5::BrokerOutcome {
            outcome: Box::new(outcome),
            originating_trigger: Box::new(originating_trigger),
        };
    }

    #[test]
    fn v5_round_trip_preserves_exact_broker_transition_identity() {
        let mut context = context();
        install_outcome(&mut context, partial_fill_outcome());
        let encoded = encode_decision_context_v5(&context).unwrap();
        assert_eq!(decode_decision_context_v5(&encoded).unwrap(), context);
    }

    #[test]
    fn v5_broker_outcome_restores_checkpoint_after_process_restart() {
        let mut context = context();
        install_outcome(&mut context, partial_fill_outcome());
        context.owner_state.sleeve.process_attempt += 1;
        context.validate().unwrap();
        assert_eq!(
            context.kernel_checkpoint.as_ref(),
            context
                .continuation
                .as_ref()
                .map(|commitment| &commitment.pre_event_checkpoint)
        );
    }

    #[test]
    fn v5_broker_outcome_requires_exact_quantity_conservation() {
        let mut context = context();
        let mut outcome = partial_fill_outcome();
        outcome.requested_quantity = 4;
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
                    quantity: 1,
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
                quantity: 1,
                filled_quantity: 0,
                remaining_quantity: 1,
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

    #[test]
    fn v5_corpus_measurements_and_v4_fixture_remain_stable() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../conformance/v5/decision-transactions.json");
        let corpus: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let vectors = corpus["valid"].as_array().unwrap();
        let expected = |id: &str| vectors.iter().find(|vector| vector["id"] == id).unwrap();

        let context = context();
        let context_bytes = encode_decision_context_v5(&context).unwrap();
        assert_measurement(
            expected("daily-high-side-aware-broker-context"),
            &context_bytes,
        );

        let result_bytes = encode_decision_result_v5(&awaiting_result()).unwrap();
        assert_measurement(
            expected("daily-high-fenced-place-continuation"),
            &result_bytes,
        );

        let v4_bytes =
            crate::decision_v4::encode_decision_context_v4(&context.owner_state).unwrap();
        assert_measurement(expected("unchanged-v4-owner-projection"), &v4_bytes);
    }

    fn assert_measurement(vector: &serde_json::Value, bytes: &[u8]) {
        assert_eq!(vector["byte_count"].as_u64().unwrap(), bytes.len() as u64);
        assert_eq!(
            vector["sha256"].as_str().unwrap(),
            hex_digest(&Sha256::digest(bytes))
        );
    }
}
