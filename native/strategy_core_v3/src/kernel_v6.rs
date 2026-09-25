//! Core-owned kernel projection and single-run runner for Decision V6.
//!
//! This module is the single definition of how a canonical [`DecisionContextV6`] is presented
//! to a `strategy_core_kernel::NativeKernel` and how one run becomes a [`DecisionResultV6`].
//! Hosts and Strategy executables consume it instead of re-interpreting fields.
//!
//! One run per event:
//! 1. Restore the kernel from its checkpoint (or create it).
//! 2. Compare the context's Broker state and command receipts with the checkpoint's runner
//!    section and deliver one `OrderUpdate` per change, in the order the commands were issued
//!    (updates the previous decision deferred first, the most deferred first; a cancel's
//!    update never before its target's).
//! 3. Deliver the trigger's event (`on_start` for Bootstrap/Recovery).
//! 4. Broker calls return tickets at once. Inside the decision the kernel sees a provisional
//!    view: its own new orders are pending (`submitted`) and reserve budget with the Broker's
//!    formula ([`fees::buy_commitment_micros`]); cancels mark their targets
//!    `cancellation_requested`; positions are unchanged.
//! 5. The result carries the post-event checkpoint (advanced by exactly one), every command in
//!    issue order, and the receipts and terminal orders the Strategy has now seen. A kernel
//!    error gives a `Rejected` result: no commands, the checkpoint unchanged.
//!
//! Projection rules:
//! - The context is projected once into the canonical owned model of `strategy_core_kernel`
//!   (`StationState`, `MarketState`, `StrategyEvent`). Every component is built from its
//!   supplied original when the context carries one and from the derived V4 owner projection
//!   otherwise; each carries its `ValueOrigin` and, when supplied, the original itself.
//! - Publication, observation, issuance, receipt and decision times are distinct: `emitted_at`
//!   on an event is the provider's publication time, never the decision clock.
//! - Weather, forecast and oracle events come from the station that triggered them, which may
//!   be any contributor station of the Sleeve's event.

mod events;
mod projection;
mod updates;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, TimeZone, Utc};
use strategy_core_kernel::{
    AnnotationValue, BrokerCommandKind, BrokerFinancialState, BrokerOrderStatus,
    CancelOrderRequest, CancelTarget, CommandTicket, ContractQuantity, ContractSide, KernelAction,
    KernelCapabilities, KernelError, KernelResult, LogAction, MarketState, NativeKernel,
    OrderAction, OrderStatusView, OrderTicket, OrderType, OrderUpdate, OrderUpdateStatus,
    ParameterValue, PendingOrderView, PendingTimer, PlaceOrderRequest, RuntimeMode, StationState,
    StrategyEvent, StrategyKernelBroker, StrategyKernelContext, StrategyKernelData,
    StrategyKernelRuntime, StrategyKernelState, StrategyKernelTelemetry, StrategyParameters,
    TimerHandle, WakeAtRequest, fees,
};

pub use self::events::KernelEvent;
use self::projection::{
    hundredths_quantity, market_state, millis, price, price_micros, station_state,
};
use self::updates::Failure;
pub use self::updates::{PROVIDER_REJECTED_CODE, PROVIDER_REJECTED_REASON};
use crate::decision_v6::{
    self as wire, AnnotationValueV6, BrokerCommandKindV6, BrokerDetailV6, BrokerOrderStatusV6,
    BrokerOrderV6, CancelTargetV6, ContractSideV6, DecisionContextV6, DecisionDispositionV6,
    DecisionPlanRows, DecisionResultV6, DecisionV6Error, DeploymentModeV6, KernelCheckpointV6,
    OrderActionV6, OrderTypeV6, OrderUpdateRecordV6, OrderUpdateStatusV6, PlaceOrderV6,
    ResultDiagnosticV6, ResultEvidenceV6, RunnerEntryV6, RunnerSectionV6, StrategyCommandV6,
    StrategyParameterValueV6, TelemetryEntryV6,
};

/// Prefix of the client order ids the host derives; a kernel's own ids may not use it.
pub const DERIVED_CLIENT_ID_PREFIX: &str = "tv3";
/// Key of a timer scheduled without a name.
pub const DEFAULT_TIMER_KEY: &str = "kernel.wake";
/// `TimerRecoveryV4::admission_state` of a timer that is scheduled and not yet delivered.
const TIMER_ACTIVE: u8 = 0;
/// Telemetry payload bytes one result keeps; entries past it are counted as lost.
const MAX_TELEMETRY_BYTES: usize = 256 * 1024;
/// Encoded bytes of a result's fixed fields and closing diagnostics (ids, fence, the kernel
/// error and overflow diagnostics).
const RESULT_OVERHEAD_BYTES: usize = 16 * 1024;

#[derive(Debug)]
pub enum KernelTransactionError {
    Contract(DecisionV6Error),
    UnsupportedStrategy(String),
    UnsupportedCheckpoint,
    Checkpoint(String),
    InvalidTime,
    InvalidQuantity,
    Kernel(String),
    /// The checkpoint's runner section holds more than `MAX_RUNNER_SECTION_ENTRIES` entries.
    RunnerSectionFull,
}

impl std::fmt::Display for KernelTransactionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl std::error::Error for KernelTransactionError {}
impl From<DecisionV6Error> for KernelTransactionError {
    fn from(value: DecisionV6Error) -> Self {
        Self::Contract(value)
    }
}

/// Identity of the checkpoint codec a factory currently writes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelCheckpointCodec {
    pub profile: String,
    pub version: u32,
}

/// A kernel as driven by the runner: an ordinary [`NativeKernel`] that can also checkpoint its
/// private state.
pub trait TransactionKernel: NativeKernel {
    /// Opaque private state for the checkpoint (at most `MAX_KERNEL_CHECKPOINT_BYTES`).
    fn encode_checkpoint_state(&self) -> Result<Vec<u8>, KernelTransactionError>;
    /// Authoritative maximum per-contract price of a Market buy, asked once when the kernel
    /// places it. The runner asks the kernel as it was restored for this decision.
    fn market_buy_price_cap_micros(
        &self,
        _request: &PlaceOrderRequest,
    ) -> Result<Option<u64>, KernelTransactionError> {
        Ok(None)
    }
}

/// Constructs and restores kernels for one Strategy identity.
pub trait TransactionKernelFactory {
    type Kernel: TransactionKernel;

    fn checkpoint_codec(
        &self,
        strategy_id: &str,
    ) -> Result<KernelCheckpointCodec, KernelTransactionError>;
    fn create(&self, context: &DecisionContextV6) -> Result<Self::Kernel, KernelTransactionError>;
    fn restore(
        &self,
        context: &DecisionContextV6,
        checkpoint: &KernelCheckpointV6,
    ) -> Result<Self::Kernel, KernelTransactionError>;
}

