//! Supplied decision inputs: provider fields at their original precision.
//!
//! The V4 owner projection embedded in a V5 context is a bounded derived projection with
//! milli/micro fixed-point numbers and millisecond times. This module carries the values the
//! provider actually supplied, at the precision it supplied them, together with the exact typed
//! event that triggered a delivery. Derived and supplied values are kept explicitly separate: a
//! kernel view field is projected from the supplied value whenever one exists and only falls
//! back to the derived projection when the host supplied nothing.
//!
//! See `traderv3/docs/spec/decision-data-field-matrix.md` for the field inventory and the
//! precision, presence and time definitions this module implements.

use std::collections::BTreeSet;

use bincode::{Decode, Encode};

use crate::decision_v5::DecisionV5Error;

pub const SUPPLIED_INPUTS_CONTRACT_VERSION: &str = "supplied-inputs/1";
pub const MAX_SUPPLIED_TEXT_BYTES: usize = 2048;
pub const MAX_SUPPLIED_DECIMAL_SCALE: u8 = 18;
pub const MAX_SUPPLIED_REPORTS: usize = 8;
pub const MAX_SUPPLIED_WEATHER_EVENTS: usize = 32;
pub const MAX_SUPPLIED_FORECAST_MODELS: usize = 32;
pub const MAX_SUPPLIED_FORECAST_POINTS: usize = 256;
pub const MAX_SUPPLIED_ORACLE_TABLES: usize = 4;
pub const MAX_SUPPLIED_ORACLE_ROWS: usize = 32;
pub const MAX_SUPPLIED_ADVERTISED_VERSIONS: usize = 32;
pub const MAX_SUPPLIED_NOTIFICATION_MODES: usize = 3;

/// Exact decimal digits of a provider JSON number.
///
/// `coefficient * 10^-scale`. The canonical form has no trailing fractional zeros, a scale of
/// at most [`MAX_SUPPLIED_DECIMAL_SCALE`], and zero is always `{0, 0}`.
#[derive(Clone, Copy, Debug, Default, Encode, Decode, Eq, PartialEq, Ord, PartialOrd)]
pub struct DecimalV5 {
    pub coefficient: i64,
    pub scale: u8,
}

impl DecimalV5 {
    /// Parses a JSON number's textual representation exactly.
    ///
    /// Accepts an optional sign, digits, an optional fraction and an optional exponent. The
    /// result is normalized. Values needing more than 64 signed bits of coefficient or more
    /// than the scale bound are rejected rather than rounded.
    pub fn parse(text: &str) -> Result<Self, DecisionV5Error> {
        let (mantissa, exponent) = match text.split_once(['e', 'E']) {
            Some((mantissa, exponent)) => (
                mantissa,
                exponent
                    .parse::<i32>()
                    .map_err(|_| DecisionV5Error::InvalidContract)?,
            ),
            None => (text, 0),
        };
        let negative = mantissa.starts_with('-');
        let unsigned = mantissa.strip_prefix(['-', '+']).unwrap_or(mantissa);
        let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
        if whole.is_empty()
            || (unsigned.contains('.') && fraction.is_empty())
            || !whole.bytes().all(|byte| byte.is_ascii_digit())
            || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(DecisionV5Error::InvalidContract);
        }
        let digits = format!("{whole}{fraction}");
        let mut coefficient = digits
            .parse::<i128>()
            .map_err(|_| DecisionV5Error::InvalidContract)?;
        if negative {
            coefficient = -coefficient;
        }
        let mut scale = i32::try_from(fraction.len())
            .map_err(|_| DecisionV5Error::InvalidContract)?
            .checked_sub(exponent)
            .ok_or(DecisionV5Error::InvalidContract)?;
        if scale < -38 {
            return Err(DecisionV5Error::InvalidContract);
        }
        while scale < 0 {
            coefficient = coefficient
                .checked_mul(10)
                .ok_or(DecisionV5Error::InvalidContract)?;
            scale += 1;
        }
        while scale > 0 && coefficient % 10 == 0 {
            coefficient /= 10;
            scale -= 1;
        }
        if coefficient == 0 {
            scale = 0;
        }
        let value = Self {
            coefficient: i64::try_from(coefficient)
                .map_err(|_| DecisionV5Error::InvalidContract)?,
            scale: u8::try_from(scale).map_err(|_| DecisionV5Error::InvalidContract)?,
        };
        if !value.is_canonical() {
            return Err(DecisionV5Error::InvalidContract);
        }
        Ok(value)
    }

    pub fn from_i64(value: i64) -> Self {
        Self {
            coefficient: value,
            scale: 0,
        }
        .normalized()
    }

    fn normalized(self) -> Self {
        let mut coefficient = self.coefficient;
        let mut scale = self.scale;
        while scale > 0 && coefficient % 10 == 0 {
            coefficient /= 10;
            scale -= 1;
        }
        if coefficient == 0 {
            scale = 0;
        }
        Self { coefficient, scale }
    }

    pub fn is_canonical(self) -> bool {
        self.scale <= MAX_SUPPLIED_DECIMAL_SCALE
            && (self.scale == 0 || self.coefficient % 10 != 0)
            && (self.coefficient != 0 || self.scale == 0)
    }

    /// The nearest IEEE-754 double, which is the provider's original double for values captured
    /// from a shortest round-trip representation.
    pub fn to_f64(self) -> f64 {
        format!("{}e-{}", self.coefficient, self.scale)
            .parse::<f64>()
            .unwrap_or(f64::NAN)
    }
}

