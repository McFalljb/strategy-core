//! Supplied decision inputs: the canonical originals inside a Decision Context V6.
//!
//! The types are owned by `strategy_core_kernel` (`supplied`, `decimal`) and are the same
//! types a kernel reads through its views; this module names them with the V6 suffix the wire
//! contract uses and owns the bounds and structural validation of a complete supplied block.
//!
//! See `traderv3/docs/spec/decision-data-field-matrix.md` for the field inventory and the
//! precision, presence and time definitions this module implements.

use std::collections::BTreeSet;

pub use strategy_core_kernel::supplied::SUPPLIED_INPUTS_CONTRACT_VERSION;
pub use strategy_core_kernel::{
    Decimal as DecimalV6, EventEnvelope as EventEnvelopeV6, ExtremeKind as ExtremeKindV6,
    SuppliedDailyExtremes as SuppliedDailyExtremesV6, SuppliedEvent as SuppliedEventV6,
    SuppliedExtreme as SuppliedExtremeV6, SuppliedForecast as SuppliedForecastV6,
    SuppliedForecastModel as SuppliedForecastModelV6,
    SuppliedForecastPoint as SuppliedForecastPointV6, SuppliedInputs as SuppliedInputsV6,
    SuppliedObservation as SuppliedObservationV6, SuppliedOracleScore as SuppliedOracleScoreV6,
    SuppliedOracleTable as SuppliedOracleTableV6, SuppliedReport as SuppliedReportV6,
    SuppliedStation as SuppliedStationV6, SuppliedWeatherEvent as SuppliedWeatherEventV6,
    SuppliedWeatherEventSource as SuppliedWeatherEventSourceV6,
};

use crate::decision_v6::DecisionV6Error;

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