/// Runs one decision: restore or create the kernel, deliver the derived order updates and the
/// trigger's event over the projected state, and assemble the result.
pub fn run_transaction<F: TransactionKernelFactory>(
    factory: &F,
    context: &DecisionContextV6,
) -> Result<DecisionResultV6, KernelTransactionError> {
    context.validate()?;
    let codec = factory.checkpoint_codec(&context.strategy.strategy_id)?;
    let restore = || match &context.kernel_checkpoint {
        Some(checkpoint) => factory.restore(context, checkpoint),
        None => factory.create(context),
    };
    let mut kernel = restore()?;
    // The Market-buy cap is asked of the kernel as restored for this decision.
    let restored = restore()?;
    let market_buy_cap =
        |request: &PlaceOrderRequest| restored.market_buy_price_cap_micros(request);
    let derived = updates::derive(context)?;
    let event = KernelEvent::from_context(context)?;
    // Deferred updates first (the most deferred first), the others in issue order; a
    // cancel's update never before its target's.
    let delivery_order = derived.delivery_order();
    let evidence = wire::order_update_evidence(
        &delivery_order
            .iter()
            .flatten()
            .filter_map(|index| derived.steps[*index].update.as_ref())
            .map(evidence_record)
            .collect::<Vec<_>>(),
    )?;
    let acknowledgeable = updates::acknowledgeable(context);
    // Everything in the result but the commands, the runner entries and the kernel's
    // telemetry, with the kernel's state at its bound: commands are admitted against the rest.
    let fixed_bytes = wire::encoded_len(&evidence)
        + wire::encoded_len(&acknowledgeable)
        + wire::MAX_KERNEL_CHECKPOINT_BYTES
        + RESULT_OVERHEAD_BYTES;
    let mut host = KernelHost::new(context, derived.entries(), !acknowledgeable.is_empty())?;
    host.market_buy_cap = Some(&market_buy_cap);
    host.fixed_bytes = fixed_bytes;
    host.reinstatable_live = derived.reinstatable_live();
    let issued_from = host.runner.len();
    host.derived_entries = issued_from;

    // Each update is delivered on its own. When the kernel fails on one, the kernel and the
    // decision go back to how they were before it and the failure is recorded; the update is
    // delivered again in the next decisions, up to MAX_DELIVERY_ATTEMPTS counted failures,
    // then counts as seen. A kernel that returns the host's refusal for room in the decision
    // that earlier updates of the same decision took is deferred instead, up to
    // MAX_DELIVERY_DEFERRALS decisions in a row. The remaining updates and the trigger are
    // still delivered.
    let mut failed = BTreeMap::new();
    let mut start = Some(host.save());
    let units = delivery_order.iter().flat_map(|unit| {
        unit.iter()
            .enumerate()
            .map(move |(position, index)| (position == 0, *index))
    });
    // What the decision had issued when the current delivery unit began.
    let mut unit_start = None;
    for (first_of_unit, index) in units {
        if first_of_unit {
            unit_start = None;
        }
        let step = &derived.steps[index];
        let Some(record) = &step.update else {
            continue;
        };
        // A cancel's update never goes before its target's: when an update of one of its
        // targets failed anywhere in this decision, the cancel's waits, as it was, to follow
        // it in a later decision. Holds are counted as deferrals; at the bound the cancel's
        // update is delivered anyway, out of order, never abandoned for it.
        let out_of_order = derived
            .targets(index)
            .iter()
            .any(|target| failed.contains_key(target));
        if out_of_order && step.deferral() < wire::MAX_DELIVERY_DEFERRALS {
            failed.insert(index, Failure::Held);
            host.outputs.push(HostOutput::UpdateError(
                format!(
                    "order update of {} held ({} of {}): an update of its target failed \
                     in this decision",
                    record.command_id,
                    step.deferral(),
                    wire::MAX_DELIVERY_DEFERRALS - 1
                ),
                Failure::Held,
            ));
            continue;
        }
        let describe = |failure: Failure, error: &dyn std::fmt::Display| {
            let attempt = match failure {
                Failure::Deferred => format!(
                    "deferred {} of {}",
                    step.deferral(),
                    wire::MAX_DELIVERY_DEFERRALS
                ),
                _ => format!(
                    "attempt {} of {}",
                    step.attempt(),
                    wire::MAX_DELIVERY_ATTEMPTS
                ),
            };
            HostOutput::UpdateError(
                format!("order update of {} ({attempt}): {error}", record.command_id),
                failure,
            )
        };
        // The kernel's own codec snapshots it; a failed update restores it through the factory.
        // A snapshot that cannot be taken is a failure of the update, which is not delivered.
        let snapshot = match kernel.encode_checkpoint_state() {
            Ok(state) => snapshot_checkpoint(context, &codec, state),
            Err(error) => {
                failed.insert(index, Failure::Counted);
                host.outputs.push(describe(
                    Failure::Counted,
                    &format!("the kernel's state could not be saved before it: {error}"),
                ));
                continue;
            }
        };
        let saved = host.save();
        host.begin_update(&mut unit_start);
        let delivered = StrategyEvent::OrderUpdate(order_update(record))
            .with_view(|view| kernel.on_event(view, &mut host));
        let deferrable = host.end_update();
        let Err(error) = delivered else {
            if out_of_order {
                host.outputs.push(HostOutput::UpdateWarning(
                    "order_update_out_of_order",
                    format!(
                        "order update of {} delivered before an update of its target, which \
                         failed in this decision, after {} decisions held or deferred",
                        record.command_id,
                        step.deferrals()
                    ),
                ));
            }
            continue;
        };
        host.restore(saved);
        match factory.restore(context, &snapshot) {
            Ok(restored) => {
                kernel = restored;
                // Deferred only when the kernel returned the refusal itself. A cancel's or
                // cancel-all's refusal that would reach the deferral bound counts instead (its
                // deferrals may come from holds behind its targets, in order or not), so a
                // cancel is never abandoned through the deferral bound.
                let cancel_at_bound = record.kind != BrokerCommandKindV6::PlaceOrder
                    && step.deferral() >= wire::MAX_DELIVERY_DEFERRALS;
                let failure = if !cancel_at_bound && deferrable.as_deref() == Some(error.message())
                {
                    Failure::Deferred
                } else {
                    Failure::Counted
                };
                failed.insert(index, failure);
                host.outputs.push(describe(failure, &error));
            }
            Err(restore_error) => {
                // The kernel cannot go back to how it was before this update (a kernel or
                // factory defect, so the failure counts): the kernel and the decision go back
                // to how they were before the first update, and every other update is
                // delivered again.
                kernel = restore()?;
                if let Some(start) = start.take() {
                    host.restore(start);
                }
                failed.insert(index, Failure::Counted);
                for (other, step) in derived.steps.iter().enumerate() {
                    if step.update.is_some() {
                        failed.entry(other).or_insert(Failure::RolledBack);
                    }
                }
                host.outputs.push(describe(
                    Failure::Counted,
                    &format!(
                        "{error}; the kernel's state could not be restored after it: \
                         {restore_error}; every order update is delivered again"
                    ),
                ));
                break;
            }
        }
    }
    let outcome = event.run(&mut kernel, &mut host);

    let overflow;
    let mut result = DecisionResultV6 {
        delivery_id: context.owner_state.delivery_id.clone(),
        sleeve_identity: context.owner_state.sleeve.sleeve_id.clone(),
        state_fence: hex(&wire::decision_fence_v6_sha256(context)?),
        expected_broker_revision: context.broker.revision,
        disposition: DecisionDispositionV6::Completed,
        kernel_checkpoint: None,
        commands: Vec::new(),
        acknowledged_command_ids: Vec::new(),
        evidence,
        diagnostics: Vec::new(),
        telemetry: Vec::new(),
    };
    match outcome {
        Ok(()) => {
            let issued = host.runner.split_off(issued_from);
            let newest_view_revision = derived.newest_view_revision;
            let (entries, notes) = derived.finalize(&failed, issued);
            result.acknowledged_command_ids = updates::acknowledgements(context, &entries);
            let sequence = context
                .kernel_checkpoint
                .as_ref()
                .map_or(Some(1), |checkpoint| checkpoint.sequence.checked_add(1))
                .ok_or(KernelTransactionError::Contract(
                    DecisionV6Error::BoundExceeded,
                ))?;
            result.kernel_checkpoint = Some(
                KernelCheckpointV6 {
                    codec_profile: codec.profile.clone(),
                    codec_version: codec.version,
                    strategy_id: context.strategy.strategy_id.clone(),
                    strategy_profile: context.strategy.profile.clone(),
                    profile_and_calculator_digest: context
                        .strategy
                        .profile_and_calculator_digest
                        .clone(),
                    sequence,
                    state: kernel.encode_checkpoint_state()?,
                    runner: RunnerSectionV6 {
                        seeded: true,
                        newest_view_revision,
                        entries,
                    },
                    state_sha256: [0; 32],
                }
                .seal(),
            );
            result.commands = std::mem::take(&mut host.commands);
            append_notes(&notes, &mut result);
            overflow = append_outputs(&host.outputs, None, &mut result);
        }
        Err(error) => {
            result.disposition = DecisionDispositionV6::Rejected;
            result.kernel_checkpoint = context.kernel_checkpoint.clone();
            overflow = append_outputs(&host.outputs, Some(&error), &mut result);
        }
    }
    wire::validate_decision_result_v6(context, &result)?;
    fit_result(&mut result, overflow)?;
    Ok(result)
}

/// A checkpoint of the kernel's state mid-decision, for restoring it after a failed update.
/// It carries an empty runner section, so taking it costs the kernel's state only.
fn snapshot_checkpoint(
    context: &DecisionContextV6,
    codec: &KernelCheckpointCodec,
    state: Vec<u8>,
) -> KernelCheckpointV6 {
    KernelCheckpointV6 {
        codec_profile: codec.profile.clone(),
        codec_version: codec.version,
        strategy_id: context.strategy.strategy_id.clone(),
        strategy_profile: context.strategy.profile.clone(),
        profile_and_calculator_digest: context.strategy.profile_and_calculator_digest.clone(),
        sequence: context
            .kernel_checkpoint
            .as_ref()
            .map_or(1, |checkpoint| checkpoint.sequence),
        state,
        // A factory restores the kernel's state; the runner section is the runner's own.
        runner: RunnerSectionV6::default(),
        state_sha256: [0; 32],
    }
    .seal()
}

