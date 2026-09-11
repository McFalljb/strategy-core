//! Core-owned kernel projection and transaction runner for Decision V5.
//!
//! This module is the single definition of how a canonical [`DecisionContextV5`] is presented
//! to a `strategy_core_kernel::NativeKernel` and how the kernel's synchronous Broker calls are
//! carried through the V5 transaction (`AwaitingBrokerOutcome` → durable continuation → exact
//! Broker return → resumed invocation). Hosts and Strategy executables consume it instead of
//! re-interpreting fields.
//!
//! Projection rules:
//! - A view field is filled from the supplied original-precision value when the context carries
//!   one (`context.supplied`), and only otherwise from the derived V4 owner projection.
//! - Publication, observation, issuance, receipt and decision times are distinct: `emitted_at`
//!   on an event view is the provider's publication time, never the decision clock.
//! - Fields with no view slot remain reachable through
//!   `StrategyKernelState::canonical_context`, which downcasts to the full context.
//! - Whole-contract view quantities (`PriceLevelView::quantity`, ticker depths) are derived by
//!   truncation from exact hundredths; the exact values stay in the canonical context.

use std::borrow::Cow;

use chrono::{DateTime, TimeZone, Utc};
use strategy_core_kernel::{
    CancelOrderRequest, ContractQuantity, ContractSide, ForecastHourlySnapshot,
    ForecastInputSnapshot, ForecastModelSnapshot, ForecastUpdatedView, HighLowView, KernelAction,
    KernelError, KernelResult, MarketBracketView, ObservationView, OracleInputSnapshot,
    OracleModelScoreSnapshot, OracleScoresUpdatedView, OrderAction, OrderResult, OrderStatus,
    OrderStatusView, PendingOrderView, PlaceOrderRequest, PriceLevelView, PriceUpdateView,
    StationReportView, StationWeatherView, StrategyEventView, StrategyKernelBroker,
    StrategyKernelContext, StrategyKernelData, StrategyKernelRuntime, StrategyKernelState,
    StrategyKernelTelemetry, TickerPriceView, TimerWakeView, WakeAtRequest, WeatherEventSourceView,
    WeatherEventView,
};

use crate::decision_v4::{
    ExtremeV4, ForecastModelV4, MarketV4, ObservationV4, RankByV4, ReportV4, StationV4,
    WeatherEventV4,
};
use crate::decision_v5::{
    self as wire, BrokerCommandReturnV5, BrokerOrderStatusV5, CancelAllOrdersReturnV5,
    CancelOrderReturnV5, CommandFenceV5, ContractSideV5, DecisionContextV5, DecisionDispositionV5,
    DecisionResultV5, DecisionV5Error, KernelCheckpointV5, KernelOrderStatusV5, OrderActionV5,
    OrderTypeV5, OriginatingTriggerV5, OwnerTriggerV5, PlaceOrderReturnV5, PlaceOrderV5,
    ResultDiagnosticV5, StrategyCommandV5, StrategyParameterValueV5, TriggerV5,
};
use crate::supplied_v5::{
    DecimalV5, EventEnvelopeV5, ExtremeKindV5, SuppliedEventV5, SuppliedExtremeV5,
    SuppliedForecastV5, SuppliedObservationV5, SuppliedOracleTableV5, SuppliedReportV5,
    SuppliedStationV5, SuppliedWeatherEventV5,
};

/// Sentinel kernel error that marks the first economic Broker call of a transaction as deferred.
pub const DEFERRED_BROKER_CALL: &str = "v5_deferred_broker_call";

#[derive(Debug)]
pub enum KernelTransactionError {
    Contract(DecisionV5Error),
    UnsupportedStrategy(String),
    UnsupportedCheckpoint,
    Checkpoint(String),
    InvalidTime,
    InvalidQuantity,
    Kernel(String),
    MissingDeferredCommand,
    UnexpectedBrokerReturn,
}

impl std::fmt::Display for KernelTransactionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl std::error::Error for KernelTransactionError {}
impl From<DecisionV5Error> for KernelTransactionError {
    fn from(value: DecisionV5Error) -> Self {
        Self::Contract(value)
    }
}

/// Identity of the checkpoint codec a factory currently writes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelCheckpointCodec {
    pub profile: String,
    pub version: u32,
}

/// A frozen kernel as driven by the transaction runner.
pub trait TransactionKernel: Clone {
    fn on_start(&mut self, context: &mut dyn StrategyKernelContext) -> KernelResult<()>;
    fn on_event(
        &mut self,
        event: StrategyEventView<'_>,
        context: &mut dyn StrategyKernelContext,
    ) -> KernelResult<()>;
    /// Opaque private state for the durable checkpoint.
    fn encode_checkpoint_state(&self) -> Result<Vec<u8>, KernelTransactionError>;
    /// Authoritative maximum per-contract price for a Market buy the kernel just requested.
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
    fn create(&self, context: &DecisionContextV5) -> Result<Self::Kernel, KernelTransactionError>;
    fn restore(
        &self,
        context: &DecisionContextV5,
        checkpoint: &KernelCheckpointV5,
    ) -> Result<Self::Kernel, KernelTransactionError>;
    /// Telemetry/log code of gate explanations that must not consume a command ordinal.
    fn gate_telemetry_code(&self) -> Option<&str> {
        None
    }
}

/// Runs one V5 transaction: restore or create the kernel, present the exact originating event
/// over the projected state, and assemble the fenced result.
pub fn run_transaction<F: TransactionKernelFactory>(
    factory: &F,
    context: &DecisionContextV5,
) -> Result<DecisionResultV5, KernelTransactionError> {
    context.validate()?;
    let codec = factory.checkpoint_codec(&context.strategy.strategy_id)?;
    let restored = match &context.kernel_checkpoint {
        Some(checkpoint) => factory.restore(context, checkpoint)?,
        None => factory.create(context)?,
    };
    let pre_event_checkpoint = match &context.kernel_checkpoint {
        Some(checkpoint) => checkpoint.clone(),
        None => build_checkpoint(context, &codec, &restored, 1)?,
    };
    let mut candidate = restored.clone();
    let snapshot = KernelSnapshot::from_context(context)?;
    let replay_return = match &context.trigger {
        TriggerV5::BrokerOutcome { outcome, .. } => Some(outcome.return_value.clone()),
        _ => None,
    };
    let mut host = KernelHost::new(snapshot, replay_return);
    let event = KernelEvent::from_context(context)?;
    let outcome = event.run(&mut candidate, &mut host);
    let gate_code = factory.gate_telemetry_code();

    let decision = (|| {
        if let Some(command) = host.deferred_command.take() {
            if outcome
                .as_ref()
                .is_err_and(|error| error.message() != DEFERRED_BROKER_CALL)
            {
                return Err(KernelTransactionError::Kernel(
                    outcome.unwrap_err().to_string(),
                ));
            }
            let generation = context.continuation.as_ref().map_or(1, |commitment| {
                commitment.continuation_generation.saturating_add(1)
            });
            let continuation_id = format!(
                "continuation.{}.{}",
                context.owner_state.delivery_id, generation
            );
            let market_buy_price_cap_micros = match &command {
                DeferredCommand::Place(request)
                    if request.action == OrderAction::Buy
                        && request.order_type == strategy_core_kernel::OrderType::Market =>
                {
                    candidate.market_buy_price_cap_micros(request)?
                }
                _ => None,
            };
            let command = command.into_wire(
                context,
                &continuation_id,
                generation,
                market_buy_price_cap_micros,
            )?;
            let awaited_command_id = command.command_id().to_owned();
            let mut result = base_result(context)?;
            result.disposition = DecisionDispositionV5::AwaitingBrokerOutcome {
                continuation_id,
                continuation_generation: generation,
                awaited_command_id,
            };
            result.kernel_checkpoint = Some(pre_event_checkpoint);
            result.commands = vec![command];
            append_non_economic_actions(context, &host.actions, gate_code, &mut result)?;
            wire::validate_decision_result_v5(context, &result)?;
            return Ok(result);
        }

        if let Err(error) = outcome {
            let mut result = base_result(context)?;
            result.disposition = DecisionDispositionV5::Rejected;
            result.kernel_checkpoint = context.kernel_checkpoint.clone();
            result.diagnostics.push(ResultDiagnosticV5 {
                severity: "error".to_owned(),
                code: "kernel_error".to_owned(),
                message: error.to_string(),
            });
            append_kernel_telemetry(&host.actions, &mut result);
            wire::validate_decision_result_v5(context, &result)?;
            return Ok(result);
        }
        if host.replay_return.is_some() {
            return Err(KernelTransactionError::UnexpectedBrokerReturn);
        }
        let next_sequence = context
            .kernel_checkpoint
            .as_ref()
            .map_or(1, |checkpoint| checkpoint.sequence.saturating_add(1));
        let mut result = base_result(context)?;
        result.kernel_checkpoint = Some(build_checkpoint(
            context,
            &codec,
            &candidate,
            next_sequence,
        )?);
        append_non_economic_actions(context, &host.actions, gate_code, &mut result)?;
        wire::validate_decision_result_v5(context, &result)?;
        Ok(result)
    })();
    if decision.is_err() {
        // Keep the failing-process semantics: when no valid V5 result can be returned, prior
        // diagnostics follow the executable's stderr path and the host captures a bounded,
        // redacted snapshot with explicit dropped-byte accounting.
        for action in &host.actions {
            match action {
                KernelAction::Log(log) => eprintln!(
                    "{}",
                    serde_json::json!({"code": "kernel_log", "level": log.level, "message": log.message})
                ),
                KernelAction::Telemetry(counter) => eprintln!(
                    "{}",
                    serde_json::json!({"code": "kernel_telemetry", "name": counter.name, "value": counter.value, "value_bits": format!("{:016x}", counter.value.to_bits()), "fields": counter.fields})
                ),
                _ => {}
            }
        }
    }
    decision
}

/// The Strategy parameters projected without loss into a JSON object for kernel initializers.
pub fn strategy_parameters_json(
    context: &DecisionContextV5,
) -> Result<serde_json::Map<String, serde_json::Value>, KernelTransactionError> {
    let mut parameters = serde_json::Map::new();
    for (key, value) in &context.strategy.parameters {
        parameters.insert(key.clone(), parameter_json(value)?);
    }
    Ok(parameters)
}

fn parameter_json(
    value: &StrategyParameterValueV5,
) -> Result<serde_json::Value, KernelTransactionError> {
    Ok(match value {
        StrategyParameterValueV5::Null => serde_json::Value::Null,
        StrategyParameterValueV5::Bool(value) => serde_json::Value::Bool(*value),
        StrategyParameterValueV5::I64(value) => (*value).into(),
        StrategyParameterValueV5::U64(value) => (*value).into(),
        StrategyParameterValueV5::Decimal { coefficient, scale } => {
            let divisor = 10_f64.powi(i32::from(*scale));
            serde_json::Number::from_f64(*coefficient as f64 / divisor)
                .map(serde_json::Value::Number)
                .ok_or(KernelTransactionError::InvalidQuantity)?
        }
        StrategyParameterValueV5::String(value) => serde_json::Value::String(value.clone()),
    })
}

fn base_result(context: &DecisionContextV5) -> Result<DecisionResultV5, KernelTransactionError> {
    Ok(DecisionResultV5 {
        delivery_id: context.owner_state.delivery_id.clone(),
        sleeve_identity: context.owner_state.sleeve.sleeve_id.clone(),
        state_fence: hex(&wire::decision_fence_v5_sha256(context)?),
        expected_broker_revision: context.broker.revision,
        disposition: DecisionDispositionV5::Completed,
        kernel_checkpoint: None,
        commands: Vec::new(),
        evidence: Vec::new(),
        diagnostics: Vec::new(),
    })
}

