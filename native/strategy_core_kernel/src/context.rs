use crate::actions::{
    CancelOrderRequest, ContractQuantity, ContractSide, KernelAction, OrderResult, OrderStatusView,
    PendingOrderView, PlaceOrderRequest, WakeAtRequest,
};
use crate::errors::{KernelError, KernelResult};
use crate::events::{
    ForecastInputSnapshot, OracleInputSnapshot, StationWeatherView, StrategyEventView,
    TickerPriceView,
};
use crate::state::{BrokerFinancialState, MarketState, StationState};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub trait NativeKernel {
    fn name(&self) -> &str;

    fn on_start(&mut self, _ctx: &mut dyn StrategyKernelContext) -> KernelResult<()> {
        Ok(())
    }

    fn on_event(
        &mut self,
        event: StrategyEventView<'_>,
        ctx: &mut dyn StrategyKernelContext,
    ) -> KernelResult<()>;

    /// Not called by the Decision V5 host, which has no final invocation for a Sleeve. The
    /// legacy v2 bot host and the backtester's kernel runner call it.
    fn on_finish(&mut self, _ctx: &mut dyn StrategyKernelContext) -> KernelResult<()> {
        Ok(())
    }
}

pub trait StrategyKernelContext {
    fn state(&self) -> &dyn StrategyKernelState;

    /// The Strategy's configured parameters, read-only and exact. Hosts that do not supply
    /// them return an empty set.
    fn parameters(&self) -> &StrategyParameters {
        StrategyParameters::empty()
    }

    /// What this host grants the kernel for this invocation. The default grants nothing and
    /// states no mode.
    fn capabilities(&self) -> KernelCapabilities {
        KernelCapabilities::default()
    }

    fn data(&self) -> &dyn StrategyKernelData;

    fn broker(&mut self) -> &mut dyn StrategyKernelBroker;

    fn runtime(&mut self) -> &mut dyn StrategyKernelRuntime;

    fn telemetry(&mut self) -> &mut dyn StrategyKernelTelemetry;

    fn emit(&mut self, action: KernelAction) -> KernelResult<()>;
}

/// Complete scoped state as the host delivered it for this invocation.
///
/// Hosts must provide the canonical model, including supplied originals, derived facts,
/// authority, revisions and provenance. Absent or out-of-scope state returns `None`.
/// References remain valid for this invocation; access must not fetch provider data.
/// Convenience access lives on the trait object below, so hosts cannot override it with
/// independent reduced mappings.
pub trait StrategyKernelState {
    fn station(&self, station_id: &str) -> Option<&StationState>;

    fn market(&self, ticker: &str) -> Option<&MarketState>;

    /// Diagnostics of host state reads made for this invocation. The Decision V5 host reads
    /// nothing at decision time (all state arrives in the context), so it returns none;
    /// `dsm_reaction_v10` and the champion kernels still record the (empty) list.
    fn state_read_diagnostics(&self) -> Vec<StateReadDiagnostic> {
        Vec::new()
    }
}

impl dyn StrategyKernelState + '_ {
    pub fn get_price(&self, ticker: &str) -> Option<TickerPriceView<'_>> {
        self.market(ticker).map(MarketState::ticker_view)
    }

    pub fn get_weather(&self, station_id: &str) -> Option<StationWeatherView> {
        self.station(station_id)
            .map(|station| station.weather.clone())
    }

    pub fn latest_forecast(&self, station_id: &str) -> Option<ForecastInputSnapshot<'_>> {
        self.station(station_id)?
            .forecast
            .as_ref()
            .map(|forecast| forecast.snapshot())
    }

    pub fn latest_oracle_scores(
        &self,
        station_id: &str,
        mode: Option<&str>,
        rank_by: Option<&str>,
        days: Option<&str>,
    ) -> Option<OracleInputSnapshot<'_>> {
        self.station(station_id)?
            .oracle_table(mode, rank_by, days)
            .map(|table| table.snapshot())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateReadDiagnostic {
    pub kind: String,
    pub key: String,
    pub status: String,
    pub reason: Option<String>,
    pub client_request_started_at: Option<DateTime<Utc>>,
    pub client_response_received_at: Option<DateTime<Utc>>,
    pub client_latency_us: Option<u64>,
    pub host_request_received_at: Option<DateTime<Utc>>,
    pub host_read_started_at: Option<DateTime<Utc>>,
    pub host_read_completed_at: Option<DateTime<Utc>>,
    pub host_response_sent_at: Option<DateTime<Utc>>,
    pub payload_bytes: Option<u64>,
    pub payload_sha256: Option<String>,
    pub host_state_seq: Option<u64>,
    pub source_feed_event_seq: Option<u64>,
    pub source_state_seq: Option<u64>,
    pub source_observed_at: Option<DateTime<Utc>>,
    pub source_updated_at: Option<DateTime<Utc>>,
}

pub trait StrategyKernelData {}

pub trait StrategyKernelBroker {
    fn financial_state(&self) -> BrokerFinancialState;

    fn buying_power(&self) -> Option<f64>;

    fn position_quantity(&self, ticker: &str, side: ContractSide) -> ContractQuantity;

    fn position_avg_price(&self, ticker: &str, side: ContractSide) -> Option<f64>;

    fn pending_orders(&self) -> Vec<PendingOrderView<'_>> {
        Vec::new()
    }

    fn order_status(&self, _client_order_id: &str) -> Option<OrderStatusView<'_>> {
        None
    }

    fn place_order(&mut self, request: PlaceOrderRequest) -> KernelResult<OrderResult>;

    fn cancel_order(&mut self, _request: CancelOrderRequest) -> KernelResult<bool> {
        Ok(false)
    }

    fn cancel_all_orders(&mut self) -> KernelResult<usize> {
        Ok(0)
    }
}

