//! Frozen shape of the `supplied` block inside the `SDCTXV5S` context encoding.
//!
//! These structs are the exact positional layout that encoding carried (`supplied-inputs/1`):
//! no independent forecast version or issuance, no apparent Celsius. They exist only to decode
//! and re-hash retained `SDCTXV5S` bytes; the canonical model lives in `strategy_core_kernel`.

#![allow(dead_code)]

use bincode::{Decode, Encode};
use strategy_core_kernel::Decimal as DecimalV5;

pub(crate) const CONTRACT_VERSION: &str = "supplied-inputs/1";

/// Provider event envelope and producer metadata shared by every WebSocket event family.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub(crate) struct EventEnvelopeV5 {
    pub(crate) event_id: String,
    pub(crate) sequence: u64,
    pub(crate) city_sequence: Option<u64>,
    pub(crate) slug: Option<String>,
    pub(crate) emitted_at_unix_ns: i64,
    pub(crate) event_key: Option<String>,
    pub(crate) source_timestamp_unix_ns: Option<i64>,
    pub(crate) wmo_emit_time_unix_ns: Option<i64>,
    pub(crate) producer_received_at_unix_ns: Option<i64>,
    pub(crate) live_published_at_unix_ns: Option<i64>,
    pub(crate) persistence_status: Option<String>,
    pub(crate) producer_sequence: Option<u64>,
    /// Host socket receipt time; Trader evidence rather than provider data.
    pub(crate) received_at_unix_ns: i64,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub(crate) struct SuppliedObservationV5 {
    /// Absent for observations obtained from a REST baseline rather than a stream event.
    pub(crate) envelope: Option<EventEnvelopeV5>,
    pub(crate) source: String,
    pub(crate) station_id: String,
    pub(crate) observed_at_unix_ns: i64,
    pub(crate) lag_seconds: Option<i64>,
    pub(crate) preliminary: bool,
    pub(crate) temperature_c: Option<DecimalV5>,
    pub(crate) temperature_f: Option<DecimalV5>,
    pub(crate) temp_min_c: Option<DecimalV5>,
    pub(crate) temp_max_c: Option<DecimalV5>,
    pub(crate) temp_min_f: Option<DecimalV5>,
    pub(crate) temp_max_f: Option<DecimalV5>,
    pub(crate) is_from_report: bool,
    pub(crate) report_type: Option<String>,
    pub(crate) source_report_id: Option<String>,
    pub(crate) dewpoint: Option<DecimalV5>,
    pub(crate) heat_index: Option<DecimalV5>,
    pub(crate) wind_chill: Option<DecimalV5>,
    pub(crate) relative_humidity: Option<DecimalV5>,
    pub(crate) wind_speed: Option<DecimalV5>,
    pub(crate) wind_direction: Option<DecimalV5>,
    pub(crate) wind_gust: Option<DecimalV5>,
    pub(crate) barometric_pressure: Option<DecimalV5>,
    pub(crate) sea_level_pressure: Option<DecimalV5>,
    pub(crate) precipitation_1h: Option<DecimalV5>,
    pub(crate) precipitation_3h: Option<DecimalV5>,
    pub(crate) precipitation_6h: Option<DecimalV5>,
    pub(crate) text_description: Option<String>,
    pub(crate) is_locf: Option<bool>,
    pub(crate) temperature_day_mode: Option<String>,
    pub(crate) temperature_day_date: Option<String>,
    pub(crate) wu_day_mode: Option<String>,
    pub(crate) wu_day_date: Option<String>,
    pub(crate) wu_current_temp_f: Option<DecimalV5>,
    pub(crate) wu_current_temp_c: Option<DecimalV5>,
    pub(crate) wu_daily_high_f: Option<DecimalV5>,
    pub(crate) wu_daily_low_f: Option<DecimalV5>,
    pub(crate) wu_daily_high_c: Option<DecimalV5>,
    pub(crate) wu_daily_low_c: Option<DecimalV5>,
    pub(crate) wu_observation_time_unix_ns: Option<i64>,
    pub(crate) wu_fetched_at_unix_ns: Option<i64>,
}