fn build_checkpoint<K: TransactionKernel>(
    context: &DecisionContextV5,
    codec: &KernelCheckpointCodec,
    kernel: &K,
    sequence: u64,
) -> Result<KernelCheckpointV5, KernelTransactionError> {
    let mut checkpoint = KernelCheckpointV5 {
        codec_profile: codec.profile.clone(),
        codec_version: codec.version,
        strategy_id: context.strategy.strategy_id.clone(),
        strategy_profile: context.strategy.profile.clone(),
        profile_and_calculator_digest: context.strategy.profile_and_calculator_digest.clone(),
        sequence,
        state: kernel.encode_checkpoint_state()?,
        state_sha256: [0; 32],
    };
    checkpoint.state_sha256 = wire::kernel_checkpoint_v5_sha256(&checkpoint);
    Ok(checkpoint)
}

// ---------------------------------------------------------------------------------------------
// State projection
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct OwnedMarket {
    id: String,
    event_ticker: String,
    event_date: String,
    series_ticker: String,
    close_time: Option<DateTime<Utc>>,
    fee_type: String,
    fee_multiplier: Option<f64>,
    strike_type: String,
    floor_strike: Option<f64>,
    cap_strike: Option<f64>,
    last_price: Option<f64>,
    yes_bid: Option<f64>,
    yes_ask: Option<f64>,
    no_bid: Option<f64>,
    no_ask: Option<f64>,
    yes_bid_depth: Option<i64>,
    yes_ask_depth: Option<i64>,
    no_bid_depth: Option<i64>,
    no_ask_depth: Option<i64>,
    yes_bid_levels: Vec<PriceLevelView>,
    yes_ask_levels: Vec<PriceLevelView>,
    no_bid_levels: Vec<PriceLevelView>,
    no_ask_levels: Vec<PriceLevelView>,
    volume: Option<f64>,
    last_update: Option<DateTime<Utc>>,
}

impl OwnedMarket {
    fn ticker_view(&self) -> TickerPriceView<'_> {
        TickerPriceView {
            ticker: &self.id,
            source: "kalshi",
            event_ticker: &self.event_ticker,
            event_date: &self.event_date,
            series_ticker: &self.series_ticker,
            close_time: self.close_time,
            fee_type: &self.fee_type,
            fee_multiplier: self.fee_multiplier,
            strike_type: &self.strike_type,
            floor_strike: self.floor_strike,
            cap_strike: self.cap_strike,
            yes_price: self
                .last_price
                .or(self.yes_ask)
                .or(self.yes_bid)
                .unwrap_or(0.0),
            no_price: self
                .last_price
                .map(|value| 1.0 - value)
                .or(self.no_ask)
                .or(self.no_bid)
                .unwrap_or(0.0),
            yes_bid: self.yes_bid,
            yes_ask: self.yes_ask,
            no_bid: self.no_bid,
            no_ask: self.no_ask,
            yes_bid_depth: self.yes_bid_depth,
            yes_ask_depth: self.yes_ask_depth,
            no_bid_depth: self.no_bid_depth,
            no_ask_depth: self.no_ask_depth,
            yes_bid_levels: &self.yes_bid_levels,
            yes_ask_levels: &self.yes_ask_levels,
            no_bid_levels: &self.no_bid_levels,
            no_ask_levels: &self.no_ask_levels,
            orderbook_depth: self.yes_ask_depth,
            volume: self.volume,
            peak_yes_ask: self.yes_ask,
            last_update: self.last_update,
        }
    }

    fn bracket_view(&self) -> MarketBracketView<'_> {
        let ticker = self.ticker_view();
        MarketBracketView {
            market_id: &self.id,
            ticker: &self.id,
            yes_price: ticker.yes_price,
            no_price: ticker.no_price,
            event_ticker: &self.event_ticker,
            event_date: &self.event_date,
            close_time: self.close_time,
            strike_type: &self.strike_type,
            floor_strike: self.floor_strike,
            cap_strike: self.cap_strike,
            snapshot_time: self.last_update,
            yes_bid: self.yes_bid,
            yes_ask: self.yes_ask,
            no_bid: self.no_bid,
            no_ask: self.no_ask,
            yes_bid_depth: self.yes_bid_depth,
            yes_ask_depth: self.yes_ask_depth,
            no_bid_depth: self.no_bid_depth,
            no_ask_depth: self.no_ask_depth,
            yes_bid_levels: &self.yes_bid_levels,
            yes_ask_levels: &self.yes_ask_levels,
            no_bid_levels: &self.no_bid_levels,
            no_ask_levels: &self.no_ask_levels,
            orderbook_depth: self.yes_ask_depth,
            volume: self.volume,
        }
    }
}

#[derive(Clone, Debug)]
struct OwnedForecastHour {
    time: String,
    temperature_f: Option<f64>,
    temperature_c: Option<f64>,
    apparent_f: Option<f64>,
    humidity: Option<f64>,
    dew_point: Option<f64>,
    pressure: Option<f64>,
    wind_speed: Option<f64>,
    wind_direction: Option<f64>,
    wind_gust: Option<f64>,
    cloud_cover: Option<f64>,
    precipitation: Option<f64>,
}
#[derive(Clone, Debug)]
struct OwnedForecastModel {
    id: String,
    version: String,
    updated_at: Option<DateTime<Utc>>,
    issued_at: Option<DateTime<Utc>>,
    hourly: Vec<OwnedForecastHour>,
}
#[derive(Clone, Debug)]
struct OwnedForecast {
    station_id: String,
    received_at: Option<DateTime<Utc>>,
    models: Vec<OwnedForecastModel>,
}
#[derive(Clone, Debug)]
struct OwnedOracleScore {
    model_id: String,
    model_name: String,
    is_public: Option<bool>,
    high_mae: Option<f64>,
    low_mae: Option<f64>,
    combined_mae: Option<f64>,
    high_bias: Option<f64>,
    low_bias: Option<f64>,
    day_count: Option<i64>,
}
#[derive(Clone, Debug)]
struct OwnedOracle {
    station_id: String,
    received_at: Option<DateTime<Utc>>,
    mode: String,
    rank_by: String,
    days: String,
    range_start: String,
    range_end: String,
    updated_at: Option<DateTime<Utc>>,
    modes: Vec<String>,
    scores: Vec<OwnedOracleScore>,
}

/// Owned projection of the canonical context that serves the `StrategyKernelState` views.
#[derive(Clone, Debug)]
pub struct KernelSnapshot {
    now: DateTime<Utc>,
    station_id: String,
    weather: StationWeatherView,
    markets: Vec<OwnedMarket>,
    forecast: OwnedForecast,
    oracles: Vec<OwnedOracle>,
    broker: wire::BrokerDetailV5,
    buying_power: f64,
    context: DecisionContextV5,
}

impl KernelSnapshot {
    pub fn from_context(context: &DecisionContextV5) -> Result<Self, KernelTransactionError> {
        let station = scoped_station(context)?;
        let supplied = context.supplied.station(&context.strategy.station_id);
        let markets = context
            .owner_state
            .markets
            .iter()
            .map(|market| owned_market(market, &context.strategy.event_date))
            .collect::<Result<Vec<_>, _>>()?;
        let allowance_remaining = context
            .owner_state
            .broker
            .allowance_limit
            .saturating_sub(context.owner_state.broker.current_commitment);
        let account_cash_remaining = context
            .owner_state
            .broker
            .provider_available_balance
            .saturating_sub(context.owner_state.broker.locally_reserved_cash);
        let buying_power_micros = allowance_remaining.min(account_cash_remaining);
        Ok(Self {
            now: millis(Some(context.decision_time_unix_ms))?
                .ok_or(KernelTransactionError::InvalidTime)?,
            station_id: context.strategy.station_id.clone(),
            weather: station_weather(station, supplied)?,
            markets,
            forecast: owned_forecast(station, supplied)?,
            oracles: owned_oracles(station, supplied)?,
            broker: context.broker.clone(),
            buying_power: buying_power_micros as f64 / 1_000_000.0,
            context: context.clone(),
        })
    }

    pub fn context(&self) -> &DecisionContextV5 {
        &self.context
    }
}

fn scoped_station(context: &DecisionContextV5) -> Result<&StationV4, KernelTransactionError> {
    context
        .owner_state
        .stations
        .iter()
        .find(|station| station.identity.station_id == context.strategy.station_id)
        .ok_or_else(|| KernelTransactionError::Kernel("station missing".to_owned()))
}

fn station_weather(
    station: &StationV4,
    supplied: Option<&SuppliedStationV5>,
) -> Result<StationWeatherView, KernelTransactionError> {
    let observation = supplied.and_then(|station| station.observation.as_ref());
    let daily = supplied.and_then(|station| station.daily_extremes.as_ref());
    let weather = &station.weather;
    let derived = &station.observation;
    Ok(StationWeatherView {
        station_id: station.identity.station_id.clone(),
        current_temp: observation
            .and_then(|value| decimal_f64(value.temperature_f))
            .or_else(|| milli_c_to_f(weather.current_temperature_milli_c)),
        running_high: daily
            .and_then(|value| decimal_f64(value.daily_high_f))
            .or_else(|| milli_c_to_f(weather.running_high_milli_c)),
        running_low: daily
            .and_then(|value| decimal_f64(value.daily_low_f))
            .or_else(|| milli_c_to_f(weather.running_low_milli_c)),
        last_metar_time: millis(weather.last_metar_at_unix_ms)?,
        temp_min_f: observation
            .and_then(|value| decimal_f64(value.temp_min_f))
            .or_else(|| milli_c_to_f(derived.temperature_min_milli_c)),
        temp_max_f: observation
            .and_then(|value| decimal_f64(value.temp_max_f))
            .or_else(|| milli_c_to_f(derived.temperature_max_milli_c)),
        temp_min_c: observation
            .and_then(|value| decimal_f64(value.temp_min_c))
            .or_else(|| milli_c(derived.temperature_min_milli_c)),
        temp_max_c: observation
            .and_then(|value| decimal_f64(value.temp_max_c))
            .or_else(|| milli_c(derived.temperature_max_milli_c)),
        preliminary: weather.preliminary,
        dsm_high: milli_c_to_f(weather.dsm_high_milli_c),
        dsm_low: milli_c_to_f(weather.dsm_low_milli_c),
        dsm_high_time: millis(weather.dsm_high_at_unix_ms)?,
        dsm_low_time: millis(weather.dsm_low_at_unix_ms)?,
        six_hr_high: milli_c_to_f(weather.six_hour_high_milli_c),
        six_hr_low: milli_c_to_f(weather.six_hour_low_milli_c),
        last_dsm_time: millis(weather.dsm_high_at_unix_ms.or(weather.dsm_low_at_unix_ms))?,
        last_six_hr_time: None,
        asos_daily_high_f: daily
            .and_then(|value| decimal_f64(value.asos_daily_high_f))
            .or_else(|| milli_c_to_f(weather.asos_daily_high_milli_c)),
        asos_daily_low_f: daily
            .and_then(|value| decimal_f64(value.asos_daily_low_f))
            .or_else(|| milli_c_to_f(weather.asos_daily_low_milli_c)),
        wu_daily_high_f: daily
            .and_then(|value| decimal_f64(value.wu_daily_high_f))
            .or_else(|| milli_c_to_f(weather.wu_daily_high_milli_c)),
        wu_daily_low_f: daily
            .and_then(|value| decimal_f64(value.wu_daily_low_f))
            .or_else(|| milli_c_to_f(weather.wu_daily_low_milli_c)),
        wu_current_temp_f: daily
            .and_then(|value| decimal_f64(value.wu_current_temp_f))
            .or_else(|| milli_c_to_f(weather.wu_current_temperature_milli_c)),
        wu_current_temp_c: daily
            .and_then(|value| decimal_f64(value.wu_current_temp_c))
            .or_else(|| milli_c(weather.wu_current_temperature_milli_c)),
        wu_daily_high_c: daily
            .and_then(|value| decimal_f64(value.wu_daily_high_c))
            .or_else(|| milli_c(weather.wu_daily_high_milli_c)),
        wu_daily_low_c: daily
            .and_then(|value| decimal_f64(value.wu_daily_low_c))
            .or_else(|| milli_c(weather.wu_daily_low_milli_c)),
        wu_observation_time: match daily.and_then(|value| value.wu_observation_time_unix_ns) {
            Some(ns) => Some(nanos(ns)),
            None => millis(derived.wu_observation_at_unix_ms)?,
        },
        wu_fetched_at: match daily.and_then(|value| value.wu_fetched_at_unix_ns) {
            Some(ns) => Some(nanos(ns)),
            None => millis(derived.wu_fetched_at_unix_ms)?,
        },
        dewpoint: observation
            .and_then(|value| decimal_f64(value.dewpoint))
            .or_else(|| micros(weather.dewpoint_micros)),
        heat_index: observation
            .and_then(|value| decimal_f64(value.heat_index))
            .or_else(|| micros(weather.heat_index_micros)),
        wind_chill: observation
            .and_then(|value| decimal_f64(value.wind_chill))
            .or_else(|| micros(weather.wind_chill_micros)),
        relative_humidity: observation
            .and_then(|value| decimal_f64(value.relative_humidity))
            .or_else(|| micros(weather.relative_humidity_micros)),
        wind_speed: observation
            .and_then(|value| decimal_f64(value.wind_speed))
            .or_else(|| micros(weather.wind_speed_micros)),
        wind_direction: observation
            .and_then(|value| decimal_f64(value.wind_direction))
            .or_else(|| micros(weather.wind_direction_micros)),
        wind_gust: observation
            .and_then(|value| decimal_f64(value.wind_gust))
            .or_else(|| micros(weather.wind_gust_micros)),
        text_description: observation
            .and_then(|value| value.text_description.clone())
            .or_else(|| weather.text_description.clone()),
        lag_seconds: observation
            .and_then(|value| value.lag_seconds)
            .or_else(|| derived.lag_ms.map(|value| value / 1_000)),
    })
}