/// The Strategy parameters projected without loss into a JSON object for kernel initializers.
pub fn strategy_parameters_json(
    context: &DecisionContextV6,
) -> Result<serde_json::Map<String, serde_json::Value>, KernelTransactionError> {
    let mut parameters = serde_json::Map::new();
    for (key, value) in &context.strategy.parameters {
        parameters.insert(key.clone(), parameter_json(value)?);
    }
    Ok(parameters)
}

fn parameter_value(value: &StrategyParameterValueV6) -> ParameterValue {
    match value {
        StrategyParameterValueV6::Null => ParameterValue::Null,
        StrategyParameterValueV6::Bool(value) => ParameterValue::Bool(*value),
        StrategyParameterValueV6::I64(value) => ParameterValue::I64(*value),
        StrategyParameterValueV6::U64(value) => ParameterValue::U64(*value),
        StrategyParameterValueV6::Decimal { coefficient, scale } => ParameterValue::Decimal {
            coefficient: *coefficient,
            scale: *scale,
        },
        StrategyParameterValueV6::String(value) => ParameterValue::String(value.clone()),
    }
}

fn parameter_json(
    value: &StrategyParameterValueV6,
) -> Result<serde_json::Value, KernelTransactionError> {
    Ok(match value {
        StrategyParameterValueV6::Null => serde_json::Value::Null,
        StrategyParameterValueV6::Bool(value) => serde_json::Value::Bool(*value),
        StrategyParameterValueV6::I64(value) => (*value).into(),
        StrategyParameterValueV6::U64(value) => (*value).into(),
        StrategyParameterValueV6::Decimal { coefficient, scale } => {
            let divisor = 10_f64.powi(i32::from(*scale));
            serde_json::Number::from_f64(*coefficient as f64 / divisor)
                .map(serde_json::Value::Number)
                .ok_or(KernelTransactionError::InvalidQuantity)?
        }
        StrategyParameterValueV6::String(value) => serde_json::Value::String(value.clone()),
    })
}

/// The update as recorded in the result's evidence: a refusal reason is cut to
/// `MAX_REJECTION_REASON_BYTES` (the kernel sees all of it), so the evidence of every update
/// always fits the result.
fn evidence_record(record: &OrderUpdateRecordV6) -> OrderUpdateRecordV6 {
    let mut record = record.clone();
    if let OrderUpdateStatusV6::Refused { reason, .. } = &mut record.status {
        truncate_utf8(reason, wire::MAX_REJECTION_REASON_BYTES);
    }
    record
}

/// Presents one derived update record to a kernel.
pub fn order_update(record: &OrderUpdateRecordV6) -> OrderUpdate {
    OrderUpdate {
        command_kind: match record.kind {
            BrokerCommandKindV6::PlaceOrder => BrokerCommandKind::PlaceOrder,
            BrokerCommandKindV6::CancelOrder => BrokerCommandKind::CancelOrder,
            BrokerCommandKindV6::CancelAllOrders => BrokerCommandKind::CancelAllOrders,
        },
        command_id: record.command_id.clone(),
        client_order_id: record.client_order_id.clone().unwrap_or_default(),
        order_id: record.order_id.clone(),
        ticker: record.market_id.clone().unwrap_or_default(),
        action: record.action.map(order_action),
        contract_side: record.side.map(contract_side),
        status: match &record.status {
            OrderUpdateStatusV6::Accepted => OrderUpdateStatus::Accepted,
            OrderUpdateStatusV6::Resting => OrderUpdateStatus::Resting,
            OrderUpdateStatusV6::PartiallyFilled => OrderUpdateStatus::PartiallyFilled,
            OrderUpdateStatusV6::Filled => OrderUpdateStatus::Filled,
            OrderUpdateStatusV6::Cancelled => OrderUpdateStatus::Cancelled,
            OrderUpdateStatusV6::Expired => OrderUpdateStatus::Expired,
            OrderUpdateStatusV6::Refused { code, reason } => OrderUpdateStatus::Refused {
                code: code.clone(),
                reason: reason.clone(),
            },
        },
        requested: hundredths_quantity(record.requested_quantity_hundredths),
        filled: hundredths_quantity(record.filled_quantity_hundredths),
        remaining: hundredths_quantity(record.remaining_quantity_hundredths),
        newly_filled: hundredths_quantity(record.newly_filled_quantity_hundredths),
        average_fill_price: record.average_fill_price_micros.map(price),
        fee_cost: record.fees_micros as f64 / 1_000_000.0,
        is_final: record.is_final,
        vanished: record.vanished,
    }
}

// ---------------------------------------------------------------------------------------------
// State projection
// ---------------------------------------------------------------------------------------------

/// The canonical scoped state of one decision, built once from the context.
///
/// Every component is projected from its supplied original when the context carries one and
/// from the V4 owner projection otherwise; the resulting [`StationState`] and [`MarketState`]
/// values are what a kernel reads through [`StrategyKernelState`].
#[derive(Clone, Debug)]
pub struct KernelSnapshot {
    now: DateTime<Utc>,
    stations: Vec<StationState>,
    markets: Vec<MarketState>,
    parameters: StrategyParameters,
    contributor_stations: Vec<String>,
    capabilities: KernelCapabilities,
    pending_timers: Vec<PendingTimer>,
}

