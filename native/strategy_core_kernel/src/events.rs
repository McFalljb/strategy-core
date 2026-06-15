use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PriceLevelView {
    pub price: f64,
    pub quantity: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MarketBracketView<'a> {
    pub market_id: &'a str,
    pub ticker: &'a str,
    pub yes_price: f64,
    pub no_price: f64,
    pub event_ticker: &'a str,
    pub event_date: &'a str,
    pub strike_type: &'a str,
    pub floor_strike: Option<f64>,
    pub cap_strike: Option<f64>,
    pub snapshot_time: Option<DateTime<Utc>>,
    pub yes_bid: Option<f64>,
    pub yes_ask: Option<f64>,
    pub no_bid: Option<f64>,
    pub no_ask: Option<f64>,
    pub yes_bid_depth: Option<i64>,
    pub yes_ask_depth: Option<i64>,
    pub no_bid_depth: Option<i64>,
    pub no_ask_depth: Option<i64>,
    pub yes_bid_levels: &'a [PriceLevelView],
    pub yes_ask_levels: &'a [PriceLevelView],
    pub no_bid_levels: &'a [PriceLevelView],
    pub no_ask_levels: &'a [PriceLevelView],
    pub orderbook_depth: Option<i64>,
    pub volume: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PriceUpdateView<'a> {
    pub event_id: Option<&'a str>,
    pub sequence: Option<i64>,
    pub city_sequence: Option<i64>,
    pub emitted_at: Option<DateTime<Utc>>,
    pub source: &'a str,
    pub slug: &'a str,
    pub station_id: &'a str,
    pub city_id: &'a str,
    pub timestamp: Option<DateTime<Utc>>,
    pub markets: &'a [MarketBracketView<'a>],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObservationView<'a> {
    pub event_id: Option<&'a str>,
    pub sequence: Option<i64>,
    pub city_sequence: Option<i64>,
    pub emitted_at: Option<DateTime<Utc>>,
    pub slug: &'a str,
    pub station_id: &'a str,
    pub observed_at: Option<DateTime<Utc>>,
    pub preliminary: bool,
    pub temperature_f: Option<f64>,
    pub temp_min_f: Option<f64>,
    pub temp_max_f: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StationReportView<'a> {
    pub event_id: Option<&'a str>,
    pub sequence: Option<i64>,
    pub city_sequence: Option<i64>,
    pub emitted_at: Option<DateTime<Utc>>,
    pub slug: &'a str,
    pub station_id: &'a str,
    pub report_id: &'a str,
    pub report_type: &'a str,
    pub report_date: &'a str,
    pub report_revision: i64,
    pub issuance_time: Option<DateTime<Utc>>,
    pub fetched_at: Option<DateTime<Utc>>,
    pub max_temp_f: Option<f64>,
    pub min_temp_f: Option<f64>,
    pub temp_f: Option<f64>,
    pub max_temp_time_utc: Option<DateTime<Utc>>,
    pub min_temp_time_utc: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StationWeatherView {
    pub station_id: String,
    pub current_temp: Option<f64>,
    pub running_high: Option<f64>,
    pub running_low: Option<f64>,
    pub last_metar_time: Option<DateTime<Utc>>,
    pub temp_min_f: Option<f64>,
    pub temp_max_f: Option<f64>,
    pub dsm_high: Option<f64>,
    pub dsm_low: Option<f64>,
    pub dsm_high_time: Option<DateTime<Utc>>,
    pub dsm_low_time: Option<DateTime<Utc>>,
    pub six_hr_high: Option<f64>,
    pub six_hr_low: Option<f64>,
    pub asos_daily_high_f: Option<f64>,
    pub asos_daily_low_f: Option<f64>,
    pub wu_daily_high_f: Option<f64>,
    pub wu_daily_low_f: Option<f64>,
    pub lag_seconds: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ForecastHourlySnapshot<'a> {
    pub time: &'a str,
    pub temperature_2m_f: Option<f64>,
    pub temperature_2m_c: Option<f64>,
    pub apparent_temperature_f: Option<f64>,
    pub relative_humidity_2m: Option<f64>,
    pub dew_point_2m: Option<f64>,
    pub pressure_msl: Option<f64>,
    pub wind_speed_10m: Option<f64>,
    pub wind_direction_10m: Option<f64>,
    pub wind_gusts_10m: Option<f64>,
    pub cloud_cover: Option<f64>,
    pub precipitation_probability: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ForecastModelSnapshot<'a> {
    pub model_id: &'a str,
    pub value: f64,
    pub version: &'a str,
    pub updated_at: Option<DateTime<Utc>>,
    pub run_issued_at: Option<DateTime<Utc>>,
    pub hourly: &'a [ForecastHourlySnapshot<'a>],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ForecastInputSnapshot<'a> {
    pub station_id: &'a str,
    pub received_at: Option<DateTime<Utc>>,
    pub source: &'a str,
    pub models: &'a [ForecastModelSnapshot<'a>],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OracleModelScoreSnapshot<'a> {
    pub model_id: &'a str,
    pub model_name: &'a str,
    pub is_public: Option<bool>,
    pub high_mae: Option<f64>,
    pub low_mae: Option<f64>,
    pub combined_mae: Option<f64>,
    pub high_bias: Option<f64>,
    pub low_bias: Option<f64>,
    pub day_count: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OracleInputSnapshot<'a> {
    pub station_id: &'a str,
    pub received_at: Option<DateTime<Utc>>,
    pub source: &'a str,
    pub score_mode: &'a str,
    pub rank_by: &'a str,
    pub days_requested: &'a str,
    pub range_start: &'a str,
    pub range_end: &'a str,
    pub scores: &'a [OracleModelScoreSnapshot<'a>],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimerWakeView<'a> {
    pub scheduled_for: DateTime<Utc>,
    pub fired_at: Option<DateTime<Utc>>,
    pub name: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShutdownView<'a> {
    pub reason: &'a str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TickerPriceView<'a> {
    pub ticker: &'a str,
    pub event_ticker: &'a str,
    pub event_date: &'a str,
    pub strike_type: &'a str,
    pub floor_strike: Option<f64>,
    pub cap_strike: Option<f64>,
    pub yes_bid: Option<f64>,
    pub yes_ask: Option<f64>,
    pub no_bid: Option<f64>,
    pub no_ask: Option<f64>,
    pub yes_bid_depth: Option<i64>,
    pub yes_ask_depth: Option<i64>,
    pub no_bid_depth: Option<i64>,
    pub no_ask_depth: Option<i64>,
    pub yes_bid_levels: &'a [PriceLevelView],
    pub yes_ask_levels: &'a [PriceLevelView],
    pub no_bid_levels: &'a [PriceLevelView],
    pub no_ask_levels: &'a [PriceLevelView],
    pub orderbook_depth: Option<i64>,
    pub last_update: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StrategyEventView<'a> {
    PriceUpdate(PriceUpdateView<'a>),
    Observation(ObservationView<'a>),
    StationReport(StationReportView<'a>),
    TimerWake(TimerWakeView<'a>),
    Shutdown(ShutdownView<'a>),
    Unknown { event_type: &'a str },
}

impl StrategyEventView<'_> {
    #[must_use]
    pub fn event_type(&self) -> &str {
        match self {
            Self::PriceUpdate(_) => "price_update",
            Self::Observation(_) => "observation",
            Self::StationReport(_) => "station_report",
            Self::TimerWake(_) => "timer_wake",
            Self::Shutdown(_) => "shutdown",
            Self::Unknown { event_type } => event_type,
        }
    }
}
