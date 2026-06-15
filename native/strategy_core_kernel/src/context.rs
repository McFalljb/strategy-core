use crate::actions::{ContractSide, KernelAction, OrderResult, PlaceOrderRequest, WakeAtRequest};
use crate::errors::KernelResult;
use crate::events::{
    ForecastInputSnapshot, OracleInputSnapshot, StationWeatherView, StrategyEventView,
    TickerPriceView,
};

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

pub trait StrategyKernelState {
    fn get_price(&self, ticker: &str) -> Option<TickerPriceView<'_>>;

    fn get_weather(&self, _station_id: &str) -> Option<StationWeatherView> {
        None
    }

    fn latest_forecast(&self, _station_id: &str) -> Option<ForecastInputSnapshot<'_>> {
        None
    }

    fn latest_oracle_scores(
        &self,
        _station_id: &str,
        _mode: Option<&str>,
        _rank_by: Option<&str>,
        _days: Option<&str>,
    ) -> Option<OracleInputSnapshot<'_>> {
        None
    }
}

pub trait StrategyKernelData {}

pub trait StrategyKernelBroker {
    fn buying_power(&self) -> Option<f64>;

    fn position_quantity(&self, ticker: &str, side: ContractSide) -> i64;

    fn position_avg_price(&self, ticker: &str, side: ContractSide) -> Option<f64>;

    fn place_order(&mut self, request: PlaceOrderRequest) -> KernelResult<OrderResult>;
}

pub trait StrategyKernelRuntime {
    fn wake_at(&mut self, request: WakeAtRequest) -> KernelResult<()>;
}

pub trait StrategyKernelTelemetry {
    fn counter(&mut self, name: &str, value: f64, fields: &[(&str, &str)]) -> KernelResult<()>;
}