impl KernelSnapshot {
    pub fn from_context(context: &DecisionContextV6) -> Result<Self, KernelTransactionError> {
        let stations = context
            .owner_state
            .stations
            .iter()
            .map(|station| {
                let station_id = &station.identity.station_id;
                station_state(
                    station,
                    context.supplied.station(station_id),
                    context
                        .current_weather
                        .iter()
                        .flatten()
                        .find(|weather| weather.station_id == *station_id)
                        .map(|weather| &weather.facts),
                    context
                        .forecast_issuance
                        .iter()
                        .flatten()
                        .find(|issued| issued.station_id == *station_id)
                        .map(|issued| issued.models.as_slice()),
                    context.current_inputs.as_ref().and_then(|current| {
                        current
                            .stations
                            .iter()
                            .find(|input| input.station_id == *station_id)
                    }),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let markets = context
            .owner_state
            .markets
            .iter()
            .map(|market| market_state(market, &context.strategy.event_date, context))
            .collect::<Result<Vec<_>, _>>()?;
        let mut capabilities = KernelCapabilities::default();
        capabilities.mode = Some(match context.deployment_mode {
            DeploymentModeV6::Paper => RuntimeMode::Paper,
            DeploymentModeV6::Live => RuntimeMode::Live,
        });
        capabilities.timers = context.capabilities.timers;
        capabilities.timer_handles = context.capabilities.timers;
        capabilities.gauges = true;
        capabilities.annotations = true;
        capabilities.external_requests = context.capabilities.external_requests.clone();
        // The Broker refuses a Market sell outside paper (until Phase 5), per command.
        capabilities.market_sell = context.deployment_mode == DeploymentModeV6::Paper;
        Ok(Self {
            now: millis(Some(context.decision_time_unix_ms))?
                .ok_or(KernelTransactionError::InvalidTime)?,
            stations,
            markets,
            parameters: context
                .strategy
                .parameters
                .iter()
                .map(|(key, value)| (key.clone(), parameter_value(value)))
                .collect(),
            contributor_stations: context.contributor_stations().to_vec(),
            capabilities,
            pending_timers: context
                .owner_state
                .timer_recovery
                .iter()
                .flatten()
                .filter(|timer| timer.admission_state == TIMER_ACTIVE)
                .filter_map(|timer| {
                    Some(PendingTimer {
                        handle: TimerHandle {
                            key: timer.key.clone(),
                            generation: timer.generation.clone(),
                        },
                        scheduled_for: Utc.timestamp_nanos(i64::try_from(timer.scheduled_at).ok()?),
                    })
                })
                .collect(),
        })
    }

    pub fn station_states(&self) -> &[StationState] {
        &self.stations
    }

    pub fn market_states(&self) -> &[MarketState] {
        &self.markets
    }
}

impl StrategyKernelState for KernelSnapshot {
    fn station(&self, station_id: &str) -> Option<&StationState> {
        self.stations
            .iter()
            .find(|station| station_id.eq_ignore_ascii_case(station.station_id()))
    }

    fn market(&self, ticker: &str) -> Option<&MarketState> {
        self.markets
            .iter()
            .find(|market| market.market_id == ticker)
    }
}
impl StrategyKernelData for KernelSnapshot {}

// ---------------------------------------------------------------------------------------------
// Host
// ---------------------------------------------------------------------------------------------

/// An order this decision placed, as the provisional view shows it.
#[derive(Clone, Debug)]
struct ProvisionalOrder {
    client_order_id: String,
    ticker: String,
    action: OrderAction,
    side: ContractSide,
    limit_price: Option<f64>,
    quantity: ContractQuantity,
    commitment_micros: u64,
    cancellation_requested: bool,
}

/// One kernel log or telemetry entry, or a failed order update, in the order produced.
enum HostOutput {
    Log(LogAction),
    Telemetry(TelemetryEntryV6),
    /// A failed order update: a deferred one is a `warn` `order_update_deferred` diagnostic,
    /// a held one `order_update_held`, any other a `kernel_error`.
    UpdateError(String, Failure),
    /// A `warn` diagnostic about the delivery of order updates.
    UpdateWarning(&'static str, String),
}

/// What the decision had issued when an order update began.
#[derive(Clone, Copy)]
struct UpdateStart {
    commands: usize,
    entries: usize,
}

/// What a decision has issued so far, saved before each order update.
struct SavedDecision {
    finances: BrokerFinancialState,
    provisional: Vec<ProvisionalOrder>,
    cancellation_requested: BTreeSet<String>,
    commands: Vec<StrategyCommandV6>,
    runner: Vec<RunnerEntryV6>,
    rows: DecisionPlanRows,
    timer_keys: BTreeSet<String>,
    outputs_len: usize,
}

type MarketBuyCap<'a> =
    &'a dyn Fn(&PlaceOrderRequest) -> Result<Option<u64>, KernelTransactionError>;

/// The context handed to the kernel for one decision.
pub struct KernelHost<'a> {
    pub snapshot: KernelSnapshot,
    context: &'a DecisionContextV6,
    finances: BrokerFinancialState,
    provisional: Vec<ProvisionalOrder>,
    /// Context orders a cancel in this decision marked, by order id.
    cancellation_requested: BTreeSet<String>,
    commands: Vec<StrategyCommandV6>,
    runner: Vec<RunnerEntryV6>,
    rows: DecisionPlanRows,
    timer_keys: BTreeSet<String>,
    outputs: Vec<HostOutput>,
    market_buy_cap: Option<MarketBuyCap<'a>>,
    /// Encoded bytes the result needs besides its commands and runner entries.
    fixed_bytes: usize,
    /// Live runner entries a failed order update could bring back.
    reinstatable_live: usize,
    /// Plan rows of a decision without commands, to count what one update alone needs.
    empty_rows: DecisionPlanRows,
    /// Runner entries before the decision's own.
    derived_entries: usize,
    /// What the decision had issued when the current order update began.
    update_start: Option<UpdateStart>,
    /// The last refusal, in the current order update, for room in the decision that earlier
    /// updates took: returned by the kernel, it defers the update.
    deferrable: RefCell<Option<String>>,
}

impl<'a> KernelHost<'a> {
    /// A host over the context's state. `runner` is the runner section after this decision's
    /// comparison; `acknowledgements` whether the result acknowledges receipts or orders.
    pub fn new(
        context: &'a DecisionContextV6,
        runner: Vec<RunnerEntryV6>,
        acknowledgements: bool,
    ) -> Result<Self, KernelTransactionError> {
        let broker = &context.owner_state.broker;
        Ok(Self {
            snapshot: KernelSnapshot::from_context(context)?,
            context,
            finances: BrokerFinancialState {
                allowance_limit_micros: broker.allowance_limit,
                current_commitment_micros: broker.current_commitment,
                provider_available_balance_micros: broker.provider_available_balance,
                locally_reserved_cash_micros: broker.locally_reserved_cash,
            },
            provisional: Vec::new(),
            cancellation_requested: BTreeSet::new(),
            commands: Vec::new(),
            runner,
            rows: DecisionPlanRows::new(&context.broker, acknowledgements, context.deployment_mode),
            timer_keys: BTreeSet::new(),
            outputs: Vec::new(),
            market_buy_cap: None,
            fixed_bytes: RESULT_OVERHEAD_BYTES + wire::MAX_KERNEL_CHECKPOINT_BYTES,
            reinstatable_live: 0,
            empty_rows: DecisionPlanRows::new(
                &context.broker,
                acknowledgements,
                context.deployment_mode,
            ),
            derived_entries: 0,
            update_start: None,
            deferrable: RefCell::new(None),
        })
    }

    /// The commands issued so far, in issue order.
    pub fn commands(&self) -> &[StrategyCommandV6] {
        &self.commands
    }

    fn save(&self) -> SavedDecision {
        SavedDecision {
            finances: self.finances,
            provisional: self.provisional.clone(),
            cancellation_requested: self.cancellation_requested.clone(),
            commands: self.commands.clone(),
            runner: self.runner.clone(),
            rows: self.rows.clone(),
            timer_keys: self.timer_keys.clone(),
            outputs_len: self.outputs.len(),
        }
    }

    fn restore(&mut self, saved: SavedDecision) {
        self.finances = saved.finances;
        self.provisional = saved.provisional;
        self.cancellation_requested = saved.cancellation_requested;
        self.commands = saved.commands;
        self.runner = saved.runner;
        self.rows = saved.rows;
        self.timer_keys = saved.timer_keys;
        // Telemetry of a rolled-back update describes work that never happened, and so does a
        // warning about an update delivered out of order; its logs and the recorded failures
        // stay.
        let tail = self
            .outputs
            .split_off(saved.outputs_len.min(self.outputs.len()));
        self.outputs.extend(
            tail.into_iter().filter(|output| {
                matches!(output, HostOutput::Log(_) | HostOutput::UpdateError(..))
            }),
        );
    }

    fn broker_detail(&self) -> &BrokerDetailV6 {
        &self.context.broker
    }

    /// Starts an order update in a delivery unit that began at `unit` (now, if not yet):
    /// commands the unit's updates issue are the update's own, when deciding whether a
    /// refusal defers it.
    fn begin_update(&mut self, unit: &mut Option<UpdateStart>) {
        let start = *unit.get_or_insert(UpdateStart {
            commands: self.commands.len(),
            entries: self.runner.len(),
        });
        self.update_start = Some(start);
        self.deferrable.replace(None);
    }

    /// The refusal that defers the update, if the kernel returns it.
    fn end_update(&mut self) -> Option<String> {
        self.update_start = None;
        self.deferrable.take()
    }

    /// The commands and runner entries issued before the current update's delivery unit, if
    /// earlier updates of this decision issued any.
    fn issued_before(&self) -> Option<(&[StrategyCommandV6], &[RunnerEntryV6])> {
        let start = self.update_start?;
        (start.commands > 0).then(|| {
            (
                &self.commands[..start.commands],
                &self.runner[self.derived_entries.min(start.entries)..start.entries],
            )
        })
    }

    /// A call refused for room in the decision. `alone` tells whether the call would fit
    /// if the decision held only the commands of the current update's delivery unit: then
    /// earlier updates took the room, and the kernel returning this refusal defers the
    /// update.
    fn capacity_error(&self, message: String, alone: impl FnOnce(&Self) -> bool) -> KernelError {
        let deferrable = self.issued_before().is_some() && alone(self);
        self.deferrable.replace(deferrable.then(|| message.clone()));
        KernelError::new(message)
    }

    fn next_ordinal(&self) -> KernelResult<usize> {
        if self.commands.len() >= wire::MAX_STRATEGY_COMMANDS {
            // With commands issued before the update, the update's own fit alone.
            return Err(self.capacity_error(
                format!(
                    "a decision carries at most {} commands",
                    wire::MAX_STRATEGY_COMMANDS
                ),
                |_| true,
            ));
        }
        Ok(self.commands.len())
    }

    /// Admits a Broker command against the row budget and the runner section's bound.
    /// The result stays within its encoded budget with `command` (and its runner entry) added,
    /// whatever the kernel's state and telemetry: a command past it is a local error, so the
    /// result never exceeds its bound.
    fn check_result_bytes(
        &self,
        command: &StrategyCommandV6,
        entry: Option<&RunnerEntryV6>,
    ) -> KernelResult<()> {
        let bytes = self.fixed_bytes
            + wire::encoded_len(&self.commands)
            + wire::encoded_len(&self.runner)
            + wire::encoded_len(command)
            + entry.map_or(0, wire::encoded_len);
        if bytes > wire::RESULT_ENCODED_BUDGET_BYTES {
            return Err(self.capacity_error(
                "the decision's commands would exceed the result size bound".to_owned(),
                |host| {
                    host.issued_before().is_some_and(|(commands, entries)| {
                        let earlier = commands.iter().map(wire::encoded_len).sum::<usize>()
                            + entries.iter().map(wire::encoded_len).sum::<usize>();
                        bytes - earlier <= wire::RESULT_ENCODED_BUDGET_BYTES
                    })
                },
            ));
        }
        Ok(())
    }

    /// Adds a timer or stop command.
    fn push_command(&mut self, command: StrategyCommandV6) -> KernelResult<()> {
        self.check_result_bytes(&command, None)?;
        self.commands.push(command);
        Ok(())
    }

    fn issue_broker_command(
        &mut self,
        command: StrategyCommandV6,
        entry: RunnerEntryV6,
    ) -> KernelResult<()> {
        self.check_result_bytes(&command, Some(&entry))?;
        let rows = self.rows.with(&command, &self.context.broker);
        if rows.total() > wire::MAX_DECISION_PLAN_ROWS {
            return Err(self.capacity_error(
                format!(
                    "the decision's Broker commands need {} plan rows, over the limit of {}",
                    rows.total(),
                    wire::MAX_DECISION_PLAN_ROWS
                ),
                |host| {
                    let Some(start) = host.update_start else {
                        return false;
                    };
                    let mut alone = host.empty_rows.clone();
                    for issued in &host.commands[start.commands..] {
                        alone.add(issued, &host.context.broker);
                    }
                    alone.with(&command, &host.context.broker).total()
                        <= wire::MAX_DECISION_PLAN_ROWS
                },
            ));
        }
        // Tombstones do not count: they never keep the Strategy from cancelling. A Sleeve-wide
        // bound: never deferred.
        let live = self.runner.iter().filter(|entry| entry.is_live()).count();
        if live + self.reinstatable_live >= wire::MAX_RUNNER_ENTRIES {
            return Err(KernelError::new(format!(
                "the runner tracks at most {} orders and commands",
                wire::MAX_RUNNER_ENTRIES
            )));
        }
        self.rows = rows;
        self.runner.push(entry);
        self.commands.push(command);
        Ok(())
    }

    fn place(&mut self, request: PlaceOrderRequest) -> KernelResult<OrderTicket> {
        let ordinal = self.next_ordinal()?;
        let invalid = |reason: &str| Err(KernelError::new(format!("invalid order: {reason}")));
        let Some(market) = self.snapshot.market(&request.ticker) else {
            return invalid("the market is not in the Sleeve's scope");
        };
        let quantity = match u64::try_from(request.quantity.hundredths()) {
            Ok(quantity) if quantity > 0 => quantity,
            _ => return invalid("the quantity is not positive"),
        };
        let limit_price_micros = match (&request.order_type, request.limit_price) {
            (OrderType::Limit, Some(price)) => match price_micros(price) {
                Ok(micros) => Some(micros),
                Err(_) => return invalid("the limit price is not within [0, 1]"),
            },
            (OrderType::Limit, None) => return invalid("a limit order needs a limit price"),
            (OrderType::Market, None) => None,
            (OrderType::Market, Some(_)) => return invalid("a market order has no limit price"),
        };
        let market_price_cap_micros = if request.action == OrderAction::Buy
            && request.order_type == OrderType::Market
        {
            match self.market_buy_cap {
                Some(cap) => cap(&request).map_err(|error| KernelError::new(error.to_string()))?,
                None => None,
            }
        } else {
            None
        };
        let commitment_micros = if request.action == OrderAction::Buy {
            let terms = market
                .fee_terms()
                .map_err(|error| KernelError::new(format!("invalid order: {error}")))?;
            let cap = limit_price_micros
                .or(market_price_cap_micros)
                .unwrap_or(wire::MAX_PRICE_MICROS);
            fees::buy_commitment_micros(cap, request.quantity, terms)
                .map_err(|error| KernelError::new(format!("invalid order: {error}")))?
        } else {
            0
        };
        let context = self.context;
        let cap = wire::max_open_orders(context.deployment_mode);
        if self.open_orders() >= cap {
            // A Sleeve-wide bound: never deferred.
            return Err(KernelError::new(format!(
                "a Sleeve holds at most {cap} open orders"
            )));
        }
        let provider_client_id = match &request.client_order_id {
            Some(client) if client.starts_with(DERIVED_CLIENT_ID_PREFIX) => {
                return invalid("client order ids starting with \"tv3\" are reserved for the host");
            }
            Some(client) => client.clone(),
            None => wire::derive_provider_client_id_v6(
                context.deployment_mode,
                &context.owner_state.sleeve.sleeve_id,
                context.owner_state.sleeve.incarnation,
                &context.owner_state.delivery_id,
                u32::try_from(ordinal).expect("bounded ordinal"),
            )
            .ok_or_else(|| KernelError::new("the Sleeve id cannot derive a client order id"))?,
        };
        if !wire::valid_provider_client_id(&provider_client_id) {
            return invalid("the client order id is not a valid identifier of at most 128 bytes");
        }
        if self
            .provisional
            .iter()
            .any(|order| order.client_order_id == provider_client_id)
            || context
                .broker
                .orders
                .iter()
                .any(|order| order.provider_client_id == provider_client_id)
            || self
                .runner
                .iter()
                .any(|entry| entry.client_order_id.as_deref() == Some(&provider_client_id))
        {
            return invalid("the client order id is already in use");
        }
        let command_id = context.command_id(ordinal);
        let command = StrategyCommandV6::PlaceOrder(PlaceOrderV6 {
            command_id: command_id.clone(),
            market_id: request.ticker.clone(),
            action: order_action_v6(&request.action),
            side: contract_side_v6(&request.contract_side),
            order_type: match request.order_type {
                OrderType::Market => OrderTypeV6::Market,
                OrderType::Limit => OrderTypeV6::Limit,
            },
            quantity_hundredths: quantity,
            limit_price_micros,
            market_price_cap_micros,
            expires_after_ms: request.expires_after_ms,
            reduce_only: request.reduce_only,
            provider_client_id: provider_client_id.clone(),
            signal_type: request.signal_type.clone(),
            signal_metadata: request.signal_metadata.clone(),
            metadata: Vec::new(),
        });
        if wire::validate_command_v6(&command).is_err() {
            return invalid("the request is outside the command bounds");
        }
        let entry = RunnerEntryV6 {
            command_id: command_id.clone(),
            kind: BrokerCommandKindV6::PlaceOrder,
            client_order_id: Some(provider_client_id.clone()),
            order_id: None,
            market_id: Some(request.ticker.clone()),
            action: Some(order_action_v6(&request.action)),
            side: Some(contract_side_v6(&request.contract_side)),
            requested_quantity_hundredths: quantity,
            last_status: None,
            filled_quantity_hundredths: 0,
            order_revision: 0,
            issued_broker_revision: self.context.broker.revision,
            vanished: false,
            vanished_revision: 0,
            absent_views: 0,
            delivery_failures: 0,
            delivery_deferrals: 0,
        };
        self.issue_broker_command(command, entry)?;
        self.finances.current_commitment_micros = self
            .finances
            .current_commitment_micros
            .saturating_add(commitment_micros);
        self.finances.locally_reserved_cash_micros = self
            .finances
            .locally_reserved_cash_micros
            .saturating_add(commitment_micros);
        self.provisional.push(ProvisionalOrder {
            client_order_id: provider_client_id.clone(),
            ticker: request.ticker,
            action: request.action,
            side: request.contract_side,
            limit_price: request.limit_price,
            quantity: request.quantity,
            commitment_micros,
            cancellation_requested: false,
        });
        Ok(OrderTicket {
            command_id,
            client_order_id: provider_client_id,
        })
    }

    fn cancel(&mut self, request: CancelOrderRequest) -> KernelResult<CommandTicket> {
        let ordinal = self.next_ordinal()?;
        let context = self.context;
        let same_decision = |client: &str| {
            self.provisional
                .iter()
                .position(|order| order.client_order_id == client)
        };
        let (target, provisional, order) = match &request.target {
            CancelTarget::OrderId(order_id) => (
                None,
                None,
                context
                    .broker
                    .orders
                    .iter()
                    .find(|order| order.order_id == *order_id),
            ),
            CancelTarget::ClientOrderId(client) => match same_decision(client) {
                Some(index) => (
                    Some(CancelTargetV6::SameDecision {
                        provider_client_id: client.clone(),
                    }),
                    Some(index),
                    None,
                ),
                None => (
                    None,
                    None,
                    context
                        .broker
                        .orders
                        .iter()
                        .find(|order| order.provider_client_id == *client),
                ),
            },
        };
        let command_id = context.command_id(ordinal);
        let entry = |client: Option<String>,
                     order_id: Option<String>,
                     market: String,
                     action: OrderActionV6,
                     side: ContractSideV6,
                     requested: u64,
                     filled: u64| RunnerEntryV6 {
            command_id: command_id.clone(),
            kind: BrokerCommandKindV6::CancelOrder,
            client_order_id: client,
            order_id,
            market_id: Some(market),
            action: Some(action),
            side: Some(side),
            requested_quantity_hundredths: requested,
            last_status: None,
            filled_quantity_hundredths: filled,
            order_revision: 0,
            issued_broker_revision: self.context.broker.revision,
            vanished: false,
            vanished_revision: 0,
            absent_views: 0,
            delivery_failures: 0,
            delivery_deferrals: 0,
        };
        let (target, entry) = match (target, provisional, order) {
            (Some(target), Some(index), _) => {
                let order = &self.provisional[index];
                let requested = u64::try_from(order.quantity.hundredths()).unwrap_or_default();
                let entry = entry(
                    Some(order.client_order_id.clone()),
                    None,
                    order.ticker.clone(),
                    order_action_v6(&order.action),
                    contract_side_v6(&order.side),
                    requested,
                    0,
                );
                (target, entry)
            }
            (_, _, Some(order)) => (
                CancelTargetV6::Order {
                    order_id: order.order_id.clone(),
                    expected_order_revision: order.revision,
                },
                entry(
                    Some(order.provider_client_id.clone()),
                    Some(order.order_id.clone()),
                    order.market_id.clone(),
                    order.action,
                    order.side,
                    order.quantity_hundredths,
                    order.filled_quantity_hundredths,
                ),
            ),
            _ => {
                return Err(KernelError::new(format!(
                    "invalid cancel: no order matches {:?}",
                    request.target
                )));
            }
        };
        let command = StrategyCommandV6::CancelOrder {
            command_id: command_id.clone(),
            target: target.clone(),
        };
        self.issue_broker_command(command, entry)?;
        match target {
            CancelTargetV6::SameDecision { .. } => {
                if let Some(index) = provisional {
                    self.provisional[index].cancellation_requested = true;
                }
            }
            CancelTargetV6::Order { order_id, .. } => {
                self.cancellation_requested.insert(order_id);
            }
        }
        Ok(CommandTicket { command_id })
    }

    fn cancel_all(&mut self) -> KernelResult<CommandTicket> {
        if !self.context.orders_complete {
            return Err(KernelError::new(
                "invalid cancel-all: the host's order view is truncated",
            ));
        }
        let ordinal = self.next_ordinal()?;
        let command_id = self.context.command_id(ordinal);
        let command = StrategyCommandV6::CancelAllOrders {
            command_id: command_id.clone(),
        };
        let entry = RunnerEntryV6 {
            command_id: command_id.clone(),
            kind: BrokerCommandKindV6::CancelAllOrders,
            client_order_id: None,
            order_id: None,
            market_id: None,
            action: None,
            side: None,
            requested_quantity_hundredths: 0,
            last_status: None,
            filled_quantity_hundredths: 0,
            order_revision: 0,
            issued_broker_revision: self.context.broker.revision,
            vanished: false,
            vanished_revision: 0,
            absent_views: 0,
            delivery_failures: 0,
            delivery_deferrals: 0,
        };
        self.issue_broker_command(command, entry)?;
        for order in &self.context.broker.orders {
            if !order.status.is_terminal() {
                self.cancellation_requested.insert(order.order_id.clone());
            }
        }
        for order in &mut self.provisional {
            order.cancellation_requested = true;
        }
        Ok(CommandTicket { command_id })
    }

    /// Open context orders plus this decision's places.
    fn open_orders(&self) -> usize {
        wire::open_orders(&self.context.broker) + self.provisional.len()
    }

    fn view_status(&self, order: &BrokerOrderV6) -> BrokerOrderStatus {
        let status = broker_order_status(order.status);
        if !status.is_terminal() && self.cancellation_requested.contains(&order.order_id) {
            BrokerOrderStatus::CancellationRequested
        } else {
            status
        }
    }

    fn check_timer_key(&self, key: &str) -> KernelResult<()> {
        if !self.context.capabilities.timers {
            return Err(KernelError::new("this Sleeve is not granted timers"));
        }
        if !wire::valid_identifier(key) {
            return Err(KernelError::new(format!("invalid timer key {key:?}")));
        }
        if self.timer_keys.contains(key) {
            return Err(KernelError::new(format!(
                "timer {key:?} is already scheduled or cancelled in this decision"
            )));
        }
        Ok(())
    }

    fn push_telemetry(&mut self, entry: TelemetryEntryV6) {
        self.outputs.push(HostOutput::Telemetry(entry));
    }
}

fn owned_fields(fields: &[(&str, &str)]) -> Vec<(String, String)> {
    fields
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

impl StrategyKernelContext for KernelHost<'_> {
    fn state(&self) -> &dyn StrategyKernelState {
        &self.snapshot
    }
    fn parameters(&self) -> &StrategyParameters {
        &self.snapshot.parameters
    }
    fn capabilities(&self) -> KernelCapabilities {
        self.snapshot.capabilities.clone()
    }
    fn contributor_stations(&self) -> &[String] {
        &self.snapshot.contributor_stations
    }
    fn data(&self) -> &dyn StrategyKernelData {
        &self.snapshot
    }
    fn broker(&mut self) -> &mut dyn StrategyKernelBroker {
        self
    }
    fn runtime(&mut self) -> &mut dyn StrategyKernelRuntime {
        self
    }
    fn telemetry(&mut self) -> &mut dyn StrategyKernelTelemetry {
        self
    }
    fn emit(&mut self, action: KernelAction) -> KernelResult<()> {
        match action {
            KernelAction::PlaceOrder(request) => self.place(request).map(drop),
            KernelAction::CancelOrder(request) => self.cancel(request).map(drop),
            KernelAction::CancelAllOrders(_) => self.cancel_all().map(drop),
            KernelAction::WakeAt(request) => self.wake_at(request).map(drop),
            KernelAction::Telemetry(counter) => {
                self.push_telemetry(TelemetryEntryV6::Counter {
                    name: counter.name,
                    value_bits: counter.value.to_bits(),
                    fields: counter.fields,
                });
                Ok(())
            }
            KernelAction::Log(log) => {
                self.outputs.push(HostOutput::Log(log));
                Ok(())
            }
            KernelAction::Stop(stop) => {
                let ordinal = self.next_ordinal()?;
                if stop.reason.is_empty() || stop.reason.len() > wire::MAX_REASON_BYTES {
                    return Err(KernelError::new("invalid stop reason"));
                }
                self.push_command(StrategyCommandV6::Stop {
                    command_id: self.context.command_id(ordinal),
                    reason: stop.reason,
                })
            }
        }
    }
}

impl StrategyKernelBroker for KernelHost<'_> {
    fn financial_state(&self) -> BrokerFinancialState {
        self.finances
    }
    fn buying_power(&self) -> Option<f64> {
        Some(self.finances.buying_power_micros() as f64 / 1_000_000.0)
    }
    fn position_quantity(&self, ticker: &str, side: ContractSide) -> ContractQuantity {
        self.broker_detail()
            .positions
            .iter()
            .find(|position| {
                position.market_id == ticker && position.side == contract_side_v6(&side)
            })
            .and_then(|position| i64::try_from(position.quantity_hundredths).ok())
            .map(ContractQuantity::from_hundredths)
            .unwrap_or(ContractQuantity::ZERO)
    }
    fn position_avg_price(&self, ticker: &str, side: ContractSide) -> Option<f64> {
        self.broker_detail()
            .positions
            .iter()
            .find(|position| {
                position.market_id == ticker && position.side == contract_side_v6(&side)
            })
            .filter(|position| position.quantity_hundredths > 0)
            .map(|position| position.average_entry_price())
    }
    fn pending_orders(&self) -> Vec<PendingOrderView<'_>> {
        let context_orders = self
            .broker_detail()
            .orders
            .iter()
            .filter(|order| !order.status.is_terminal())
            .map(|order| PendingOrderView {
                order_id: &order.order_id,
                ticker: &order.market_id,
                status: self.view_status(order).as_str(),
                action: match order.action {
                    OrderActionV6::Buy => "buy",
                    OrderActionV6::Sell => "sell",
                },
                contract_side: match order.side {
                    ContractSideV6::Yes => "yes",
                    ContractSideV6::No => "no",
                },
                limit_price: order.limit_price_micros.map(price),
                requested_quantity: hundredths_quantity(order.quantity_hundredths),
                filled_quantity: hundredths_quantity(order.filled_quantity_hundredths),
                remaining_quantity: hundredths_quantity(order.remaining_quantity_hundredths),
                reserved_cost: (order.reserved_principal_micros + order.reserved_fee_micros) as f64
                    / 1_000_000.0,
                client_order_id: Some(&order.provider_client_id),
                created_at: millis(order.created_at_unix_ms).ok().flatten(),
                updated_at: millis(order.updated_at_unix_ms).ok().flatten(),
            });
        let provisional = self.provisional.iter().map(|order| PendingOrderView {
            order_id: "",
            ticker: &order.ticker,
            status: provisional_status(order).as_str(),
            action: match order.action {
                OrderAction::Buy => "buy",
                OrderAction::Sell => "sell",
            },
            contract_side: match order.side {
                ContractSide::Yes => "yes",
                ContractSide::No => "no",
            },
            limit_price: order.limit_price,
            requested_quantity: order.quantity,
            filled_quantity: ContractQuantity::ZERO,
            remaining_quantity: order.quantity,
            reserved_cost: order.commitment_micros as f64 / 1_000_000.0,
            client_order_id: Some(&order.client_order_id),
            created_at: None,
            updated_at: None,
        });
        context_orders.chain(provisional).collect()
    }
    fn order_status(&self, client_order_id: &str) -> Option<OrderStatusView<'_>> {
        if let Some(order) = self
            .provisional
            .iter()
            .find(|order| order.client_order_id == client_order_id)
        {
            let status = provisional_status(order);
            return Some(OrderStatusView {
                order_id: "",
                client_order_id: &order.client_order_id,
                status,
                requested_quantity: order.quantity,
                filled_quantity: ContractQuantity::ZERO,
                remaining_quantity: order.quantity,
                reason: status.as_str(),
                updated_at: None,
            });
        }
        self.broker_detail()
            .orders
            .iter()
            .find(|order| {
                order.provider_client_id == client_order_id || order.order_id == client_order_id
            })
            .map(|order| {
                let status = self.view_status(order);
                OrderStatusView {
                    order_id: &order.order_id,
                    client_order_id: &order.provider_client_id,
                    status,
                    requested_quantity: hundredths_quantity(order.quantity_hundredths),
                    filled_quantity: hundredths_quantity(order.filled_quantity_hundredths),
                    remaining_quantity: hundredths_quantity(order.remaining_quantity_hundredths),
                    reason: status.as_str(),
                    updated_at: millis(order.updated_at_unix_ms).ok().flatten(),
                }
            })
    }
    fn place_order(&mut self, request: PlaceOrderRequest) -> KernelResult<OrderTicket> {
        self.place(request)
    }
    fn cancel_order(&mut self, request: CancelOrderRequest) -> KernelResult<CommandTicket> {
        self.cancel(request)
    }
    fn cancel_all_orders(&mut self) -> KernelResult<CommandTicket> {
        self.cancel_all()
    }
}

