//! Supplied decision inputs: the canonical originals inside a Decision Context V5.
//!
//! The types are owned by `strategy_core_kernel` (`supplied`, `decimal`) and are the same
//! types a kernel reads through its views; this module names them with the V5 suffix the wire
//! contract uses, owns the bounds and structural validation of a complete supplied block, and
//! converts the frozen `SDCTXV5S` shape of that block into the canonical one.
//!
//! See `traderv3/docs/spec/decision-data-field-matrix.md` for the field inventory and the
//! precision, presence and time definitions this module implements.

use std::collections::BTreeSet;

pub use strategy_core_kernel::supplied::SUPPLIED_INPUTS_CONTRACT_VERSION;
pub use strategy_core_kernel::{
    Decimal as DecimalV5, EventEnvelope as EventEnvelopeV5, ExtremeKind as ExtremeKindV5,
    SuppliedDailyExtremes as SuppliedDailyExtremesV5, SuppliedEvent as SuppliedEventV5,
    SuppliedExtreme as SuppliedExtremeV5, SuppliedForecast as SuppliedForecastV5,
    SuppliedForecastModel as SuppliedForecastModelV5,
    SuppliedForecastPoint as SuppliedForecastPointV5, SuppliedInputs as SuppliedInputsV5,
    SuppliedObservation as SuppliedObservationV5, SuppliedOracleScore as SuppliedOracleScoreV5,
    SuppliedOracleTable as SuppliedOracleTableV5, SuppliedReport as SuppliedReportV5,
    SuppliedStation as SuppliedStationV5, SuppliedWeatherEvent as SuppliedWeatherEventV5,
    SuppliedWeatherEventSource as SuppliedWeatherEventSourceV5,
};

use crate::decision_v5::DecisionV5Error;
use crate::supplied_s;

pub const MAX_SUPPLIED_TEXT_BYTES: usize = 2048;
pub const MAX_SUPPLIED_DECIMAL_SCALE: u8 = strategy_core_kernel::MAX_DECIMAL_SCALE;
pub const MAX_SUPPLIED_REPORTS: usize = 8;
pub const MAX_SUPPLIED_WEATHER_EVENTS: usize = 32;
pub const MAX_SUPPLIED_FORECAST_MODELS: usize = 32;
pub const MAX_SUPPLIED_FORECAST_POINTS: usize = 256;
pub const MAX_SUPPLIED_ORACLE_TABLES: usize = 4;
pub const MAX_SUPPLIED_ORACLE_ROWS: usize = 32;
pub const MAX_SUPPLIED_ADVERTISED_VERSIONS: usize = 32;
pub const MAX_SUPPLIED_NOTIFICATION_MODES: usize = 3;

/// Fields that cannot be represented or attested by a historical C/S packet.
pub(crate) fn has_current_only_fields(inputs: &SuppliedInputsV5) -> bool {
    inputs
        .stations
        .iter()
        .flat_map(|station| &station.oracle_tables)
        .any(|table| {
            table.updated_at_unix_ns.is_some()
                || table.scores.iter().any(|score| score.rank.is_some())
        })
}

