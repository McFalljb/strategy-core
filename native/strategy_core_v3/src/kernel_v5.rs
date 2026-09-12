//! Core-owned kernel projection and transaction runner for Decision V5.
//!
//! This module is the single definition of how a canonical [`DecisionContextV5`] is presented
//! to a `strategy_core_kernel::NativeKernel` and how the kernel's synchronous Broker calls are
//! carried through the V5 transaction (`AwaitingBrokerOutcome` → durable continuation → exact
//! Broker return → resumed invocation). Hosts and Strategy executables consume it instead of
//! re-interpreting fields.
//!
//! Projection rules:
//! - The context is projected once into the canonical owned model of `strategy_core_kernel`
//!   (`StationState`, `MarketState`, `StrategyEvent`). Every component is built from its
//!   supplied original when the context carries one and from the derived V4 owner projection
//!   otherwise; each carries its `ValueOrigin` and, when supplied, the original itself.
//! - Publication, observation, issuance, receipt and decision times are distinct: `emitted_at`
//!   on an event is the provider's publication time, never the decision clock.
//! - The `get_weather` summary follows one replacement rule (`weather_summary`): a supplied
//!   original is presented only while it is still the owner's current fact; a newer accepted
//!   derived value wins and the superseded original stays retained on the station.
//! - Whole-contract quantities are explicit floors carried beside the exact hundredths.

use chrono::{DateTime, TimeZone, Utc};
use strategy_core_kernel::{
    Book, BookLevel, CancelOrderRequest, ClimateDay, ComponentAuthority, ComponentMeta,
    ContractQuantity, ContractSide, DailyExtremes, EventProvenance, Extreme, FinalFact, Forecast,
    ForecastModel, ForecastPoint, ForecastUpdated, KernelAction, KernelError, KernelResult,
    LastTrade, MarketComponents, MarketLifecycle, MarketState, NativeKernel, Observation,
    OracleScore, OracleScoresUpdated, OracleTable, OrderAction, OrderResult, OrderStatus,
    OrderStatusView, PendingOrderView, PlaceOrderRequest, PriceUpdate, Report, StationComponents,
    StationIdentity, StationState, StationWeatherView, StrategyEvent, StrategyEventView,
    StrategyKernelBroker, StrategyKernelContext, StrategyKernelData, StrategyKernelRuntime,
    StrategyKernelState, StrategyKernelTelemetry, TickerQuote, TimerWake, ValueOrigin,
    WakeAtRequest, WeatherEvent, WeatherEventSource,
};

use crate::decision_v4::{
    AuthorityV4, BookLevelV4, ComponentMetaV4, ExtremeV4, ForecastModelV4, MarketMetaV4, MarketV4,
    ObservationV4, ProvenanceV4, RankByV4, ReportV4, StationV4, WeatherEventV4,
};
use crate::decision_v5::{
    self as wire, BrokerCommandReturnV5, BrokerOrderStatusV5, CancelAllOrdersReturnV5,
    CancelOrderReturnV5, CommandFenceV5, ContractSideV5, DecisionContextV5, DecisionDispositionV5,
    DecisionResultV5, DecisionV5Error, KernelCheckpointV5, KernelOrderStatusV5, OrderActionV5,
    OrderTypeV5, OriginatingTriggerV5, OwnerTriggerV5, PlaceOrderReturnV5, PlaceOrderV5,
    ResultDiagnosticV5, StrategyCommandV5, StrategyParameterValueV5, TriggerV5,
};
use crate::supplied_v5::{ExtremeKindV5, SuppliedEventV5, SuppliedStationV5};

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