/// Structural validation of a supplied block independent of the owner projection.
pub fn validate_supplied_inputs(inputs: &SuppliedInputsV6) -> Result<(), DecisionV6Error> {
    if inputs.is_absent() {
        return Ok(());
    }
    if inputs.contract_version != SUPPLIED_INPUTS_CONTRACT_VERSION {
        return Err(DecisionV6Error::InvalidContract);
    }
    if inputs.stations.len() > crate::decision_v4::MAX_STATIONS {
        return Err(DecisionV6Error::BoundExceeded);
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

fn validate_station(station: &SuppliedStationV6) -> Result<(), DecisionV6Error> {
    identifier(&station.station_id)?;
    if station.reports.len() > MAX_SUPPLIED_REPORTS
        || station.weather_events.len() > MAX_SUPPLIED_WEATHER_EVENTS
        || station.oracle_tables.len() > MAX_SUPPLIED_ORACLE_TABLES
    {
        return Err(DecisionV6Error::BoundExceeded);
    }
    if let Some(observation) = &station.observation {
        validate_observation(observation)?;
        if observation.station_id != station.station_id {
            return Err(DecisionV6Error::InvalidContract);
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
            return Err(DecisionV6Error::InvalidContract);
        }
    }
    for (extreme, kind) in [
        (station.extreme_high.as_ref(), ExtremeKindV6::High),
        (station.extreme_low.as_ref(), ExtremeKindV6::Low),
    ] {
        if let Some(extreme) = extreme {
            validate_extreme(extreme)?;
            if extreme.kind != kind || extreme.station_id != station.station_id {
                return Err(DecisionV6Error::InvalidContract);
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
            return Err(DecisionV6Error::InvalidContract);
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
pub fn validate_event(event: &SuppliedEventV6) -> Result<(), DecisionV6Error> {
    match event {
        SuppliedEventV6::Observation(event) => validate_observation(event),
        SuppliedEventV6::Report(event) => validate_report(event),
        SuppliedEventV6::Extreme(event) => validate_extreme(event),
        SuppliedEventV6::WeatherEvent(event) => validate_weather_event(event),
    }
}

pub(crate) fn validate_envelope(envelope: &EventEnvelopeV6) -> Result<(), DecisionV6Error> {
    identifier(&envelope.event_id)?;
    if envelope.sequence == 0 || envelope.city_sequence == Some(0) {
        return Err(DecisionV6Error::InvalidContract);
    }
    optional_text(&envelope.slug)?;
    optional_text(&envelope.event_key)?;
    optional_text(&envelope.persistence_status)?;
    Ok(())
}

fn validate_observation(observation: &SuppliedObservationV6) -> Result<(), DecisionV6Error> {
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
        return Err(DecisionV6Error::InvalidContract);
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

fn validate_daily_extremes(daily: &SuppliedDailyExtremesV6) -> Result<(), DecisionV6Error> {
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

fn validate_report(report: &SuppliedReportV6) -> Result<(), DecisionV6Error> {
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

fn validate_extreme(extreme: &SuppliedExtremeV6) -> Result<(), DecisionV6Error> {
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
        return Err(DecisionV6Error::InvalidContract);
    }
    for value in [extreme.value_f, extreme.value_c, extreme.prev_value_f] {
        optional_decimal(value)?;
    }
    Ok(())
}

fn validate_weather_event(event: &SuppliedWeatherEventV6) -> Result<(), DecisionV6Error> {
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
        return Err(DecisionV6Error::InvalidContract);
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

fn validate_forecast(forecast: &SuppliedForecastV6) -> Result<(), DecisionV6Error> {
    text(&forecast.source)?;
    if forecast.models.len() > MAX_SUPPLIED_FORECAST_MODELS
        || forecast.advertised_versions.len() > MAX_SUPPLIED_ADVERTISED_VERSIONS
    {
        return Err(DecisionV6Error::BoundExceeded);
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
            return Err(DecisionV6Error::BoundExceeded);
        }
        if model
            .hourly
            .windows(2)
            .any(|points| points[0].time_unix_ns >= points[1].time_unix_ns)
        {
            return Err(DecisionV6Error::NonCanonicalOrder);
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

pub(crate) fn validate_oracle_table(table: &SuppliedOracleTableV6) -> Result<(), DecisionV6Error> {
    text(&table.source)?;
    identifier(&table.station_id)?;
    text(&table.range_start)?;
    text(&table.range_end)?;
    optional_text(&table.score_mode)?;
    optional_text(&table.rank_by)?;
    if table.scores.len() > MAX_SUPPLIED_ORACLE_ROWS
        || table.notification_modes.len() > MAX_SUPPLIED_NOTIFICATION_MODES
    {
        return Err(DecisionV6Error::BoundExceeded);
    }
    for mode in &table.notification_modes {
        text(mode)?;
    }
    let mut seen = BTreeSet::new();
    for (index, score) in table.scores.iter().enumerate() {
        if score.rank.is_some_and(|rank| rank != index as u64 + 1) {
            return Err(DecisionV6Error::InvalidContract);
        }
        text(&score.model_id)?;
        text(&score.model_name)?;
        if !seen.insert(score.model_id.as_str()) {
            return Err(DecisionV6Error::DuplicateIdentity);
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

fn identifier(value: &str) -> Result<(), DecisionV6Error> {
    if value.is_empty()
        || value.len() > crate::decision_v6::MAX_IDENTIFIER_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        return Err(DecisionV6Error::InvalidContract);
    }
    Ok(())
}

fn text(value: &str) -> Result<(), DecisionV6Error> {
    if value.is_empty() || value.len() > MAX_SUPPLIED_TEXT_BYTES {
        return Err(DecisionV6Error::BoundExceeded);
    }
    Ok(())
}

/// Supplied optional strings may be present and empty (providers emit some fields without
/// `omitempty`); only the byte bound applies.
fn optional_text(value: &Option<String>) -> Result<(), DecisionV6Error> {
    if value
        .as_ref()
        .is_some_and(|value| value.len() > MAX_SUPPLIED_TEXT_BYTES)
    {
        return Err(DecisionV6Error::BoundExceeded);
    }
    Ok(())
}

fn optional_decimal(value: Option<DecimalV6>) -> Result<(), DecisionV6Error> {
    if value.is_some_and(|value| !value.is_canonical()) {
        return Err(DecisionV6Error::InvalidContract);
    }
    Ok(())
}

fn strictly_sorted<T: Ord>(values: impl IntoIterator<Item = T>) -> Result<(), DecisionV6Error> {
    let mut previous = None;
    for value in values {
        if previous.as_ref().is_some_and(|previous| previous >= &value) {
            return Err(DecisionV6Error::NonCanonicalOrder);
        }
        previous = Some(value);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_supplied_inputs_validate_and_present_inputs_require_the_version() {
        assert!(validate_supplied_inputs(&SuppliedInputsV6::default()).is_ok());
        let unversioned = SuppliedInputsV6 {
            contract_version: String::new(),
            stations: vec![SuppliedStationV6 {
                station_id: "KSEA".to_owned(),
                ..Default::default()
            }],
            originating_event: None,
        };
        assert_eq!(
            validate_supplied_inputs(&unversioned),
            Err(DecisionV6Error::InvalidContract)
        );
    }
}