/// REST latest-observation context: authoritative merged and ASOS-only daily extremes.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub(crate) struct SuppliedDailyExtremesV5 {
    pub(crate) source: String,
    pub(crate) received_at_unix_ns: i64,
    pub(crate) daily_high_f: Option<DecimalV5>,
    pub(crate) daily_low_f: Option<DecimalV5>,
    pub(crate) daily_high_c: Option<DecimalV5>,
    pub(crate) daily_low_c: Option<DecimalV5>,
    pub(crate) asos_daily_high_f: Option<DecimalV5>,
    pub(crate) asos_daily_low_f: Option<DecimalV5>,
    pub(crate) asos_daily_high_c: Option<DecimalV5>,
    pub(crate) asos_daily_low_c: Option<DecimalV5>,
    pub(crate) temperature_day_mode: Option<String>,
    pub(crate) temperature_day_date: Option<String>,
    pub(crate) wu_day_mode: Option<String>,
    pub(crate) wu_day_date: Option<String>,
    pub(crate) temperature_unit: Option<String>,
    pub(crate) uses_nws_climate_day: Option<bool>,
    pub(crate) wu_current_temp_f: Option<DecimalV5>,
    pub(crate) wu_current_temp_c: Option<DecimalV5>,
    pub(crate) wu_daily_high_f: Option<DecimalV5>,
    pub(crate) wu_daily_low_f: Option<DecimalV5>,
    pub(crate) wu_daily_high_c: Option<DecimalV5>,
    pub(crate) wu_daily_low_c: Option<DecimalV5>,
    pub(crate) wu_observation_time_unix_ns: Option<i64>,
    pub(crate) wu_fetched_at_unix_ns: Option<i64>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub(crate) struct SuppliedReportV5 {
    pub(crate) envelope: Option<EventEnvelopeV5>,
    pub(crate) source: String,
    pub(crate) station_id: String,
    pub(crate) report_id: String,
    pub(crate) report_fingerprint: Option<String>,
    pub(crate) report_revision: Option<u64>,
    pub(crate) report_updated_at_unix_ns: Option<i64>,
    pub(crate) report_type: String,
    pub(crate) report_date: String,
    pub(crate) issuance_time_unix_ns: Option<i64>,
    pub(crate) fetched_at_unix_ns: Option<i64>,
    pub(crate) source_url: Option<String>,
    pub(crate) max_temp_f: Option<DecimalV5>,
    pub(crate) max_temp_c: Option<DecimalV5>,
    pub(crate) max_temp_time_unix_ns: Option<i64>,
    pub(crate) min_temp_f: Option<DecimalV5>,
    pub(crate) min_temp_c: Option<DecimalV5>,
    pub(crate) min_temp_time_unix_ns: Option<i64>,
    pub(crate) temp_f: Option<DecimalV5>,
    pub(crate) temp_c: Option<DecimalV5>,
    pub(crate) provider: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub(crate) enum ExtremeKindV5 {
    #[default]
    High,
    Low,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub(crate) struct SuppliedExtremeV5 {
    pub(crate) envelope: Option<EventEnvelopeV5>,
    pub(crate) source: String,
    pub(crate) kind: ExtremeKindV5,
    pub(crate) station_id: String,
    pub(crate) value_f: Option<DecimalV5>,
    pub(crate) value_c: Option<DecimalV5>,
    pub(crate) prev_value_f: Option<DecimalV5>,
    pub(crate) observed_at_unix_ns: Option<i64>,
    pub(crate) temperature_day_mode: Option<String>,
    pub(crate) temperature_day_date: Option<String>,
    pub(crate) is_from_report: bool,
    pub(crate) report_type: Option<String>,
    pub(crate) source_report_id: Option<String>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub(crate) struct SuppliedWeatherEventSourceV5 {
    pub(crate) metar_type: Option<String>,
    pub(crate) flight_category: Option<String>,
    pub(crate) wx_string: Option<String>,
    pub(crate) wx_token: Option<String>,
    pub(crate) wind_speed_kt: Option<DecimalV5>,
    pub(crate) wind_gust_kt: Option<DecimalV5>,
    pub(crate) peak_wind_kt: Option<DecimalV5>,
    pub(crate) peak_wind_direction: Option<i64>,
    pub(crate) visibility_mi: Option<DecimalV5>,
    pub(crate) cb_location: Option<String>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub(crate) struct SuppliedWeatherEventV5 {
    pub(crate) envelope: Option<EventEnvelopeV5>,
    pub(crate) source: String,
    pub(crate) station_id: String,
    /// The provider's stable episode identifier (`id`), distinct from the envelope `event_id`.
    pub(crate) episode_id: String,
    pub(crate) event_type: String,
    pub(crate) tier: String,
    pub(crate) state: String,
    pub(crate) name: String,
    pub(crate) badge: Option<String>,
    pub(crate) detail: Option<String>,
    pub(crate) summary: Option<String>,
    pub(crate) started_at_unix_ns: Option<i64>,
    pub(crate) last_confirmed_at_unix_ns: Option<i64>,
    pub(crate) ended_at_unix_ns: Option<i64>,
    pub(crate) source_snapshot: Option<SuppliedWeatherEventSourceV5>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub(crate) struct SuppliedForecastPointV5 {
    /// Original RFC3339 text as supplied.
    pub(crate) time: String,
    pub(crate) time_unix_ns: i64,
    pub(crate) temperature_2m_f: Option<DecimalV5>,
    pub(crate) temperature_2m_c: Option<DecimalV5>,
    pub(crate) apparent_temperature_f: Option<DecimalV5>,
    pub(crate) relative_humidity_2m: Option<DecimalV5>,
    pub(crate) dew_point_2m: Option<DecimalV5>,
    pub(crate) pressure_msl: Option<DecimalV5>,
    pub(crate) wind_speed_10m: Option<DecimalV5>,
    pub(crate) wind_direction_10m: Option<DecimalV5>,
    pub(crate) wind_gusts_10m: Option<DecimalV5>,
    pub(crate) cloud_cover: Option<DecimalV5>,
    pub(crate) precipitation_probability: Option<DecimalV5>,
    pub(crate) weather_code: Option<i64>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub(crate) struct SuppliedForecastModelV5 {
    pub(crate) model_id: String,
    pub(crate) run_id: Option<String>,
    /// Original `forecast_run.fetched_at` text, which is also the advertised version.
    pub(crate) fetched_at: Option<String>,
    pub(crate) fetched_at_unix_ns: Option<i64>,
    pub(crate) timezone: Option<String>,
    pub(crate) utc_offset_seconds: Option<i64>,
    pub(crate) hourly: Vec<SuppliedForecastPointV5>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub(crate) struct SuppliedForecastV5 {
    pub(crate) source: String,
    pub(crate) received_at_unix_ns: i64,
    /// Sorted by model id.
    pub(crate) advertised_versions: Vec<(String, String)>,
    /// Sorted by model id.
    pub(crate) models: Vec<SuppliedForecastModelV5>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub(crate) struct SuppliedOracleScoreV5 {
    pub(crate) model_id: String,
    pub(crate) model_name: String,
    pub(crate) is_public: Option<bool>,
    pub(crate) high_mae: Option<DecimalV5>,
    pub(crate) low_mae: Option<DecimalV5>,
    pub(crate) high_bias: Option<DecimalV5>,
    pub(crate) low_bias: Option<DecimalV5>,
    pub(crate) combined_mae: Option<DecimalV5>,
    pub(crate) day_count: Option<i64>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub(crate) struct SuppliedOracleTableV5 {
    pub(crate) source: String,
    pub(crate) received_at_unix_ns: i64,
    pub(crate) station_id: String,
    pub(crate) range_start: String,
    pub(crate) range_end: String,
    pub(crate) days_requested: Option<i64>,
    pub(crate) all_time: Option<bool>,
    pub(crate) score_mode: Option<String>,
    pub(crate) rank_by: Option<String>,
    /// `modes` from the `oracle_scores_updated` notification that delivered this table.
    pub(crate) notification_modes: Vec<String>,
    /// Top-level `updated_at` from that notification.
    pub(crate) notification_updated_at_unix_ns: Option<i64>,
    /// Provider rank order.
    pub(crate) scores: Vec<SuppliedOracleScoreV5>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub(crate) struct SuppliedStationV5 {
    pub(crate) station_id: String,
    pub(crate) observation: Option<SuppliedObservationV5>,
    pub(crate) daily_extremes: Option<SuppliedDailyExtremesV5>,
    /// Current report per type, sorted by report type.
    pub(crate) reports: Vec<SuppliedReportV5>,
    pub(crate) extreme_high: Option<SuppliedExtremeV5>,
    pub(crate) extreme_low: Option<SuppliedExtremeV5>,
    /// Current (not ended) episodes, sorted by episode id.
    pub(crate) weather_events: Vec<SuppliedWeatherEventV5>,
    pub(crate) forecast: Option<SuppliedForecastV5>,
    /// Sorted by `(score_mode, rank_by, days_requested)`.
    pub(crate) oracle_tables: Vec<SuppliedOracleTableV5>,
}

/// The exact typed event that triggered a delivery, as supplied.
// Keep the historical codec shape explicit rather than optimizing this frozen evidence type.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub(crate) enum SuppliedEventV5 {
    Observation(SuppliedObservationV5),
    Report(SuppliedReportV5),
    Extreme(SuppliedExtremeV5),
    WeatherEvent(SuppliedWeatherEventV5),
}

impl SuppliedEventV5 {
    pub(crate) fn station_id(&self) -> &str {
        match self {
            Self::Observation(event) => &event.station_id,
            Self::Report(event) => &event.station_id,
            Self::Extreme(event) => &event.station_id,
            Self::WeatherEvent(event) => &event.station_id,
        }
    }

    pub(crate) fn envelope(&self) -> Option<&EventEnvelopeV5> {
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
pub(crate) struct SuppliedInputsV5 {
    pub(crate) contract_version: String,
    /// Sorted by station id; one per owner station when present.
    pub(crate) stations: Vec<SuppliedStationV5>,
    pub(crate) originating_event: Option<SuppliedEventV5>,
}