fn owned_forecast(
    station: &StationV4,
    supplied: Option<&SuppliedStationV5>,
) -> Result<OwnedForecast, KernelTransactionError> {
    if let Some(forecast) = supplied.and_then(|station| station.forecast.as_ref()) {
        return Ok(supplied_forecast(&station.identity.station_id, forecast));
    }
    Ok(OwnedForecast {
        station_id: station.identity.station_id.clone(),
        received_at: millis(station.forecast_meta.updated_at_unix_ms)?,
        models: station
            .forecast
            .models
            .iter()
            .map(derived_forecast_model)
            .collect(),
    })
}

fn supplied_forecast(station_id: &str, forecast: &SuppliedForecastV5) -> OwnedForecast {
    OwnedForecast {
        station_id: station_id.to_owned(),
        received_at: Some(nanos(forecast.received_at_unix_ns)),
        models: forecast
            .models
            .iter()
            .map(|model| OwnedForecastModel {
                id: model.model_id.clone(),
                version: model.fetched_at.clone().unwrap_or_default(),
                updated_at: model.fetched_at_unix_ns.map(nanos),
                issued_at: None,
                hourly: model
                    .hourly
                    .iter()
                    .map(|point| OwnedForecastHour {
                        time: point.time.clone(),
                        temperature_f: decimal_f64(point.temperature_2m_f),
                        temperature_c: decimal_f64(point.temperature_2m_c),
                        apparent_f: decimal_f64(point.apparent_temperature_f),
                        humidity: decimal_f64(point.relative_humidity_2m),
                        dew_point: decimal_f64(point.dew_point_2m),
                        pressure: decimal_f64(point.pressure_msl),
                        wind_speed: decimal_f64(point.wind_speed_10m),
                        wind_direction: decimal_f64(point.wind_direction_10m),
                        wind_gust: decimal_f64(point.wind_gusts_10m),
                        cloud_cover: decimal_f64(point.cloud_cover),
                        precipitation: decimal_f64(point.precipitation_probability),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn derived_forecast_model(model: &ForecastModelV4) -> OwnedForecastModel {
    OwnedForecastModel {
        id: model.model_id.clone(),
        version: model.version.clone(),
        updated_at: millis(model.fetched_at_unix_ms).ok().flatten(),
        issued_at: millis(model.issued_at_unix_ms).ok().flatten(),
        hourly: model
            .hourly
            .iter()
            .map(|point| OwnedForecastHour {
                time: millis(Some(point.at_unix_ms))
                    .ok()
                    .flatten()
                    .map(|value| value.to_rfc3339())
                    .unwrap_or_default(),
                temperature_f: milli_c_to_f(point.temperature_milli_c),
                temperature_c: milli_c(point.temperature_milli_c),
                apparent_f: milli_c_to_f(point.apparent_temperature_milli_c),
                humidity: millionths(point.humidity_millionths),
                dew_point: milli_c(point.dew_point_milli_c),
                pressure: micros(point.pressure_msl_micros),
                wind_speed: micros(point.wind_speed_micros),
                wind_direction: micros(point.wind_direction_micros),
                wind_gust: micros(point.wind_gust_micros),
                cloud_cover: millionths(point.cloud_cover_millionths),
                precipitation: millionths(point.precipitation_probability_millionths),
            })
            .collect(),
    }
}

fn owned_oracles(
    station: &StationV4,
    supplied: Option<&SuppliedStationV5>,
) -> Result<Vec<OwnedOracle>, KernelTransactionError> {
    if let Some(tables) = supplied
        .map(|station| &station.oracle_tables)
        .filter(|tables| !tables.is_empty())
    {
        return Ok(tables.iter().map(supplied_oracle).collect());
    }
    let oracle = &station.oracle;
    Ok(vec![OwnedOracle {
        station_id: oracle.query.station_id.clone(),
        received_at: millis(oracle.updated_at_unix_ms)?,
        mode: oracle.query.mode.clone(),
        rank_by: match oracle.query.rank_by {
            RankByV4::High => "high",
            RankByV4::Low => "low",
        }
        .to_owned(),
        days: oracle.query.days.to_string(),
        range_start: oracle.range_start.clone(),
        range_end: oracle.range_end.clone(),
        updated_at: millis(station.oracle_meta.updated_at_unix_ms)?,
        modes: vec![oracle.query.mode.clone()],
        scores: oracle
            .rows
            .iter()
            .map(|row| OwnedOracleScore {
                model_id: row.model_id.clone(),
                model_name: row.model_name.clone(),
                is_public: row.is_public,
                high_mae: millionths(row.high_mae_millionths),
                low_mae: millionths(row.low_mae_millionths),
                combined_mae: millionths(row.combined_mae_millionths),
                high_bias: millionths(row.high_bias_millionths),
                low_bias: millionths(row.low_bias_millionths),
                day_count: row.day_count.map(i64::from),
            })
            .collect(),
    }])
}

fn supplied_oracle(table: &SuppliedOracleTableV5) -> OwnedOracle {
    OwnedOracle {
        station_id: table.station_id.clone(),
        received_at: Some(nanos(table.received_at_unix_ns)),
        mode: table.score_mode.clone().unwrap_or_default(),
        rank_by: table.rank_by.clone().unwrap_or_default(),
        days: table
            .days_requested
            .map(|days| days.to_string())
            .unwrap_or_default(),
        range_start: table.range_start.clone(),
        range_end: table.range_end.clone(),
        updated_at: table.notification_updated_at_unix_ns.map(nanos),
        modes: table.notification_modes.clone(),
        scores: table
            .scores
            .iter()
            .map(|score| OwnedOracleScore {
                model_id: score.model_id.clone(),
                model_name: score.model_name.clone(),
                is_public: score.is_public,
                high_mae: decimal_f64(score.high_mae),
                low_mae: decimal_f64(score.low_mae),
                combined_mae: decimal_f64(score.combined_mae),
                high_bias: decimal_f64(score.high_bias),
                low_bias: decimal_f64(score.low_bias),
                day_count: score.day_count,
            })
            .collect(),
    }
}

impl OwnedOracle {
    fn snapshot(&self) -> OracleInputSnapshot<'_> {
        OracleInputSnapshot {
            station_id: &self.station_id,
            received_at: self.received_at,
            source: "minutetemp",
            score_mode: &self.mode,
            rank_by: &self.rank_by,
            days_requested: &self.days,
            range_start: &self.range_start,
            range_end: &self.range_end,
            scores: Cow::Owned(
                self.scores
                    .iter()
                    .map(|score| OracleModelScoreSnapshot {
                        model_id: &score.model_id,
                        model_name: &score.model_name,
                        is_public: score.is_public,
                        high_mae: score.high_mae,
                        low_mae: score.low_mae,
                        combined_mae: score.combined_mae,
                        high_bias: score.high_bias,
                        low_bias: score.low_bias,
                        day_count: score.day_count,
                    })
                    .collect(),
            ),
        }
    }
}

impl StrategyKernelState for KernelSnapshot {
    fn get_price(&self, ticker: &str) -> Option<TickerPriceView<'_>> {
        self.markets
            .iter()
            .find(|market| market.id == ticker)
            .map(OwnedMarket::ticker_view)
    }
    fn get_weather(&self, station_id: &str) -> Option<StationWeatherView> {
        (station_id.eq_ignore_ascii_case(&self.station_id)).then(|| self.weather.clone())
    }
    fn latest_forecast(&self, station_id: &str) -> Option<ForecastInputSnapshot<'_>> {
        if !station_id.eq_ignore_ascii_case(&self.forecast.station_id) {
            return None;
        }
        let models = self
            .forecast
            .models
            .iter()
            .map(|model| {
                let hourly = model
                    .hourly
                    .iter()
                    .map(|point| ForecastHourlySnapshot {
                        time: &point.time,
                        temperature_2m_f: point.temperature_f,
                        temperature_2m_c: point.temperature_c,
                        apparent_temperature_f: point.apparent_f,
                        relative_humidity_2m: point.humidity,
                        dew_point_2m: point.dew_point,
                        pressure_msl: point.pressure,
                        wind_speed_10m: point.wind_speed,
                        wind_direction_10m: point.wind_direction,
                        wind_gusts_10m: point.wind_gust,
                        cloud_cover: point.cloud_cover,
                        precipitation_probability: point.precipitation,
                    })
                    .collect::<Vec<_>>();
                ForecastModelSnapshot {
                    model_id: &model.id,
                    // Derived summary retained for the frozen kernel contract: the maximum
                    // supplied point temperature.
                    value: model
                        .hourly
                        .iter()
                        .filter_map(|point| point.temperature_f)
                        .fold(f64::NEG_INFINITY, f64::max),
                    version: &model.version,
                    updated_at: model.updated_at,
                    run_issued_at: model.issued_at,
                    hourly: Cow::Owned(hourly),
                }
            })
            .collect::<Vec<_>>();
        Some(ForecastInputSnapshot {
            station_id: &self.forecast.station_id,
            received_at: self.forecast.received_at,
            source: "minutetemp",
            models: Cow::Owned(models),
        })
    }
    fn latest_oracle_scores(
        &self,
        station_id: &str,
        mode: Option<&str>,
        rank_by: Option<&str>,
        days: Option<&str>,
    ) -> Option<OracleInputSnapshot<'_>> {
        self.oracles
            .iter()
            .find(|oracle| {
                station_id.eq_ignore_ascii_case(&oracle.station_id)
                    && mode.is_none_or(|value| value == oracle.mode)
                    && rank_by.is_none_or(|value| value == oracle.rank_by)
                    && days.is_none_or(|value| value == oracle.days)
            })
            .map(OwnedOracle::snapshot)
    }
    fn canonical_context(&self) -> Option<&dyn std::any::Any> {
        Some(&self.context)
    }
}
impl StrategyKernelData for KernelSnapshot {}

// ---------------------------------------------------------------------------------------------
// Broker bridge
// ---------------------------------------------------------------------------------------------

/// Host context handed to the kernel: projected state, deferred economic call, replayed return.
pub struct KernelHost {
    pub snapshot: KernelSnapshot,
    pub replay_return: Option<BrokerCommandReturnV5>,
    pub deferred_command: Option<DeferredCommand>,
    pub actions: Vec<KernelAction>,
}

impl KernelHost {
    pub fn new(snapshot: KernelSnapshot, replay_return: Option<BrokerCommandReturnV5>) -> Self {
        Self {
            snapshot,
            replay_return,
            deferred_command: None,
            actions: Vec::new(),
        }
    }

    fn emit_economic(&mut self, command: DeferredCommand) -> KernelResult<()> {
        match self.replay_return.take() {
            Some(BrokerCommandReturnV5::PlaceOrder(PlaceOrderReturnV5::Ok(_)))
                if matches!(&command, DeferredCommand::Place(_)) =>
            {
                Ok(())
            }
            Some(BrokerCommandReturnV5::PlaceOrder(PlaceOrderReturnV5::Err(error)))
                if matches!(&command, DeferredCommand::Place(_)) =>
            {
                Err(KernelError::new(error.message))
            }
            Some(BrokerCommandReturnV5::CancelOrder(CancelOrderReturnV5::Ok(_)))
                if matches!(&command, DeferredCommand::Cancel(_)) =>
            {
                Ok(())
            }
            Some(BrokerCommandReturnV5::CancelOrder(CancelOrderReturnV5::Err(error)))
                if matches!(&command, DeferredCommand::Cancel(_)) =>
            {
                Err(KernelError::new(error.message))
            }
            Some(BrokerCommandReturnV5::CancelAllOrders(CancelAllOrdersReturnV5::Ok {
                ..
            })) if matches!(&command, DeferredCommand::CancelAll) => Ok(()),
            Some(BrokerCommandReturnV5::CancelAllOrders(CancelAllOrdersReturnV5::Err(error)))
                if matches!(&command, DeferredCommand::CancelAll) =>
            {
                Err(KernelError::new(error.message))
            }
            Some(other) => {
                self.replay_return = Some(other);
                Err(KernelError::new("unexpected broker return kind"))
            }
            None => self.defer(command),
        }
    }

    fn defer(&mut self, command: DeferredCommand) -> KernelResult<()> {
        if self.deferred_command.replace(command).is_some() {
            return Err(KernelError::new("multiple deferred broker commands"));
        }
        Err(KernelError::new(DEFERRED_BROKER_CALL))
    }
}

impl StrategyKernelContext for KernelHost {
    fn state(&self) -> &dyn StrategyKernelState {
        &self.snapshot
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
            KernelAction::PlaceOrder(request) => {
                self.emit_economic(DeferredCommand::Place(request))
            }
            KernelAction::CancelOrder(request) => {
                self.emit_economic(DeferredCommand::Cancel(request))
            }
            KernelAction::CancelAllOrders(_) => self.emit_economic(DeferredCommand::CancelAll),
            other => {
                self.actions.push(other);
                Ok(())
            }
        }
    }
}

impl StrategyKernelBroker for KernelHost {
    fn buying_power(&self) -> Option<f64> {
        Some(self.snapshot.buying_power)
    }
    fn position_quantity(&self, ticker: &str, side: ContractSide) -> ContractQuantity {
        self.snapshot
            .broker
            .positions
            .iter()
            .find(|position| position.market_id == ticker && side_matches(position.side, &side))
            .and_then(|position| i64::try_from(position.quantity_hundredths).ok())
            .map(ContractQuantity::from_hundredths)
            .unwrap_or_else(|| ContractQuantity::from_hundredths(0))
    }
    fn position_avg_price(&self, ticker: &str, side: ContractSide) -> Option<f64> {
        self.snapshot
            .broker
            .positions
            .iter()
            .find(|position| position.market_id == ticker && side_matches(position.side, &side))
            .filter(|position| position.quantity_hundredths > 0)
            .map(|position| position.average_entry_price())
    }
    fn pending_orders(&self) -> Vec<PendingOrderView<'_>> {
        self.snapshot
            .broker
            .orders
            .iter()
            .filter(|order| {
                !matches!(
                    order.status,
                    BrokerOrderStatusV5::Filled
                        | BrokerOrderStatusV5::Cancelled
                        | BrokerOrderStatusV5::Expired
                        | BrokerOrderStatusV5::Rejected
                )
            })
            .map(|order| PendingOrderView {
                order_id: &order.order_id,
                ticker: &order.market_id,
                status: order_status_str(order.status),
                action: match order.action {
                    OrderActionV5::Buy => "buy",
                    OrderActionV5::Sell => "sell",
                },
                contract_side: match order.side {
                    ContractSideV5::Yes => "yes",
                    ContractSideV5::No => "no",
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
            })
            .collect()
    }
    fn order_status(&self, client_order_id: &str) -> Option<OrderStatusView<'_>> {
        self.snapshot
            .broker
            .orders
            .iter()
            .find(|order| {
                order.provider_client_id == client_order_id || order.order_id == client_order_id
            })
            .map(|order| OrderStatusView {
                order_id: &order.order_id,
                client_order_id: &order.provider_client_id,
                status: kernel_order_status(order.status),
                requested_quantity: hundredths_quantity(order.quantity_hundredths),
                filled_quantity: hundredths_quantity(order.filled_quantity_hundredths),
                remaining_quantity: hundredths_quantity(order.remaining_quantity_hundredths),
                reason: order_status_str(order.status),
                updated_at: millis(order.updated_at_unix_ms).ok().flatten(),
            })
    }
    fn place_order(&mut self, request: PlaceOrderRequest) -> KernelResult<OrderResult> {
        match self.replay_return.take() {
            Some(BrokerCommandReturnV5::PlaceOrder(value)) => map_place_return(value),
            Some(other) => {
                self.replay_return = Some(other);
                Err(KernelError::new("unexpected broker return kind"))
            }
            None => self
                .defer(DeferredCommand::Place(request))
                .and_then(|()| Err(KernelError::new(DEFERRED_BROKER_CALL))),
        }
    }
    fn cancel_order(&mut self, request: CancelOrderRequest) -> KernelResult<bool> {
        match self.replay_return.take() {
            Some(BrokerCommandReturnV5::CancelOrder(CancelOrderReturnV5::Ok(value))) => Ok(value),
            Some(BrokerCommandReturnV5::CancelOrder(CancelOrderReturnV5::Err(error))) => {
                Err(KernelError::new(error.message))
            }
            Some(other) => {
                self.replay_return = Some(other);
                Err(KernelError::new("unexpected broker return kind"))
            }
            None => self.defer(DeferredCommand::Cancel(request)).map(|()| false),
        }
    }
    fn cancel_all_orders(&mut self) -> KernelResult<usize> {
        match self.replay_return.take() {
            Some(BrokerCommandReturnV5::CancelAllOrders(CancelAllOrdersReturnV5::Ok {
                cancelled_order_ids,
            })) => Ok(cancelled_order_ids.len()),
            Some(BrokerCommandReturnV5::CancelAllOrders(CancelAllOrdersReturnV5::Err(error))) => {
                Err(KernelError::new(error.message))
            }
            Some(other) => {
                self.replay_return = Some(other);
                Err(KernelError::new("unexpected broker return kind"))
            }
            None => self.defer(DeferredCommand::CancelAll).map(|()| 0),
        }
    }
}

