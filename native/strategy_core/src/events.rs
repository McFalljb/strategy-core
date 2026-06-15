use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, de};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemperatureDayMode {
    CalendarDay,
    NwsClimateDay,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WuDayMode {
    CalendarDay,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PersistenceStatus {
    Uncommitted,
    Committed,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OracleScoreMode {
    Overall,
    DayAhead,
    DayOf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MarketBracket {
    #[serde(default)]
    pub market_id: String,
    #[serde(deserialize_with = "deserialize_required_text")]
    pub ticker: String,
    #[serde(default)]
    pub yes_price: f64,
    #[serde(default)]
    pub no_price: f64,
    #[serde(default)]
    pub event_ticker: String,
    #[serde(default)]
    pub event_date: String,
    #[serde(default)]
    pub strike_type: String,
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
    #[serde(default)]
    pub yes_bid_levels: Vec<(f64, i64)>,
    #[serde(default)]
    pub yes_ask_levels: Vec<(f64, i64)>,
    #[serde(default)]
    pub no_bid_levels: Vec<(f64, i64)>,
    #[serde(default)]
    pub no_ask_levels: Vec<(f64, i64)>,
    pub orderbook_depth: Option<i64>,
    pub volume: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WeatherEventSource {
    pub metar_type: Option<String>,
    pub flight_category: Option<String>,
    pub wx_string: Option<String>,
    pub wx_token: Option<String>,
    pub wind_speed_kt: Option<f64>,
    pub wind_gust_kt: Option<f64>,
    pub peak_wind_kt: Option<f64>,
    pub peak_wind_direction: Option<i64>,
    pub visibility_mi: Option<f64>,
    pub cb_location: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    #[serde(rename = "type", deserialize_with = "deserialize_observation_type")]
    pub event_type: String,
    pub event_id: Option<String>,
    pub sequence: Option<i64>,
    pub city_sequence: Option<i64>,
    pub emitted_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub slug: String,
    #[serde(deserialize_with = "deserialize_required_text")]
    pub station_id: String,
    pub observed_at: Option<DateTime<Utc>>,
    pub lag_seconds: Option<i64>,
    #[serde(default)]
    pub preliminary: bool,
    pub temperature_f: Option<f64>,
    pub temperature_c: Option<f64>,
    pub temp_min_f: Option<f64>,
    pub temp_max_f: Option<f64>,
    pub temp_min_c: Option<f64>,
    pub temp_max_c: Option<f64>,
    #[serde(default)]
    pub is_from_report: bool,
    pub report_type: Option<String>,
    pub source_report_id: Option<String>,
    pub wu_current_temp_f: Option<f64>,
    pub wu_current_temp_c: Option<f64>,
    pub wu_daily_high_f: Option<f64>,
    pub wu_daily_low_f: Option<f64>,
    pub wu_daily_high_c: Option<f64>,
    pub wu_daily_low_c: Option<f64>,
    pub wu_observation_time: Option<DateTime<Utc>>,
    pub wu_fetched_at: Option<DateTime<Utc>>,
    pub temperature_day_mode: Option<TemperatureDayMode>,
    pub temperature_day_date: Option<String>,
    pub wu_day_mode: Option<WuDayMode>,
    pub wu_day_date: Option<String>,
    pub dewpoint: Option<f64>,
    pub heat_index: Option<f64>,
    pub wind_chill: Option<f64>,
    pub relative_humidity: Option<f64>,
    pub wind_speed: Option<f64>,
    pub wind_direction: Option<f64>,
    pub wind_gust: Option<f64>,
    pub text_description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PriceUpdate {
    #[serde(rename = "type", deserialize_with = "deserialize_price_update_type")]
    pub event_type: String,
    pub event_id: Option<String>,
    pub sequence: Option<i64>,
    pub city_sequence: Option<i64>,
    pub emitted_at: Option<DateTime<Utc>>,
    #[serde(deserialize_with = "deserialize_required_text")]
    pub source: String,
    #[serde(default)]
    pub slug: String,
    #[serde(deserialize_with = "deserialize_required_text")]
    pub station_id: String,
    #[serde(default)]
    pub city_id: String,
    pub timestamp: Option<DateTime<Utc>>,
    #[serde(default)]
    pub markets: Vec<MarketBracket>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ForecastUpdated {
    #[serde(
        rename = "type",
        deserialize_with = "deserialize_forecast_updated_type"
    )]
    pub event_type: String,
    pub event_id: Option<String>,
    pub sequence: Option<i64>,
    pub emitted_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub slug: String,
    #[serde(deserialize_with = "deserialize_required_text")]
    pub station_id: String,
    #[serde(deserialize_with = "deserialize_required_text")]
    pub model_id: String,
    #[serde(deserialize_with = "deserialize_required_text")]
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ForecastVersions {
    #[serde(
        rename = "type",
        deserialize_with = "deserialize_forecast_versions_type"
    )]
    pub event_type: String,
    pub event_id: Option<String>,
    pub sequence: Option<i64>,
    pub emitted_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub slug: String,
    #[serde(deserialize_with = "deserialize_required_text")]
    pub station_id: String,
    #[serde(default)]
    pub versions: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OracleScoreRow {
    #[serde(deserialize_with = "deserialize_required_text")]
    pub model_id: String,
    #[serde(default)]
    pub model_name: String,
    pub is_public: Option<bool>,
    pub combined_mae: Option<f64>,
    pub high_mae: Option<f64>,
    pub low_mae: Option<f64>,
    pub high_bias: Option<f64>,
    pub low_bias: Option<f64>,
    pub day_count: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OracleScoreTable {
    #[serde(deserialize_with = "deserialize_required_text")]
    pub station_id: String,
    #[serde(default)]
    pub range_start: String,
    #[serde(default)]
    pub range_end: String,
    pub days_requested: Option<i64>,
    pub all_time: Option<bool>,
    #[serde(default)]
    pub score_mode: String,
    #[serde(default)]
    pub rank_by: String,
    #[serde(default)]
    pub scores: Vec<OracleScoreRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OracleScoresUpdated {
    #[serde(
        rename = "type",
        deserialize_with = "deserialize_oracle_scores_updated_type"
    )]
    pub event_type: String,
    pub event_id: Option<String>,
    pub sequence: Option<i64>,
    pub emitted_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub slug: String,
    #[serde(deserialize_with = "deserialize_required_text")]
    pub station_id: String,
    #[serde(default)]
    pub modes: Vec<OracleScoreMode>,
    pub updated_at: Option<DateTime<Utc>>,
    pub overall: Option<OracleScoreTable>,
    pub day_ahead: Option<OracleScoreTable>,
    pub day_of: Option<OracleScoreTable>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StationReport {
    #[serde(rename = "type", deserialize_with = "deserialize_station_report_type")]
    pub event_type: String,
    pub event_id: Option<String>,
    pub sequence: Option<i64>,
    pub city_sequence: Option<i64>,
    pub emitted_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub slug: String,
    #[serde(deserialize_with = "deserialize_required_text")]
    pub station_id: String,
    #[serde(deserialize_with = "deserialize_required_text")]
    pub report_id: String,
    #[serde(default)]
    pub report_revision: i64,
    pub report_updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub report_type: String,
    #[serde(default)]
    pub report_date: String,
    pub issuance_time: Option<DateTime<Utc>>,
    pub fetched_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub source_url: String,
    #[serde(default)]
    pub provider: String,
    pub max_temp_f: Option<f64>,
    pub max_temp_c: Option<f64>,
    pub max_temp_time_utc: Option<DateTime<Utc>>,
    pub min_temp_f: Option<f64>,
    pub min_temp_c: Option<f64>,
    pub min_temp_time_utc: Option<DateTime<Utc>>,
    pub temp_f: Option<f64>,
    pub temp_c: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeatherEvent {
    #[serde(rename = "type", deserialize_with = "deserialize_weather_event_type")]
    pub event_type: String,
    pub event_id: Option<String>,
    pub sequence: Option<i64>,
    pub city_sequence: Option<i64>,
    pub emitted_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub slug: String,
    #[serde(deserialize_with = "deserialize_required_text")]
    pub station_id: String,
    #[serde(deserialize_with = "deserialize_required_text")]
    pub id: String,
    #[serde(rename = "event_type")]
    #[serde(deserialize_with = "deserialize_required_text")]
    pub event_type_name: String,
    #[serde(default)]
    pub tier: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub badge: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub summary: String,
    pub started_at: Option<DateTime<Utc>>,
    pub last_confirmed_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub source: Option<WeatherEventSource>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewHigh {
    #[serde(rename = "type", deserialize_with = "deserialize_new_high_type")]
    pub event_type: String,
    pub event_id: Option<String>,
    pub sequence: Option<i64>,
    pub city_sequence: Option<i64>,
    pub emitted_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub event_key: String,
    pub source_timestamp: Option<DateTime<Utc>>,
    pub wmo_emit_time: Option<DateTime<Utc>>,
    pub producer_received_at: Option<DateTime<Utc>>,
    pub live_published_at: Option<DateTime<Utc>>,
    pub persistence_status: Option<PersistenceStatus>,
    pub producer_sequence: Option<i64>,
    #[serde(default)]
    pub slug: String,
    #[serde(deserialize_with = "deserialize_required_text")]
    pub station_id: String,
    pub value_f: f64,
    pub value_c: f64,
    pub prev_value_f: Option<f64>,
    pub observed_at: Option<DateTime<Utc>>,
    pub temperature_day_mode: Option<TemperatureDayMode>,
    pub temperature_day_date: Option<String>,
    #[serde(default)]
    pub is_from_report: bool,
    pub report_type: Option<String>,
    pub source_report_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewLow {
    #[serde(rename = "type", deserialize_with = "deserialize_new_low_type")]
    pub event_type: String,
    pub event_id: Option<String>,
    pub sequence: Option<i64>,
    pub city_sequence: Option<i64>,
    pub emitted_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub event_key: String,
    pub source_timestamp: Option<DateTime<Utc>>,
    pub wmo_emit_time: Option<DateTime<Utc>>,
    pub producer_received_at: Option<DateTime<Utc>>,
    pub live_published_at: Option<DateTime<Utc>>,
    pub persistence_status: Option<PersistenceStatus>,
    pub producer_sequence: Option<i64>,
    #[serde(default)]
    pub slug: String,
    #[serde(deserialize_with = "deserialize_required_text")]
    pub station_id: String,
    pub value_f: f64,
    pub value_c: f64,
    pub prev_value_f: Option<f64>,
    pub observed_at: Option<DateTime<Utc>>,
    pub temperature_day_mode: Option<TemperatureDayMode>,
    pub temperature_day_date: Option<String>,
    #[serde(default)]
    pub is_from_report: bool,
    pub report_type: Option<String>,
    pub source_report_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimerWake {
    #[serde(rename = "type", deserialize_with = "deserialize_timer_wake_type")]
    pub event_type: String,
    pub scheduled_for: DateTime<Utc>,
    pub fired_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShutdownEvent {
    #[serde(rename = "type", deserialize_with = "deserialize_shutdown_type")]
    pub event_type: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum StrategyEvent {
    Observation(Observation),
    PriceUpdate(PriceUpdate),
    ForecastUpdated(ForecastUpdated),
    ForecastVersions(ForecastVersions),
    OracleScoresUpdated(OracleScoresUpdated),
    StationReport(StationReport),
    WeatherEvent(WeatherEvent),
    NewHigh(NewHigh),
    NewLow(NewLow),
    TimerWake(TimerWake),
    ShutdownEvent(ShutdownEvent),
}

pub type EngineEvent = StrategyEvent;

impl<'de> Deserialize<'de> for StrategyEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let event_type = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| de::Error::custom("strategy event type is required"))?;

        match event_type {
            "observation" => decode_strategy_event(value).map(Self::Observation),
            "price_update" => decode_strategy_event(value).map(Self::PriceUpdate),
            "forecast_updated" => decode_strategy_event(value).map(Self::ForecastUpdated),
            "forecast_versions" => decode_strategy_event(value).map(Self::ForecastVersions),
            "oracle_scores_updated" => decode_strategy_event(value).map(Self::OracleScoresUpdated),
            "station_report" => decode_strategy_event(value).map(Self::StationReport),
            "weather_event" => decode_strategy_event(value).map(Self::WeatherEvent),
            "new_high" => decode_strategy_event(value).map(Self::NewHigh),
            "new_low" => decode_strategy_event(value).map(Self::NewLow),
            "timer_wake" => decode_strategy_event(value).map(Self::TimerWake),
            "shutdown" => decode_strategy_event(value).map(Self::ShutdownEvent),
            other => Err(de::Error::custom(format!(
                "unknown strategy event type {other:?}"
            ))),
        }
    }
}

fn decode_strategy_event<T, E>(value: serde_json::Value) -> Result<T, E>
where
    T: for<'de> Deserialize<'de>,
    E: de::Error,
{
    serde_json::from_value(value).map_err(E::custom)
}

fn deserialize_required_text<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.is_empty() {
        return Err(de::Error::custom("required text field must not be blank"));
    }
    Ok(value)
}

fn deserialize_exact_type<'de, D>(
    deserializer: D,
    expected: &'static str,
) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value != expected {
        return Err(de::Error::custom(format!(
            "expected event type {expected:?}, got {value:?}"
        )));
    }
    Ok(value)
}

macro_rules! event_type_deserializer {
    ($name:ident, $expected:literal) => {
        fn $name<'de, D>(deserializer: D) -> Result<String, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserialize_exact_type(deserializer, $expected)
        }
    };
}

event_type_deserializer!(deserialize_observation_type, "observation");
event_type_deserializer!(deserialize_price_update_type, "price_update");
event_type_deserializer!(deserialize_forecast_updated_type, "forecast_updated");
event_type_deserializer!(deserialize_forecast_versions_type, "forecast_versions");
event_type_deserializer!(
    deserialize_oracle_scores_updated_type,
    "oracle_scores_updated"
);
event_type_deserializer!(deserialize_station_report_type, "station_report");
event_type_deserializer!(deserialize_weather_event_type, "weather_event");
event_type_deserializer!(deserialize_new_high_type, "new_high");
event_type_deserializer!(deserialize_new_low_type, "new_low");
event_type_deserializer!(deserialize_timer_wake_type, "timer_wake");
event_type_deserializer!(deserialize_shutdown_type, "shutdown");