/// A frozen kernel as driven by the transaction runner: an ordinary [`NativeKernel`] that can
/// also checkpoint its private state.
pub trait TransactionKernel: NativeKernel + Clone {
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

/// The canonical scoped state of one transaction, built once from the durable context.
///
/// Every component is projected from its supplied original when the context carries one and
/// from the V4 owner projection otherwise; the resulting [`StationState`] and [`MarketState`]
/// values are what a kernel reads through [`StrategyKernelState`].
#[derive(Clone, Debug)]
pub struct KernelSnapshot {
    now: DateTime<Utc>,
    stations: Vec<StationState>,
    markets: Vec<MarketState>,
    broker: wire::BrokerDetailV5,
    buying_power: f64,
}

impl KernelSnapshot {
    pub fn from_context(context: &DecisionContextV5) -> Result<Self, KernelTransactionError> {
        scoped_station(context)?;
        let stations = context
            .owner_state
            .stations
            .iter()
            .map(|station| {
                station_state(
                    station,
                    context.supplied.station(&station.identity.station_id),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let markets = context
            .owner_state
            .markets
            .iter()
            .map(|market| market_state(market, &context.strategy.event_date))
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
            stations,
            markets,
            broker: context.broker.clone(),
            buying_power: buying_power_micros as f64 / 1_000_000.0,
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

fn scoped_station(context: &DecisionContextV5) -> Result<&StationV4, KernelTransactionError> {
    context
        .owner_state
        .stations
        .iter()
        .find(|station| station.identity.station_id == context.strategy.station_id)
        .ok_or_else(|| KernelTransactionError::Kernel("station missing".to_owned()))
}

fn station_state(
    station: &StationV4,
    supplied: Option<&SuppliedStationV5>,
) -> Result<StationState, KernelTransactionError> {
    let station_id = station.identity.station_id.as_str();
    let event_date = station.climate_event_date.as_str();
    let supplied_observation = supplied.and_then(|station| station.observation.as_ref());
    let observation = match supplied_observation {
        Some(event) => Some(Observation::from_supplied(event)),
        None => (station.observation.observed_at_unix_ms != 0
            || station.observation.temperature_milli_c.is_some())
        .then(|| derived_observation(&station.observation, station_id))
        .transpose()?,
    };
    let daily_extremes = supplied
        .and_then(|station| station.daily_extremes.as_ref())
        .map(DailyExtremes::from_supplied);
    let mut reports = Vec::with_capacity(station.reports.len());
    for report in &station.reports {
        let supplied_report = supplied.and_then(|station| {
            station
                .reports
                .iter()
                .find(|candidate| candidate.report_type == report.report_type)
                .filter(|candidate| {
                    candidate.report_id == report.report_id
                        && candidate.report_revision.unwrap_or(0) == report.revision
                })
        });
        let mut model = match supplied_report {
            Some(event) => Report::from_supplied(event),
            None => derived_report(report, station_id),
        };
        model.meta = ComponentMeta {
            authority: authority(&report.authority),
            revision: report.revision,
            generation: report.generation,
            updated_at: millis(report.updated_at_unix_ms)?,
            expected_version: None,
            refresh_error: None,
        };
        reports.push(model);
    }
    reports.sort_by(|left, right| left.report_type.cmp(&right.report_type));
    let mut weather_events = Vec::with_capacity(station.weather_events.len());
    for event in &station.weather_events {
        let supplied_event = supplied.and_then(|station| {
            station
                .weather_events
                .iter()
                .find(|candidate| candidate.episode_id == event.event_id)
        });
        let mut model = match supplied_event {
            Some(supplied) => WeatherEvent::from_supplied(supplied),
            None => derived_weather_event(event, station_id)?,
        };
        model.meta = ComponentMeta {
            authority: authority(&event.authority),
            revision: event.revision,
            generation: event.generation,
            updated_at: None,
            expected_version: None,
            refresh_error: None,
        };
        weather_events.push(model);
    }
    let extreme_high = match supplied.and_then(|station| station.extreme_high.as_ref()) {
        Some(event) => Some(Extreme::from_supplied(event, event_date)),
        None => station
            .extrema
            .high
            .as_ref()
            .map(|extreme| derived_extreme(extreme, station_id, ExtremeKindV5::High, event_date))
            .transpose()?,
    };
    let extreme_low = match supplied.and_then(|station| station.extreme_low.as_ref()) {
        Some(event) => Some(Extreme::from_supplied(event, event_date)),
        None => station
            .extrema
            .low
            .as_ref()
            .map(|extreme| derived_extreme(extreme, station_id, ExtremeKindV5::Low, event_date))
            .transpose()?,
    };
    let mut forecast = match supplied.and_then(|station| station.forecast.as_ref()) {
        Some(supplied) => Some(Forecast::from_supplied(station_id, supplied)),
        None => (!station.forecast.models.is_empty())
            .then(|| derived_forecast(station, station_id))
            .transpose()?,
    };
    if let Some(forecast) = &mut forecast {
        forecast.meta = component_meta(&station.forecast_meta)?;
    }
    let mut oracle_tables = match supplied
        .map(|station| &station.oracle_tables)
        .filter(|tables| !tables.is_empty())
    {
        Some(tables) => tables.iter().map(OracleTable::from_supplied).collect(),
        None => (!station.oracle.rows.is_empty() || station.oracle.updated_at_unix_ms.is_some())
            .then(|| derived_oracle(station))
            .transpose()?
            .into_iter()
            .collect::<Vec<_>>(),
    };
    for table in &mut oracle_tables {
        table.meta = component_meta(&station.oracle_meta)?;
    }
    let weather = weather_summary(station, observation.as_ref(), daily_extremes.as_ref())?;
    Ok(StationState {
        identity: StationIdentity {
            station_id: station_id.to_owned(),
            city_id: station.identity.city_id.clone(),
            city_slug: station.identity.city_slug.clone(),
            logical_location: station.identity.logical_location.clone(),
            name: station.identity.name.clone(),
            latitude: micros(station.identity.latitude_micros),
            longitude: micros(station.identity.longitude_micros),
            timezone: station.identity.timezone.clone(),
        },
        climate_day: ClimateDay {
            event_date: event_date.to_owned(),
            start: millis(Some(station.climate_day_start_utc_unix_ms))?,
            end: millis(Some(station.climate_day_end_utc_unix_ms))?,
        },
        components: StationComponents {
            observation: component_meta(&station.observation_meta)?,
            weather: component_meta(&station.weather_meta)?,
            extrema: component_meta(&station.extrema_meta)?,
            reports: component_meta(&station.reports_meta)?,
            weather_events: component_meta(&station.weather_events_meta)?,
            forecast: component_meta(&station.forecast_meta)?,
            oracle: component_meta(&station.oracle_meta)?,
        },
        weather,
        observation,
        daily_extremes,
        extreme_high,
        extreme_low,
        reports,
        weather_events,
        forecast,
        oracle_tables,
    })
}

/// The derived current summary for `get_weather`.
///
/// Replacement rule: a fact is projected from its supplied original only while that original
/// is still the current fact in the owner's merged state, i.e. the owner's derived value agrees
/// with the original at the derived precision. A newer accepted update that changed the
/// derived value wins, and the superseded original stays retained on the station
/// (`daily_extremes`, `observation`) as evidence.
fn weather_summary(
    station: &StationV4,
    observation: Option<&Observation>,
    daily: Option<&DailyExtremes>,
) -> Result<StationWeatherView, KernelTransactionError> {
    let weather = &station.weather;
    let derived = &station.observation;
    let supplied_observation = observation
        .filter(|observation| observation.origin == ValueOrigin::Supplied)
        .filter(|observation| {
            observation.observed_at.map(|at| at.timestamp_millis())
                == Some(derived.observed_at_unix_ms)
        });
    // A REST daily original is current while the owner's running summary still equals it.
    // No derived value means nothing has superseded the original. A derived value must agree
    // with the original within the derived milli-Celsius rounding.
    let current_daily =
        |original_c: Option<f64>, original_f: Option<f64>, derived_milli_c: Option<i32>| {
            let Some(derived_milli_c) = derived_milli_c else {
                return true;
            };
            original_c
                .or_else(|| original_f.map(strategy_core_kernel::state::fahrenheit_to_celsius))
                .map(|value| (value * 1_000.0).round() as i32)
                .is_some_and(|original_milli_c| (original_milli_c - derived_milli_c).abs() <= 1)
        };
    let daily_value = |original_f: Option<f64>,
                       original_c: Option<f64>,
                       derived_milli_c: Option<i32>| {
        match daily {
            Some(_) if current_daily(original_c, original_f, derived_milli_c) => original_f
                .or_else(|| original_c.map(strategy_core_kernel::state::celsius_to_fahrenheit))
                .or_else(|| milli_c_to_f(derived_milli_c)),
            _ => milli_c_to_f(derived_milli_c),
        }
    };
    let observation_value =
        |original: Option<f64>, derived_value: Option<f64>| match supplied_observation {
            Some(_) => original.or(derived_value),
            None => derived_value,
        };
    Ok(StationWeatherView {
        station_id: station.identity.station_id.clone(),
        current_temp: observation_value(
            supplied_observation.and_then(|observation| observation.temperature_f),
            milli_c_to_f(weather.current_temperature_milli_c),
        ),
        running_high: daily_value(
            daily.and_then(|daily| daily.daily_high_f),
            daily.and_then(|daily| daily.daily_high_c),
            weather.running_high_milli_c,
        ),
        running_low: daily_value(
            daily.and_then(|daily| daily.daily_low_f),
            daily.and_then(|daily| daily.daily_low_c),
            weather.running_low_milli_c,
        ),
        last_metar_time: millis(weather.last_metar_at_unix_ms)?,
        temp_min_f: observation_value(
            supplied_observation.and_then(|observation| observation.temp_min_f),
            milli_c_to_f(derived.temperature_min_milli_c),
        ),
        temp_max_f: observation_value(
            supplied_observation.and_then(|observation| observation.temp_max_f),
            milli_c_to_f(derived.temperature_max_milli_c),
        ),
        temp_min_c: observation_value(
            supplied_observation.and_then(|observation| observation.temp_min_c),
            milli_c(derived.temperature_min_milli_c),
        ),
        temp_max_c: observation_value(
            supplied_observation.and_then(|observation| observation.temp_max_c),
            milli_c(derived.temperature_max_milli_c),
        ),
        preliminary: weather.preliminary,
        dsm_high: milli_c_to_f(weather.dsm_high_milli_c),
        dsm_low: milli_c_to_f(weather.dsm_low_milli_c),
        dsm_high_time: millis(weather.dsm_high_at_unix_ms)?,
        dsm_low_time: millis(weather.dsm_low_at_unix_ms)?,
        six_hr_high: milli_c_to_f(weather.six_hour_high_milli_c),
        six_hr_low: milli_c_to_f(weather.six_hour_low_milli_c),
        last_dsm_time: millis(weather.dsm_high_at_unix_ms.or(weather.dsm_low_at_unix_ms))?,
        last_six_hr_time: None,
        asos_daily_high_f: daily_value(
            daily.and_then(|daily| daily.asos_daily_high_f),
            daily.and_then(|daily| daily.asos_daily_high_c),
            weather.asos_daily_high_milli_c,
        ),
        asos_daily_low_f: daily_value(
            daily.and_then(|daily| daily.asos_daily_low_f),
            daily.and_then(|daily| daily.asos_daily_low_c),
            weather.asos_daily_low_milli_c,
        ),
        dewpoint: observation_value(
            supplied_observation.and_then(|observation| observation.dewpoint),
            micros(weather.dewpoint_micros),
        ),
        heat_index: observation_value(
            supplied_observation.and_then(|observation| observation.heat_index),
            micros(weather.heat_index_micros),
        ),
        wind_chill: observation_value(
            supplied_observation.and_then(|observation| observation.wind_chill),
            micros(weather.wind_chill_micros),
        ),
        relative_humidity: observation_value(
            supplied_observation.and_then(|observation| observation.relative_humidity),
            micros(weather.relative_humidity_micros),
        ),
        wind_speed: observation_value(
            supplied_observation.and_then(|observation| observation.wind_speed),
            micros(weather.wind_speed_micros),
        ),
        wind_direction: observation_value(
            supplied_observation.and_then(|observation| observation.wind_direction),
            micros(weather.wind_direction_micros),
        ),
        wind_gust: observation_value(
            supplied_observation.and_then(|observation| observation.wind_gust),
            micros(weather.wind_gust_micros),
        ),
        text_description: supplied_observation
            .and_then(|observation| observation.text_description.clone())
            .or_else(|| weather.text_description.clone()),
        lag_seconds: supplied_observation
            .and_then(|observation| observation.lag_seconds)
            .or_else(|| derived.lag_ms.map(|value| value / 1_000)),
    })
}

fn component_meta(meta: &ComponentMetaV4) -> Result<ComponentMeta, KernelTransactionError> {
    Ok(ComponentMeta {
        authority: authority(&meta.authority),
        revision: meta.revision,
        generation: meta.generation,
        updated_at: millis(meta.updated_at_unix_ms)?,
        expected_version: meta.expected_version.clone(),
        refresh_error: meta.refresh_error.clone(),
    })
}

fn market_meta(meta: &MarketMetaV4) -> Result<ComponentMeta, KernelTransactionError> {
    Ok(ComponentMeta {
        authority: authority(&meta.authority),
        revision: meta.revision,
        generation: meta.generation,
        updated_at: millis(meta.updated_at_unix_ms)?,
        expected_version: meta.expected_version.clone(),
        refresh_error: meta.refresh_error.clone(),
    })
}

fn authority(value: &AuthorityV4) -> ComponentAuthority {
    match value {
        AuthorityV4::Warming => ComponentAuthority::Warming,
        AuthorityV4::Current => ComponentAuthority::Current,
        AuthorityV4::RefreshPending => ComponentAuthority::RefreshPending,
        AuthorityV4::Uncertain => ComponentAuthority::Uncertain,
        AuthorityV4::Unavailable => ComponentAuthority::Unavailable,
    }
}

/// Provenance of a derived V4 fact; `station_id` is the owner identity, never the copy inside
/// the V4 component.
fn derived_provenance(provenance: &ProvenanceV4, station_id: &str) -> EventProvenance {
    EventProvenance {
        provider: provenance.provider.clone(),
        source: provenance.source.clone(),
        event_id: provenance.event_id.clone(),
        sequence: provenance
            .sequence
            .and_then(|value| i64::try_from(value).ok()),
        city_sequence: provenance
            .city_sequence
            .and_then(|value| i64::try_from(value).ok()),
        producer_sequence: provenance
            .producer_sequence
            .and_then(|value| i64::try_from(value).ok()),
        emitted_at: millis(provenance.provider_at_unix_ms).ok().flatten(),
        slug: station_id.to_owned(),
        event_key: None,
        source_timestamp: None,
        wmo_emit_time: None,
        producer_received_at: None,
        live_published_at: None,
        persistence_status: None,
        received_at: millis(Some(provenance.received_at_unix_ms)).ok().flatten(),
        connection_epoch: provenance.connection_epoch,
        sid: provenance.sid,
        received_frame_ordinal: provenance.received_frame_ordinal,
    }
}

fn derived_observation(
    observation: &ObservationV4,
    station_id: &str,
) -> Result<Observation, KernelTransactionError> {
    let mut provenance = derived_provenance(&observation.provenance, station_id);
    provenance.source_timestamp = millis(observation.source_timestamp_unix_ms)?;
    provenance.producer_received_at = millis(observation.producer_received_at_unix_ms)?;
    provenance.live_published_at = millis(observation.live_published_at_unix_ms)?;
    provenance.persistence_status = observation.persistence_status.clone();
    Ok(Observation {
        provenance,
        station_id: station_id.to_owned(),
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
        temperature_day_mode: observation.temperature_day_mode.clone(),
        temperature_day_date: observation.temperature_day_date.clone(),
        dewpoint: micros(observation.dewpoint_micros),
        heat_index: micros(observation.heat_index_micros),
        wind_chill: micros(observation.wind_chill_micros),
        relative_humidity: micros(observation.relative_humidity_micros),
        wind_speed: micros(observation.wind_speed_micros),
        wind_direction: micros(observation.wind_direction_micros),
        wind_gust: micros(observation.wind_gust_micros),
        text_description: observation.text_description.clone(),
        barometric_pressure: None,
        sea_level_pressure: None,
        precipitation_1h: None,
        precipitation_3h: None,
        precipitation_6h: None,
        is_locf: None,
        origin: ValueOrigin::Derived,
        supplied: None,
    })
}

fn derived_report(report: &ReportV4, station_id: &str) -> Report {
    Report {
        provenance: derived_provenance(&report.provenance, station_id),
        station_id: station_id.to_owned(),
        report_id: report.report_id.clone(),
        report_type: report.report_type.clone(),
        report_date: report.report_date.clone(),
        report_revision: i64::try_from(report.revision).unwrap_or(i64::MAX),
        report_fingerprint: None,
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
        meta: ComponentMeta::default(),
        origin: ValueOrigin::Derived,
        supplied: None,
    }
}

fn derived_extreme(
    extreme: &ExtremeV4,
    station_id: &str,
    kind: ExtremeKindV5,
    event_date: &str,
) -> Result<Extreme, KernelTransactionError> {
    let value_c = f64::from(extreme.value_milli_c) / 1_000.0;
    Ok(Extreme {
        provenance: derived_provenance(&extreme.provenance, station_id),
        station_id: station_id.to_owned(),
        kind,
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
        origin: ValueOrigin::Derived,
        supplied: None,
    })
}

fn derived_weather_event(
    event: &WeatherEventV4,
    station_id: &str,
) -> Result<WeatherEvent, KernelTransactionError> {
    Ok(WeatherEvent {
        provenance: derived_provenance(&event.provenance, station_id),
        station_id: station_id.to_owned(),
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
        source: event.source.as_ref().map(|source| WeatherEventSource {
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
        meta: ComponentMeta::default(),
        origin: ValueOrigin::Derived,
        supplied: None,
    })
}

fn derived_forecast(
    station: &StationV4,
    station_id: &str,
) -> Result<Forecast, KernelTransactionError> {
    Ok(Forecast {
        station_id: station_id.to_owned(),
        source: "minutetemp".to_owned(),
        received_at: millis(station.forecast_meta.updated_at_unix_ms)?,
        advertised_versions: station.forecast.advertised_versions.clone(),
        models: station
            .forecast
            .models
            .iter()
            .map(derived_forecast_model)
            .collect::<Result<_, _>>()?,
        meta: ComponentMeta::default(),
        origin: ValueOrigin::Derived,
        supplied: None,
    })
}

fn derived_forecast_model(
    model: &ForecastModelV4,
) -> Result<ForecastModel, KernelTransactionError> {
    Ok(ForecastModel {
        id: model.model_id.clone(),
        version: model.version.clone(),
        run_id: model.run_id.clone(),
        fetched_at: millis(model.fetched_at_unix_ms)?,
        issued_at: millis(model.issued_at_unix_ms)?,
        timezone: model.timezone.clone(),
        utc_offset_seconds: model.utc_offset_seconds.map(i64::from),
        hourly: model
            .hourly
            .iter()
            .map(|point| {
                let at = millis(Some(point.at_unix_ms))?;
                Ok(ForecastPoint {
                    time: at.map(|value| value.to_rfc3339()).unwrap_or_default(),
                    at,
                    temperature_f: milli_c_to_f(point.temperature_milli_c),
                    temperature_c: milli_c(point.temperature_milli_c),
                    apparent_f: milli_c_to_f(point.apparent_temperature_milli_c),
                    apparent_c: milli_c(point.apparent_temperature_milli_c),
                    humidity: millionths(point.humidity_millionths),
                    dew_point: milli_c(point.dew_point_milli_c),
                    pressure: micros(point.pressure_msl_micros),
                    wind_speed: micros(point.wind_speed_micros),
                    wind_direction: micros(point.wind_direction_micros),
                    wind_gust: micros(point.wind_gust_micros),
                    cloud_cover: millionths(point.cloud_cover_millionths),
                    precipitation: millionths(point.precipitation_probability_millionths),
                    weather_code: point.weather_code.map(i64::from),
                })
            })
            .collect::<Result<_, KernelTransactionError>>()?,
        origin: ValueOrigin::Derived,
        supplied: None,
    })
}

fn derived_oracle(station: &StationV4) -> Result<OracleTable, KernelTransactionError> {
    let oracle = &station.oracle;
    Ok(OracleTable {
        station_id: oracle.query.station_id.clone(),
        source: "minutetemp".to_owned(),
        received_at: millis(oracle.updated_at_unix_ms)?,
        mode: oracle.query.mode.clone(),
        rank_by: match oracle.query.rank_by {
            RankByV4::High => "high",
            RankByV4::Low => "low",
        }
        .to_owned(),
        days: oracle.query.days.to_string(),
        all_time: None,
        range_start: oracle.range_start.clone(),
        range_end: oracle.range_end.clone(),
        updated_at: millis(station.oracle_meta.updated_at_unix_ms)?,
        modes: vec![oracle.query.mode.clone()],
        scores: oracle
            .rows
            .iter()
            .map(|row| OracleScore {
                rank: Some(row.rank),
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
        meta: ComponentMeta::default(),
        origin: ValueOrigin::Derived,
        supplied: None,
    })
}

fn market_state(
    market: &MarketV4,
    event_date: &str,
) -> Result<MarketState, KernelTransactionError> {
    let ticker = market.ticker.as_ref();
    let quantity = |value: Option<u64>| value.map(hundredths_quantity);
    let mut state = MarketState {
        market_id: market.identity.market_id.clone(),
        opportunity_id: market.identity.opportunity_id.clone(),
        venue: market.identity.venue.clone(),
        source: "kalshi".to_owned(),
        event_ticker: market.identity.event_ticker.clone(),
        series_ticker: market.identity.series_ticker.clone(),
        event_date: event_date.to_owned(),
        strike_type: market.identity.strike_type.clone(),
        fee_type: market.identity.fee_type.clone(),
        fee_multiplier: market
            .identity
            .fee_multiplier_millionths
            .map(|value| value as f64 / 1_000_000.0),
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
        floor_strike_milli_c: market.identity.floor_strike_milli_c,
        floor_strike_milli_f: market.identity.floor_strike_milli_f,
        cap_strike_milli_c: market.identity.cap_strike_milli_c,
        close_time: millis(market.identity.close_at_unix_ms)?,
        expiration_time: millis(market.identity.expiration_at_unix_ms)?,
        components: MarketComponents {
            lifecycle: market_meta(&market.lifecycle_meta)?,
            ticker: market_meta(&market.ticker_meta)?,
            book: market_meta(&market.book_meta)?,
            last_trade: market_meta(&market.last_trade_meta)?,
            final_fact: market_meta(&market.final_fact_meta)?,
        },
        lifecycle: market
            .lifecycle
            .as_ref()
            .map(|lifecycle| {
                Ok::<_, KernelTransactionError>(MarketLifecycle {
                    status: lifecycle.status.clone(),
                    result: lifecycle.result.clone(),
                    connection_epoch: lifecycle.connection_epoch,
                    received_frame_ordinal: lifecycle.received_frame_ordinal,
                    open_at: millis(lifecycle.open_at_unix_ms)?,
                    close_at: millis(lifecycle.close_at_unix_ms)?,
                    settled_at: millis(lifecycle.settled_at_unix_ms)?,
                    updated_at: millis(lifecycle.updated_at_unix_ms)?,
                })
            })
            .transpose()?,
        ticker: ticker
            .map(|ticker| {
                Ok::<_, KernelTransactionError>(TickerQuote {
                    yes_bid: ticker.yes_bid_micros.map(price),
                    yes_ask: ticker.yes_ask_micros.map(price),
                    no_bid: ticker.no_bid_micros.map(price),
                    no_ask: ticker.no_ask_micros.map(price),
                    yes_bid_quantity: quantity(ticker.yes_bid_quantity_hundredths),
                    yes_ask_quantity: quantity(ticker.yes_ask_quantity_hundredths),
                    no_bid_quantity: quantity(ticker.no_bid_quantity_hundredths),
                    no_ask_quantity: quantity(ticker.no_ask_quantity_hundredths),
                    last_price: ticker.last_price_micros.map(price),
                    last_trade_quantity: quantity(ticker.last_trade_quantity_hundredths),
                    volume: quantity(ticker.volume_hundredths),
                    volume_24h: quantity(ticker.volume_24h_hundredths),
                    open_interest: quantity(ticker.open_interest_hundredths),
                    provider_at: millis(ticker.provider_at_unix_ms)?,
                })
            })
            .transpose()?,
        book: market
            .book
            .as_ref()
            .map(|book| {
                let levels = |levels: &[BookLevelV4]| {
                    levels
                        .iter()
                        .map(|level| BookLevel {
                            price: price(level.price_micros),
                            quantity: hundredths_quantity(level.quantity_hundredths),
                        })
                        .collect::<Vec<_>>()
                };
                Ok::<_, KernelTransactionError>(Book {
                    connection_epoch: book.connection_epoch,
                    sid: book.sid,
                    sequence: book.sequence,
                    snapshot_at: millis(book.snapshot_at_unix_ms)?,
                    resync_required: book.resync_required,
                    yes_bids: levels(&book.yes_bids),
                    no_bids: levels(&book.no_bids),
                })
            })
            .transpose()?,
        last_trade: market
            .last_trade
            .as_ref()
            .map(|trade| {
                Ok::<_, KernelTransactionError>(LastTrade {
                    trade_id: trade.trade_id.clone(),
                    yes_price: price(trade.yes_price_micros),
                    no_price: price(trade.no_price_micros),
                    quantity: hundredths_quantity(trade.quantity_hundredths),
                    taker_side: trade.taker_side.clone(),
                    traded_at: millis(Some(trade.traded_at_unix_ms))?,
                })
            })
            .transpose()?,
        final_fact: market
            .final_fact
            .as_ref()
            .map(|fact| {
                Ok::<_, KernelTransactionError>(FinalFact {
                    status: fact.status.clone(),
                    result: fact.result.clone(),
                    settlement_value: micros(fact.settlement_value_micros),
                    settled_price: fact.settled_price_micros.map(price),
                    provider_at: millis(fact.provider_at_unix_ms)?,
                })
            })
            .transpose()?,
        uncertain_fields: market.uncertain_fields.clone(),
        peak_yes_ask: None,
        last_update: ticker
            .and_then(|value| value.provider_at_unix_ms)
            .and_then(|value| millis(Some(value)).ok().flatten()),
        yes_bid_levels: Vec::new(),
        yes_ask_levels: Vec::new(),
        no_bid_levels: Vec::new(),
        no_ask_levels: Vec::new(),
    };
    state.derive_levels();
    Ok(state)
}

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

/// The exact typed event for one transaction. `None` for bootstrap/recovery, which invoke
/// `on_start` instead of `on_event`.
pub struct KernelEvent {
    event: Option<StrategyEvent>,
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
            TriggerRef::BrokerState => Some(StrategyEvent::Unknown {
                event_type: "broker_state".to_owned(),
                emitted_at: Some(decision_at),
            }),
            TriggerRef::Owner(OwnerTriggerV5::Bootstrap | OwnerTriggerV5::Recovery) => None,
            TriggerRef::Owner(OwnerTriggerV5::Observation { .. }) => {
                Some(StrategyEvent::Observation(Box::new(match supplied_event {
                    Some(SuppliedEventV5::Observation(event)) => Observation::from_supplied(event),
                    _ => derived_observation(&station.observation, station_id)?,
                })))
            }
            TriggerRef::Owner(OwnerTriggerV5::StationReport { report_id, .. }) => Some(
                StrategyEvent::StationReport(Box::new(match supplied_event {
                    Some(SuppliedEventV5::Report(event)) => Report::from_supplied(event),
                    _ => derived_report(
                        station
                            .reports
                            .iter()
                            .find(|report| report.report_id == *report_id)
                            .ok_or_else(|| {
                                KernelTransactionError::Kernel("report missing".to_owned())
                            })?,
                        station_id,
                    ),
                })),
            ),
            TriggerRef::Owner(OwnerTriggerV5::NewHigh { .. }) => {
                Some(StrategyEvent::NewHigh(Box::new(match supplied_event {
                    Some(SuppliedEventV5::Extreme(event)) => {
                        Extreme::from_supplied(event, event_date)
                    }
                    _ => derived_extreme(
                        station.extrema.high.as_ref().ok_or_else(|| {
                            KernelTransactionError::Kernel("new high missing".to_owned())
                        })?,
                        station_id,
                        ExtremeKindV5::High,
                        event_date,
                    )?,
                })))
            }
            TriggerRef::Owner(OwnerTriggerV5::NewLow { .. }) => {
                Some(StrategyEvent::NewLow(Box::new(match supplied_event {
                    Some(SuppliedEventV5::Extreme(event)) => {
                        Extreme::from_supplied(event, event_date)
                    }
                    _ => derived_extreme(
                        station.extrema.low.as_ref().ok_or_else(|| {
                            KernelTransactionError::Kernel("new low missing".to_owned())
                        })?,
                        station_id,
                        ExtremeKindV5::Low,
                        event_date,
                    )?,
                })))
            }
            TriggerRef::Owner(OwnerTriggerV5::WeatherEvent { episode_id, .. }) => {
                match supplied_event {
                    Some(SuppliedEventV5::WeatherEvent(event)) => Some(
                        StrategyEvent::WeatherEvent(Box::new(WeatherEvent::from_supplied(event))),
                    ),
                    _ => match station
                        .weather_events
                        .iter()
                        .find(|event| event.event_id == *episode_id)
                    {
                        Some(event) => Some(StrategyEvent::WeatherEvent(Box::new(
                            derived_weather_event(event, station_id)?,
                        ))),
                        // An ended episode is absent from current state and cannot be
                        // reconstructed without its supplied event.
                        None => Some(StrategyEvent::Unknown {
                            event_type: "weather_event".to_owned(),
                            emitted_at: Some(decision_at),
                        }),
                    },
                }
            }
            TriggerRef::Owner(OwnerTriggerV5::MarketPrice {
                market_id,
                emitted_at_unix_ms,
                ..
            }) => {
                let emitted_at = millis(Some(*emitted_at_unix_ms))?
                    .ok_or(KernelTransactionError::InvalidTime)?;
                Some(StrategyEvent::PriceUpdate(Box::new(PriceUpdate {
                    provenance: EventProvenance {
                        source: "kalshi".to_owned(),
                        event_id: Some(market_id.clone()),
                        emitted_at: Some(emitted_at),
                        slug: station_id.to_owned(),
                        ..EventProvenance::default()
                    },
                    station_id: station_id.to_owned(),
                    city_id: String::new(),
                    timestamp: Some(emitted_at),
                    markets: context
                        .owner_state
                        .markets
                        .iter()
                        .map(|market| market_state(market, event_date))
                        .collect::<Result<_, _>>()?,
                })))
            }
            TriggerRef::Owner(OwnerTriggerV5::ForecastUpdated {
                emitted_at_unix_ms, ..
            }) => {
                let model = station.forecast.models.first().ok_or_else(|| {
                    KernelTransactionError::Kernel("forecast model missing".to_owned())
                })?;
                Some(StrategyEvent::ForecastUpdated(Box::new(ForecastUpdated {
                    provenance: EventProvenance {
                        source: "minutetemp".to_owned(),
                        emitted_at: millis(Some(*emitted_at_unix_ms))?,
                        slug: station_id.to_owned(),
                        ..EventProvenance::default()
                    },
                    station_id: station_id.to_owned(),
                    model_id: model.model_id.clone(),
                    version: model.version.clone(),
                })))
            }
            TriggerRef::Owner(OwnerTriggerV5::OracleScoresUpdated {
                emitted_at_unix_ms, ..
            }) => {
                let snapshot_station =
                    station_state(station, context.supplied.station(station_id))?;
                let day_of = snapshot_station
                    .oracle_tables
                    .iter()
                    .find(|table| table.mode == "day_of")
                    .cloned();
                let modes = day_of
                    .as_ref()
                    .map(|table| table.modes.clone())
                    .filter(|modes| !modes.is_empty())
                    .unwrap_or_else(|| vec![station.oracle.query.mode.clone()]);
                let emitted_at = millis(Some(*emitted_at_unix_ms))?
                    .ok_or(KernelTransactionError::InvalidTime)?;
                Some(StrategyEvent::OracleScoresUpdated(Box::new(
                    OracleScoresUpdated {
                        provenance: EventProvenance {
                            source: "minutetemp".to_owned(),
                            emitted_at: Some(emitted_at),
                            slug: station_id.to_owned(),
                            ..EventProvenance::default()
                        },
                        station_id: station_id.to_owned(),
                        modes,
                        updated_at: day_of
                            .as_ref()
                            .and_then(|table| table.updated_at)
                            .or(Some(emitted_at)),
                        overall: None,
                        day_ahead: None,
                        day_of,
                    },
                )))
            }
            TriggerRef::Owner(OwnerTriggerV5::Timer {
                key,
                scheduled_at_epoch_ns,
                ..
            }) => Some(StrategyEvent::TimerWake(TimerWake {
                scheduled_for: Utc.timestamp_nanos(
                    i64::try_from(*scheduled_at_epoch_ns)
                        .map_err(|_| KernelTransactionError::InvalidTime)?,
                ),
                fired_at: Some(decision_at),
                name: key.clone(),
            })),
        };
        Ok(Self { event })
    }

    /// The canonical owned event, or `None` for bootstrap/recovery.
    pub fn event(&self) -> Option<&StrategyEvent> {
        self.event.as_ref()
    }

    pub fn run<K: TransactionKernel>(
        &self,
        kernel: &mut K,
        context: &mut dyn StrategyKernelContext,
    ) -> KernelResult<()> {
        match &self.event {
            Some(event) => event.with_view(|view| kernel.on_event(view, context)),
            None => kernel.on_start(context),
        }
    }

    /// The projected event view; `None` for bootstrap/recovery and for price updates, which
    /// are only presented inside [`KernelEvent::run`] because they borrow per-market views.
    pub fn view(&self) -> Option<StrategyEventView<'_>> {
        self.event.as_ref().and_then(StrategyEvent::view)
    }
}

enum TriggerRef<'a> {
    Owner(&'a OwnerTriggerV5),
    BrokerState,
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
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut output, byte| {
        write!(&mut output, "{byte:02x}").expect("String formatting cannot fail");
        output
    })
}