impl core::fmt::Display for DecimalV5 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.scale == 0 {
            return write!(f, "{}", self.coefficient);
        }
        let negative = self.coefficient < 0;
        let digits = self.coefficient.unsigned_abs().to_string();
        let scale = usize::from(self.scale);
        let (whole, fraction) = if digits.len() > scale {
            let (whole, fraction) = digits.split_at(digits.len() - scale);
            (whole.to_owned(), fraction.to_owned())
        } else {
            ("0".to_owned(), format!("{:0>scale$}", digits))
        };
        write!(f, "{}{whole}.{fraction}", if negative { "-" } else { "" })
    }
}

/// Provider event envelope and producer metadata shared by every WebSocket event family.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct EventEnvelopeV5 {
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
    /// Host socket receipt time; Trader evidence rather than provider data.
    pub received_at_unix_ns: i64,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedObservationV5 {
    /// Absent for observations obtained from a REST baseline rather than a stream event.
    pub envelope: Option<EventEnvelopeV5>,
    pub source: String,
    pub station_id: String,
    pub observed_at_unix_ns: i64,
    pub lag_seconds: Option<i64>,
    pub preliminary: bool,
    pub temperature_c: Option<DecimalV5>,
    pub temperature_f: Option<DecimalV5>,
    pub temp_min_c: Option<DecimalV5>,
    pub temp_max_c: Option<DecimalV5>,
    pub temp_min_f: Option<DecimalV5>,
    pub temp_max_f: Option<DecimalV5>,
    pub is_from_report: bool,
    pub report_type: Option<String>,
    pub source_report_id: Option<String>,
    pub dewpoint: Option<DecimalV5>,
    pub heat_index: Option<DecimalV5>,
    pub wind_chill: Option<DecimalV5>,
    pub relative_humidity: Option<DecimalV5>,
    pub wind_speed: Option<DecimalV5>,
    pub wind_direction: Option<DecimalV5>,
    pub wind_gust: Option<DecimalV5>,
    pub barometric_pressure: Option<DecimalV5>,
    pub sea_level_pressure: Option<DecimalV5>,
    pub precipitation_1h: Option<DecimalV5>,
    pub precipitation_3h: Option<DecimalV5>,
    pub precipitation_6h: Option<DecimalV5>,
    pub text_description: Option<String>,
    pub is_locf: Option<bool>,
    pub temperature_day_mode: Option<String>,
    pub temperature_day_date: Option<String>,
    pub wu_day_mode: Option<String>,
    pub wu_day_date: Option<String>,
    pub wu_current_temp_f: Option<DecimalV5>,
    pub wu_current_temp_c: Option<DecimalV5>,
    pub wu_daily_high_f: Option<DecimalV5>,
    pub wu_daily_low_f: Option<DecimalV5>,
    pub wu_daily_high_c: Option<DecimalV5>,
    pub wu_daily_low_c: Option<DecimalV5>,
    pub wu_observation_time_unix_ns: Option<i64>,
    pub wu_fetched_at_unix_ns: Option<i64>,
}

