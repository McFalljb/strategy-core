use crate::actions::{
    CancelOrderRequest, ContractQuantity, ContractSide, KernelAction, OrderResult, OrderStatusView,
    PendingOrderView, PlaceOrderRequest, WakeAtRequest,
};
use crate::errors::KernelResult;
use crate::events::{
    ForecastInputSnapshot, OracleInputSnapshot, StationWeatherView, StrategyEventView,
    TickerPriceView,
};
use crate::state::{BrokerFinancialState, MarketState, StationState};
use chrono::{DateTime, Utc};

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

    fn on_finish(&mut self, _ctx: &mut dyn StrategyKernelContext) -> KernelResult<()> {
        Ok(())
    }
}

pub trait StrategyKernelContext {
    fn state(&self) -> &dyn StrategyKernelState;

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
}

pub trait StrategyKernelTelemetry {
    fn counter(&mut self, name: &str, value: f64, fields: &[(&str, &str)]) -> KernelResult<()>;
}