impl StrategyKernelRuntime for KernelHost<'_> {
    fn now(&self) -> Option<DateTime<Utc>> {
        Some(self.snapshot.now)
    }
    fn wake_at(&mut self, request: WakeAtRequest) -> KernelResult<TimerHandle> {
        let key = request
            .name
            .clone()
            .unwrap_or_else(|| DEFAULT_TIMER_KEY.to_owned());
        self.check_timer_key(&key)?;
        let ordinal = self.next_ordinal()?;
        let scheduled_at_epoch_ns = request
            .when
            .timestamp_nanos_opt()
            .and_then(|nanos| u64::try_from(nanos).ok())
            .ok_or_else(|| KernelError::new("the timer time is outside the epoch range"))?;
        let generation = self.context.timer_generation();
        self.push_command(StrategyCommandV6::ScheduleTimer {
            command_id: self.context.command_id(ordinal),
            key: key.clone(),
            scheduled_at_epoch_ns,
            generation: generation.clone(),
            semantics: Vec::new(),
        })?;
        self.timer_keys.insert(key.clone());
        Ok(TimerHandle { key, generation })
    }
    fn cancel_timer(&mut self, handle: &TimerHandle) -> KernelResult<()> {
        self.check_timer_key(&handle.key)?;
        if !wire::valid_identifier(&handle.generation) {
            return Err(KernelError::new(format!(
                "invalid timer generation {:?}",
                handle.generation
            )));
        }
        let ordinal = self.next_ordinal()?;
        self.push_command(StrategyCommandV6::CancelTimer {
            command_id: self.context.command_id(ordinal),
            key: handle.key.clone(),
            generation: handle.generation.clone(),
        })?;
        self.timer_keys.insert(handle.key.clone());
        Ok(())
    }
    fn pending_timers(&self) -> Vec<PendingTimer> {
        self.snapshot.pending_timers.clone()
    }
}

