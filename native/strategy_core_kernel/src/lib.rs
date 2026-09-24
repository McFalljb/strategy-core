//! Shared native strategy-kernel contract and the canonical strategy-facing data model.
//!
//! This crate defines the contract only: the traits a kernel is driven through, the owned
//! canonical state and event model (`state`, `event`), the supplied originals those carry
//! (`supplied`, `decimal`), the borrowed views kernels read (`events`), pure execution fee
//! calculations (`fees`), and pure lookups: stations and series tickers (`stations`), station
//! time zones and climate days (`climate_day`), and component age (`freshness`). Trader and
//! Backtester own their runtime adapters, codecs and broker/risk/accounting implementations.

pub mod actions;
pub mod climate_day;
pub mod context;
pub mod decimal;
pub mod errors;
pub mod event;
pub mod events;
pub mod fees;
pub mod forecast;
pub mod freshness;
pub mod state;
pub mod stations;
pub mod supplied;
pub mod weather;

pub use weather::{WeatherFact, WeatherFactProvenance, WeatherFacts, WeatherField, WeatherValue};

pub use actions::{
    CancelAllOrdersRequest, CancelOrderRequest, ContractQuantity, ContractSide, KernelAction,
    LogAction, OrderAction, OrderResult, OrderStatus, OrderStatusView, OrderType, PendingOrderView,
    PlaceOrderRequest, StopAction, TelemetryAction, WakeAtRequest,
};
pub use context::{
    AnnotationValue, KernelCapabilities, NativeKernel, ParameterValue, PendingTimer, RuntimeMode,
    StateReadDiagnostic, StrategyKernelBroker, StrategyKernelContext, StrategyKernelData,
    StrategyKernelRuntime, StrategyKernelState, StrategyKernelTelemetry, StrategyParameters,
    TimerHandle,
};
pub use decimal::{Decimal, DecimalError, MAX_DECIMAL_SCALE};
pub use errors::{KernelError, KernelResult};
pub use event::{
    ForecastUpdated, ForecastVersions, OracleScoresUpdated, PriceUpdate, StrategyEvent, TimerWake,
};
pub use events::{
    EventProvenanceView, ForecastHourlySnapshot, ForecastInputSnapshot, ForecastModelSnapshot,
    ForecastUpdatedView, ForecastVersionsView, HighLowView, MarketBracketView, ObservationView,
    OracleInputSnapshot, OracleModelScoreSnapshot, OracleScoresUpdatedView, PriceLevelView,
    PriceUpdateView, ShutdownView, StationReportView, StationWeatherView, StrategyEventView,
    TickerPriceView, TimerWakeView, ValueOrigin, WeatherEventSourceView, WeatherEventView,
};
pub use state::{
    Book, BookLevel, BrokerFinancialState, ClimateDay, ComponentAuthority, ComponentMeta,
    DailyExtremes, EventProvenance, Extreme, FinalFact, Forecast, ForecastModel, ForecastPoint,
    LastTrade, MarketComponents, MarketLifecycle, MarketState, Observation, OracleScore,
    OracleTable, Report, StationComponents, StationIdentity, StationState, TickerQuote,
    WeatherEvent, WeatherEventSource,
};
pub use supplied::{
    EventEnvelope, ExtremeKind, SUPPLIED_INPUTS_CONTRACT_VERSION, SuppliedDailyExtremes,
    SuppliedEvent, SuppliedExtreme, SuppliedForecast, SuppliedForecastModel, SuppliedForecastPoint,
    SuppliedInputs, SuppliedObservation, SuppliedOracleScore, SuppliedOracleTable, SuppliedReport,
    SuppliedStation, SuppliedWeatherEvent, SuppliedWeatherEventSource,
};