pub trait StrategyKernelRuntime {
    fn now(&self) -> Option<DateTime<Utc>> {
        None
    }

    fn wake_at(&mut self, request: WakeAtRequest) -> KernelResult<()>;

    /// Schedules a timer as [`Self::wake_at`] does and returns its handle, which
    /// [`Self::cancel_timer`] accepts in this or a later decision; keep it in the checkpoint to
    /// cancel later. Hosts without handles (`KernelCapabilities::timer_handles` false) refuse
    /// and schedule nothing.
    fn schedule_timer(&mut self, _request: WakeAtRequest) -> KernelResult<TimerHandle> {
        Err(KernelError::new("this host does not issue timer handles"))
    }

    /// Cancels the timer the handle names if it is still pending with that generation; a
    /// timer that already fired or was replaced is left alone. Hosts without handles refuse.
    fn cancel_timer(&mut self, _handle: &TimerHandle) -> KernelResult<()> {
        Err(KernelError::new("this host does not cancel timers"))
    }

    /// This Sleeve's pending timers as the host delivered them with this decision, before
    /// anything the kernel schedules or cancels in it. Empty when the host does not report them.
    fn pending_timers(&self) -> Vec<PendingTimer> {
        Vec::new()
    }
}

/// Names one scheduled timer: its key (the `WakeAtRequest` name, or the host's default key)
/// and the generation the host scheduled it under. A later schedule with the same key
/// replaces the timer under a new generation, so an old handle no longer cancels it.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct TimerHandle {
    pub key: String,
    pub generation: String,
}

/// A timer that has not fired or been cancelled.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingTimer {
    pub handle: TimerHandle,
    pub scheduled_for: DateTime<Utc>,
}

pub trait StrategyKernelTelemetry {
    fn counter(&mut self, name: &str, value: f64, fields: &[(&str, &str)]) -> KernelResult<()>;

    /// Records the current level of `name`. Hosts that do not record gauges
    /// (`KernelCapabilities::gauges` is false) drop it.
    fn gauge(&mut self, _name: &str, _value: f64, _fields: &[(&str, &str)]) -> KernelResult<()> {
        Ok(())
    }

    /// Records one typed fact about this decision. Hosts that do not record annotations
    /// (`KernelCapabilities::annotations` is false) drop it.
    fn annotate(
        &mut self,
        _name: &str,
        _value: AnnotationValue<'_>,
        _fields: &[(&str, &str)],
    ) -> KernelResult<()> {
        Ok(())
    }
}

/// The value of one telemetry annotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnnotationValue<'a> {
    Text(&'a str),
    Integer(i64),
    Float(f64),
    Bool(bool),
    Null,
}

/// One configured Strategy parameter value. Decimals keep the configured digits exactly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParameterValue {
    Null,
    Bool(bool),
    I64(i64),
    U64(u64),
    /// `coefficient * 10^-scale`, as configured (trailing zeros are kept).
    Decimal {
        coefficient: i64,
        scale: u8,
    },
    String(String),
}

impl ParameterValue {
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// An integer parameter that fits `i64`.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::I64(value) => Some(*value),
            Self::U64(value) => i64::try_from(*value).ok(),
            _ => None,
        }
    }

    /// An integer parameter that fits `u64`.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::I64(value) => u64::try_from(*value).ok(),
            Self::U64(value) => Some(*value),
            _ => None,
        }
    }

    /// A numeric parameter as `f64`, computed as the kernel initializer's JSON projection
    /// computes it (`coefficient / 10^scale` for a decimal).
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::I64(value) => Some(*value as f64),
            Self::U64(value) => Some(*value as f64),
            Self::Decimal { coefficient, scale } => {
                Some(*coefficient as f64 / 10_f64.powi(i32::from(*scale)))
            }
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

/// The Strategy's configured parameters by key.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StrategyParameters {
    values: BTreeMap<String, ParameterValue>,
}

impl StrategyParameters {
    /// An empty set with a `'static` lifetime, for hosts without parameters.
    pub fn empty() -> &'static Self {
        static EMPTY: StrategyParameters = StrategyParameters {
            values: BTreeMap::new(),
        };
        &EMPTY
    }

    pub fn get(&self, key: &str) -> Option<&ParameterValue> {
        self.values.get(key)
    }

    /// Parameters in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &ParameterValue)> {
        self.values.iter().map(|(key, value)| (key.as_str(), value))
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// A later value for the same key replaces an earlier one.
impl FromIterator<(String, ParameterValue)> for StrategyParameters {
    fn from_iter<I: IntoIterator<Item = (String, ParameterValue)>>(values: I) -> Self {
        Self {
            values: values.into_iter().collect(),
        }
    }
}

/// The deployment a host runs the kernel in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeMode {
    Paper,
    Live,
    /// A backtest or an audit re-run of recorded decisions.
    Replay,
}

/// What a host grants the kernel. Fields are added as hosts gain capabilities, so hosts
/// construct it from [`Default`] (nothing granted) and set what they support.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub struct KernelCapabilities {
    /// The deployment mode, when the host states it.
    pub mode: Option<RuntimeMode>,
    /// `StrategyKernelRuntime::wake_at` schedules one-shot timers.
    pub timers: bool,
    /// `schedule_timer` returns handles that `cancel_timer` accepts, and `pending_timers`
    /// reports the Sleeve's pending timers.
    pub timer_handles: bool,
    /// `StrategyKernelTelemetry::gauge` is recorded.
    pub gauges: bool,
    /// `StrategyKernelTelemetry::annotate` is recorded.
    pub annotations: bool,
}
