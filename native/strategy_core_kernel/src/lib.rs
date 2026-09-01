//! Shared native strategy-kernel contract.
//!
//! This crate intentionally defines contracts only. Backtester and Trader own
//! their runtime adapters and broker/risk/accounting implementations.

pub mod actions;
pub mod context;
pub mod errors;
pub mod events;

pub use actions::{
    CancelAllOrdersRequest, CancelOrderRequest, ContractQuantity, ContractSide, KernelAction,
    LogAction, OrderAction, OrderResult, OrderStatus, OrderStatusView, OrderType, PendingOrderView,
    PlaceOrderRequest, StopAction, TelemetryAction, WakeAtRequest,
};
pub use context::{
    NativeKernel, StateReadDiagnostic, StrategyKernelBroker, StrategyKernelContext,
    StrategyKernelData, StrategyKernelRuntime, StrategyKernelState, StrategyKernelTelemetry,
};
pub use errors::{KernelError, KernelResult};
pub use events::{
    ForecastHourlySnapshot, ForecastInputSnapshot, ForecastModelSnapshot, ForecastUpdatedView,
    ForecastVersionsView, HighLowView, MarketBracketView, ObservationView, OracleInputSnapshot,
    OracleModelScoreSnapshot, OracleScoresUpdatedView, PriceLevelView, PriceUpdateView,
    ShutdownView, StationReportView, StationWeatherView, StrategyEventView, TickerPriceView,
    TimerWakeView, WeatherEventSourceView, WeatherEventView,
};