impl StrategyKernelRuntime for KernelHost {
    fn now(&self) -> Option<DateTime<Utc>> {
        Some(self.snapshot.now)
    }
    fn wake_at(&mut self, request: WakeAtRequest) -> KernelResult<()> {
        self.actions.push(KernelAction::WakeAt(request));
        Ok(())
    }
}

impl StrategyKernelTelemetry for KernelHost {
    fn counter(&mut self, name: &str, value: f64, fields: &[(&str, &str)]) -> KernelResult<()> {
        self.actions.push(KernelAction::Telemetry(
            strategy_core_kernel::TelemetryAction {
                name: name.to_owned(),
                value,
                fields: fields
                    .iter()
                    .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                    .collect(),
            },
        ));
        Ok(())
    }
}

pub enum DeferredCommand {
    Place(PlaceOrderRequest),
    Cancel(CancelOrderRequest),
    CancelAll,
}

impl DeferredCommand {
    pub fn into_wire(
        self,
        context: &DecisionContextV5,
        continuation_id: &str,
        generation: u64,
        market_buy_price_cap_micros: Option<u64>,
    ) -> Result<StrategyCommandV5, KernelTransactionError> {
        let fence = CommandFenceV5 {
            continuation_id: continuation_id.to_owned(),
            continuation_generation: generation,
            expected_broker_revision: context.broker.revision,
        };
        match self {
            Self::Place(request) => {
                let provider_client_id = request
                    .client_order_id
                    .clone()
                    .ok_or(KernelTransactionError::MissingDeferredCommand)?;
                Ok(StrategyCommandV5::PlaceOrder(PlaceOrderV5 {
                    command_id: format!("command.{provider_client_id}"),
                    fence,
                    market_id: request.ticker,
                    action: match request.action {
                        OrderAction::Buy => OrderActionV5::Buy,
                        OrderAction::Sell => OrderActionV5::Sell,
                    },
                    side: match request.contract_side {
                        ContractSide::Yes => ContractSideV5::Yes,
                        ContractSide::No => ContractSideV5::No,
                    },
                    order_type: match request.order_type {
                        strategy_core_kernel::OrderType::Market => OrderTypeV5::Market,
                        strategy_core_kernel::OrderType::Limit => OrderTypeV5::Limit,
                    },
                    quantity_hundredths: u64::try_from(request.quantity.hundredths())
                        .map_err(|_| KernelTransactionError::InvalidQuantity)?,
                    limit_price_micros: request.limit_price.map(price_micros).transpose()?,
                    market_price_cap_micros: market_buy_price_cap_micros,
                    expires_after_ms: request.expires_after_ms,
                    reduce_only: request.reduce_only,
                    provider_client_id,
                    signal_type: request.signal_type,
                    signal_metadata: request.signal_metadata,
                    metadata: Vec::new(),
                }))
            }
            Self::Cancel(request) => {
                let order = context
                    .broker
                    .orders
                    .iter()
                    .find(|order| {
                        order.order_id == request.order_id
                            || order.provider_client_id == request.order_id
                    })
                    .ok_or(KernelTransactionError::MissingDeferredCommand)?;
                Ok(StrategyCommandV5::CancelOrder {
                    command_id: format!("command.cancel.{}.{}", order.order_id, generation),
                    fence,
                    order_id: order.order_id.clone(),
                    expected_order_revision: order.revision,
                })
            }
            Self::CancelAll => Ok(StrategyCommandV5::CancelAllOrders {
                command_id: format!(
                    "command.cancel-all.{}.{}",
                    context.owner_state.delivery_id, generation
                ),
                fence,
            }),
        }
    }
}