impl StrategyKernelTelemetry for KernelHost<'_> {
    fn counter(&mut self, name: &str, value: f64, fields: &[(&str, &str)]) -> KernelResult<()> {
        self.push_telemetry(TelemetryEntryV6::Counter {
            name: name.to_owned(),
            value_bits: value.to_bits(),
            fields: owned_fields(fields),
        });
        Ok(())
    }
    fn gauge(&mut self, name: &str, value: f64, fields: &[(&str, &str)]) -> KernelResult<()> {
        self.push_telemetry(TelemetryEntryV6::Gauge {
            name: name.to_owned(),
            value_bits: value.to_bits(),
            fields: owned_fields(fields),
        });
        Ok(())
    }
    fn annotate(
        &mut self,
        name: &str,
        value: AnnotationValue<'_>,
        fields: &[(&str, &str)],
    ) -> KernelResult<()> {
        self.push_telemetry(TelemetryEntryV6::Annotation {
            name: name.to_owned(),
            value: match value {
                AnnotationValue::Text(value) => AnnotationValueV6::Text(value.to_owned()),
                AnnotationValue::Integer(value) => AnnotationValueV6::Integer(value),
                AnnotationValue::Float(value) => AnnotationValueV6::FloatBits(value.to_bits()),
                AnnotationValue::Bool(value) => AnnotationValueV6::Bool(value),
                AnnotationValue::Null => AnnotationValueV6::Null,
            },
            fields: owned_fields(fields),
        });
        Ok(())
    }
}