/// Structural validation of a supplied block independent of the owner projection.
pub fn validate_supplied_inputs(inputs: &SuppliedInputsV5) -> Result<(), DecisionV5Error> {
    if inputs.is_absent() {
        return Ok(());
    }
    if inputs.contract_version != SUPPLIED_INPUTS_CONTRACT_VERSION {
        return Err(DecisionV5Error::InvalidContract);
    }
    if inputs.stations.len() > crate::decision_v4::MAX_STATIONS {
        return Err(DecisionV5Error::BoundExceeded);
    }
    strictly_sorted(
        inputs
            .stations
            .iter()
            .map(|station| station.station_id.as_str()),
    )?;
    for station in &inputs.stations {
        validate_station(station)?;
    }
    if let Some(event) = &inputs.originating_event {
        validate_event(event)?;
    }
    Ok(())
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

/// Validate a supplied event before host retention or admission.
pub fn validate_event(event: &SuppliedEventV5) -> Result<(), DecisionV5Error> {
    match event {
        SuppliedEventV5::Observation(event) => validate_observation(event),
        SuppliedEventV5::Report(event) => validate_report(event),
        SuppliedEventV5::Extreme(event) => validate_extreme(event),
        SuppliedEventV5::WeatherEvent(event) => validate_weather_event(event),
    }
}

pub(crate) fn validate_envelope(envelope: &EventEnvelopeV5) -> Result<(), DecisionV5Error> {
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
        optional_text(&model.version)?;
        optional_text(&model.fetched_at)?;
        optional_text(&model.issued_at)?;
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
                point.apparent_temperature_c,
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

pub(crate) fn validate_oracle_table(table: &SuppliedOracleTableV5) -> Result<(), DecisionV5Error> {
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
    for (index, score) in table.scores.iter().enumerate() {
        if score.rank.is_some_and(|rank| rank != index as u64 + 1) {
            return Err(DecisionV5Error::InvalidContract);
        }
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

// ---------------------------------------------------------------------------------------------
// Frozen `SDCTXV5S` shape
// ---------------------------------------------------------------------------------------------

/// Lifts the frozen `SDCTXV5S` supplied block into the canonical model. The S shape carried no
/// independent forecast version, issuance or apparent Celsius; those stay absent.
pub(crate) fn from_frozen_s(inputs: supplied_s::SuppliedInputsV5) -> SuppliedInputsV5 {
    let absent = inputs.contract_version.is_empty()
        && inputs.stations.is_empty()
        && inputs.originating_event.is_none();
    SuppliedInputsV5 {
        contract_version: if absent {
            String::new()
        } else {
            SUPPLIED_INPUTS_CONTRACT_VERSION.to_owned()
        },
        stations: inputs.stations.into_iter().map(station_from_s).collect(),
        originating_event: inputs.originating_event.map(event_from_s),
    }
}

/// The frozen `SDCTXV5S` shape of a canonical block. Only a block whose fields all existed in
/// that shape has one; a block carrying an independent forecast version, issuance or apparent
/// Celsius cannot be re-encoded as `SDCTXV5S`.
pub(crate) fn to_frozen_s(
    inputs: &SuppliedInputsV5,
) -> Result<supplied_s::SuppliedInputsV5, DecisionV5Error> {
    if has_current_only_fields(inputs) {
        return Err(DecisionV5Error::InvalidContract);
    }
    Ok(supplied_s::SuppliedInputsV5 {
        contract_version: if inputs.is_absent() {
            String::new()
        } else {
            supplied_s::CONTRACT_VERSION.to_owned()
        },
        stations: inputs
            .stations
            .iter()
            .map(station_to_s)
            .collect::<Result<_, _>>()?,
        originating_event: inputs.originating_event.as_ref().map(event_to_s),
    })
}

fn station_from_s(station: supplied_s::SuppliedStationV5) -> SuppliedStationV5 {
    SuppliedStationV5 {
        station_id: station.station_id,
        observation: station.observation.map(observation_from_s),
        daily_extremes: station.daily_extremes.map(daily_from_s),
        reports: station.reports.into_iter().map(report_from_s).collect(),
        extreme_high: station.extreme_high.map(extreme_from_s),
        extreme_low: station.extreme_low.map(extreme_from_s),
        weather_events: station
            .weather_events
            .into_iter()
            .map(weather_event_from_s)
            .collect(),
        forecast: station.forecast.map(forecast_from_s),
        oracle_tables: station
            .oracle_tables
            .into_iter()
            .map(oracle_from_s)
            .collect(),
    }
}

fn station_to_s(
    station: &SuppliedStationV5,
) -> Result<supplied_s::SuppliedStationV5, DecisionV5Error> {
    Ok(supplied_s::SuppliedStationV5 {
        station_id: station.station_id.clone(),
        observation: station.observation.as_ref().map(observation_to_s),
        daily_extremes: station.daily_extremes.as_ref().map(daily_to_s),
        reports: station.reports.iter().map(report_to_s).collect(),
        extreme_high: station.extreme_high.as_ref().map(extreme_to_s),
        extreme_low: station.extreme_low.as_ref().map(extreme_to_s),
        weather_events: station
            .weather_events
            .iter()
            .map(weather_event_to_s)
            .collect(),
        forecast: station.forecast.as_ref().map(forecast_to_s).transpose()?,
        oracle_tables: station.oracle_tables.iter().map(oracle_to_s).collect(),
    })
}

fn event_from_s(event: supplied_s::SuppliedEventV5) -> SuppliedEventV5 {
    match event {
        supplied_s::SuppliedEventV5::Observation(event) => {
            SuppliedEventV5::Observation(observation_from_s(event))
        }
        supplied_s::SuppliedEventV5::Report(event) => SuppliedEventV5::Report(report_from_s(event)),
        supplied_s::SuppliedEventV5::Extreme(event) => {
            SuppliedEventV5::Extreme(extreme_from_s(event))
        }
        supplied_s::SuppliedEventV5::WeatherEvent(event) => {
            SuppliedEventV5::WeatherEvent(weather_event_from_s(event))
        }
    }
}

fn event_to_s(event: &SuppliedEventV5) -> supplied_s::SuppliedEventV5 {
    match event {
        SuppliedEventV5::Observation(event) => {
            supplied_s::SuppliedEventV5::Observation(observation_to_s(event))
        }
        SuppliedEventV5::Report(event) => supplied_s::SuppliedEventV5::Report(report_to_s(event)),
        SuppliedEventV5::Extreme(event) => {
            supplied_s::SuppliedEventV5::Extreme(extreme_to_s(event))
        }
        SuppliedEventV5::WeatherEvent(event) => {
            supplied_s::SuppliedEventV5::WeatherEvent(weather_event_to_s(event))
        }
    }
}

fn envelope_from_s(envelope: supplied_s::EventEnvelopeV5) -> EventEnvelopeV5 {
    EventEnvelopeV5 {
        event_id: envelope.event_id,
        sequence: envelope.sequence,
        city_sequence: envelope.city_sequence,
        slug: envelope.slug,
        emitted_at_unix_ns: envelope.emitted_at_unix_ns,
        event_key: envelope.event_key,
        source_timestamp_unix_ns: envelope.source_timestamp_unix_ns,
        wmo_emit_time_unix_ns: envelope.wmo_emit_time_unix_ns,
        producer_received_at_unix_ns: envelope.producer_received_at_unix_ns,
        live_published_at_unix_ns: envelope.live_published_at_unix_ns,
        persistence_status: envelope.persistence_status,
        producer_sequence: envelope.producer_sequence,
        received_at_unix_ns: envelope.received_at_unix_ns,
    }
}

fn envelope_to_s(envelope: &EventEnvelopeV5) -> supplied_s::EventEnvelopeV5 {
    supplied_s::EventEnvelopeV5 {
        event_id: envelope.event_id.clone(),
        sequence: envelope.sequence,
        city_sequence: envelope.city_sequence,
        slug: envelope.slug.clone(),
        emitted_at_unix_ns: envelope.emitted_at_unix_ns,
        event_key: envelope.event_key.clone(),
        source_timestamp_unix_ns: envelope.source_timestamp_unix_ns,
        wmo_emit_time_unix_ns: envelope.wmo_emit_time_unix_ns,
        producer_received_at_unix_ns: envelope.producer_received_at_unix_ns,
        live_published_at_unix_ns: envelope.live_published_at_unix_ns,
        persistence_status: envelope.persistence_status.clone(),
        producer_sequence: envelope.producer_sequence,
        received_at_unix_ns: envelope.received_at_unix_ns,
    }
}

pub(crate) fn observation_from_s(
    value: supplied_s::SuppliedObservationV5,
) -> SuppliedObservationV5 {
    SuppliedObservationV5 {
        envelope: value.envelope.map(envelope_from_s),
        source: value.source,
        station_id: value.station_id,
        observed_at_unix_ns: value.observed_at_unix_ns,
        lag_seconds: value.lag_seconds,
        preliminary: value.preliminary,
        temperature_c: value.temperature_c,
        temperature_f: value.temperature_f,
        temp_min_c: value.temp_min_c,
        temp_max_c: value.temp_max_c,
        temp_min_f: value.temp_min_f,
        temp_max_f: value.temp_max_f,
        is_from_report: value.is_from_report,
        report_type: value.report_type,
        source_report_id: value.source_report_id,
        dewpoint: value.dewpoint,
        heat_index: value.heat_index,
        wind_chill: value.wind_chill,
        relative_humidity: value.relative_humidity,
        wind_speed: value.wind_speed,
        wind_direction: value.wind_direction,
        wind_gust: value.wind_gust,
        barometric_pressure: value.barometric_pressure,
        sea_level_pressure: value.sea_level_pressure,
        precipitation_1h: value.precipitation_1h,
        precipitation_3h: value.precipitation_3h,
        precipitation_6h: value.precipitation_6h,
        text_description: value.text_description,
        is_locf: value.is_locf,
        temperature_day_mode: value.temperature_day_mode,
        temperature_day_date: value.temperature_day_date,
    }
}

pub(crate) fn observation_to_s(value: &SuppliedObservationV5) -> supplied_s::SuppliedObservationV5 {
    supplied_s::SuppliedObservationV5 {
        envelope: value.envelope.as_ref().map(envelope_to_s),
        source: value.source.clone(),
        station_id: value.station_id.clone(),
        observed_at_unix_ns: value.observed_at_unix_ns,
        lag_seconds: value.lag_seconds,
        preliminary: value.preliminary,
        temperature_c: value.temperature_c,
        temperature_f: value.temperature_f,
        temp_min_c: value.temp_min_c,
        temp_max_c: value.temp_max_c,
        temp_min_f: value.temp_min_f,
        temp_max_f: value.temp_max_f,
        is_from_report: value.is_from_report,
        report_type: value.report_type.clone(),
        source_report_id: value.source_report_id.clone(),
        dewpoint: value.dewpoint,
        heat_index: value.heat_index,
        wind_chill: value.wind_chill,
        relative_humidity: value.relative_humidity,
        wind_speed: value.wind_speed,
        wind_direction: value.wind_direction,
        wind_gust: value.wind_gust,
        barometric_pressure: value.barometric_pressure,
        sea_level_pressure: value.sea_level_pressure,
        precipitation_1h: value.precipitation_1h,
        precipitation_3h: value.precipitation_3h,
        precipitation_6h: value.precipitation_6h,
        text_description: value.text_description.clone(),
        is_locf: value.is_locf,
        temperature_day_mode: value.temperature_day_mode.clone(),
        temperature_day_date: value.temperature_day_date.clone(),
        ..Default::default()
    }
}

pub(crate) fn daily_from_s(value: supplied_s::SuppliedDailyExtremesV5) -> SuppliedDailyExtremesV5 {
    SuppliedDailyExtremesV5 {
        source: value.source,
        received_at_unix_ns: value.received_at_unix_ns,
        daily_high_f: value.daily_high_f,
        daily_low_f: value.daily_low_f,
        daily_high_c: value.daily_high_c,
        daily_low_c: value.daily_low_c,
        asos_daily_high_f: value.asos_daily_high_f,
        asos_daily_low_f: value.asos_daily_low_f,
        asos_daily_high_c: value.asos_daily_high_c,
        asos_daily_low_c: value.asos_daily_low_c,
        temperature_day_mode: value.temperature_day_mode,
        temperature_day_date: value.temperature_day_date,
        temperature_unit: value.temperature_unit,
        uses_nws_climate_day: value.uses_nws_climate_day,
    }
}

pub(crate) fn daily_to_s(value: &SuppliedDailyExtremesV5) -> supplied_s::SuppliedDailyExtremesV5 {
    supplied_s::SuppliedDailyExtremesV5 {
        source: value.source.clone(),
        received_at_unix_ns: value.received_at_unix_ns,
        daily_high_f: value.daily_high_f,
        daily_low_f: value.daily_low_f,
        daily_high_c: value.daily_high_c,
        daily_low_c: value.daily_low_c,
        asos_daily_high_f: value.asos_daily_high_f,
        asos_daily_low_f: value.asos_daily_low_f,
        asos_daily_high_c: value.asos_daily_high_c,
        asos_daily_low_c: value.asos_daily_low_c,
        temperature_day_mode: value.temperature_day_mode.clone(),
        temperature_day_date: value.temperature_day_date.clone(),
        temperature_unit: value.temperature_unit.clone(),
        uses_nws_climate_day: value.uses_nws_climate_day,
        ..Default::default()
    }
}

fn report_from_s(value: supplied_s::SuppliedReportV5) -> SuppliedReportV5 {
    SuppliedReportV5 {
        envelope: value.envelope.map(envelope_from_s),
        source: value.source,
        station_id: value.station_id,
        report_id: value.report_id,
        report_fingerprint: value.report_fingerprint,
        report_revision: value.report_revision,
        report_updated_at_unix_ns: value.report_updated_at_unix_ns,
        report_type: value.report_type,
        report_date: value.report_date,
        issuance_time_unix_ns: value.issuance_time_unix_ns,
        fetched_at_unix_ns: value.fetched_at_unix_ns,
        source_url: value.source_url,
        max_temp_f: value.max_temp_f,
        max_temp_c: value.max_temp_c,
        max_temp_time_unix_ns: value.max_temp_time_unix_ns,
        min_temp_f: value.min_temp_f,
        min_temp_c: value.min_temp_c,
        min_temp_time_unix_ns: value.min_temp_time_unix_ns,
        temp_f: value.temp_f,
        temp_c: value.temp_c,
        provider: value.provider,
    }
}

fn report_to_s(value: &SuppliedReportV5) -> supplied_s::SuppliedReportV5 {
    supplied_s::SuppliedReportV5 {
        envelope: value.envelope.as_ref().map(envelope_to_s),
        source: value.source.clone(),
        station_id: value.station_id.clone(),
        report_id: value.report_id.clone(),
        report_fingerprint: value.report_fingerprint.clone(),
        report_revision: value.report_revision,
        report_updated_at_unix_ns: value.report_updated_at_unix_ns,
        report_type: value.report_type.clone(),
        report_date: value.report_date.clone(),
        issuance_time_unix_ns: value.issuance_time_unix_ns,
        fetched_at_unix_ns: value.fetched_at_unix_ns,
        source_url: value.source_url.clone(),
        max_temp_f: value.max_temp_f,
        max_temp_c: value.max_temp_c,
        max_temp_time_unix_ns: value.max_temp_time_unix_ns,
        min_temp_f: value.min_temp_f,
        min_temp_c: value.min_temp_c,
        min_temp_time_unix_ns: value.min_temp_time_unix_ns,
        temp_f: value.temp_f,
        temp_c: value.temp_c,
        provider: value.provider.clone(),
    }
}

fn extreme_from_s(value: supplied_s::SuppliedExtremeV5) -> SuppliedExtremeV5 {
    SuppliedExtremeV5 {
        envelope: value.envelope.map(envelope_from_s),
        source: value.source,
        kind: match value.kind {
            supplied_s::ExtremeKindV5::High => ExtremeKindV5::High,
            supplied_s::ExtremeKindV5::Low => ExtremeKindV5::Low,
        },
        station_id: value.station_id,
        value_f: value.value_f,
        value_c: value.value_c,
        prev_value_f: value.prev_value_f,
        observed_at_unix_ns: value.observed_at_unix_ns,
        temperature_day_mode: value.temperature_day_mode,
        temperature_day_date: value.temperature_day_date,
        is_from_report: value.is_from_report,
        report_type: value.report_type,
        source_report_id: value.source_report_id,
    }
}

fn extreme_to_s(value: &SuppliedExtremeV5) -> supplied_s::SuppliedExtremeV5 {
    supplied_s::SuppliedExtremeV5 {
        envelope: value.envelope.as_ref().map(envelope_to_s),
        source: value.source.clone(),
        kind: match value.kind {
            ExtremeKindV5::High => supplied_s::ExtremeKindV5::High,
            ExtremeKindV5::Low => supplied_s::ExtremeKindV5::Low,
        },
        station_id: value.station_id.clone(),
        value_f: value.value_f,
        value_c: value.value_c,
        prev_value_f: value.prev_value_f,
        observed_at_unix_ns: value.observed_at_unix_ns,
        temperature_day_mode: value.temperature_day_mode.clone(),
        temperature_day_date: value.temperature_day_date.clone(),
        is_from_report: value.is_from_report,
        report_type: value.report_type.clone(),
        source_report_id: value.source_report_id.clone(),
    }
}

fn weather_event_from_s(value: supplied_s::SuppliedWeatherEventV5) -> SuppliedWeatherEventV5 {
    SuppliedWeatherEventV5 {
        envelope: value.envelope.map(envelope_from_s),
        source: value.source,
        station_id: value.station_id,
        episode_id: value.episode_id,
        event_type: value.event_type,
        tier: value.tier,
        state: value.state,
        name: value.name,
        badge: value.badge,
        detail: value.detail,
        summary: value.summary,
        started_at_unix_ns: value.started_at_unix_ns,
        last_confirmed_at_unix_ns: value.last_confirmed_at_unix_ns,
        ended_at_unix_ns: value.ended_at_unix_ns,
        source_snapshot: value
            .source_snapshot
            .map(|source| SuppliedWeatherEventSourceV5 {
                metar_type: source.metar_type,
                flight_category: source.flight_category,
                wx_string: source.wx_string,
                wx_token: source.wx_token,
                wind_speed_kt: source.wind_speed_kt,
                wind_gust_kt: source.wind_gust_kt,
                peak_wind_kt: source.peak_wind_kt,
                peak_wind_direction: source.peak_wind_direction,
                visibility_mi: source.visibility_mi,
                cb_location: source.cb_location,
            }),
    }
}

fn weather_event_to_s(value: &SuppliedWeatherEventV5) -> supplied_s::SuppliedWeatherEventV5 {
    supplied_s::SuppliedWeatherEventV5 {
        envelope: value.envelope.as_ref().map(envelope_to_s),
        source: value.source.clone(),
        station_id: value.station_id.clone(),
        episode_id: value.episode_id.clone(),
        event_type: value.event_type.clone(),
        tier: value.tier.clone(),
        state: value.state.clone(),
        name: value.name.clone(),
        badge: value.badge.clone(),
        detail: value.detail.clone(),
        summary: value.summary.clone(),
        started_at_unix_ns: value.started_at_unix_ns,
        last_confirmed_at_unix_ns: value.last_confirmed_at_unix_ns,
        ended_at_unix_ns: value.ended_at_unix_ns,
        source_snapshot: value.source_snapshot.as_ref().map(|source| {
            supplied_s::SuppliedWeatherEventSourceV5 {
                metar_type: source.metar_type.clone(),
                flight_category: source.flight_category.clone(),
                wx_string: source.wx_string.clone(),
                wx_token: source.wx_token.clone(),
                wind_speed_kt: source.wind_speed_kt,
                wind_gust_kt: source.wind_gust_kt,
                peak_wind_kt: source.peak_wind_kt,
                peak_wind_direction: source.peak_wind_direction,
                visibility_mi: source.visibility_mi,
                cb_location: source.cb_location.clone(),
            }
        }),
    }
}

fn forecast_from_s(value: supplied_s::SuppliedForecastV5) -> SuppliedForecastV5 {
    SuppliedForecastV5 {
        source: value.source,
        received_at_unix_ns: value.received_at_unix_ns,
        advertised_versions: value.advertised_versions,
        models: value
            .models
            .into_iter()
            .map(|model| SuppliedForecastModelV5 {
                model_id: model.model_id,
                run_id: model.run_id,
                // The S shape advertised the fetch text as the version.
                version: model.fetched_at.clone(),
                fetched_at: model.fetched_at,
                fetched_at_unix_ns: model.fetched_at_unix_ns,
                issued_at: None,
                issued_at_unix_ns: None,
                timezone: model.timezone,
                utc_offset_seconds: model.utc_offset_seconds,
                hourly: model
                    .hourly
                    .into_iter()
                    .map(|point| SuppliedForecastPointV5 {
                        time: point.time,
                        time_unix_ns: point.time_unix_ns,
                        temperature_2m_f: point.temperature_2m_f,
                        temperature_2m_c: point.temperature_2m_c,
                        apparent_temperature_f: point.apparent_temperature_f,
                        apparent_temperature_c: None,
                        relative_humidity_2m: point.relative_humidity_2m,
                        dew_point_2m: point.dew_point_2m,
                        pressure_msl: point.pressure_msl,
                        wind_speed_10m: point.wind_speed_10m,
                        wind_direction_10m: point.wind_direction_10m,
                        wind_gusts_10m: point.wind_gusts_10m,
                        cloud_cover: point.cloud_cover,
                        precipitation_probability: point.precipitation_probability,
                        weather_code: point.weather_code,
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn forecast_to_s(
    value: &SuppliedForecastV5,
) -> Result<supplied_s::SuppliedForecastV5, DecisionV5Error> {
    Ok(supplied_s::SuppliedForecastV5 {
        source: value.source.clone(),
        received_at_unix_ns: value.received_at_unix_ns,
        advertised_versions: value.advertised_versions.clone(),
        models: value
            .models
            .iter()
            .map(|model| {
                if model.version != model.fetched_at
                    || model.issued_at.is_some()
                    || model.issued_at_unix_ns.is_some()
                    || model
                        .hourly
                        .iter()
                        .any(|point| point.apparent_temperature_c.is_some())
                {
                    return Err(DecisionV5Error::InvalidContract);
                }
                Ok(supplied_s::SuppliedForecastModelV5 {
                    model_id: model.model_id.clone(),
                    run_id: model.run_id.clone(),
                    fetched_at: model.fetched_at.clone(),
                    fetched_at_unix_ns: model.fetched_at_unix_ns,
                    timezone: model.timezone.clone(),
                    utc_offset_seconds: model.utc_offset_seconds,
                    hourly: model
                        .hourly
                        .iter()
                        .map(|point| supplied_s::SuppliedForecastPointV5 {
                            time: point.time.clone(),
                            time_unix_ns: point.time_unix_ns,
                            temperature_2m_f: point.temperature_2m_f,
                            temperature_2m_c: point.temperature_2m_c,
                            apparent_temperature_f: point.apparent_temperature_f,
                            relative_humidity_2m: point.relative_humidity_2m,
                            dew_point_2m: point.dew_point_2m,
                            pressure_msl: point.pressure_msl,
                            wind_speed_10m: point.wind_speed_10m,
                            wind_direction_10m: point.wind_direction_10m,
                            wind_gusts_10m: point.wind_gusts_10m,
                            cloud_cover: point.cloud_cover,
                            precipitation_probability: point.precipitation_probability,
                            weather_code: point.weather_code,
                        })
                        .collect(),
                })
            })
            .collect::<Result<_, _>>()?,
    })
}

pub(crate) fn oracle_from_s(value: supplied_s::SuppliedOracleTableV5) -> SuppliedOracleTableV5 {
    SuppliedOracleTableV5 {
        updated_at_unix_ns: None,
        source: value.source,
        received_at_unix_ns: value.received_at_unix_ns,
        station_id: value.station_id,
        range_start: value.range_start,
        range_end: value.range_end,
        days_requested: value.days_requested,
        all_time: value.all_time,
        score_mode: value.score_mode,
        rank_by: value.rank_by,
        notification_modes: value.notification_modes,
        notification_updated_at_unix_ns: value.notification_updated_at_unix_ns,
        scores: value
            .scores
            .into_iter()
            .map(|score| SuppliedOracleScoreV5 {
                rank: None,
                model_id: score.model_id,
                model_name: score.model_name,
                is_public: score.is_public,
                high_mae: score.high_mae,
                low_mae: score.low_mae,
                high_bias: score.high_bias,
                low_bias: score.low_bias,
                combined_mae: score.combined_mae,
                day_count: score.day_count,
            })
            .collect(),
    }
}

pub(crate) fn oracle_to_s(value: &SuppliedOracleTableV5) -> supplied_s::SuppliedOracleTableV5 {
    supplied_s::SuppliedOracleTableV5 {
        source: value.source.clone(),
        received_at_unix_ns: value.received_at_unix_ns,
        station_id: value.station_id.clone(),
        range_start: value.range_start.clone(),
        range_end: value.range_end.clone(),
        days_requested: value.days_requested,
        all_time: value.all_time,
        score_mode: value.score_mode.clone(),
        rank_by: value.rank_by.clone(),
        notification_modes: value.notification_modes.clone(),
        notification_updated_at_unix_ns: value.notification_updated_at_unix_ns,
        scores: value
            .scores
            .iter()
            .map(|score| supplied_s::SuppliedOracleScoreV5 {
                model_id: score.model_id.clone(),
                model_name: score.model_name.clone(),
                is_public: score.is_public,
                high_mae: score.high_mae,
                low_mae: score.low_mae,
                high_bias: score.high_bias,
                low_bias: score.low_bias,
                combined_mae: score.combined_mae,
                day_count: score.day_count,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_supplied_inputs_validate_and_present_inputs_require_the_version() {
        assert!(validate_supplied_inputs(&SuppliedInputsV5::default()).is_ok());
        let unversioned = SuppliedInputsV5 {
            contract_version: String::new(),
            stations: vec![SuppliedStationV5 {
                station_id: "KSEA".to_owned(),
                ..Default::default()
            }],
            originating_event: None,
        };
        assert_eq!(
            validate_supplied_inputs(&unversioned),
            Err(DecisionV5Error::InvalidContract)
        );
    }

    #[test]
    fn frozen_s_round_trips_only_shapes_it_can_carry() {
        let mut inputs = SuppliedInputsV5 {
            contract_version: SUPPLIED_INPUTS_CONTRACT_VERSION.to_owned(),
            stations: vec![SuppliedStationV5 {
                station_id: "KSEA".to_owned(),
                forecast: Some(SuppliedForecastV5 {
                    source: "minutetemp.rest.forecast".to_owned(),
                    received_at_unix_ns: 1,
                    advertised_versions: vec![(
                        "hrrr".to_owned(),
                        "2026-08-30T18:00:00Z".to_owned(),
                    )],
                    models: vec![SuppliedForecastModelV5 {
                        model_id: "hrrr".to_owned(),
                        version: Some("2026-08-30T18:00:00Z".to_owned()),
                        fetched_at: Some("2026-08-30T18:00:00Z".to_owned()),
                        fetched_at_unix_ns: Some(1_788_112_800_000_000_000),
                        hourly: vec![SuppliedForecastPointV5 {
                            time: "2026-08-30T19:00:00Z".to_owned(),
                            time_unix_ns: 1_788_116_400_000_000_000,
                            temperature_2m_f: Some(DecimalV5::parse("78.6").unwrap()),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }],
                }),
                ..Default::default()
            }],
            originating_event: None,
        };
        let frozen = to_frozen_s(&inputs).unwrap();
        assert_eq!(frozen.contract_version, supplied_s::CONTRACT_VERSION);
        assert_eq!(from_frozen_s(frozen), inputs);
        assert_eq!(
            to_frozen_s(&SuppliedInputsV5::default())
                .unwrap()
                .contract_version,
            ""
        );
        let model = &mut inputs.stations[0].forecast.as_mut().unwrap().models[0];
        model.issued_at_unix_ns = Some(7);
        assert_eq!(to_frozen_s(&inputs), Err(DecisionV5Error::InvalidContract));
    }
}