/// Maps non-economic kernel actions to timer/stop commands with stable ordinals, then appends
/// bounded telemetry. Gate explanations identified by `gate_code` never consume an ordinal.
pub fn append_non_economic_actions(
    context: &DecisionContextV5,
    actions: &[KernelAction],
    gate_code: Option<&str>,
    result: &mut DecisionResultV5,
) -> Result<(), KernelTransactionError> {
    let mut index = 0;
    for action in actions {
        let added_gate = match (action, gate_code) {
            (KernelAction::Telemetry(counter), Some(code)) => counter.name == code,
            (KernelAction::Log(log), Some(code)) => {
                serde_json::from_str::<serde_json::Value>(&log.message)
                    .ok()
                    .is_some_and(|value| value["code"] == code)
            }
            _ => false,
        };
        if added_gate {
            continue;
        }
        let id = format!(
            "command.{}.{}",
            context.owner_state.delivery_id,
            index + 100
        );
        index += 1;
        match action {
            KernelAction::WakeAt(request) => {
                result.commands.push(StrategyCommandV5::ScheduleTimer {
                    command_id: id,
                    key: request
                        .name
                        .clone()
                        .unwrap_or_else(|| "kernel.wake".to_owned()),
                    scheduled_at_epoch_ns: u64::try_from(
                        request
                            .when
                            .timestamp_nanos_opt()
                            .ok_or(KernelTransactionError::InvalidTime)?,
                    )
                    .map_err(|_| KernelTransactionError::InvalidTime)?,
                    generation: format!("timer.{}", context.owner_state.delivery_id),
                    semantics: Vec::new(),
                })
            }
            KernelAction::Stop(stop) => result.commands.push(StrategyCommandV5::Stop {
                command_id: id,
                reason: stop.reason.clone(),
            }),
            KernelAction::Log(_) | KernelAction::Telemetry(_) => {}
            KernelAction::PlaceOrder(_)
            | KernelAction::CancelOrder(_)
            | KernelAction::CancelAllOrders(_) => {
                return Err(KernelTransactionError::MissingDeferredCommand);
            }
        }
    }
    append_kernel_telemetry(actions, result);
    Ok(())
}

/// Preserves complete log/telemetry messages within the result bound and accounts overflow.
/// Overflow is diagnostic-only: commands and checkpoints are never removed to fit telemetry.
pub fn append_kernel_telemetry(actions: &[KernelAction], result: &mut DecisionResultV5) {
    let mut lost = 0usize;
    let mut lost_bytes = 0usize;
    for action in actions {
        let (code, severity, message) = match action {
            KernelAction::Log(log) => (
                "kernel_log",
                match log.level.as_str() {
                    "error" | "warn" | "info" | "debug" => log.level.as_str(),
                    _ => "info",
                },
                if !log.message.is_empty()
                    && log.message.len() <= wire::MAX_RESULT_DIAGNOSTIC_BYTES
                    && matches!(log.level.as_str(), "error" | "warn" | "info" | "debug")
                {
                    log.message.clone()
                } else {
                    serde_json::json!({"level": log.level, "message": log.message}).to_string()
                },
            ),
            KernelAction::Telemetry(counter) => (
                "kernel_telemetry",
                "info",
                serde_json::json!({
                    "name": counter.name, "value": counter.value,
                    "value_bits": format!("{:016x}", counter.value.to_bits()),
                    "fields": counter.fields,
                })
                .to_string(),
            ),
            _ => continue,
        };
        let length = message.len();
        let diagnostic = length <= wire::MAX_RESULT_DIAGNOSTIC_BYTES
            && !message.is_empty()
            && result.diagnostics.len() < wire::MAX_RESULT_DIAGNOSTICS - 1;
        let evidence = !diagnostic
            && length <= wire::MAX_COMMAND_METADATA_BYTES
            && result.evidence.len() < wire::MAX_RESULT_EVIDENCE;
        if diagnostic {
            result.diagnostics.push(ResultDiagnosticV5 {
                severity: severity.to_owned(),
                code: code.to_owned(),
                message,
            });
        } else if evidence {
            result.evidence.push(wire::ResultEvidenceV5 {
                code: code.to_owned(),
                payload: message.into_bytes(),
            });
        }
        // Leave room for an explicit overflow diagnostic in the total frame bound.
        if (!diagnostic && !evidence)
            || wire::encode_decision_result_v5(result).map_or(true, |bytes| {
                bytes.len() > wire::MAX_DECISION_RESULT_V5_BYTES - 512
            })
        {
            if diagnostic {
                result.diagnostics.pop();
            }
            if evidence {
                result.evidence.pop();
            }
            lost += 1;
            lost_bytes += length;
        }
    }
    if lost > 0 {
        result.diagnostics.push(ResultDiagnosticV5 {
            severity: "error".to_owned(),
            code: "kernel_telemetry_overflow".to_owned(),
            message: serde_json::json!({
                "lost_actions": lost, "lost_payload_bytes": lost_bytes, "reason": "v5_result_bound"
            })
            .to_string(),
        });
    }
}

// ---------------------------------------------------------------------------------------------
// Event projection
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct OwnedEnvelope {
    event_id: Option<String>,
    sequence: Option<i64>,
    city_sequence: Option<i64>,
    emitted_at: Option<DateTime<Utc>>,
    slug: String,
    source_timestamp: Option<DateTime<Utc>>,
    wmo_emit_time: Option<DateTime<Utc>>,
    producer_received_at: Option<DateTime<Utc>>,
    live_published_at: Option<DateTime<Utc>>,
    persistence_status: Option<String>,
    producer_sequence: Option<i64>,
}

impl OwnedEnvelope {
    fn supplied(envelope: Option<&EventEnvelopeV5>, station_id: &str) -> Self {
        let Some(envelope) = envelope else {
            return Self::empty(station_id);
        };
        Self {
            event_id: Some(envelope.event_id.clone()),
            sequence: i64::try_from(envelope.sequence).ok(),
            city_sequence: envelope
                .city_sequence
                .and_then(|value| i64::try_from(value).ok()),
            emitted_at: Some(nanos(envelope.emitted_at_unix_ns)),
            slug: envelope
                .slug
                .clone()
                .unwrap_or_else(|| station_id.to_owned()),
            source_timestamp: envelope.source_timestamp_unix_ns.map(nanos),
            wmo_emit_time: envelope.wmo_emit_time_unix_ns.map(nanos),
            producer_received_at: envelope.producer_received_at_unix_ns.map(nanos),
            live_published_at: envelope.live_published_at_unix_ns.map(nanos),
            persistence_status: envelope.persistence_status.clone(),
            producer_sequence: envelope
                .producer_sequence
                .and_then(|value| i64::try_from(value).ok()),
        }
    }

    fn derived(provenance: &crate::decision_v4::ProvenanceV4, station_id: &str) -> Self {
        Self {
            event_id: provenance.event_id.clone(),
            sequence: provenance
                .sequence
                .and_then(|value| i64::try_from(value).ok()),
            city_sequence: provenance
                .city_sequence
                .and_then(|value| i64::try_from(value).ok()),
            emitted_at: millis(provenance.provider_at_unix_ms).ok().flatten(),
            slug: station_id.to_owned(),
            source_timestamp: None,
            wmo_emit_time: None,
            producer_received_at: None,
            live_published_at: None,
            persistence_status: None,
            producer_sequence: provenance
                .producer_sequence
                .and_then(|value| i64::try_from(value).ok()),
        }
    }

    fn empty(station_id: &str) -> Self {
        Self {
            event_id: None,
            sequence: None,
            city_sequence: None,
            emitted_at: None,
            slug: station_id.to_owned(),
            source_timestamp: None,
            wmo_emit_time: None,
            producer_received_at: None,
            live_published_at: None,
            persistence_status: None,
            producer_sequence: None,
        }
    }
}

#[derive(Clone, Debug)]
struct OwnedObservationEvent {
    envelope: OwnedEnvelope,
    station: String,
    observed_at: Option<DateTime<Utc>>,
    lag_seconds: Option<i64>,
    preliminary: bool,
    temperature_f: Option<f64>,
    temperature_c: Option<f64>,
    temp_min_f: Option<f64>,
    temp_max_f: Option<f64>,
    temp_min_c: Option<f64>,
    temp_max_c: Option<f64>,
    is_from_report: bool,
    report_type: Option<String>,
    source_report_id: Option<String>,
    wu_current_temp_f: Option<f64>,
    wu_current_temp_c: Option<f64>,
    wu_daily_high_f: Option<f64>,
    wu_daily_low_f: Option<f64>,
    wu_daily_high_c: Option<f64>,
    wu_daily_low_c: Option<f64>,
    wu_observation_time: Option<DateTime<Utc>>,
    wu_fetched_at: Option<DateTime<Utc>>,
    temperature_day_mode: Option<String>,
    temperature_day_date: Option<String>,
    wu_day_mode: Option<String>,
    wu_day_date: Option<String>,
    dewpoint: Option<f64>,
    heat_index: Option<f64>,
    wind_chill: Option<f64>,
    relative_humidity: Option<f64>,
    wind_speed: Option<f64>,
    wind_direction: Option<f64>,
    wind_gust: Option<f64>,
    text_description: Option<String>,
}

impl OwnedObservationEvent {
    fn supplied(event: &SuppliedObservationV5) -> Self {
        Self {
            envelope: OwnedEnvelope::supplied(event.envelope.as_ref(), &event.station_id),
            station: event.station_id.clone(),
            observed_at: Some(nanos(event.observed_at_unix_ns)),
            lag_seconds: event.lag_seconds,
            preliminary: event.preliminary,
            temperature_f: decimal_f64(event.temperature_f),
            temperature_c: decimal_f64(event.temperature_c),
            temp_min_f: decimal_f64(event.temp_min_f),
            temp_max_f: decimal_f64(event.temp_max_f),
            temp_min_c: decimal_f64(event.temp_min_c),
            temp_max_c: decimal_f64(event.temp_max_c),
            is_from_report: event.is_from_report,
            report_type: event.report_type.clone(),
            source_report_id: event.source_report_id.clone(),
            wu_current_temp_f: decimal_f64(event.wu_current_temp_f),
            wu_current_temp_c: decimal_f64(event.wu_current_temp_c),
            wu_daily_high_f: decimal_f64(event.wu_daily_high_f),
            wu_daily_low_f: decimal_f64(event.wu_daily_low_f),
            wu_daily_high_c: decimal_f64(event.wu_daily_high_c),
            wu_daily_low_c: decimal_f64(event.wu_daily_low_c),
            wu_observation_time: event.wu_observation_time_unix_ns.map(nanos),
            wu_fetched_at: event.wu_fetched_at_unix_ns.map(nanos),
            temperature_day_mode: event.temperature_day_mode.clone(),
            temperature_day_date: event.temperature_day_date.clone(),
            wu_day_mode: event.wu_day_mode.clone(),
            wu_day_date: event.wu_day_date.clone(),
            dewpoint: decimal_f64(event.dewpoint),
            heat_index: decimal_f64(event.heat_index),
            wind_chill: decimal_f64(event.wind_chill),
            relative_humidity: decimal_f64(event.relative_humidity),
            wind_speed: decimal_f64(event.wind_speed),
            wind_direction: decimal_f64(event.wind_direction),
            wind_gust: decimal_f64(event.wind_gust),
            text_description: event.text_description.clone(),
        }
    }

    fn derived(observation: &ObservationV4) -> Result<Self, KernelTransactionError> {
        Ok(Self {
            envelope: OwnedEnvelope::derived(&observation.provenance, &observation.station_id),
            station: observation.station_id.clone(),
            observed_at: millis(Some(observation.observed_at_unix_ms))?,
            lag_seconds: observation.lag_ms.map(|value| value / 1_000),
            preliminary: observation.preliminary,
            temperature_f: milli_c_to_f(observation.temperature_milli_c),
            temperature_c: milli_c(observation.temperature_milli_c),
            temp_min_f: milli_c_to_f(observation.temperature_min_milli_c),
            temp_max_f: milli_c_to_f(observation.temperature_max_milli_c),
            temp_min_c: milli_c(observation.temperature_min_milli_c),
            temp_max_c: milli_c(observation.temperature_max_milli_c),
            is_from_report: observation.is_from_report,
            report_type: observation.report_type.clone(),
            source_report_id: observation.source_report_id.clone(),
            wu_current_temp_f: milli_c_to_f(observation.wu_current_temperature_milli_c),
            wu_current_temp_c: milli_c(observation.wu_current_temperature_milli_c),
            wu_daily_high_f: milli_c_to_f(observation.wu_daily_high_milli_c),
            wu_daily_low_f: milli_c_to_f(observation.wu_daily_low_milli_c),
            wu_daily_high_c: milli_c(observation.wu_daily_high_milli_c),
            wu_daily_low_c: milli_c(observation.wu_daily_low_milli_c),
            wu_observation_time: millis(observation.wu_observation_at_unix_ms)?,
            wu_fetched_at: millis(observation.wu_fetched_at_unix_ms)?,
            temperature_day_mode: observation.temperature_day_mode.clone(),
            temperature_day_date: observation.temperature_day_date.clone(),
            wu_day_mode: observation.wu_day_mode.clone(),
            wu_day_date: observation.wu_day_date.clone(),
            dewpoint: micros(observation.dewpoint_micros),
            heat_index: micros(observation.heat_index_micros),
            wind_chill: micros(observation.wind_chill_micros),
            relative_humidity: micros(observation.relative_humidity_micros),
            wind_speed: micros(observation.wind_speed_micros),
            wind_direction: micros(observation.wind_direction_micros),
            wind_gust: micros(observation.wind_gust_micros),
            text_description: observation.text_description.clone(),
        })
    }