fn provisional_status(order: &ProvisionalOrder) -> BrokerOrderStatus {
    if order.cancellation_requested {
        BrokerOrderStatus::CancellationRequested
    } else {
        BrokerOrderStatus::Submitted
    }
}

fn broker_order_status(status: BrokerOrderStatusV6) -> BrokerOrderStatus {
    match status {
        BrokerOrderStatusV6::DurablyAccepted => BrokerOrderStatus::Accepted,
        BrokerOrderStatusV6::Dispatched => BrokerOrderStatus::Dispatched,
        BrokerOrderStatusV6::Resting => BrokerOrderStatus::Resting,
        BrokerOrderStatusV6::PartiallyFilled => BrokerOrderStatus::PartiallyFilled,
        BrokerOrderStatusV6::Filled => BrokerOrderStatus::Filled,
        BrokerOrderStatusV6::CancellationRequested => BrokerOrderStatus::CancellationRequested,
        BrokerOrderStatusV6::Cancelled => BrokerOrderStatus::Cancelled,
        BrokerOrderStatusV6::Expired => BrokerOrderStatus::Expired,
        BrokerOrderStatusV6::Rejected => BrokerOrderStatus::Rejected,
        BrokerOrderStatusV6::RecoveryRequired => BrokerOrderStatus::RecoveryRequired,
    }
}

fn order_action(action: OrderActionV6) -> OrderAction {
    match action {
        OrderActionV6::Buy => OrderAction::Buy,
        OrderActionV6::Sell => OrderAction::Sell,
    }
}

fn order_action_v6(action: &OrderAction) -> OrderActionV6 {
    match action {
        OrderAction::Buy => OrderActionV6::Buy,
        OrderAction::Sell => OrderActionV6::Sell,
    }
}

fn contract_side(side: ContractSideV6) -> ContractSide {
    match side {
        ContractSideV6::Yes => ContractSide::Yes,
        ContractSideV6::No => ContractSide::No,
    }
}

fn contract_side_v6(side: &ContractSide) -> ContractSideV6 {
    match side {
        ContractSide::Yes => ContractSideV6::Yes,
        ContractSide::No => ContractSideV6::No,
    }
}

// ---------------------------------------------------------------------------------------------
// Result outputs
// ---------------------------------------------------------------------------------------------

