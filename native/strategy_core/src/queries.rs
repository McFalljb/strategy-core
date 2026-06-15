use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DateLike {
    DateTime(DateTime<Utc>),
    String(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LocalDateLike {
    Date(NaiveDate),
    String(String),
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct LimitsQuery {
    #[serde(default)]
    pub refresh: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ForecastQuery {
    pub model_id: Option<String>,
    #[serde(default)]
    pub refresh: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OracleScoresQuery {
    #[serde(default = "default_oracle_days")]
    pub days: String,
    #[serde(default = "default_oracle_mode")]
    pub mode: String,
    #[serde(default = "default_oracle_rank_by")]
    pub rank_by: String,
    #[serde(default)]
    pub refresh: bool,
}

impl Default for OracleScoresQuery {
    fn default() -> Self {
        Self {
            days: default_oracle_days(),
            mode: default_oracle_mode(),
            rank_by: default_oracle_rank_by(),
            refresh: false,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ForecastRunsQuery {
    pub model_id: Option<String>,
    pub start: Option<DateLike>,
    pub end: Option<DateLike>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
    #[serde(default)]
    pub refresh: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ForecastRunQuery {
    pub run_id: String,
    #[serde(default)]
    pub refresh: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct LatestReportsQuery {
    #[serde(default)]
    pub refresh: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReportsQuery {
    pub report_type: Option<String>,
    pub date: Option<LocalDateLike>,
    #[serde(default)]
    pub refresh: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReportHistoryQuery {
    pub report_type: Option<String>,
    pub start: Option<LocalDateLike>,
    pub end: Option<LocalDateLike>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
    #[serde(default)]
    pub refresh: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct LatestObservationQuery {
    pub day_mode: Option<String>,
    #[serde(default)]
    pub refresh: bool,
}

fn default_oracle_days() -> String {
    "7".to_string()
}

fn default_oracle_mode() -> String {
    "day_ahead".to_string()
}

fn default_oracle_rank_by() -> String {
    "high".to_string()
}
