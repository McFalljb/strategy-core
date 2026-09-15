//! Provider inputs at the precision and presence the provider supplied them.
//!
//! These structs are the originals inside the canonical strategy-facing model: every kernel
//! view that presents a convenience `f64` also exposes the supplied original it was projected
//! from. Current Decision Contexts serialize these typed facts directly; historical codecs
//! use frozen wire shapes. Excluded historical WU evidence is not part of these models. Numbers are exact
//! decimal digits, times are nanosecond instants,
//! `Option` is absent-or-null, and strings are present even when empty.
//!
//! Bounds, ordering and validation of a complete supplied block are owned by the codec that
//! carries it (`strategy_core_v3::supplied_v5`).

use bincode::{Decode, Encode};

use crate::decimal::Decimal;

/// Identity of the supplied-inputs shape carried by [`SuppliedInputs::contract_version`].
pub const SUPPLIED_INPUTS_CONTRACT_VERSION: &str = "supplied-inputs/3";

/// Provider event envelope and producer metadata shared by every WebSocket event family.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct EventEnvelope {
    pub event_id: String,
    pub sequence: u64,
    pub city_sequence: Option<u64>,
    pub slug: Option<String>,
    pub emitted_at_unix_ns: i64,
    pub event_key: Option<String>,
    pub source_timestamp_unix_ns: Option<i64>,
    pub wmo_emit_time_unix_ns: Option<i64>,
    pub producer_received_at_unix_ns: Option<i64>,
    pub live_published_at_unix_ns: Option<i64>,
    pub persistence_status: Option<String>,
    pub producer_sequence: Option<u64>,
    /// Host socket receipt time; host evidence rather than provider data.
    pub received_at_unix_ns: i64,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedObservation {
    /// Absent for observations obtained from a REST baseline rather than a stream event.
    pub envelope: Option<EventEnvelope>,
    pub source: String,
    pub station_id: String,
    pub observed_at_unix_ns: i64,
    pub lag_seconds: Option<i64>,
    pub preliminary: bool,
    pub temperature_c: Option<Decimal>,
    pub temperature_f: Option<Decimal>,
    pub temp_min_c: Option<Decimal>,
    pub temp_max_c: Option<Decimal>,
    pub temp_min_f: Option<Decimal>,
    pub temp_max_f: Option<Decimal>,
    pub is_from_report: bool,
    pub report_type: Option<String>,
    pub source_report_id: Option<String>,
    pub dewpoint: Option<Decimal>,
    pub heat_index: Option<Decimal>,
    pub wind_chill: Option<Decimal>,
    pub relative_humidity: Option<Decimal>,
    pub wind_speed: Option<Decimal>,
    pub wind_direction: Option<Decimal>,
    pub wind_gust: Option<Decimal>,
    pub barometric_pressure: Option<Decimal>,
    pub sea_level_pressure: Option<Decimal>,
    pub precipitation_1h: Option<Decimal>,
    pub precipitation_3h: Option<Decimal>,
    pub precipitation_6h: Option<Decimal>,
    pub text_description: Option<String>,
    pub is_locf: Option<bool>,
    pub temperature_day_mode: Option<String>,
    pub temperature_day_date: Option<String>,
}