/// Appends the kernel's logs (as diagnostics) and telemetry in order within the result bounds,
/// and a kernel error. Overflow is counted in an explicit diagnostic: commands, checkpoints and
/// order updates are never removed to fit telemetry.
fn append_outputs(
    outputs: &[HostOutput],
    error: Option<&KernelError>,
    result: &mut DecisionResultV6,
) -> Overflow {
    // Room for the kernel error and the overflow diagnostic.
    let diagnostic_room = wire::MAX_RESULT_DIAGNOSTICS - 2;
    // Logs and telemetry fill only the bytes the checkpoint, commands and updates leave.
    let mut room = wire::RESULT_ENCODED_BUDGET_BYTES
        .saturating_sub(wire::encoded_len(result))
        .saturating_sub(RESULT_OVERHEAD_BYTES);
    let mut take = |bytes: usize| {
        let fits = bytes <= room;
        if fits {
            room -= bytes;
        }
        fits
    };
    let mut telemetry_bytes = 0usize;
    let mut lost = 0usize;
    let mut lost_bytes = 0usize;
    for output in outputs {
        match output {
            HostOutput::Log(log) => {
                let valid_level = matches!(log.level.as_str(), "error" | "warn" | "info" | "debug");
                let message = if valid_level
                    && !log.message.is_empty()
                    && log.message.len() <= wire::MAX_RESULT_DIAGNOSTIC_BYTES
                {
                    log.message.clone()
                } else {
                    serde_json::json!({"level": log.level, "message": log.message}).to_string()
                };
                let severity = if valid_level {
                    log.level.as_str()
                } else {
                    "info"
                };
                let bytes = message.len() + 32;
                if message.len() <= wire::MAX_RESULT_DIAGNOSTIC_BYTES
                    && result.diagnostics.len() < diagnostic_room
                    && take(bytes)
                {
                    result.diagnostics.push(ResultDiagnosticV6 {
                        severity: severity.to_owned(),
                        code: "kernel_log".to_owned(),
                        message,
                    });
                } else if message.len() <= wire::MAX_EVIDENCE_PAYLOAD_BYTES
                    && result.evidence.len() < wire::MAX_RESULT_EVIDENCE
                    && take(bytes)
                {
                    result.evidence.push(ResultEvidenceV6 {
                        code: "kernel_log".to_owned(),
                        payload: message.into_bytes(),
                    });
                } else {
                    lost += 1;
                    lost_bytes += message.len();
                }
            }
            HostOutput::UpdateError(..) | HostOutput::UpdateWarning(..) => {
                let (severity, code, message) = match output {
                    HostOutput::UpdateError(message, failure) => {
                        let (severity, code) = match failure {
                            Failure::Counted => ("error", "kernel_error"),
                            Failure::Deferred => ("warn", "order_update_deferred"),
                            Failure::Held => ("warn", "order_update_held"),
                            Failure::RolledBack => ("warn", "order_update_rolled_back"),
                        };
                        (severity, code, message)
                    }
                    HostOutput::UpdateWarning(code, message) => ("warn", *code, message),
                    _ => unreachable!("matched above"),
                };
                let mut message = message.clone();
                truncate_utf8(&mut message, wire::MAX_RESULT_DIAGNOSTIC_BYTES);
                if result.diagnostics.len() < diagnostic_room && take(message.len() + 32) {
                    result.diagnostics.push(ResultDiagnosticV6 {
                        severity: severity.to_owned(),
                        code: code.to_owned(),
                        message,
                    });
                } else {
                    lost += 1;
                    lost_bytes += message.len();
                }
            }
            HostOutput::Telemetry(entry) => {
                let bytes = bincode::encode_to_vec(entry, wire::wire_config())
                    .map_or(usize::MAX, |bytes| bytes.len());
                if entry.is_valid()
                    && result.telemetry.len() < wire::MAX_RESULT_TELEMETRY
                    && telemetry_bytes.saturating_add(bytes) <= MAX_TELEMETRY_BYTES
                    && take(bytes)
                {
                    telemetry_bytes += bytes;
                    result.telemetry.push(entry.clone());
                } else {
                    lost += 1;
                    lost_bytes = lost_bytes.saturating_add(bytes);
                }
            }
        }
    }
    if let Some(error) = error {
        let mut message = error.to_string();
        if message.is_empty() {
            message = "kernel error".to_owned();
        }
        truncate_utf8(&mut message, wire::MAX_RESULT_DIAGNOSTIC_BYTES);
        result.diagnostics.push(ResultDiagnosticV6 {
            severity: "error".to_owned(),
            code: "kernel_error".to_owned(),
            message,
        });
    }
    Overflow { lost, lost_bytes }
}

/// Logs and telemetry the result could not keep.
#[derive(Clone, Copy, Default)]
struct Overflow {
    lost: usize,
    lost_bytes: usize,
}

/// Makes the result encode and decode within the result bound. The decoder charges every
/// integer at its full width, so a result within the encoded budget can still overcharge
/// (many tiny telemetry fields, for instance): while it does, telemetry and then logs are shed
/// from the end and counted in `kernel_telemetry_overflow`. Commands, the checkpoint and the
/// order updates are never shed; they fit the budget with room to spare.
fn fit_result(
    result: &mut DecisionResultV6,
    mut overflow: Overflow,
) -> Result<(), KernelTransactionError> {
    loop {
        result
            .diagnostics
            .retain(|diagnostic| diagnostic.code != "kernel_telemetry_overflow");
        if overflow.lost > 0 {
            result.diagnostics.push(ResultDiagnosticV6 {
                severity: "error".to_owned(),
                code: "kernel_telemetry_overflow".to_owned(),
                message: serde_json::json!({
                    "lost_entries": overflow.lost,
                    "lost_payload_bytes": overflow.lost_bytes,
                    "reason": "v6_result_bound"
                })
                .to_string(),
            });
        }
        match wire::encode_decision_result_v6(result) {
            Ok(_) => return Ok(()),
            Err(DecisionV6Error::BoundExceeded) => {}
            Err(error) => return Err(error.into()),
        }
        // Shed an eighth of what is left of the kind being shed, at least one entry.
        let telemetry = result.telemetry.len();
        let logs = result
            .evidence
            .iter()
            .filter(|evidence| evidence.code == "kernel_log")
            .count()
            + result
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "kernel_log")
                .count();
        if telemetry > 0 {
            for entry in result
                .telemetry
                .split_off(telemetry - telemetry.div_ceil(8))
            {
                overflow.lost += 1;
                overflow.lost_bytes += wire::encoded_len(&entry);
            }
        } else if logs > 0 {
            for _ in 0..logs.div_ceil(8) {
                let shed = if let Some(index) = result
                    .evidence
                    .iter()
                    .rposition(|evidence| evidence.code == "kernel_log")
                {
                    result.evidence.remove(index).payload.len()
                } else if let Some(index) = result
                    .diagnostics
                    .iter()
                    .rposition(|diagnostic| diagnostic.code == "kernel_log")
                {
                    result.diagnostics.remove(index).message.len()
                } else {
                    break;
                };
                overflow.lost += 1;
                overflow.lost_bytes += shed;
            }
        } else {
            return Err(DecisionV6Error::BoundExceeded.into());
        }
    }
}

/// Reports what the order-update comparison did, one diagnostic per kind.
/// One diagnostic per note code. An abandoned update is an error: the kernel was never told
/// of the fill it carried, which the diagnostic reports.
fn append_notes(notes: &[updates::Note], result: &mut DecisionResultV6) {
    let mut by_code = BTreeMap::<&str, Vec<&updates::Note>>::new();
    for note in notes {
        by_code.entry(note.code).or_default().push(note);
    }
    for (code, notes) in by_code {
        let named = notes
            .iter()
            .take(8)
            .map(|note| note.command_id.as_str())
            .collect::<Vec<_>>();
        let mut message = serde_json::json!({"count": notes.len(), "command_ids": named});
        let severity = if code == "order_update_abandoned" {
            message["lost_newly_filled_hundredths"] = notes
                .iter()
                .take(8)
                .map(|note| note.lost_hundredths)
                .collect::<Vec<_>>()
                .into();
            message["total_lost_newly_filled_hundredths"] = notes
                .iter()
                .map(|note| note.lost_hundredths)
                .fold(0_u64, u64::saturating_add)
                .into();
            "error"
        } else {
            "warn"
        };
        result.diagnostics.push(ResultDiagnosticV6 {
            severity: severity.to_owned(),
            code: code.to_owned(),
            message: message.to_string(),
        });
    }
}

fn truncate_utf8(value: &mut String, max_bytes: usize) {
    if value.len() > max_bytes {
        let mut end = max_bytes;
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        value.truncate(end);
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut output, byte| {
        write!(&mut output, "{byte:02x}").expect("String formatting cannot fail");
        output
    })
}