/// REST latest-observation context: authoritative merged and ASOS-only daily extremes.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedDailyExtremesV5 {
    pub source: String,
    pub received_at_unix_ns: i64,
    pub daily_high_f: Option<DecimalV5>,
    pub daily_low_f: Option<DecimalV5>,
    pub daily_high_c: Option<DecimalV5>,
    pub daily_low_c: Option<DecimalV5>,
    pub asos_daily_high_f: Option<DecimalV5>,
    pub asos_daily_low_f: Option<DecimalV5>,
    pub asos_daily_high_c: Option<DecimalV5>,
    pub asos_daily_low_c: Option<DecimalV5>,
    pub temperature_day_mode: Option<String>,
    pub temperature_day_date: Option<String>,
    pub wu_day_mode: Option<String>,
    pub wu_day_date: Option<String>,
    pub temperature_unit: Option<String>,
    pub uses_nws_climate_day: Option<bool>,
    pub wu_current_temp_f: Option<DecimalV5>,
    pub wu_current_temp_c: Option<DecimalV5>,
    pub wu_daily_high_f: Option<DecimalV5>,
    pub wu_daily_low_f: Option<DecimalV5>,
    pub wu_daily_high_c: Option<DecimalV5>,
    pub wu_daily_low_c: Option<DecimalV5>,
    pub wu_observation_time_unix_ns: Option<i64>,
    pub wu_fetched_at_unix_ns: Option<i64>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedReportV5 {
    pub envelope: Option<EventEnvelopeV5>,
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
    pub max_temp_f: Option<DecimalV5>,
    pub max_temp_c: Option<DecimalV5>,
    pub max_temp_time_unix_ns: Option<i64>,
    pub min_temp_f: Option<DecimalV5>,
    pub min_temp_c: Option<DecimalV5>,
    pub min_temp_time_unix_ns: Option<i64>,
    pub temp_f: Option<DecimalV5>,
    pub temp_c: Option<DecimalV5>,
    pub provider: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub enum ExtremeKindV5 {
    #[default]
    High,
    Low,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedExtremeV5 {
    pub envelope: Option<EventEnvelopeV5>,
    pub source: String,
    pub kind: ExtremeKindV5,
    pub station_id: String,
    pub value_f: Option<DecimalV5>,
    pub value_c: Option<DecimalV5>,
    pub prev_value_f: Option<DecimalV5>,
    pub observed_at_unix_ns: Option<i64>,
    pub temperature_day_mode: Option<String>,
    pub temperature_day_date: Option<String>,
    pub is_from_report: bool,
    pub report_type: Option<String>,
    pub source_report_id: Option<String>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedWeatherEventSourceV5 {
    pub metar_type: Option<String>,
    pub flight_category: Option<String>,
    pub wx_string: Option<String>,
    pub wx_token: Option<String>,
    pub wind_speed_kt: Option<DecimalV5>,
    pub wind_gust_kt: Option<DecimalV5>,
    pub peak_wind_kt: Option<DecimalV5>,
    pub peak_wind_direction: Option<i64>,
    pub visibility_mi: Option<DecimalV5>,
    pub cb_location: Option<String>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedWeatherEventV5 {
    pub envelope: Option<EventEnvelopeV5>,
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
    pub source_snapshot: Option<SuppliedWeatherEventSourceV5>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedForecastPointV5 {
    /// Original RFC3339 text as supplied.
    pub time: String,
    pub time_unix_ns: i64,
    pub temperature_2m_f: Option<DecimalV5>,
    pub temperature_2m_c: Option<DecimalV5>,
    pub apparent_temperature_f: Option<DecimalV5>,
    pub relative_humidity_2m: Option<DecimalV5>,
    pub dew_point_2m: Option<DecimalV5>,
    pub pressure_msl: Option<DecimalV5>,
    pub wind_speed_10m: Option<DecimalV5>,
    pub wind_direction_10m: Option<DecimalV5>,
    pub wind_gusts_10m: Option<DecimalV5>,
    pub cloud_cover: Option<DecimalV5>,
    pub precipitation_probability: Option<DecimalV5>,
    pub weather_code: Option<i64>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedForecastModelV5 {
    pub model_id: String,
    pub run_id: Option<String>,
    /// Original `forecast_run.fetched_at` text, which is also the advertised version.
    pub fetched_at: Option<String>,
    pub fetched_at_unix_ns: Option<i64>,
    pub timezone: Option<String>,
    pub utc_offset_seconds: Option<i64>,
    pub hourly: Vec<SuppliedForecastPointV5>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedForecastV5 {
    pub source: String,
    pub received_at_unix_ns: i64,
    /// Sorted by model id.
    pub advertised_versions: Vec<(String, String)>,
    /// Sorted by model id.
    pub models: Vec<SuppliedForecastModelV5>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedOracleScoreV5 {
    pub model_id: String,
    pub model_name: String,
    pub is_public: Option<bool>,
    pub high_mae: Option<DecimalV5>,
    pub low_mae: Option<DecimalV5>,
    pub high_bias: Option<DecimalV5>,
    pub low_bias: Option<DecimalV5>,
    pub combined_mae: Option<DecimalV5>,
    pub day_count: Option<i64>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedOracleTableV5 {
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
    pub scores: Vec<SuppliedOracleScoreV5>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedStationV5 {
    pub station_id: String,
    pub observation: Option<SuppliedObservationV5>,
    pub daily_extremes: Option<SuppliedDailyExtremesV5>,
    /// Current report per type, sorted by report type.
    pub reports: Vec<SuppliedReportV5>,
    pub extreme_high: Option<SuppliedExtremeV5>,
    pub extreme_low: Option<SuppliedExtremeV5>,
    /// Current (not ended) episodes, sorted by episode id.
    pub weather_events: Vec<SuppliedWeatherEventV5>,
    pub forecast: Option<SuppliedForecastV5>,
    /// Sorted by `(score_mode, rank_by, days_requested)`.
    pub oracle_tables: Vec<SuppliedOracleTableV5>,
}

/// The exact typed event that triggered a delivery, as supplied.
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum SuppliedEventV5 {
    Observation(SuppliedObservationV5),
    Report(SuppliedReportV5),
    Extreme(SuppliedExtremeV5),
    WeatherEvent(SuppliedWeatherEventV5),
}

impl SuppliedEventV5 {
    pub fn station_id(&self) -> &str {
        match self {
            Self::Observation(event) => &event.station_id,
            Self::Report(event) => &event.station_id,
            Self::Extreme(event) => &event.station_id,
            Self::WeatherEvent(event) => &event.station_id,
        }
    }

    pub fn envelope(&self) -> Option<&EventEnvelopeV5> {
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
pub struct SuppliedInputsV5 {
    pub contract_version: String,
    /// Sorted by station id; one per owner station when present.
    pub stations: Vec<SuppliedStationV5>,
    pub originating_event: Option<SuppliedEventV5>,
}

impl SuppliedInputsV5 {
    pub fn is_absent(&self) -> bool {
        self.contract_version.is_empty()
            && self.stations.is_empty()
            && self.originating_event.is_none()
    }

    pub fn station(&self, station_id: &str) -> Option<&SuppliedStationV5> {
        self.stations
            .iter()
            .find(|station| station.station_id == station_id)
    }

    /// Structural validation independent of the owner projection.
    pub fn validate(&self) -> Result<(), DecisionV5Error> {
        if self.is_absent() {
            return Ok(());
        }
        if self.contract_version != SUPPLIED_INPUTS_CONTRACT_VERSION {
            return Err(DecisionV5Error::InvalidContract);
        }
        if self.stations.len() > crate::decision_v4::MAX_STATIONS {
            return Err(DecisionV5Error::BoundExceeded);
        }
        strictly_sorted(
            self.stations
                .iter()
                .map(|station| station.station_id.as_str()),
        )?;
        for station in &self.stations {
            validate_station(station)?;
        }
        if let Some(event) = &self.originating_event {
            validate_event(event)?;
        }
        Ok(())
    }
}

fn validate_station(station: &SuppliedStationV5) -> Result<(), DecisionV5Error> {
    identifier(&station.station_id)?;
    if station.reports.len() > MAX_SUPPLIED_REPORTS
        || station.weather_events.len() > MAX_SUPPLIED_WEATHER_EVENTS
        || station.oracle_tables.len() > MAX_SUPPLIED_ORACLE_TABLES
    {
        return Err(DecisionV5Error::BoundExceeded);
    }
    if let Some(observation) = &station.observation {
        validate_observation(observation)?;
        if observation.station_id != station.station_id {
            return Err(DecisionV5Error::InvalidContract);
        }
    }
    if let Some(daily) = &station.daily_extremes {
        validate_daily_extremes(daily)?;
    }
    strictly_sorted(
        station
            .reports
            .iter()
            .map(|report| report.report_type.as_str()),
    )?;
    for report in &station.reports {
        validate_report(report)?;
        if report.station_id != station.station_id {
            return Err(DecisionV5Error::InvalidContract);
        }
    }
    for (extreme, kind) in [
        (station.extreme_high.as_ref(), ExtremeKindV5::High),
        (station.extreme_low.as_ref(), ExtremeKindV5::Low),
    ] {
        if let Some(extreme) = extreme {
            validate_extreme(extreme)?;
            if extreme.kind != kind || extreme.station_id != station.station_id {
                return Err(DecisionV5Error::InvalidContract);
            }
        }
    }
    strictly_sorted(
        station
            .weather_events
            .iter()
            .map(|event| event.episode_id.as_str()),
    )?;
    for event in &station.weather_events {
        validate_weather_event(event)?;
        if event.station_id != station.station_id || event.state == "ended" {
            return Err(DecisionV5Error::InvalidContract);
        }
    }
    if let Some(forecast) = &station.forecast {
        validate_forecast(forecast)?;
    }
    strictly_sorted(station.oracle_tables.iter().map(|table| {
        (
            table.score_mode.as_deref().unwrap_or_default(),
            table.rank_by.as_deref().unwrap_or_default(),
            table.days_requested,
        )
    }))?;
    for table in &station.oracle_tables {
        validate_oracle_table(table)?;
    }
    Ok(())
}

pub(crate) fn validate_event(event: &SuppliedEventV5) -> Result<(), DecisionV5Error> {
    match event {
        SuppliedEventV5::Observation(event) => validate_observation(event),
        SuppliedEventV5::Report(event) => validate_report(event),
        SuppliedEventV5::Extreme(event) => validate_extreme(event),
        SuppliedEventV5::WeatherEvent(event) => validate_weather_event(event),
    }
}

fn validate_envelope(envelope: &EventEnvelopeV5) -> Result<(), DecisionV5Error> {
    identifier(&envelope.event_id)?;
    if envelope.sequence == 0 || envelope.city_sequence == Some(0) {
        return Err(DecisionV5Error::InvalidContract);
    }
    optional_text(&envelope.slug)?;
    optional_text(&envelope.event_key)?;
    optional_text(&envelope.persistence_status)?;
    Ok(())
}

fn validate_observation(observation: &SuppliedObservationV5) -> Result<(), DecisionV5Error> {
    if let Some(envelope) = &observation.envelope {
        validate_envelope(envelope)?;
    }
    text(&observation.source)?;
    identifier(&observation.station_id)?;
    for value in [
        &observation.report_type,
        &observation.source_report_id,
        &observation.text_description,
        &observation.temperature_day_mode,
        &observation.temperature_day_date,
        &observation.wu_day_mode,
        &observation.wu_day_date,
    ] {
        optional_text(value)?;
    }
    if !matches!(
        (
            observation.is_from_report,
            observation.report_type.as_deref(),
            observation.source_report_id.as_deref(),
        ),
        (true, Some(_), Some(_)) | (false, None, None)
    ) {
        return Err(DecisionV5Error::InvalidContract);
    }
    for value in [
        observation.temperature_c,
        observation.temperature_f,
        observation.temp_min_c,
        observation.temp_max_c,
        observation.temp_min_f,
        observation.temp_max_f,
        observation.dewpoint,
        observation.heat_index,
        observation.wind_chill,
        observation.relative_humidity,
        observation.wind_speed,
        observation.wind_direction,
        observation.wind_gust,
        observation.barometric_pressure,
        observation.sea_level_pressure,
        observation.precipitation_1h,
        observation.precipitation_3h,
        observation.precipitation_6h,
        observation.wu_current_temp_f,
        observation.wu_current_temp_c,
        observation.wu_daily_high_f,
        observation.wu_daily_low_f,
        observation.wu_daily_high_c,
        observation.wu_daily_low_c,
    ] {
        optional_decimal(value)?;
    }
    Ok(())
}

fn validate_daily_extremes(daily: &SuppliedDailyExtremesV5) -> Result<(), DecisionV5Error> {
    text(&daily.source)?;
    for value in [
        &daily.temperature_day_mode,
        &daily.temperature_day_date,
        &daily.wu_day_mode,
        &daily.wu_day_date,
        &daily.temperature_unit,
    ] {
        optional_text(value)?;
    }
    for value in [
        daily.daily_high_f,
        daily.daily_low_f,
        daily.daily_high_c,
        daily.daily_low_c,
        daily.asos_daily_high_f,
        daily.asos_daily_low_f,
        daily.asos_daily_high_c,
        daily.asos_daily_low_c,
        daily.wu_current_temp_f,
        daily.wu_current_temp_c,
        daily.wu_daily_high_f,
        daily.wu_daily_low_f,
        daily.wu_daily_high_c,
        daily.wu_daily_low_c,
    ] {
        optional_decimal(value)?;
    }
    Ok(())
}

fn validate_report(report: &SuppliedReportV5) -> Result<(), DecisionV5Error> {
    if let Some(envelope) = &report.envelope {
        validate_envelope(envelope)?;
    }
    text(&report.source)?;
    identifier(&report.station_id)?;
    text(&report.report_id)?;
    text(&report.report_type)?;
    text(&report.report_date)?;
    optional_text(&report.report_fingerprint)?;
    optional_text(&report.source_url)?;
    optional_text(&report.provider)?;
    for value in [
        report.max_temp_f,
        report.max_temp_c,
        report.min_temp_f,
        report.min_temp_c,
        report.temp_f,
        report.temp_c,
    ] {
        optional_decimal(value)?;
    }
    Ok(())
}

fn validate_extreme(extreme: &SuppliedExtremeV5) -> Result<(), DecisionV5Error> {
    if let Some(envelope) = &extreme.envelope {
        validate_envelope(envelope)?;
    }
    text(&extreme.source)?;
    identifier(&extreme.station_id)?;
    optional_text(&extreme.temperature_day_mode)?;
    optional_text(&extreme.temperature_day_date)?;
    optional_text(&extreme.report_type)?;
    optional_text(&extreme.source_report_id)?;
    if !matches!(
        (
            extreme.is_from_report,
            extreme.report_type.as_deref(),
            extreme.source_report_id.as_deref(),
        ),
        (true, Some(_), Some(_)) | (false, None, None)
    ) || (extreme.value_f.is_none() && extreme.value_c.is_none())
    {
        return Err(DecisionV5Error::InvalidContract);
    }
    for value in [extreme.value_f, extreme.value_c, extreme.prev_value_f] {
        optional_decimal(value)?;
    }
    Ok(())
}

fn validate_weather_event(event: &SuppliedWeatherEventV5) -> Result<(), DecisionV5Error> {
    if let Some(envelope) = &event.envelope {
        validate_envelope(envelope)?;
    }
    text(&event.source)?;
    identifier(&event.station_id)?;
    text(&event.episode_id)?;
    text(&event.event_type)?;
    text(&event.tier)?;
    text(&event.state)?;
    text(&event.name)?;
    optional_text(&event.badge)?;
    optional_text(&event.detail)?;
    optional_text(&event.summary)?;
    if (event.state == "ended") != event.ended_at_unix_ns.is_some() {
        return Err(DecisionV5Error::InvalidContract);
    }
    if let Some(source) = &event.source_snapshot {
        for value in [
            &source.metar_type,
            &source.flight_category,
            &source.wx_string,
            &source.wx_token,
            &source.cb_location,
        ] {
            optional_text(value)?;
        }
        for value in [
            source.wind_speed_kt,
            source.wind_gust_kt,
            source.peak_wind_kt,
            source.visibility_mi,
        ] {
            optional_decimal(value)?;
        }
    }
    Ok(())
}

fn validate_forecast(forecast: &SuppliedForecastV5) -> Result<(), DecisionV5Error> {
    text(&forecast.source)?;
    if forecast.models.len() > MAX_SUPPLIED_FORECAST_MODELS
        || forecast.advertised_versions.len() > MAX_SUPPLIED_ADVERTISED_VERSIONS
    {
        return Err(DecisionV5Error::BoundExceeded);
    }
    strictly_sorted(
        forecast
            .advertised_versions
            .iter()
            .map(|(model, _)| model.as_str()),
    )?;
    for (model, version) in &forecast.advertised_versions {
        text(model)?;
        text(version)?;
    }
    strictly_sorted(forecast.models.iter().map(|model| model.model_id.as_str()))?;
    for model in &forecast.models {
        text(&model.model_id)?;
        optional_text(&model.run_id)?;
        optional_text(&model.fetched_at)?;
        optional_text(&model.timezone)?;
        if model.hourly.is_empty() || model.hourly.len() > MAX_SUPPLIED_FORECAST_POINTS {
            return Err(DecisionV5Error::BoundExceeded);
        }
        if model
            .hourly
            .windows(2)
            .any(|points| points[0].time_unix_ns >= points[1].time_unix_ns)
        {
            return Err(DecisionV5Error::NonCanonicalOrder);
        }
        for point in &model.hourly {
            text(&point.time)?;
            for value in [
                point.temperature_2m_f,
                point.temperature_2m_c,
                point.apparent_temperature_f,
                point.relative_humidity_2m,
                point.dew_point_2m,
                point.pressure_msl,
                point.wind_speed_10m,
                point.wind_direction_10m,
                point.wind_gusts_10m,
                point.cloud_cover,
                point.precipitation_probability,
            ] {
                optional_decimal(value)?;
            }
        }
    }
    Ok(())
}

fn validate_oracle_table(table: &SuppliedOracleTableV5) -> Result<(), DecisionV5Error> {
    text(&table.source)?;
    identifier(&table.station_id)?;
    text(&table.range_start)?;
    text(&table.range_end)?;
    optional_text(&table.score_mode)?;
    optional_text(&table.rank_by)?;
    if table.scores.len() > MAX_SUPPLIED_ORACLE_ROWS
        || table.notification_modes.len() > MAX_SUPPLIED_NOTIFICATION_MODES
    {
        return Err(DecisionV5Error::BoundExceeded);
    }
    for mode in &table.notification_modes {
        text(mode)?;
    }
    let mut seen = BTreeSet::new();
    for score in &table.scores {
        text(&score.model_id)?;
        text(&score.model_name)?;
        if !seen.insert(score.model_id.as_str()) {
            return Err(DecisionV5Error::DuplicateIdentity);
        }
        for value in [
            score.high_mae,
            score.low_mae,
            score.high_bias,
            score.low_bias,
            score.combined_mae,
        ] {
            optional_decimal(value)?;
        }
    }
    Ok(())
}

fn identifier(value: &str) -> Result<(), DecisionV5Error> {
    if value.is_empty()
        || value.len() > crate::decision_v5::MAX_IDENTIFIER_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        return Err(DecisionV5Error::InvalidContract);
    }
    Ok(())
}

fn text(value: &str) -> Result<(), DecisionV5Error> {
    if value.is_empty() || value.len() > MAX_SUPPLIED_TEXT_BYTES {
        return Err(DecisionV5Error::BoundExceeded);
    }
    Ok(())
}

/// Supplied optional strings may be present and empty (providers emit some fields without
/// `omitempty`); only the byte bound applies.
fn optional_text(value: &Option<String>) -> Result<(), DecisionV5Error> {
    if value
        .as_ref()
        .is_some_and(|value| value.len() > MAX_SUPPLIED_TEXT_BYTES)
    {
        return Err(DecisionV5Error::BoundExceeded);
    }
    Ok(())
}

fn optional_decimal(value: Option<DecimalV5>) -> Result<(), DecisionV5Error> {
    if value.is_some_and(|value| !value.is_canonical()) {
        return Err(DecisionV5Error::InvalidContract);
    }
    Ok(())
}

fn strictly_sorted<T: Ord>(values: impl IntoIterator<Item = T>) -> Result<(), DecisionV5Error> {
    let mut previous = None;
    for value in values {
        if previous.as_ref().is_some_and(|previous| previous >= &value) {
            return Err(DecisionV5Error::NonCanonicalOrder);
        }
        previous = Some(value);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_parse_preserves_exact_digits_and_normalizes() {
        for (text, coefficient, scale, rendered) in [
            ("72.5", 72_5, 1, "72.5"),
            (
                "22.77777777777778",
                22_777_777_777_777_78,
                14,
                "22.77777777777778",
            ),
            ("80", 80, 0, "80"),
            ("80.0", 80, 0, "80"),
            ("-0", 0, 0, "0"),
            ("0.000001", 1, 6, "0.000001"),
            ("1e2", 100, 0, "100"),
            ("1.25e-3", 125, 5, "0.00125"),
            ("-3.5", -35, 1, "-3.5"),
        ] {
            let value = DecimalV5::parse(text).unwrap();
            assert_eq!(
                (value.coefficient, value.scale),
                (coefficient, scale),
                "{text}"
            );
            assert_eq!(value.to_string(), rendered, "{text}");
            assert_eq!(value.to_f64(), text.parse::<f64>().unwrap(), "{text}");
        }
        for text in [
            "",
            "abc",
            "1.",
            ".5",
            "1e400",
            "99999999999999999999",
            "1.2.3",
        ] {
            assert!(DecimalV5::parse(text).is_err(), "{text}");
        }
        assert!(
            !DecimalV5 {
                coefficient: 10,
                scale: 1
            }
            .is_canonical()
        );
        assert!(
            !DecimalV5 {
                coefficient: 0,
                scale: 1
            }
            .is_canonical()
        );
        assert!(
            !DecimalV5 {
                coefficient: 1,
                scale: 19
            }
            .is_canonical()
        );
    }

    #[test]
    fn absent_supplied_inputs_validate_and_present_inputs_require_the_version() {
        assert!(SuppliedInputsV5::default().validate().is_ok());
        let unversioned = SuppliedInputsV5 {
            contract_version: String::new(),
            stations: vec![SuppliedStationV5 {
                station_id: "KSEA".to_owned(),
                ..Default::default()
            }],
            originating_event: None,
        };
        assert_eq!(
            unversioned.validate(),
            Err(DecisionV5Error::InvalidContract)
        );
    }
}
