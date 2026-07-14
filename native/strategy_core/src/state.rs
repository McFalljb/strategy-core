use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub type PriceLevel = (f64, i64);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FreshnessStatus {
    Fresh,
    Stale,
    Missing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FreshnessDomain {
    Weather,
    Forecast,
    Oracle,
    Price,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FreshnessSnapshot {
    pub domain: FreshnessDomain,
    pub key: String,
    pub status: FreshnessStatus,
    pub source: Option<String>,
    pub updated_at: Option<DateTime<Utc>>,
    pub observed_at: Option<DateTime<Utc>>,
    pub stale_after_seconds: Option<f64>,
    pub age_seconds: Option<f64>,
    pub invalidation_reason: Option<String>,
    pub detail: Option<String>,
}

impl FreshnessSnapshot {
    #[must_use]
    pub const fn is_stale(&self) -> bool {
        matches!(self.status, FreshnessStatus::Stale)
    }

    #[must_use]
    pub const fn is_missing(&self) -> bool {
        matches!(self.status, FreshnessStatus::Missing)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FreshnessDomainSummary {
    pub domain: FreshnessDomain,
    pub tracked_count: i64,
    pub fresh_count: i64,
    pub stale_count: i64,
    pub stalest_age_seconds: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FreshnessSummary {
    pub as_of: DateTime<Utc>,
    pub domains: Vec<FreshnessDomainSummary>,
}

impl FreshnessSummary {
    #[must_use]
    pub fn tracked_count(&self) -> i64 {
        self.domains.iter().map(|domain| domain.tracked_count).sum()
    }

    #[must_use]
    pub fn stale_count(&self) -> i64 {
        self.domains.iter().map(|domain| domain.stale_count).sum()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ForecastHourly {
    #[serde(default)]
    pub time: String,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelForecast {
    pub model_id: String,
    pub value: f64,
    #[serde(default)]
    pub version: String,
    pub updated_at: Option<DateTime<Utc>>,
    pub run_issued_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub hourly: Vec<ForecastHourly>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StationForecast {
    #[serde(default)]
    pub model_forecasts: BTreeMap<String, ModelForecast>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OracleModelScore {
    pub model_id: String,
    #[serde(default)]
    pub model_name: String,
    pub combined_mae: Option<f64>,
    pub high_mae: Option<f64>,
    pub low_mae: Option<f64>,
    pub high_bias: Option<f64>,
    pub low_bias: Option<f64>,
    pub day_count: Option<i64>,
    pub is_public: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StationOracleScores {
    pub station_id: String,
    #[serde(default)]
    pub scores: Vec<OracleModelScore>,
    #[serde(default)]
    pub rank_by: String,
    #[serde(default)]
    pub score_mode: String,
    #[serde(default)]
    pub days_requested: String,
    #[serde(default)]
    pub range_start: String,
    #[serde(default)]
    pub range_end: String,
    pub updated_at: Option<DateTime<Utc>>,
}

/// A caller-supplied oracle lookback selector.
///
/// Python accepts either an integer or string for `days` and compares the
/// normalized string value with `StationOracleScores::days_requested`. This
/// enum preserves the same semantics without accepting unrelated Rust types.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OracleScoreDays<'a> {
    Text(&'a str),
    Count(i64),
}

impl OracleScoreDays<'_> {
    #[must_use]
    pub fn matches(self, days_requested: &str) -> bool {
        match self {
            Self::Text(value) => value == days_requested,
            Self::Count(value) => value.to_string() == days_requested,
        }
    }
}

impl<'a> From<&'a str> for OracleScoreDays<'a> {
    fn from(value: &'a str) -> Self {
        Self::Text(value)
    }
}

impl From<i64> for OracleScoreDays<'_> {
    fn from(value: i64) -> Self {
        Self::Count(value)
    }
}

impl StationOracleScores {
    /// Return whether the stored metadata satisfies every supplied selector.
    #[must_use]
    pub fn matches_selectors(
        &self,
        days: Option<OracleScoreDays<'_>>,
        mode: Option<&str>,
        rank_by: Option<&str>,
    ) -> bool {
        days.is_none_or(|value| value.matches(&self.days_requested))
            && mode.is_none_or(|value| value == self.score_mode)
            && rank_by.is_none_or(|value| value == self.rank_by)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StationWeather {
    pub current_temp: Option<f64>,
    pub running_high: Option<f64>,
    pub running_low: Option<f64>,
    pub last_metar_time: Option<DateTime<Utc>>,
    pub temp_min_f: Option<f64>,
    pub temp_max_f: Option<f64>,
    pub temp_min_c: Option<f64>,
    pub temp_max_c: Option<f64>,
    #[serde(default)]
    pub preliminary: bool,
    pub lag_seconds: Option<i64>,
    pub wu_current_temp_f: Option<f64>,
    pub wu_current_temp_c: Option<f64>,
    pub wu_daily_high_f: Option<f64>,
    pub wu_daily_low_f: Option<f64>,
    pub wu_daily_high_c: Option<f64>,
    pub wu_daily_low_c: Option<f64>,
    pub wu_observation_time: Option<DateTime<Utc>>,
    pub wu_fetched_at: Option<DateTime<Utc>>,
    pub asos_daily_high_f: Option<f64>,
    pub asos_daily_low_f: Option<f64>,
    pub dewpoint: Option<f64>,
    pub heat_index: Option<f64>,
    pub wind_chill: Option<f64>,
    pub relative_humidity: Option<f64>,
    pub wind_speed: Option<f64>,
    pub wind_direction: Option<f64>,
    pub wind_gust: Option<f64>,
    pub text_description: Option<String>,
    pub dsm_high: Option<f64>,
    pub dsm_low: Option<f64>,
    pub dsm_high_time: Option<DateTime<Utc>>,
    pub dsm_low_time: Option<DateTime<Utc>>,
    pub six_hr_high: Option<f64>,
    pub six_hr_low: Option<f64>,
    pub last_dsm_time: Option<DateTime<Utc>>,
    pub last_six_hr_time: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TickerPrices {
    #[serde(default)]
    pub ticker: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub event_ticker: String,
    #[serde(default)]
    pub event_date: String,
    #[serde(default)]
    pub series_ticker: String,
    #[serde(default)]
    pub fee_type: String,
    pub fee_multiplier: Option<f64>,
    #[serde(default)]
    pub strike_type: String,
    pub floor_strike: Option<f64>,
    pub cap_strike: Option<f64>,
    #[serde(default)]
    pub yes_price: f64,
    #[serde(default)]
    pub no_price: f64,
    pub yes_bid: Option<f64>,
    pub yes_ask: Option<f64>,
    pub no_bid: Option<f64>,
    pub no_ask: Option<f64>,
    pub yes_bid_depth: Option<i64>,
    pub yes_ask_depth: Option<i64>,
    pub no_bid_depth: Option<i64>,
    pub no_ask_depth: Option<i64>,
    #[serde(default)]
    pub yes_bid_levels: Vec<PriceLevel>,
    #[serde(default)]
    pub yes_ask_levels: Vec<PriceLevel>,
    #[serde(default)]
    pub no_bid_levels: Vec<PriceLevel>,
    #[serde(default)]
    pub no_ask_levels: Vec<PriceLevel>,
    pub orderbook_depth: Option<i64>,
    pub volume: Option<f64>,
    pub peak_yes_ask: Option<f64>,
    pub last_update: Option<DateTime<Utc>>,
}

impl Default for TickerPrices {
    fn default() -> Self {
        Self {
            ticker: String::new(),
            source: String::new(),
            event_ticker: String::new(),
            event_date: String::new(),
            series_ticker: String::new(),
            fee_type: String::new(),
            fee_multiplier: None,
            strike_type: String::new(),
            floor_strike: None,
            cap_strike: None,
            yes_price: 0.0,
            no_price: 0.0,
            yes_bid: None,
            yes_ask: None,
            no_bid: None,
            no_ask: None,
            yes_bid_depth: None,
            yes_ask_depth: None,
            no_bid_depth: None,
            no_ask_depth: None,
            yes_bid_levels: Vec::new(),
            yes_ask_levels: Vec::new(),
            no_bid_levels: Vec::new(),
            no_ask_levels: Vec::new(),
            orderbook_depth: None,
            volume: None,
            peak_yes_ask: None,
            last_update: None,
        }
    }
}

pub trait MarketStateView {
    fn get_weather(&self, station: &str) -> Option<&StationWeather>;
    fn get_forecast(&self, station: &str) -> Option<&StationForecast>;
    fn get_oracle_scores(
        &self,
        station: &str,
        days: Option<OracleScoreDays<'_>>,
        mode: Option<&str>,
        rank_by: Option<&str>,
    ) -> Option<&StationOracleScores>;
    fn get_prices(&self, ticker: &str) -> Option<&TickerPrices>;
    fn get_weather_freshness(&self, station: &str) -> FreshnessSnapshot;
    fn get_forecast_freshness(&self, station: &str) -> FreshnessSnapshot;
    fn get_oracle_scores_freshness(&self, station: &str) -> FreshnessSnapshot;
    fn get_price_freshness(&self, ticker: &str) -> FreshnessSnapshot;
    fn freshness_summary(&self) -> FreshnessSummary;
}