    fn view(&self) -> ObservationView<'_> {
        ObservationView {
            event_id: self.envelope.event_id.as_deref(),
            sequence: self.envelope.sequence,
            city_sequence: self.envelope.city_sequence,
            emitted_at: self.envelope.emitted_at,
            slug: &self.envelope.slug,
            station_id: &self.station,
            observed_at: self.observed_at,
            lag_seconds: self.lag_seconds,
            preliminary: self.preliminary,
            temperature_f: self.temperature_f,
            temperature_c: self.temperature_c,
            temp_min_f: self.temp_min_f,
            temp_max_f: self.temp_max_f,
            temp_min_c: self.temp_min_c,
            temp_max_c: self.temp_max_c,
            is_from_report: self.is_from_report,
            report_type: self.report_type.as_deref(),
            source_report_id: self.source_report_id.as_deref(),
            wu_current_temp_f: self.wu_current_temp_f,
            wu_current_temp_c: self.wu_current_temp_c,
            wu_daily_high_f: self.wu_daily_high_f,
            wu_daily_low_f: self.wu_daily_low_f,
            wu_daily_high_c: self.wu_daily_high_c,
            wu_daily_low_c: self.wu_daily_low_c,
            wu_observation_time: self.wu_observation_time,
            wu_fetched_at: self.wu_fetched_at,
            temperature_day_mode: self.temperature_day_mode.as_deref(),
            temperature_day_date: self.temperature_day_date.as_deref(),
            wu_day_mode: self.wu_day_mode.as_deref(),
            wu_day_date: self.wu_day_date.as_deref(),
            dewpoint: self.dewpoint,
            heat_index: self.heat_index,
            wind_chill: self.wind_chill,
            relative_humidity: self.relative_humidity,
            wind_speed: self.wind_speed,
            wind_direction: self.wind_direction,
            wind_gust: self.wind_gust,
            text_description: self.text_description.as_deref(),
        }
    }
}

#[derive(Clone, Debug)]
struct OwnedReportEvent {
    envelope: OwnedEnvelope,
    station: String,
    report_id: String,
    report_type: String,
    report_date: String,
    report_revision: i64,
    report_updated_at: Option<DateTime<Utc>>,
    issuance_time: Option<DateTime<Utc>>,
    fetched_at: Option<DateTime<Utc>>,
    source_url: String,
    provider: String,
    max_temp_f: Option<f64>,
    max_temp_c: Option<f64>,
    min_temp_f: Option<f64>,
    min_temp_c: Option<f64>,
    temp_f: Option<f64>,
    temp_c: Option<f64>,
    max_temp_time_utc: Option<DateTime<Utc>>,
    min_temp_time_utc: Option<DateTime<Utc>>,
}

impl OwnedReportEvent {
    fn supplied(event: &SuppliedReportV5) -> Self {
        Self {
            envelope: OwnedEnvelope::supplied(event.envelope.as_ref(), &event.station_id),
            station: event.station_id.clone(),
            report_id: event.report_id.clone(),
            report_type: event.report_type.clone(),
            report_date: event.report_date.clone(),
            report_revision: event
                .report_revision
                .and_then(|value| i64::try_from(value).ok())
                .unwrap_or(0),
            report_updated_at: event.report_updated_at_unix_ns.map(nanos),
            issuance_time: event.issuance_time_unix_ns.map(nanos),
            fetched_at: event.fetched_at_unix_ns.map(nanos),
            source_url: event.source_url.clone().unwrap_or_default(),
            provider: event.provider.clone().unwrap_or_default(),
            max_temp_f: decimal_f64(event.max_temp_f),
            max_temp_c: decimal_f64(event.max_temp_c),
            min_temp_f: decimal_f64(event.min_temp_f),
            min_temp_c: decimal_f64(event.min_temp_c),
            temp_f: decimal_f64(event.temp_f),
            temp_c: decimal_f64(event.temp_c),
            max_temp_time_utc: event.max_temp_time_unix_ns.map(nanos),
            min_temp_time_utc: event.min_temp_time_unix_ns.map(nanos),
        }
    }

    fn derived(report: &ReportV4, station_id: &str) -> Self {
        Self {
            envelope: OwnedEnvelope::derived(&report.provenance, station_id),
            station: station_id.to_owned(),
            report_id: report.report_id.clone(),
            report_type: report.report_type.clone(),
            report_date: report.report_date.clone(),
            report_revision: i64::try_from(report.revision).unwrap_or(i64::MAX),
            report_updated_at: millis(report.updated_at_unix_ms).ok().flatten(),
            issuance_time: millis(report.issued_at_unix_ms).ok().flatten(),
            fetched_at: millis(report.fetched_at_unix_ms).ok().flatten(),
            source_url: report.source_url.clone(),
            provider: report.provider.clone(),
            max_temp_f: milli_c_to_f(report.max_temperature_milli_c),
            max_temp_c: milli_c(report.max_temperature_milli_c),
            min_temp_f: milli_c_to_f(report.min_temperature_milli_c),
            min_temp_c: milli_c(report.min_temperature_milli_c),
            // The V4 projection carries the supplied point temperature in milli-F.
            temp_f: report
                .temperature_milli_f
                .map(|value| f64::from(value) / 1_000.0)
                .or_else(|| milli_c_to_f(report.temperature_milli_c)),
            temp_c: milli_c(report.temperature_milli_c),
            max_temp_time_utc: millis(report.max_temperature_at_unix_ms).ok().flatten(),
            min_temp_time_utc: millis(report.min_temperature_at_unix_ms).ok().flatten(),
        }
    }

    fn view(&self) -> StationReportView<'_> {
        StationReportView {
            event_id: self.envelope.event_id.as_deref(),
            sequence: self.envelope.sequence,
            city_sequence: self.envelope.city_sequence,
            emitted_at: self.envelope.emitted_at,
            slug: &self.envelope.slug,
            station_id: &self.station,
            report_id: &self.report_id,
            report_type: &self.report_type,
            report_date: &self.report_date,
            report_revision: self.report_revision,
            report_updated_at: self.report_updated_at,
            issuance_time: self.issuance_time,
            fetched_at: self.fetched_at,
            source_url: &self.source_url,
            provider: &self.provider,
            max_temp_f: self.max_temp_f,
            max_temp_c: self.max_temp_c,
            min_temp_f: self.min_temp_f,
            min_temp_c: self.min_temp_c,
            temp_f: self.temp_f,
            temp_c: self.temp_c,
            max_temp_time_utc: self.max_temp_time_utc,
            min_temp_time_utc: self.min_temp_time_utc,
        }
    }
}

#[derive(Clone, Debug)]
struct OwnedExtremeEvent {
    envelope: OwnedEnvelope,
    station: String,
    high: bool,
    event_key: String,
    value_f: f64,
    value_c: f64,
    prev_value_f: Option<f64>,
    observed_at: Option<DateTime<Utc>>,
    temperature_day_mode: Option<String>,
    temperature_day_date: Option<String>,
    is_from_report: bool,
    report_type: Option<String>,
    source_report_id: Option<String>,
}

impl OwnedExtremeEvent {
    fn supplied(event: &SuppliedExtremeV5, event_date: &str) -> Self {
        let value_f = decimal_f64(event.value_f);
        let value_c = decimal_f64(event.value_c);
        Self {
            envelope: OwnedEnvelope::supplied(event.envelope.as_ref(), &event.station_id),
            station: event.station_id.clone(),
            high: event.kind == ExtremeKindV5::High,
            event_key: event
                .temperature_day_date
                .clone()
                .unwrap_or_else(|| event_date.to_owned()),
            // Each unit is the supplied original; only an absent unit is converted from the other.
            value_f: value_f
                .or_else(|| value_c.map(|value| value * 9.0 / 5.0 + 32.0))
                .unwrap_or(f64::NAN),
            value_c: value_c
                .or_else(|| value_f.map(|value| (value - 32.0) * 5.0 / 9.0))
                .unwrap_or(f64::NAN),
            prev_value_f: decimal_f64(event.prev_value_f),
            observed_at: event.observed_at_unix_ns.map(nanos),
            temperature_day_mode: event.temperature_day_mode.clone(),
            temperature_day_date: event.temperature_day_date.clone(),
            is_from_report: event.is_from_report,
            report_type: event.report_type.clone(),
            source_report_id: event.source_report_id.clone(),
        }
    }

    fn derived(
        extreme: &ExtremeV4,
        station_id: &str,
        high: bool,
        event_date: &str,
    ) -> Result<Self, KernelTransactionError> {
        let value_c = f64::from(extreme.value_milli_c) / 1_000.0;
        Ok(Self {
            envelope: OwnedEnvelope::derived(&extreme.provenance, station_id),
            station: station_id.to_owned(),
            high,
            event_key: extreme
                .temperature_day_date
                .clone()
                .unwrap_or_else(|| event_date.to_owned()),
            value_f: value_c * 9.0 / 5.0 + 32.0,
            value_c,
            prev_value_f: milli_c_to_f(extreme.previous_value_milli_c),
            observed_at: millis(extreme.observed_at_unix_ms)?,
            temperature_day_mode: extreme.temperature_day_mode.clone(),
            temperature_day_date: extreme.temperature_day_date.clone(),
            is_from_report: extreme.is_from_report,
            report_type: extreme.report_type.clone(),
            source_report_id: extreme.source_report_id.clone(),
        })
    }

    fn view(&self) -> HighLowView<'_> {
        HighLowView {
            event_id: self.envelope.event_id.as_deref(),
            sequence: self.envelope.sequence,
            city_sequence: self.envelope.city_sequence,
            emitted_at: self.envelope.emitted_at,
            event_key: &self.event_key,
            source_timestamp: self.envelope.source_timestamp.or(self.observed_at),
            wmo_emit_time: self.envelope.wmo_emit_time,
            producer_received_at: self.envelope.producer_received_at,
            live_published_at: self.envelope.live_published_at,
            persistence_status: self.envelope.persistence_status.as_deref(),
            producer_sequence: self.envelope.producer_sequence,
            slug: &self.envelope.slug,
            station_id: &self.station,
            value_f: self.value_f,
            value_c: self.value_c,
            prev_value_f: self.prev_value_f,
            observed_at: self.observed_at,
            temperature_day_mode: self.temperature_day_mode.as_deref(),
            temperature_day_date: self.temperature_day_date.as_deref(),
            is_from_report: self.is_from_report,
            report_type: self.report_type.as_deref(),
            source_report_id: self.source_report_id.as_deref(),
        }
    }
}

