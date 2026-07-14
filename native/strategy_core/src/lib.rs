//! Rust parity surface for the shared `strategy-core` strategy contract.
//!
//! The existing `strategy_core_kernel` crate remains the narrow hot-loop
//! boundary for native strategies. This crate mirrors the broader Python
//! package surface so Trader and Backtester can share one Rust contract instead
//! of each engine inventing local copies of broker, fee, runtime, and helper
//! types.

pub mod broker;
pub mod capabilities;
pub mod climate_day;
pub mod context;
pub mod data;
pub mod events;
pub mod fees;
pub mod http;
pub mod kalshi;
pub mod minutetemp;
pub mod models;
pub mod native;
pub mod queries;
pub mod runtime;
pub mod signals;
pub mod state;
pub mod stations;
pub mod telemetry;

pub use broker::{
    Action, Broker, BrokerOrderUpdate, BrokerUpdateStatus, ContractSide, OrderExecutionStyle,
    OrderIntent, OrderResult, OrderStatus, OrderTimePolicy, OrderType, PendingOrder, Position,
};
pub use capabilities::{EventDelivery, RuntimeCapabilities};
pub use climate_day::{
    ClimateDayError, climate_day_date, climate_day_end, climate_day_has_ended, parse_climate_date,
    station_timezone,
};
pub use context::{StrategyContext, StrategyHandler};
pub use data::{ForecastRunLookup, StrategyDataClient};
pub use events::{
    EngineEvent, ForecastUpdated, ForecastVersions, MarketBracket, NewHigh, NewLow, Observation,
    OracleScoreMode, OracleScoreRow, OracleScoreTable, OracleScoresUpdated, PersistenceStatus,
    PriceUpdate, ShutdownEvent, StationReport, StrategyEvent, TemperatureDayMode, TimerWake,
    WeatherEvent, WeatherEventSource, WuDayMode,
};
pub use fees::{
    FeeCalculation, FeeError, FeeResult, FeeType, LiquidityRole, apply_fee_rounding,
    calculate_fill_fee, calculate_trade_fee,
};
pub use http::{HttpClient, HttpMethod, HttpRequest, HttpResponse};
pub use kalshi::{
    KalshiCollateralReturnType, KalshiCreateOrderResponse, KalshiEventLifecycleMessage,
    KalshiFixedCount, KalshiFixedPrice, KalshiGetMarketResponse, KalshiGetOrderResponse,
    KalshiGetOrderbookResponse, KalshiGetOrderbooksResponse, KalshiGetOrdersResponse,
    KalshiImmediateTimeInForce, KalshiListSubscriptionsCommand, KalshiMarket,
    KalshiMarketLifecycleEventType, KalshiMarketLifecycleMessage, KalshiMarketLifecycleMetadata,
    KalshiMarketOrderbook, KalshiMarketPositionMessage, KalshiMarketResult, KalshiMarketSide,
    KalshiMarketStatus, KalshiMarketsPage, KalshiMveSelectedLeg, KalshiOrder, KalshiOrderAction,
    KalshiOrderCreateRequest, KalshiOrderStatus, KalshiOrderType, KalshiOrderbook,
    KalshiOrderbookDeltaMessage, KalshiOrderbookLevel, KalshiOrderbookSnapshotMessage,
    KalshiPriceLevelStructure, KalshiPriceRange, KalshiSelfTradePreventionType,
    KalshiSubscribeCommand, KalshiSubscriptionUpdateAction, KalshiTickerMessage, KalshiTimeInForce,
    KalshiTradeMessage, KalshiUnsubscribeCommand, KalshiUpdateSubscriptionCommand,
    KalshiUserFillMessage, KalshiUserOrderMessage, KalshiWsChannel, KalshiWsMessage,
};
pub use kernel::NativeKernel;
pub use minutetemp::{
    CityInfo, CursorPage, DataResolution, EffectiveLimits, ForecastBundle, ForecastBundleRun,
    ForecastRunData, ForecastRunSummary, ForecastRunsPage, HourlyForecastRecord, IpGuardLimits,
    LatestObservationData, LatestReportsData, ObservationRecord, OracleModelScoreRecord,
    OracleRankBy, OracleScoreData, PlanTier, ReportClockSchedule, ReportIntervalSchedule,
    ReportMultiHourSchedule, ReportSchedule, ReportScheduleBasis, ReportScheduleEntry, ReportType,
    StationForecastData, StationInfo, StationReportHistoryPage, StationReportRecord,
    StationReportsData, TemperatureUnit,
};
pub use models::{
    JSONObject, JSONValue, JsonObject, JsonValue, OrderId, StrategyConfig, TelemetryField,
    TelemetryFields,
};
pub use native::{
    NativeKernelFactory, NativeKernelResult, NativeKernelRunError, NativeKernelRunner,
    NativeKernelStatus, NativeKernelUnavailable, NativeKernelUnavailableError,
    NativeStrategyContext, get_native_kernel_runner, run_native_or_fallback,
};
pub use queries::{
    DateLike, ForecastQuery, ForecastRunQuery, ForecastRunsQuery, LatestObservationQuery,
    LatestReportsQuery, LimitsQuery, LocalDateLike, OracleScoresQuery, ReportHistoryQuery,
    ReportsQuery,
};
pub use runtime::{
    EngineClock, MarketType, RuntimeMode, StrategyRuntime, StrategyScope, TimerHandle, WorkHandle,
};
pub use signals::{SIGNAL_DSM_REACTION, SIGNAL_METAR_6HR_LOW, SIGNAL_METAR_6HR_NEW_LOW};
pub use state::{
    ForecastHourly, FreshnessDomain, FreshnessDomainSummary, FreshnessSnapshot, FreshnessStatus,
    FreshnessSummary, MarketStateView, ModelForecast, OracleModelScore, OracleScoreDays,
    PriceLevel, StationForecast, StationOracleScores, StationWeather, TickerPrices,
};
pub use stations::{
    CITY_TO_ICAO, ICAO_TO_CITY_CODES, MARKET_TYPE_PREFIX, STATION_TIMEZONES, StationError,
    TICKER_PREFIXES, city_codes_for_market_type, primary_city_code_for_market_type,
    primary_city_code_for_series, station_from_event_ticker, ticker_prefixes_for_station,
};
pub use telemetry::{StrategyLogger, Telemetry};

pub mod kernel {
    pub use strategy_core_kernel::*;
}
