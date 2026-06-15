//! Shared native strategy-kernel contract.
//!
//! This crate intentionally defines contracts only. Backtester and Trader own
//! their runtime adapters and broker/risk/accounting implementations.

pub mod actions;
pub mod context;
pub mod errors;
pub mod events;

pub use actions::{
    CancelOrderRequest, ContractSide, KernelAction, LogAction, OrderAction, OrderResult,
    OrderStatus, OrderType, PlaceOrderRequest, StopAction, TelemetryAction, WakeAtRequest,
};
pub use context::{
    NativeKernel, StrategyKernelBroker, StrategyKernelContext, StrategyKernelData,
    StrategyKernelRuntime, StrategyKernelState, StrategyKernelTelemetry,
};
pub use errors::{KernelError, KernelResult};
pub use events::{
    ForecastHourlySnapshot, ForecastInputSnapshot, ForecastModelSnapshot, MarketBracketView,
    ObservationView, OracleInputSnapshot, OracleModelScoreSnapshot, PriceLevelView,
    PriceUpdateView, ShutdownView, StationReportView, StationWeatherView, StrategyEventView,
    TickerPriceView, TimerWakeView,
};