#[derive(Clone, Debug)]
struct OwnedWeatherEvent {
    envelope: OwnedEnvelope,
    station: String,
    id: String,
    event_type: String,
    tier: String,
    state: String,
    name: String,
    badge: String,
    detail: String,
    summary: String,
    started_at: Option<DateTime<Utc>>,
    last_confirmed_at: Option<DateTime<Utc>>,
    ended_at: Option<DateTime<Utc>>,
    source: Option<OwnedWeatherEventSource>,
}

#[derive(Clone, Debug)]
struct OwnedWeatherEventSource {
    metar_type: Option<String>,
    flight_category: Option<String>,
    wx_string: Option<String>,
    wx_token: Option<String>,
    wind_speed_kt: Option<f64>,
    wind_gust_kt: Option<f64>,
    peak_wind_kt: Option<f64>,
    peak_wind_direction: Option<i64>,
    visibility_mi: Option<f64>,
    cb_location: Option<String>,
}

impl OwnedWeatherEvent {
    fn supplied(event: &SuppliedWeatherEventV5) -> Self {
        Self {
            envelope: OwnedEnvelope::supplied(event.envelope.as_ref(), &event.station_id),
            station: event.station_id.clone(),
            id: event.episode_id.clone(),
            event_type: event.event_type.clone(),
            tier: event.tier.clone(),
            state: event.state.clone(),
            name: event.name.clone(),
            badge: event.badge.clone().unwrap_or_default(),
            detail: event.detail.clone().unwrap_or_default(),
            summary: event.summary.clone().unwrap_or_default(),
            started_at: event.started_at_unix_ns.map(nanos),
            last_confirmed_at: event.last_confirmed_at_unix_ns.map(nanos),
            ended_at: event.ended_at_unix_ns.map(nanos),
            source: event
                .source_snapshot
                .as_ref()
                .map(|source| OwnedWeatherEventSource {
                    metar_type: source.metar_type.clone(),
                    flight_category: source.flight_category.clone(),
                    wx_string: source.wx_string.clone(),
                    wx_token: source.wx_token.clone(),
                    wind_speed_kt: decimal_f64(source.wind_speed_kt),
                    wind_gust_kt: decimal_f64(source.wind_gust_kt),
                    peak_wind_kt: decimal_f64(source.peak_wind_kt),
                    peak_wind_direction: source.peak_wind_direction,
                    visibility_mi: decimal_f64(source.visibility_mi),
                    cb_location: source.cb_location.clone(),
                }),
        }
    }

    fn derived(event: &WeatherEventV4, station_id: &str) -> Result<Self, KernelTransactionError> {
        Ok(Self {
            envelope: OwnedEnvelope::derived(&event.provenance, station_id),
            station: station_id.to_owned(),
            id: event.event_id.clone(),
            event_type: event.event_type.clone(),
            tier: event.tier.clone(),
            state: event.state.clone(),
            name: event.name.clone(),
            badge: event.badge.clone(),
            detail: event.detail.clone(),
            summary: event.summary.clone(),
            started_at: millis(event.started_at_unix_ms)?,
            last_confirmed_at: millis(event.last_confirmed_at_unix_ms)?,
            ended_at: millis(event.ended_at_unix_ms)?,
            source: event.source.as_ref().map(|source| OwnedWeatherEventSource {
                metar_type: source.metar_type.clone(),
                flight_category: source.flight_category.clone(),
                wx_string: source.wx_string.clone(),
                wx_token: source.wx_token.clone(),
                wind_speed_kt: micros(source.wind_speed_knots_micros),
                wind_gust_kt: micros(source.wind_gust_knots_micros),
                peak_wind_kt: micros(source.peak_wind_knots_micros),
                peak_wind_direction: source.peak_wind_direction,
                visibility_mi: micros(source.visibility_miles_micros),
                cb_location: source.cb_location.clone(),
            }),
        })
    }

    fn view(&self) -> WeatherEventView<'_> {
        WeatherEventView {
            event_id: self.envelope.event_id.as_deref(),
            sequence: self.envelope.sequence,
            city_sequence: self.envelope.city_sequence,
            emitted_at: self.envelope.emitted_at,
            slug: &self.envelope.slug,
            station_id: &self.station,
            id: &self.id,
            event_type_name: &self.event_type,
            tier: &self.tier,
            state: &self.state,
            name: &self.name,
            badge: &self.badge,
            detail: &self.detail,
            summary: &self.summary,
            started_at: self.started_at,
            last_confirmed_at: self.last_confirmed_at,
            ended_at: self.ended_at,
            source: self.source.as_ref().map(|source| WeatherEventSourceView {
                metar_type: source.metar_type.as_deref(),
                flight_category: source.flight_category.as_deref(),
                wx_string: source.wx_string.as_deref(),
                wx_token: source.wx_token.as_deref(),
                wind_speed_kt: source.wind_speed_kt,
                wind_gust_kt: source.wind_gust_kt,
                peak_wind_kt: source.peak_wind_kt,
                peak_wind_direction: source.peak_wind_direction,
                visibility_mi: source.visibility_mi,
                cb_location: source.cb_location.as_deref(),
            }),
        }
    }
}

/// The owned typed event for one transaction. `None` for bootstrap/recovery, which invoke
/// `on_start` instead of `on_event`.
pub struct KernelEvent {
    event: Option<OwnedEvent>,
}

enum OwnedEvent {
    Observation(Box<OwnedObservationEvent>),
    Report(Box<OwnedReportEvent>),
    Extreme(Box<OwnedExtremeEvent>),
    WeatherEvent(Box<OwnedWeatherEvent>),
    Price {
        station: String,
        market_id: String,
        emitted_at: DateTime<Utc>,
        markets: Vec<OwnedMarket>,
    },
    Forecast {
        station: String,
        emitted_at: DateTime<Utc>,
        model_id: String,
        version: String,
    },
    Oracle {
        station: String,
        emitted_at: DateTime<Utc>,
        modes: Vec<String>,
        updated_at: Option<DateTime<Utc>>,
        day_of: Option<OwnedOracle>,
    },
    Timer {
        key: String,
        scheduled_at: DateTime<Utc>,
        decision_at: DateTime<Utc>,
    },
    Unknown {
        kind: &'static str,
        emitted_at: DateTime<Utc>,
    },
}

impl KernelEvent {
    pub fn from_context(context: &DecisionContextV5) -> Result<Self, KernelTransactionError> {
        let trigger = match &context.trigger {
            TriggerV5::BrokerOutcome {
                originating_trigger,
                ..
            } => match originating_trigger.as_ref() {
                OriginatingTriggerV5::Owner(trigger) => TriggerRef::Owner(trigger),
                OriginatingTriggerV5::BrokerState { .. } => TriggerRef::BrokerState,
            },
            TriggerV5::Owner(trigger) => TriggerRef::Owner(trigger),
            TriggerV5::BrokerState { .. } => TriggerRef::BrokerState,
        };
        let decision_at = millis(Some(context.decision_time_unix_ms))?
            .ok_or(KernelTransactionError::InvalidTime)?;
        let station = scoped_station(context)?;
        let station_id = station.identity.station_id.as_str();
        let supplied_event = context.supplied.originating_event.as_ref();
        let event_date = context.strategy.event_date.as_str();
        let event = match trigger {
            TriggerRef::BrokerState => Some(OwnedEvent::Unknown {
                kind: "broker_state",
                emitted_at: decision_at,
            }),
            TriggerRef::Owner(OwnerTriggerV5::Bootstrap | OwnerTriggerV5::Recovery) => None,
            TriggerRef::Owner(OwnerTriggerV5::Observation { .. }) => {
                Some(OwnedEvent::Observation(Box::new(match supplied_event {
                    Some(SuppliedEventV5::Observation(event)) => {
                        OwnedObservationEvent::supplied(event)
                    }
                    _ => OwnedObservationEvent::derived(&station.observation)?,
                })))
            }
            TriggerRef::Owner(OwnerTriggerV5::StationReport { report_id, .. }) => {
                Some(OwnedEvent::Report(Box::new(match supplied_event {
                    Some(SuppliedEventV5::Report(event)) => OwnedReportEvent::supplied(event),
                    _ => OwnedReportEvent::derived(
                        station
                            .reports
                            .iter()
                            .find(|report| report.report_id == *report_id)
                            .ok_or_else(|| {
                                KernelTransactionError::Kernel("report missing".to_owned())
                            })?,
                        station_id,
                    ),
                })))
            }
            TriggerRef::Owner(OwnerTriggerV5::NewHigh { .. }) => {
                Some(OwnedEvent::Extreme(Box::new(match supplied_event {
                    Some(SuppliedEventV5::Extreme(event)) => {
                        OwnedExtremeEvent::supplied(event, event_date)
                    }
                    _ => OwnedExtremeEvent::derived(
                        station.extrema.high.as_ref().ok_or_else(|| {
                            KernelTransactionError::Kernel("new high missing".to_owned())
                        })?,
                        station_id,
                        true,
                        event_date,
                    )?,
                })))
            }
            TriggerRef::Owner(OwnerTriggerV5::NewLow { .. }) => {
                Some(OwnedEvent::Extreme(Box::new(match supplied_event {
                    Some(SuppliedEventV5::Extreme(event)) => {
                        OwnedExtremeEvent::supplied(event, event_date)
                    }
                    _ => OwnedExtremeEvent::derived(
                        station.extrema.low.as_ref().ok_or_else(|| {
                            KernelTransactionError::Kernel("new low missing".to_owned())
                        })?,
                        station_id,
                        false,
                        event_date,
                    )?,
                })))
            }
            TriggerRef::Owner(OwnerTriggerV5::WeatherEvent { episode_id, .. }) => {
                match supplied_event {
                    Some(SuppliedEventV5::WeatherEvent(event)) => Some(OwnedEvent::WeatherEvent(
                        Box::new(OwnedWeatherEvent::supplied(event)),
                    )),
                    _ => match station
                        .weather_events
                        .iter()
                        .find(|event| event.event_id == *episode_id)
                    {
                        Some(event) => Some(OwnedEvent::WeatherEvent(Box::new(
                            OwnedWeatherEvent::derived(event, station_id)?,
                        ))),
                        // An ended episode is absent from current state and cannot be
                        // reconstructed without its supplied event.
                        None => Some(OwnedEvent::Unknown {
                            kind: "weather_event",
                            emitted_at: decision_at,
                        }),
                    },
                }
            }
            TriggerRef::Owner(OwnerTriggerV5::MarketPrice {
                market_id,
                emitted_at_unix_ms,
                ..
            }) => Some(OwnedEvent::Price {
                station: station_id.to_owned(),
                market_id: market_id.clone(),
                emitted_at: millis(Some(*emitted_at_unix_ms))?
                    .ok_or(KernelTransactionError::InvalidTime)?,
                markets: context
                    .owner_state
                    .markets
                    .iter()
                    .map(|market| owned_market(market, event_date))
                    .collect::<Result<_, _>>()?,
            }),
            TriggerRef::Owner(OwnerTriggerV5::ForecastUpdated {
                emitted_at_unix_ms, ..
            }) => {
                let model = station.forecast.models.first().ok_or_else(|| {
                    KernelTransactionError::Kernel("forecast model missing".to_owned())
                })?;
                Some(OwnedEvent::Forecast {
                    station: station_id.to_owned(),
                    emitted_at: millis(Some(*emitted_at_unix_ms))?
                        .ok_or(KernelTransactionError::InvalidTime)?,
                    model_id: model.model_id.clone(),
                    version: model.version.clone(),
                })
            }
            TriggerRef::Owner(OwnerTriggerV5::OracleScoresUpdated {
                emitted_at_unix_ms, ..
            }) => {
                let oracles = owned_oracles(station, context.supplied.station(station_id))?;
                let day_of = oracles
                    .iter()
                    .find(|oracle| oracle.mode == "day_of")
                    .cloned();
                let modes = day_of
                    .as_ref()
                    .map(|oracle| oracle.modes.clone())
                    .filter(|modes| !modes.is_empty())
                    .unwrap_or_else(|| vec![station.oracle.query.mode.clone()]);
                Some(OwnedEvent::Oracle {
                    station: station_id.to_owned(),
                    emitted_at: millis(Some(*emitted_at_unix_ms))?
                        .ok_or(KernelTransactionError::InvalidTime)?,
                    updated_at: day_of.as_ref().and_then(|oracle| oracle.updated_at),
                    modes,
                    day_of,
                })
            }
            TriggerRef::Owner(OwnerTriggerV5::Timer {
                key,
                scheduled_at_epoch_ns,
                ..
            }) => Some(OwnedEvent::Timer {
                key: key.clone(),
                scheduled_at: Utc.timestamp_nanos(
                    i64::try_from(*scheduled_at_epoch_ns)
                        .map_err(|_| KernelTransactionError::InvalidTime)?,
                ),
                decision_at,
            }),
        };
        Ok(Self { event })
    }