/// REST latest-observation context: authoritative merged and ASOS-only daily extremes as the
/// provider reported them at `received_at_unix_ns`. Retained as evidence beside the owner's
/// current summaries; never replaces a newer accepted fact.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedDailyExtremes {
    pub source: String,
    pub received_at_unix_ns: i64,
    pub daily_high_f: Option<Decimal>,
    pub daily_low_f: Option<Decimal>,
    pub daily_high_c: Option<Decimal>,
    pub daily_low_c: Option<Decimal>,
    pub asos_daily_high_f: Option<Decimal>,
    pub asos_daily_low_f: Option<Decimal>,
    pub asos_daily_high_c: Option<Decimal>,
    pub asos_daily_low_c: Option<Decimal>,
    pub temperature_day_mode: Option<String>,
    pub temperature_day_date: Option<String>,
    pub temperature_unit: Option<String>,
    pub uses_nws_climate_day: Option<bool>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedReport {
    pub envelope: Option<EventEnvelope>,
    pub source: String,
    pub station_id: String,
    pub report_id: String,
    pub report_fingerprint: Option<String>,
    pub report_revision: Option<u64>,
    pub report_updated_at_unix_ns: Option<i64>,
    pub report_type: String,
    pub report_date: String,
    pub issuance_time_unix_ns: Option<i64>,
    pub fetched_at_unix_ns: Option<i64>,
    pub source_url: Option<String>,
    pub max_temp_f: Option<Decimal>,
    pub max_temp_c: Option<Decimal>,
    pub max_temp_time_unix_ns: Option<i64>,
    pub min_temp_f: Option<Decimal>,
    pub min_temp_c: Option<Decimal>,
    pub min_temp_time_unix_ns: Option<i64>,
    pub temp_f: Option<Decimal>,
    pub temp_c: Option<Decimal>,
    pub provider: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub enum ExtremeKind {
    #[default]
    High,
    Low,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedExtreme {
    pub envelope: Option<EventEnvelope>,
    pub source: String,
    pub kind: ExtremeKind,
    pub station_id: String,
    pub value_f: Option<Decimal>,
    pub value_c: Option<Decimal>,
    pub prev_value_f: Option<Decimal>,
    pub observed_at_unix_ns: Option<i64>,
    pub temperature_day_mode: Option<String>,
    pub temperature_day_date: Option<String>,
    pub is_from_report: bool,
    pub report_type: Option<String>,
    pub source_report_id: Option<String>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedWeatherEventSource {
    pub metar_type: Option<String>,
    pub flight_category: Option<String>,
    pub wx_string: Option<String>,
    pub wx_token: Option<String>,
    pub wind_speed_kt: Option<Decimal>,
    pub wind_gust_kt: Option<Decimal>,
    pub peak_wind_kt: Option<Decimal>,
    pub peak_wind_direction: Option<i64>,
    pub visibility_mi: Option<Decimal>,
    pub cb_location: Option<String>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedWeatherEvent {
    pub envelope: Option<EventEnvelope>,
    pub source: String,
    pub station_id: String,
    /// The provider's stable episode identifier (`id`), distinct from the envelope `event_id`.
    pub episode_id: String,
    pub event_type: String,
    pub tier: String,
    pub state: String,
    pub name: String,
    pub badge: Option<String>,
    pub detail: Option<String>,
    pub summary: Option<String>,
    pub started_at_unix_ns: Option<i64>,
    pub last_confirmed_at_unix_ns: Option<i64>,
    pub ended_at_unix_ns: Option<i64>,
    pub source_snapshot: Option<SuppliedWeatherEventSource>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedForecastPoint {
    /// Original RFC3339 text as supplied.
    pub time: String,
    pub time_unix_ns: i64,
    pub temperature_2m_f: Option<Decimal>,
    pub temperature_2m_c: Option<Decimal>,
    pub apparent_temperature_f: Option<Decimal>,
    pub apparent_temperature_c: Option<Decimal>,
    pub relative_humidity_2m: Option<Decimal>,
    pub dew_point_2m: Option<Decimal>,
    pub pressure_msl: Option<Decimal>,
    pub wind_speed_10m: Option<Decimal>,
    pub wind_direction_10m: Option<Decimal>,
    pub wind_gusts_10m: Option<Decimal>,
    pub cloud_cover: Option<Decimal>,
    pub precipitation_probability: Option<Decimal>,
    pub weather_code: Option<i64>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedForecastModel {
    pub model_id: String,
    pub run_id: Option<String>,
    /// The version the provider advertised for this model, as supplied. Independent of the
    /// fetch time: a legacy-shaped bundle carries `version`, a run-shaped bundle advertises the
    /// run's `fetched_at` text.
    pub version: Option<String>,
    /// Original `fetched_at` text as supplied.
    pub fetched_at: Option<String>,
    pub fetched_at_unix_ns: Option<i64>,
    /// Original issuance text as supplied, when the bundle carries one.
    pub issued_at: Option<String>,
    pub issued_at_unix_ns: Option<i64>,
    pub timezone: Option<String>,
    pub utc_offset_seconds: Option<i64>,
    pub hourly: Vec<SuppliedForecastPoint>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedForecast {
    pub source: String,
    pub received_at_unix_ns: i64,
    /// Sorted by model id.
    pub advertised_versions: Vec<(String, String)>,
    /// Sorted by model id.
    pub models: Vec<SuppliedForecastModel>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedOracleScore {
    /// Explicit provider rank; absence is distinct from the row's position.
    pub rank: Option<u64>,
    pub model_id: String,
    pub model_name: String,
    pub is_public: Option<bool>,
    pub high_mae: Option<Decimal>,
    pub low_mae: Option<Decimal>,
    pub high_bias: Option<Decimal>,
    pub low_bias: Option<Decimal>,
    pub combined_mae: Option<Decimal>,
    pub day_count: Option<i64>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedOracleTable {
    /// The table's own update instant, distinct from host receipt and notification time.
    pub updated_at_unix_ns: Option<i64>,
    pub source: String,
    pub received_at_unix_ns: i64,
    pub station_id: String,
    pub range_start: String,
    pub range_end: String,
    pub days_requested: Option<i64>,
    pub all_time: Option<bool>,
    pub score_mode: Option<String>,
    pub rank_by: Option<String>,
    /// `modes` from the `oracle_scores_updated` notification that delivered this table.
    pub notification_modes: Vec<String>,
    /// Top-level `updated_at` from that notification.
    pub notification_updated_at_unix_ns: Option<i64>,
    /// Provider rank order.
    pub scores: Vec<SuppliedOracleScore>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedStation {
    pub station_id: String,
    pub observation: Option<SuppliedObservation>,
    pub daily_extremes: Option<SuppliedDailyExtremes>,
    /// Current report per type, sorted by report type.
    pub reports: Vec<SuppliedReport>,
    pub extreme_high: Option<SuppliedExtreme>,
    pub extreme_low: Option<SuppliedExtreme>,
    /// Current (not ended) episodes, sorted by episode id.
    pub weather_events: Vec<SuppliedWeatherEvent>,
    pub forecast: Option<SuppliedForecast>,
    /// Sorted by `(score_mode, rank_by, days_requested)`.
    pub oracle_tables: Vec<SuppliedOracleTable>,
}

/// The exact typed event that triggered a delivery, as supplied.
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum SuppliedEvent {
    Observation(SuppliedObservation),
    Report(SuppliedReport),
    Extreme(SuppliedExtreme),
    WeatherEvent(SuppliedWeatherEvent),
}

impl SuppliedEvent {
    pub fn station_id(&self) -> &str {
        match self {
            Self::Observation(event) => &event.station_id,
            Self::Report(event) => &event.station_id,
            Self::Extreme(event) => &event.station_id,
            Self::WeatherEvent(event) => &event.station_id,
        }
    }

    pub fn envelope(&self) -> Option<&EventEnvelope> {
        match self {
            Self::Observation(event) => event.envelope.as_ref(),
            Self::Report(event) => event.envelope.as_ref(),
            Self::Extreme(event) => event.envelope.as_ref(),
            Self::WeatherEvent(event) => event.envelope.as_ref(),
        }
    }
}

/// Complete supplied inputs for one delivery.
///
/// An absent block (`contract_version` empty, no stations, no event) means the host did not
/// supply original-precision inputs; contexts decoded from pre-supplied encodings and fixture
/// contexts take this form. When present, every owner station has exactly one supplied station.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedInputs {
    pub contract_version: String,
    /// Sorted by station id; one per owner station when present.
    pub stations: Vec<SuppliedStation>,
    pub originating_event: Option<SuppliedEvent>,
}

impl SuppliedInputs {
    pub fn is_absent(&self) -> bool {
        self.contract_version.is_empty()
            && self.stations.is_empty()
            && self.originating_event.is_none()
    }

    pub fn station(&self, station_id: &str) -> Option<&SuppliedStation> {
        self.stations
            .iter()
            .find(|station| station.station_id == station_id)
    }
}
