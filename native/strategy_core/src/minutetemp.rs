use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Deserializer, Serialize, de};

pub type ReportType = String;
pub type TemperatureUnit = String;
pub type PlanTier = String;
pub type DataResolution = String;
pub type TemperatureDayMode = String;
pub type WuDayMode = String;
pub type OracleScoreMode = String;
pub type OracleRankBy = String;
pub type ReportScheduleBasis = String;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CityInfo {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub timezone: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StationInfo {
    #[serde(default)]
    pub station_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_temperature_unit")]
    pub temperature_unit: String,
    pub uses_nws_climate_day: Option<bool>,
}

impl Default for StationInfo {
    fn default() -> Self {
        Self {
            station_id: String::new(),
            name: String::new(),
            temperature_unit: default_temperature_unit(),
            uses_nws_climate_day: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ObservationRecord {
    pub observation_time: Option<DateTime<Utc>>,
    pub temperature_f: Option<f64>,
    pub temperature_c: Option<f64>,
    pub dewpoint: Option<f64>,
    pub heat_index: Option<f64>,
    pub wind_chill: Option<f64>,
    pub relative_humidity: Option<f64>,
    pub barometric_pressure: Option<f64>,
    pub sea_level_pressure: Option<f64>,
    pub wind_speed: Option<f64>,
    pub wind_direction: Option<f64>,
    pub wind_gust: Option<f64>,
    pub text_description: Option<String>,
    pub precipitation_1h: Option<f64>,
    pub precipitation_3h: Option<f64>,
    pub precipitation_6h: Option<f64>,
    #[serde(default)]
    pub is_locf: bool,
    #[serde(default)]
    pub is_from_report: bool,
    pub report_type: Option<String>,
    pub source_report_id: Option<String>,
    pub temp_min_f: Option<f64>,
    pub temp_max_f: Option<f64>,
    pub temp_min_c: Option<f64>,
    pub temp_max_c: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StationReportRecord {
    #[serde(default)]
    pub report_id: String,
    #[serde(default)]
    pub report_revision: i64,
    pub report_updated_at: Option<DateTime<Utc>>,
    pub report_type: Option<String>,
    pub report_date: Option<NaiveDate>,
    pub issuance_time: Option<DateTime<Utc>>,
    pub fetched_at: Option<DateTime<Utc>>,
    pub max_temp_f: Option<f64>,
    pub max_temp_c: Option<f64>,
    pub max_temp_time_utc: Option<DateTime<Utc>>,
    pub min_temp_f: Option<f64>,
    pub min_temp_c: Option<f64>,
    pub min_temp_time_utc: Option<DateTime<Utc>>,
    pub temp_f: Option<f64>,
    pub temp_c: Option<f64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReportClockSchedule {
    #[serde(default = "default_report_schedule_basis")]
    pub basis: String,
    pub hour: Option<i64>,
    pub minute: Option<i64>,
    pub utc_hour: Option<i64>,
    pub utc_minute: Option<i64>,
    pub local_hour: Option<i64>,
    pub local_minute: Option<i64>,
    #[serde(default)]
    pub label: String,
}

impl Default for ReportClockSchedule {
    fn default() -> Self {
        Self {
            basis: default_report_schedule_basis(),
            hour: None,
            minute: None,
            utc_hour: None,
            utc_minute: None,
            local_hour: None,
            local_minute: None,
            label: String::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReportIntervalSchedule {
    pub interval_minutes: i64,
    pub utc_minute: Option<i64>,
    pub local_minute: Option<i64>,
    #[serde(default)]
    pub label: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReportMultiHourSchedule {
    #[serde(default)]
    pub utc_hours: Vec<i64>,
    #[serde(default)]
    pub local_hours: Vec<i64>,
    pub utc_minute: Option<i64>,
    pub local_minute: Option<i64>,
    #[serde(default)]
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ReportSchedule {
    Clock(ReportClockSchedule),
    Interval(ReportIntervalSchedule),
    MultiHour(ReportMultiHourSchedule),
}

impl<'de> Deserialize<'de> for ReportSchedule {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| de::Error::custom("report schedule must be an object"))?;

        if object.contains_key("interval_minutes") {
            return serde_json::from_value(value)
                .map(Self::Interval)
                .map_err(de::Error::custom);
        }
        if object.contains_key("utc_hours") || object.contains_key("local_hours") {
            return serde_json::from_value(value)
                .map(Self::MultiHour)
                .map_err(de::Error::custom);
        }
        serde_json::from_value(value)
            .map(Self::Clock)
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ReportScheduleEntry {
    Single(ReportSchedule),
    Multiple(Vec<ReportSchedule>),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HourlyForecastRecord {
    pub time: Option<DateTime<Utc>>,
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ForecastBundleRun {
    #[serde(default)]
    pub id: String,
    pub fetched_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub timezone: String,
    pub utc_offset_seconds: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ForecastBundle {
    #[serde(default)]
    pub model_id: String,
    pub forecast_run: Option<ForecastBundleRun>,
    #[serde(default)]
    pub hourly: Vec<HourlyForecastRecord>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OracleModelScoreRecord {
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub model_name: String,
    pub is_public: Option<bool>,
    pub high_mae: Option<f64>,
    pub low_mae: Option<f64>,
    pub high_bias: Option<f64>,
    pub low_bias: Option<f64>,
    pub combined_mae: Option<f64>,
    pub day_count: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OracleScoreData {
    #[serde(default)]
    pub station_id: String,
    pub range_start: Option<NaiveDate>,
    pub range_end: Option<NaiveDate>,
    pub days_requested: Option<i64>,
    pub all_time: Option<bool>,
    #[serde(default = "default_oracle_score_mode")]
    pub score_mode: String,
    #[serde(default = "default_oracle_rank_by")]
    pub rank_by: String,
    #[serde(default)]
    pub scores: Vec<OracleModelScoreRecord>,
}

impl Default for OracleScoreData {
    fn default() -> Self {
        Self {
            station_id: String::new(),
            range_start: None,
            range_end: None,
            days_requested: None,
            all_time: None,
            score_mode: default_oracle_score_mode(),
            rank_by: default_oracle_rank_by(),
            scores: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CursorPage {
    pub limit: Option<i64>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ForecastRunSummary {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub station_id: String,
    #[serde(default)]
    pub model_id: String,
    pub forecast_time: Option<DateTime<Utc>>,
    pub fetched_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub timezone: String,
    pub utc_offset_seconds: Option<i64>,
    #[serde(default)]
    pub data_hash: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct IpGuardLimits {
    pub requests_per_second: Option<i64>,
    pub burst: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectiveLimits {
    #[serde(default = "default_plan_tier")]
    pub tier: String,
    pub requests_per_minute: Option<i64>,
    pub daily_max: Option<i64>,
    pub max_history_days: Option<i64>,
    pub ip_guard: Option<IpGuardLimits>,
    pub rate_limit_remaining: Option<i64>,
    pub rate_limit_reset_seconds: Option<i64>,
}

impl Default for EffectiveLimits {
    fn default() -> Self {
        Self {
            tier: default_plan_tier(),
            requests_per_minute: None,
            daily_max: None,
            max_history_days: None,
            ip_guard: None,
            rate_limit_remaining: None,
            rate_limit_reset_seconds: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LatestObservationData {
    pub city: Option<CityInfo>,
    pub station: Option<StationInfo>,
    pub observation: Option<ObservationRecord>,
    pub daily_high_f: Option<f64>,
    pub daily_low_f: Option<f64>,
    pub daily_high_c: Option<f64>,
    pub daily_low_c: Option<f64>,
    pub asos_daily_high_f: Option<f64>,
    pub asos_daily_low_f: Option<f64>,
    pub asos_daily_high_c: Option<f64>,
    pub asos_daily_low_c: Option<f64>,
    pub wu_current_temp_f: Option<f64>,
    pub wu_current_temp_c: Option<f64>,
    pub wu_daily_high_f: Option<f64>,
    pub wu_daily_low_f: Option<f64>,
    pub wu_daily_high_c: Option<f64>,
    pub wu_daily_low_c: Option<f64>,
    pub wu_observation_time: Option<DateTime<Utc>>,
    pub wu_fetched_at: Option<DateTime<Utc>>,
    pub temperature_day_mode: Option<String>,
    pub temperature_day_date: Option<NaiveDate>,
    pub wu_day_mode: Option<String>,
    pub wu_day_date: Option<NaiveDate>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StationForecastData {
    pub city: Option<CityInfo>,
    pub station: Option<StationInfo>,
    #[serde(default)]
    pub forecasts: Vec<ForecastBundle>,
    pub count: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LatestReportsData {
    #[serde(default)]
    pub reports: Vec<StationReportRecord>,
    pub report_schedules: Option<BTreeMap<String, ReportScheduleEntry>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StationReportsData {
    #[serde(default)]
    pub reports: Vec<StationReportRecord>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StationReportHistoryPage {
    pub city: Option<CityInfo>,
    pub station: Option<StationInfo>,
    #[serde(default)]
    pub reports: Vec<StationReportRecord>,
    pub count: Option<i64>,
    pub page: Option<CursorPage>,
    pub report_schedules: Option<BTreeMap<String, ReportScheduleEntry>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ForecastRunsPage {
    pub city: Option<CityInfo>,
    pub station: Option<StationInfo>,
    #[serde(default)]
    pub runs: Vec<ForecastRunSummary>,
    pub count: Option<i64>,
    pub page: Option<CursorPage>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ForecastRunData {
    pub city: Option<CityInfo>,
    pub station: Option<StationInfo>,
    pub forecast_run: Option<ForecastRunSummary>,
    #[serde(default)]
    pub hourly: Vec<HourlyForecastRecord>,
    pub count: Option<i64>,
}

fn default_temperature_unit() -> String {
    "F".to_string()
}

fn default_report_schedule_basis() -> String {
    "utc".to_string()
}

fn default_oracle_score_mode() -> String {
    "overall".to_string()
}

fn default_oracle_rank_by() -> String {
    "combined".to_string()
}

fn default_plan_tier() -> String {
    "starter".to_string()
}