    pub fn run<K: TransactionKernel>(
        &self,
        kernel: &mut K,
        context: &mut dyn StrategyKernelContext,
    ) -> KernelResult<()> {
        if let Some(OwnedEvent::Price {
            station,
            market_id,
            emitted_at,
            markets,
        }) = self.event.as_ref()
        {
            let views = markets
                .iter()
                .map(OwnedMarket::bracket_view)
                .collect::<Vec<_>>();
            return kernel.on_event(
                StrategyEventView::PriceUpdate(PriceUpdateView {
                    event_id: Some(market_id),
                    sequence: None,
                    city_sequence: None,
                    emitted_at: Some(*emitted_at),
                    source: "kalshi",
                    slug: station,
                    station_id: station,
                    city_id: "",
                    timestamp: Some(*emitted_at),
                    markets: &views,
                }),
                context,
            );
        }
        match self.view() {
            Some(event) => kernel.on_event(event, context),
            None => kernel.on_start(context),
        }
    }

    /// The projected event view; `None` for bootstrap/recovery and for price updates, which
    /// are built inside [`KernelEvent::run`] because they borrow per-market views.
    pub fn view(&self) -> Option<StrategyEventView<'_>> {
        match self.event.as_ref()? {
            OwnedEvent::Observation(event) => Some(StrategyEventView::Observation(event.view())),
            OwnedEvent::Report(event) => Some(StrategyEventView::StationReport(event.view())),
            OwnedEvent::Extreme(event) if event.high => {
                Some(StrategyEventView::NewHigh(event.view()))
            }
            OwnedEvent::Extreme(event) => Some(StrategyEventView::NewLow(event.view())),
            OwnedEvent::WeatherEvent(event) => Some(StrategyEventView::WeatherEvent(event.view())),
            OwnedEvent::Price { .. } => None,
            OwnedEvent::Forecast {
                station,
                emitted_at,
                model_id,
                version,
            } => Some(StrategyEventView::ForecastUpdated(ForecastUpdatedView {
                event_id: None,
                sequence: None,
                emitted_at: Some(*emitted_at),
                slug: station,
                station_id: station,
                model_id,
                version,
            })),
            OwnedEvent::Oracle {
                station,
                emitted_at,
                modes,
                updated_at,
                day_of,
            } => Some(StrategyEventView::OracleScoresUpdated(
                OracleScoresUpdatedView {
                    event_id: None,
                    sequence: None,
                    emitted_at: Some(*emitted_at),
                    slug: station,
                    station_id: station,
                    modes,
                    updated_at: updated_at.or(Some(*emitted_at)),
                    overall: None,
                    day_ahead: None,
                    day_of: day_of.as_ref().map(OwnedOracle::snapshot),
                },
            )),
            OwnedEvent::Timer {
                key,
                scheduled_at,
                decision_at,
            } => Some(StrategyEventView::TimerWake(TimerWakeView {
                scheduled_for: *scheduled_at,
                fired_at: Some(*decision_at),
                name: key,
            })),
            OwnedEvent::Unknown { kind, emitted_at } => Some(StrategyEventView::Unknown {
                event_type: kind,
                emitted_at: Some(*emitted_at),
            }),
        }
    }
}

enum TriggerRef<'a> {
    Owner(&'a OwnerTriggerV5),
    BrokerState,
}

fn owned_market(
    market: &MarketV4,
    event_date: &str,
) -> Result<OwnedMarket, KernelTransactionError> {
    let ticker = market.ticker.as_ref();
    let book = market.book.as_ref();
    let levels = |levels: &[crate::decision_v4::BookLevelV4], invert: bool| {
        levels
            .iter()
            .map(|level| PriceLevelView {
                price: if invert {
                    1.0 - price(level.price_micros)
                } else {
                    price(level.price_micros)
                },
                quantity: whole_contracts(level.quantity_hundredths),
            })
            .collect::<Vec<_>>()
    };
    Ok(OwnedMarket {
        id: market.identity.market_id.clone(),
        event_ticker: market.identity.event_ticker.clone(),
        event_date: event_date.to_owned(),
        series_ticker: market.identity.series_ticker.clone(),
        close_time: millis(market.identity.close_at_unix_ms)?,
        fee_type: market.identity.fee_type.clone(),
        fee_multiplier: market
            .identity
            .fee_multiplier_millionths
            .map(|value| value as f64 / 1_000_000.0),
        strike_type: market.identity.strike_type.clone(),
        floor_strike: market
            .identity
            .floor_strike_milli_f
            .map(|value| value as f64 / 1_000.0)
            .or_else(|| {
                market
                    .identity
                    .floor_strike_milli_c
                    .map(|value| value as f64 / 1_000.0 * 9.0 / 5.0 + 32.0)
            }),
        cap_strike: market
            .identity
            .cap_strike_milli_c
            .map(|value| value as f64 / 1_000.0 * 9.0 / 5.0 + 32.0),
        last_price: ticker.and_then(|value| value.last_price_micros).map(price),
        yes_bid: ticker.and_then(|value| value.yes_bid_micros).map(price),
        yes_ask: ticker.and_then(|value| value.yes_ask_micros).map(price),
        no_bid: ticker.and_then(|value| value.no_bid_micros).map(price),
        no_ask: ticker.and_then(|value| value.no_ask_micros).map(price),
        yes_bid_depth: ticker
            .and_then(|value| value.yes_bid_quantity_hundredths)
            .map(whole_contracts),
        yes_ask_depth: ticker
            .and_then(|value| value.yes_ask_quantity_hundredths)
            .map(whole_contracts),
        no_bid_depth: ticker
            .and_then(|value| value.no_bid_quantity_hundredths)
            .map(whole_contracts),
        no_ask_depth: ticker
            .and_then(|value| value.no_ask_quantity_hundredths)
            .map(whole_contracts),
        yes_bid_levels: book
            .map(|book| levels(&book.yes_bids, false))
            .unwrap_or_default(),
        yes_ask_levels: book
            .map(|book| levels(&book.no_bids, true))
            .unwrap_or_default(),
        no_bid_levels: book
            .map(|book| levels(&book.no_bids, false))
            .unwrap_or_default(),
        no_ask_levels: book
            .map(|book| levels(&book.yes_bids, true))
            .unwrap_or_default(),
        volume: ticker
            .and_then(|value| value.volume_hundredths)
            .map(|value| value as f64 / 100.0),
        last_update: ticker
            .and_then(|value| value.provider_at_unix_ms)
            .and_then(|value| millis(Some(value)).ok().flatten()),
    })
}

/// Maps an exact place return onto the frozen kernel's synchronous order result.
pub fn map_place_return(value: PlaceOrderReturnV5) -> KernelResult<OrderResult> {
    match value {
        PlaceOrderReturnV5::Ok(result) => Ok(OrderResult {
            order_id: result.order_id,
            sleeve_id: String::new(),
            status: match result.status {
                KernelOrderStatusV5::Filled => OrderStatus::Filled,
                KernelOrderStatusV5::Partial => OrderStatus::Partial,
                KernelOrderStatusV5::Pending => OrderStatus::Pending,
                KernelOrderStatusV5::Rejected => OrderStatus::Rejected,
                KernelOrderStatusV5::Cancelled => OrderStatus::Cancelled,
            },
            filled_quantity: hundredths_quantity(result.filled_quantity_hundredths),
            fill_price: price(result.fill_price_micros),
            fee_cost: result.fee_cost_micros as f64 / 1_000_000.0,
            reason: result.reason,
        }),
        PlaceOrderReturnV5::Err(error) => Err(KernelError::new(error.message)),
    }
}

fn side_matches(side: ContractSideV5, kernel: &ContractSide) -> bool {
    matches!(
        (side, kernel),
        (ContractSideV5::Yes, ContractSide::Yes) | (ContractSideV5::No, ContractSide::No)
    )
}
fn order_status_str(status: BrokerOrderStatusV5) -> &'static str {
    match status {
        BrokerOrderStatusV5::DurablyAccepted => "accepted",
        BrokerOrderStatusV5::Dispatched => "dispatched",
        BrokerOrderStatusV5::Resting => "pending",
        BrokerOrderStatusV5::PartiallyFilled => "partial",
        BrokerOrderStatusV5::Filled => "filled",
        BrokerOrderStatusV5::CancellationRequested => "cancellation_requested",
        BrokerOrderStatusV5::Cancelled => "cancelled",
        BrokerOrderStatusV5::Expired => "expired",
        BrokerOrderStatusV5::Rejected => "rejected",
        BrokerOrderStatusV5::RecoveryRequired => "recovery_required",
    }
}
fn kernel_order_status(status: BrokerOrderStatusV5) -> OrderStatus {
    match status {
        BrokerOrderStatusV5::PartiallyFilled => OrderStatus::Partial,
        BrokerOrderStatusV5::Filled => OrderStatus::Filled,
        BrokerOrderStatusV5::Cancelled | BrokerOrderStatusV5::Expired => OrderStatus::Cancelled,
        BrokerOrderStatusV5::Rejected | BrokerOrderStatusV5::RecoveryRequired => {
            OrderStatus::Rejected
        }
        _ => OrderStatus::Pending,
    }
}
fn hundredths_quantity(value: u64) -> ContractQuantity {
    ContractQuantity::from_hundredths(i64::try_from(value).unwrap_or(i64::MAX))
}
fn millis(value: Option<i64>) -> Result<Option<DateTime<Utc>>, KernelTransactionError> {
    value
        .map(|value| {
            Utc.timestamp_millis_opt(value)
                .single()
                .ok_or(KernelTransactionError::InvalidTime)
        })
        .transpose()
}
fn nanos(value: i64) -> DateTime<Utc> {
    Utc.timestamp_nanos(value)
}
fn decimal_f64(value: Option<DecimalV5>) -> Option<f64> {
    value.map(DecimalV5::to_f64)
}
fn milli_c(value: Option<i32>) -> Option<f64> {
    value.map(|value| f64::from(value) / 1_000.0)
}
fn milli_c_to_f(value: Option<i32>) -> Option<f64> {
    milli_c(value).map(|value| value * 9.0 / 5.0 + 32.0)
}
fn micros(value: Option<i64>) -> Option<f64> {
    value.map(|value| value as f64 / 1_000_000.0)
}
fn millionths(value: Option<i64>) -> Option<f64> {
    micros(value)
}
fn price(value: u64) -> f64 {
    value as f64 / 1_000_000.0
}
fn price_micros(value: f64) -> Result<u64, KernelTransactionError> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(KernelTransactionError::InvalidQuantity);
    }
    Ok((value * 1_000_000.0).round() as u64)
}
/// Whole-contract view quantity derived by truncation; the exact hundredths stay canonical.
fn whole_contracts(value: u64) -> i64 {
    i64::try_from(value / 100).unwrap_or(i64::MAX)
}
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut output, byte| {
        write!(&mut output, "{byte:02x}").expect("String formatting cannot fail");
        output
    })
}
