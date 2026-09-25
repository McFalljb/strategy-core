//! The canonical scoped state of one decision, projected once from the context.
//!
//! Every component is projected from its supplied original when the context carries one and
//! from the V4 owner projection otherwise; each carries its `ValueOrigin` and, when supplied,
//! the original itself. Whole-contract quantities are explicit floors beside the exact
//! hundredths.

use chrono::{DateTime, TimeZone, Utc};
use strategy_core_kernel::{
    Book, BookLevel, ClimateDay, ComponentAuthority, ComponentMeta, ContractQuantity,
    DailyExtremes, EventProvenance, Extreme, FinalFact, Forecast, ForecastModel, ForecastPoint,
    LastTrade, MarketComponents, MarketLifecycle, MarketState, Observation, OracleScore,
    OracleTable, Report, StationComponents, StationIdentity, StationState, StationWeatherView,
    TickerQuote, ValueOrigin, WeatherEvent, WeatherEventSource,
};

use super::KernelTransactionError;
use crate::decision_v4::{
    AuthorityV4, BookLevelV4, ComponentMetaV4, ExtremeV4, ForecastModelV4, MarketMetaV4, MarketV4,
    ObservationV4, ProvenanceV4, RankByV4, ReportV4, StationV4, WeatherEventV4,
};
use crate::decision_v6::{DecisionContextV6, DecisionV6Error};
use crate::supplied_v6::{ExtremeKindV6, SuppliedStationV6};

pub(super) fn station_state(
    station: &StationV4,
    supplied: Option<&SuppliedStationV6>,
    weather_facts: Option<&strategy_core_kernel::WeatherFacts>,
    forecast_issuance: Option<&[strategy_core_kernel::forecast::ForecastIssuance]>,
    current: Option<&crate::current_v6::StationInputsV6>,
) -> Result<StationState, KernelTransactionError> {
    let station_id = station.identity.station_id.as_str();
    let event_date = station.climate_event_date.as_str();
    let supplied_observation = supplied.and_then(|station| station.observation.as_ref());
    let observation = match supplied_observation {
        Some(event) => Some(Observation::from_supplied(event)),
        None => (station.observation.observed_at_unix_ms != 0
            || station.observation.temperature_milli_c.is_some())
        .then(|| derived_observation(&station.observation, station_id))
        .transpose()?,
    };
    let daily_extremes = supplied
        .and_then(|station| station.daily_extremes.as_ref())
        .map(DailyExtremes::from_supplied);
    let mut reports = Vec::with_capacity(station.reports.len());
    for report in &station.reports {
        let supplied_report = supplied.and_then(|station| {
            station
                .reports
                .iter()
                .find(|candidate| candidate.report_type == report.report_type)
                .filter(|candidate| {
                    candidate.report_id == report.report_id
                        && candidate.report_revision.unwrap_or(0) == report.revision
                })
        });
        let mut model = match supplied_report {
            Some(event) => Report::from_supplied(event),
            None => derived_report(report, station_id),
        };
        model.meta = ComponentMeta {
            authority: authority(&report.authority),
            revision: report.revision,
            generation: report.generation,
            updated_at: millis(report.updated_at_unix_ms)?,
            expected_version: None,
            refresh_error: None,
            provenance: vec![component_provenance(&report.provenance)],
        };
        reports.push(model);
    }
    reports.sort_by(|left, right| left.report_type.cmp(&right.report_type));
    let mut weather_events = Vec::with_capacity(station.weather_events.len());
    for event in &station.weather_events {
        let supplied_event = supplied.and_then(|station| {
            station
                .weather_events
                .iter()
                .find(|candidate| candidate.episode_id == event.event_id)
        });
        let mut model = match supplied_event {
            Some(supplied) => WeatherEvent::from_supplied(supplied),
            None => derived_weather_event(event, station_id)?,
        };
        model.meta = ComponentMeta {
            authority: authority(&event.authority),
            revision: event.revision,
            generation: event.generation,
            updated_at: None,
            expected_version: None,
            refresh_error: None,
            provenance: vec![component_provenance(&event.provenance)],
        };
        weather_events.push(model);
    }
    let mut extreme_high = match supplied.and_then(|station| station.extreme_high.as_ref()) {
        Some(event) => Some(Extreme::from_supplied(event, event_date)),
        None => station
            .extrema
            .high
            .as_ref()
            .map(|extreme| derived_extreme(extreme, station_id, ExtremeKindV6::High, event_date))
            .transpose()?,
    };
    let mut extreme_low = match supplied.and_then(|station| station.extreme_low.as_ref()) {
        Some(event) => Some(Extreme::from_supplied(event, event_date)),
        None => station
            .extrema
            .low
            .as_ref()
            .map(|extreme| derived_extreme(extreme, station_id, ExtremeKindV6::Low, event_date))
            .transpose()?,
    };
    // Observation/report extrema are not fabricated new-high/new-low events. Their exact
    // per-field originals remain in weather_facts; this component is an explicit derivation.
    if let Some(facts) = weather_facts {
        for (field, extreme) in [
            (
                strategy_core_kernel::WeatherField::ExtremeHigh,
                &mut extreme_high,
            ),
            (
                strategy_core_kernel::WeatherField::ExtremeLow,
                &mut extreme_low,
            ),
        ] {
            if let Some(extreme) = extreme
                .as_mut()
                .filter(|extreme| extreme.supplied.is_none())
            {
                if let Some(fact) = facts.fields.get(&field) {
                    extreme.value_f =
                        facts
                            .temperature_f(field)
                            .ok_or(KernelTransactionError::Contract(
                                DecisionV6Error::InvalidContract,
                            ))?;
                    extreme.value_c =
                        facts
                            .temperature_c(field)
                            .ok_or(KernelTransactionError::Contract(
                                DecisionV6Error::InvalidContract,
                            ))?;
                    if fact.provenance.supplied {
                        extreme.provenance = EventProvenance::from_envelope(
                            fact.provenance.envelope.as_ref(),
                            &fact.provenance.source,
                            station_id,
                        );
                        extreme.provenance.received_at = fact
                            .provenance
                            .received_at_unix_ns
                            .map(DateTime::from_timestamp_nanos);
                        extreme.observed_at = fact
                            .provenance
                            .observed_at_unix_ns
                            .map(DateTime::from_timestamp_nanos);
                        extreme.report_type = fact.provenance.report_type.clone();
                        extreme.source_report_id = fact.provenance.report_id.clone();
                        extreme.temperature_day_mode = fact.provenance.temperature_day_mode.clone();
                        extreme.temperature_day_date = fact.provenance.temperature_day_date.clone();
                    }
                }
            }
        }
    }
    let mut forecast = match supplied.and_then(|station| station.forecast.as_ref()) {
        Some(supplied) => Some(Forecast::from_supplied(station_id, supplied)),
        None => (!station.forecast.models.is_empty())
            .then(|| derived_forecast(station, station_id))
            .transpose()?,
    };
    if let Some(forecast) = &mut forecast {
        forecast.meta = component_meta(&station.forecast_meta)?;
        if let Some(accepted) = forecast_issuance {
            forecast.advertised_versions = station.forecast.advertised_versions.clone();
            forecast.models = station
                .forecast
                .models
                .iter()
                .zip(accepted)
                .map(|(model, issued)| {
                    let mut projected = forecast
                        .models
                        .iter()
                        .find(|value| value.id == model.model_id)
                        .cloned()
                        .map_or_else(|| derived_forecast_model(model), Ok)?;
                    projected.version = model.version.clone();
                    projected.issued_at = Some(DateTime::from_timestamp_nanos(issued.at_unix_ns));
                    projected.issuance = Some(issued.clone());
                    Ok(projected)
                })
                .collect::<Result<Vec<_>, KernelTransactionError>>()?;
        }
    }
    let mut oracle_tables = match supplied
        .map(|station| &station.oracle_tables)
        .filter(|tables| !tables.is_empty())
    {
        Some(tables) => tables.iter().map(OracleTable::from_supplied).collect(),
        None => (!station.oracle.rows.is_empty() || station.oracle.updated_at_unix_ms.is_some())
            .then(|| derived_oracle(&station.oracle))
            .transpose()?
            .into_iter()
            .collect::<Vec<_>>(),
    };
    if let Some(current) = current {
        oracle_tables = current
            .oracles
            .iter()
            .map(|input| {
                let mut table = input
                    .supplied
                    .as_ref()
                    .map(OracleTable::from_supplied)
                    .map_or_else(|| derived_oracle(&input.table), Ok)?;
                table.mode = input.table.query.mode.clone();
                table.rank_by = match input.table.query.rank_by {
                    RankByV4::High => "high",
                    RankByV4::Low => "low",
                }
                .to_owned();
                table.days = input.table.query.days.to_string();
                table.meta = component_meta(&input.meta)?;
                Ok(table)
            })
            .collect::<Result<Vec<_>, KernelTransactionError>>()?;
    } else {
        for table in &mut oracle_tables {
            table.meta = component_meta(&station.oracle_meta)?;
        }
    }
    let weather = match weather_facts {
        Some(facts) => facts.summary(station_id),
        None => legacy_weather_summary(station, observation.as_ref(), daily_extremes.as_ref())?,
    };
    Ok(StationState {
        identity: StationIdentity {
            station_id: station_id.to_owned(),
            city_id: station.identity.city_id.clone(),
            city_slug: station.identity.city_slug.clone(),
            logical_location: station.identity.logical_location.clone(),
            name: station.identity.name.clone(),
            latitude: micros(station.identity.latitude_micros),
            longitude: micros(station.identity.longitude_micros),
            timezone: station.identity.timezone.clone(),
        },
        climate_day: ClimateDay {
            event_date: event_date.to_owned(),
            start: millis(Some(station.climate_day_start_utc_unix_ms))?,
            end: millis(Some(station.climate_day_end_utc_unix_ms))?,
        },
        components: StationComponents {
            observation: component_meta(&station.observation_meta)?,
            weather: component_meta(&station.weather_meta)?,
            extrema: component_meta(&station.extrema_meta)?,
            reports: component_meta(&station.reports_meta)?,
            weather_events: component_meta(&station.weather_events_meta)?,
            forecast: component_meta(&station.forecast_meta)?,
            oracle: component_meta(&station.oracle_meta)?,
        },
        weather,
        weather_facts: weather_facts.cloned(),
        observation,
        daily_extremes,
        extreme_high,
        extreme_low,
        reports,
        weather_events,
        forecast,
        oracle_tables,
    })
}

/// Compatibility projection for contexts captured before per-field acceptance was retained.
/// New Trader deliveries use explicit host winners instead; this is not a freshness authority.
///
/// Historical replacement rule: a fact is projected from its supplied original only while that original
/// is still the current fact in the owner's merged state, i.e. the owner's derived value agrees
/// with the original at the derived precision. A newer accepted update that changed the
/// derived value wins, and the superseded original stays retained on the station
/// (`daily_extremes`, `observation`) as evidence.
pub(super) fn legacy_weather_summary(
    station: &StationV4,
    observation: Option<&Observation>,
    daily: Option<&DailyExtremes>,
) -> Result<StationWeatherView, KernelTransactionError> {
    let weather = &station.weather;
    let derived = &station.observation;
    let supplied_observation = observation
        .filter(|observation| observation.origin == ValueOrigin::Supplied)
        .filter(|observation| {
            observation.observed_at.map(|at| at.timestamp_millis())
                == Some(derived.observed_at_unix_ms)
        });
    // A REST daily original is current while the owner's running summary still equals it.
    // No derived value means nothing has superseded the original. A derived value must agree
    // with the original within the derived milli-Celsius rounding.
    let current_daily =
        |original_c: Option<f64>, original_f: Option<f64>, derived_milli_c: Option<i32>| {
            let Some(derived_milli_c) = derived_milli_c else {
                return true;
            };
            original_c
                .or_else(|| original_f.map(strategy_core_kernel::state::fahrenheit_to_celsius))
                .map(|value| (value * 1_000.0).round() as i32)
                .is_some_and(|original_milli_c| (original_milli_c - derived_milli_c).abs() <= 1)
        };
    let daily_value = |original_f: Option<f64>,
                       original_c: Option<f64>,
                       derived_milli_c: Option<i32>| {
        match daily {
            Some(_) if current_daily(original_c, original_f, derived_milli_c) => original_f
                .or_else(|| original_c.map(strategy_core_kernel::state::celsius_to_fahrenheit))
                .or_else(|| milli_c_to_f(derived_milli_c)),
            _ => milli_c_to_f(derived_milli_c),
        }
    };
    let observation_value =
        |original: Option<f64>, derived_value: Option<f64>| match supplied_observation {
            Some(_) => original.or(derived_value),
            None => derived_value,
        };
    Ok(StationWeatherView {
        station_id: station.identity.station_id.clone(),
        current_temp: observation_value(
            supplied_observation.and_then(|observation| observation.temperature_f),
            milli_c_to_f(weather.current_temperature_milli_c),
        ),
        running_high: daily_value(
            daily.and_then(|daily| daily.daily_high_f),
            daily.and_then(|daily| daily.daily_high_c),
            weather.running_high_milli_c,
        ),
        running_low: daily_value(
            daily.and_then(|daily| daily.daily_low_f),
            daily.and_then(|daily| daily.daily_low_c),
            weather.running_low_milli_c,
        ),
        last_metar_time: millis(weather.last_metar_at_unix_ms)?,
        temp_min_f: observation_value(
            supplied_observation.and_then(|observation| observation.temp_min_f),
            milli_c_to_f(derived.temperature_min_milli_c),
        ),
        temp_max_f: observation_value(
            supplied_observation.and_then(|observation| observation.temp_max_f),
            milli_c_to_f(derived.temperature_max_milli_c),
        ),
        temp_min_c: observation_value(
            supplied_observation.and_then(|observation| observation.temp_min_c),
            milli_c(derived.temperature_min_milli_c),
        ),
        temp_max_c: observation_value(
            supplied_observation.and_then(|observation| observation.temp_max_c),
            milli_c(derived.temperature_max_milli_c),
        ),
        preliminary: weather.preliminary,
        dsm_high: milli_c_to_f(weather.dsm_high_milli_c),
        dsm_low: milli_c_to_f(weather.dsm_low_milli_c),
        dsm_high_time: millis(weather.dsm_high_at_unix_ms)?,
        dsm_low_time: millis(weather.dsm_low_at_unix_ms)?,
        six_hr_high: milli_c_to_f(weather.six_hour_high_milli_c),
        six_hr_low: milli_c_to_f(weather.six_hour_low_milli_c),
        last_dsm_time: millis(weather.dsm_high_at_unix_ms.or(weather.dsm_low_at_unix_ms))?,
        last_six_hr_time: None,
        asos_daily_high_f: daily_value(
            daily.and_then(|daily| daily.asos_daily_high_f),
            daily.and_then(|daily| daily.asos_daily_high_c),
            weather.asos_daily_high_milli_c,
        ),
        asos_daily_low_f: daily_value(
            daily.and_then(|daily| daily.asos_daily_low_f),
            daily.and_then(|daily| daily.asos_daily_low_c),
            weather.asos_daily_low_milli_c,
        ),
        dewpoint: observation_value(
            supplied_observation.and_then(|observation| observation.dewpoint),
            micros(weather.dewpoint_micros),
        ),
        heat_index: observation_value(
            supplied_observation.and_then(|observation| observation.heat_index),
            micros(weather.heat_index_micros),
        ),
        wind_chill: observation_value(
            supplied_observation.and_then(|observation| observation.wind_chill),
            micros(weather.wind_chill_micros),
        ),
        relative_humidity: observation_value(
            supplied_observation.and_then(|observation| observation.relative_humidity),
            micros(weather.relative_humidity_micros),
        ),
        wind_speed: observation_value(
            supplied_observation.and_then(|observation| observation.wind_speed),
            micros(weather.wind_speed_micros),
        ),
        wind_direction: observation_value(
            supplied_observation.and_then(|observation| observation.wind_direction),
            micros(weather.wind_direction_micros),
        ),
        wind_gust: observation_value(
            supplied_observation.and_then(|observation| observation.wind_gust),
            micros(weather.wind_gust_micros),
        ),
        text_description: supplied_observation
            .and_then(|observation| observation.text_description.clone())
            .or_else(|| weather.text_description.clone()),
        lag_seconds: supplied_observation
            .and_then(|observation| observation.lag_seconds)
            .or_else(|| derived.lag_ms.map(|value| value / 1_000)),
    })
}

pub(super) fn component_provenance(
    value: &ProvenanceV4,
) -> strategy_core_kernel::state::ComponentProvenance {
    strategy_core_kernel::state::ComponentProvenance {
        provider: value.provider.clone(),
        source: value.source.clone(),
        event_id: value.event_id.clone(),
        connection_epoch: value.connection_epoch,
        sid: value.sid,
        sequence: value.sequence,
        city_sequence: value.city_sequence,
        producer_sequence: value.producer_sequence,
        received_frame_ordinal: value.received_frame_ordinal,
        provider_at_unix_ms: value.provider_at_unix_ms,
        received_at_unix_ms: value.received_at_unix_ms,
    }
}

pub(super) fn component_meta(
    meta: &ComponentMetaV4,
) -> Result<ComponentMeta, KernelTransactionError> {
    Ok(ComponentMeta {
        authority: authority(&meta.authority),
        revision: meta.revision,
        generation: meta.generation,
        updated_at: millis(meta.updated_at_unix_ms)?,
        expected_version: meta.expected_version.clone(),
        refresh_error: meta.refresh_error.clone(),
        provenance: meta.provenance.iter().map(component_provenance).collect(),
    })
}

pub(super) fn market_meta(meta: &MarketMetaV4) -> Result<ComponentMeta, KernelTransactionError> {
    Ok(ComponentMeta {
        authority: authority(&meta.authority),
        revision: meta.revision,
        generation: meta.generation,
        updated_at: millis(meta.updated_at_unix_ms)?,
        expected_version: meta.expected_version.clone(),
        refresh_error: meta.refresh_error.clone(),
        provenance: meta.provenance.iter().map(component_provenance).collect(),
    })
}

pub(super) fn authority(value: &AuthorityV4) -> ComponentAuthority {
    match value {
        AuthorityV4::Warming => ComponentAuthority::Warming,
        AuthorityV4::Current => ComponentAuthority::Current,
        AuthorityV4::RefreshPending => ComponentAuthority::RefreshPending,
        AuthorityV4::Uncertain => ComponentAuthority::Uncertain,
        AuthorityV4::Unavailable => ComponentAuthority::Unavailable,
    }
}

/// Provenance of a derived V4 fact; `station_id` is the owner identity, never the copy inside
/// the V4 component.
pub(super) fn derived_provenance(provenance: &ProvenanceV4, station_id: &str) -> EventProvenance {
    EventProvenance {
        provider: provenance.provider.clone(),
        source: provenance.source.clone(),
        event_id: provenance.event_id.clone(),
        sequence: provenance
            .sequence
            .and_then(|value| i64::try_from(value).ok()),
        city_sequence: provenance
            .city_sequence
            .and_then(|value| i64::try_from(value).ok()),
        producer_sequence: provenance
            .producer_sequence
            .and_then(|value| i64::try_from(value).ok()),
        emitted_at: millis(provenance.provider_at_unix_ms).ok().flatten(),
        slug: station_id.to_owned(),
        event_key: None,
        source_timestamp: None,
        wmo_emit_time: None,
        producer_received_at: None,
        live_published_at: None,
        persistence_status: None,
        received_at: millis(Some(provenance.received_at_unix_ms)).ok().flatten(),
        connection_epoch: provenance.connection_epoch,
        sid: provenance.sid,
        received_frame_ordinal: provenance.received_frame_ordinal,
        acceptance: None,
    }
}

pub(super) fn derived_observation(
    observation: &ObservationV4,
    station_id: &str,
) -> Result<Observation, KernelTransactionError> {
    let mut provenance = derived_provenance(&observation.provenance, station_id);
    provenance.source_timestamp = millis(observation.source_timestamp_unix_ms)?;
    provenance.producer_received_at = millis(observation.producer_received_at_unix_ms)?;
    provenance.live_published_at = millis(observation.live_published_at_unix_ms)?;
    provenance.persistence_status = observation.persistence_status.clone();
    Ok(Observation {
        provenance,
        station_id: station_id.to_owned(),
        observed_at: millis(Some(observation.observed_at_unix_ms))?,
        lag_seconds: observation.lag_ms.map(|value| value / 1_000),
        preliminary: observation.preliminary,
        temperature_f: milli_c_to_f(observation.temperature_milli_c),
        temperature_c: milli_c(observation.temperature_milli_c),
        temp_min_f: milli_c_to_f(observation.temperature_min_milli_c),
        temp_max_f: milli_c_to_f(observation.temperature_max_milli_c),
        temp_min_c: milli_c(observation.temperature_min_milli_c),
        temp_max_c: milli_c(observation.temperature_max_milli_c),
        is_from_report: observation.is_from_report,
        report_type: observation.report_type.clone(),
        source_report_id: observation.source_report_id.clone(),
        temperature_day_mode: observation.temperature_day_mode.clone(),
        temperature_day_date: observation.temperature_day_date.clone(),
        dewpoint: micros(observation.dewpoint_micros),
        heat_index: micros(observation.heat_index_micros),
        wind_chill: micros(observation.wind_chill_micros),
        relative_humidity: micros(observation.relative_humidity_micros),
        wind_speed: micros(observation.wind_speed_micros),
        wind_direction: micros(observation.wind_direction_micros),
        wind_gust: micros(observation.wind_gust_micros),
        text_description: observation.text_description.clone(),
        barometric_pressure: None,
        sea_level_pressure: None,
        precipitation_1h: None,
        precipitation_3h: None,
        precipitation_6h: None,
        is_locf: None,
        origin: ValueOrigin::Derived,
        supplied: None,
    })
}

pub(super) fn derived_report(report: &ReportV4, station_id: &str) -> Report {
    Report {
        provenance: derived_provenance(&report.provenance, station_id),
        station_id: station_id.to_owned(),
        report_id: report.report_id.clone(),
        report_type: report.report_type.clone(),
        report_date: report.report_date.clone(),
        report_revision: i64::try_from(report.revision).unwrap_or(i64::MAX),
        report_fingerprint: None,
        report_updated_at: millis(report.updated_at_unix_ms).ok().flatten(),
        issuance_time: millis(report.issued_at_unix_ms).ok().flatten(),
        fetched_at: millis(report.fetched_at_unix_ms).ok().flatten(),
        source_url: report.source_url.clone(),
        provider: report.provider.clone(),
        max_temp_f: milli_c_to_f(report.max_temperature_milli_c),
        max_temp_c: milli_c(report.max_temperature_milli_c),
        min_temp_f: milli_c_to_f(report.min_temperature_milli_c),
        min_temp_c: milli_c(report.min_temperature_milli_c),
        // The V4 projection carries the supplied point temperature in milli-F.
        temp_f: report
            .temperature_milli_f
            .map(|value| f64::from(value) / 1_000.0)
            .or_else(|| milli_c_to_f(report.temperature_milli_c)),
        temp_c: milli_c(report.temperature_milli_c),
        max_temp_time_utc: millis(report.max_temperature_at_unix_ms).ok().flatten(),
        min_temp_time_utc: millis(report.min_temperature_at_unix_ms).ok().flatten(),
        meta: ComponentMeta::default(),
        origin: ValueOrigin::Derived,
        supplied: None,
    }
}

pub(super) fn derived_extreme(
    extreme: &ExtremeV4,
    station_id: &str,
    kind: ExtremeKindV6,
    event_date: &str,
) -> Result<Extreme, KernelTransactionError> {
    let value_c = f64::from(extreme.value_milli_c) / 1_000.0;
    Ok(Extreme {
        provenance: derived_provenance(&extreme.provenance, station_id),
        station_id: station_id.to_owned(),
        kind,
        event_key: extreme
            .temperature_day_date
            .clone()
            .unwrap_or_else(|| event_date.to_owned()),
        value_f: value_c * 9.0 / 5.0 + 32.0,
        value_c,
        prev_value_f: milli_c_to_f(extreme.previous_value_milli_c),
        observed_at: millis(extreme.observed_at_unix_ms)?,
        temperature_day_mode: extreme.temperature_day_mode.clone(),
        temperature_day_date: extreme.temperature_day_date.clone(),
        is_from_report: extreme.is_from_report,
        report_type: extreme.report_type.clone(),
        source_report_id: extreme.source_report_id.clone(),
        origin: ValueOrigin::Derived,
        supplied: None,
    })
}

pub(super) fn derived_weather_event(
    event: &WeatherEventV4,
    station_id: &str,
) -> Result<WeatherEvent, KernelTransactionError> {
    Ok(WeatherEvent {
        provenance: derived_provenance(&event.provenance, station_id),
        station_id: station_id.to_owned(),
        id: event.event_id.clone(),
        event_type: event.event_type.clone(),
        tier: event.tier.clone(),
        state: event.state.clone(),
        name: event.name.clone(),
        badge: event.badge.clone(),
        detail: event.detail.clone(),
        summary: event.summary.clone(),
        started_at: millis(event.started_at_unix_ms)?,
        last_confirmed_at: millis(event.last_confirmed_at_unix_ms)?,
        ended_at: millis(event.ended_at_unix_ms)?,
        source: event.source.as_ref().map(|source| WeatherEventSource {
            metar_type: source.metar_type.clone(),
            flight_category: source.flight_category.clone(),
            wx_string: source.wx_string.clone(),
            wx_token: source.wx_token.clone(),
            wind_speed_kt: micros(source.wind_speed_knots_micros),
            wind_gust_kt: micros(source.wind_gust_knots_micros),
            peak_wind_kt: micros(source.peak_wind_knots_micros),
            peak_wind_direction: source.peak_wind_direction,
            visibility_mi: micros(source.visibility_miles_micros),
            cb_location: source.cb_location.clone(),
        }),
        meta: ComponentMeta::default(),
        origin: ValueOrigin::Derived,
        supplied: None,
    })
}

pub(super) fn derived_forecast(
    station: &StationV4,
    station_id: &str,
) -> Result<Forecast, KernelTransactionError> {
    Ok(Forecast {
        station_id: station_id.to_owned(),
        source: "minutetemp".to_owned(),
        received_at: millis(station.forecast_meta.updated_at_unix_ms)?,
        advertised_versions: station.forecast.advertised_versions.clone(),
        models: station
            .forecast
            .models
            .iter()
            .map(derived_forecast_model)
            .collect::<Result<_, _>>()?,
        meta: ComponentMeta::default(),
        origin: ValueOrigin::Derived,
        supplied: None,
    })
}

pub(super) fn derived_forecast_model(
    model: &ForecastModelV4,
) -> Result<ForecastModel, KernelTransactionError> {
    Ok(ForecastModel {
        id: model.model_id.clone(),
        version: model.version.clone(),
        run_id: model.run_id.clone(),
        fetched_at: millis(model.fetched_at_unix_ms)?,
        issued_at: millis(model.issued_at_unix_ms)?,
        issuance: None,
        timezone: model.timezone.clone(),
        utc_offset_seconds: model.utc_offset_seconds.map(i64::from),
        hourly: model
            .hourly
            .iter()
            .map(|point| {
                let at = millis(Some(point.at_unix_ms))?;
                Ok(ForecastPoint {
                    time: at.map(|value| value.to_rfc3339()).unwrap_or_default(),
                    at,
                    temperature_f: milli_c_to_f(point.temperature_milli_c),
                    temperature_c: milli_c(point.temperature_milli_c),
                    apparent_f: milli_c_to_f(point.apparent_temperature_milli_c),
                    apparent_c: milli_c(point.apparent_temperature_milli_c),
                    humidity: millionths(point.humidity_millionths),
                    dew_point: milli_c(point.dew_point_milli_c),
                    pressure: micros(point.pressure_msl_micros),
                    wind_speed: micros(point.wind_speed_micros),
                    wind_direction: micros(point.wind_direction_micros),
                    wind_gust: micros(point.wind_gust_micros),
                    cloud_cover: millionths(point.cloud_cover_millionths),
                    precipitation: millionths(point.precipitation_probability_millionths),
                    weather_code: point.weather_code.map(i64::from),
                })
            })
            .collect::<Result<_, KernelTransactionError>>()?,
        origin: ValueOrigin::Derived,
        supplied: None,
    })
}

pub(super) fn derived_oracle(
    oracle: &crate::decision_v4::OracleTableV4,
) -> Result<OracleTable, KernelTransactionError> {
    Ok(OracleTable {
        station_id: oracle.query.station_id.clone(),
        source: "minutetemp".to_owned(),
        received_at: millis(oracle.updated_at_unix_ms)?,
        mode: oracle.query.mode.clone(),
        rank_by: match oracle.query.rank_by {
            RankByV4::High => "high",
            RankByV4::Low => "low",
        }
        .to_owned(),
        days: oracle.query.days.to_string(),
        all_time: None,
        range_start: oracle.range_start.clone(),
        range_end: oracle.range_end.clone(),
        updated_at: millis(oracle.updated_at_unix_ms)?,
        modes: vec![oracle.query.mode.clone()],
        scores: oracle
            .rows
            .iter()
            .map(|row| OracleScore {
                rank: Some(row.rank),
                model_id: row.model_id.clone(),
                model_name: row.model_name.clone(),
                is_public: row.is_public,
                high_mae: millionths(row.high_mae_millionths),
                low_mae: millionths(row.low_mae_millionths),
                combined_mae: millionths(row.combined_mae_millionths),
                high_bias: millionths(row.high_bias_millionths),
                low_bias: millionths(row.low_bias_millionths),
                day_count: row.day_count.map(i64::from),
            })
            .collect(),
        meta: ComponentMeta::default(),
        origin: ValueOrigin::Derived,
        supplied: None,
    })
}

pub(super) fn market_state(
    market: &MarketV4,
    event_date: &str,
    context: &DecisionContextV6,
) -> Result<MarketState, KernelTransactionError> {
    let cap_strike_milli_f = context
        .market_strikes
        .as_ref()
        .and_then(|strikes| {
            strikes
                .iter()
                .find(|strike| strike.market_id == market.identity.market_id)
        })
        .and_then(|strike| strike.cap_strike_milli_f);
    let ticker = market.ticker.as_ref();
    let quantity = |value: Option<u64>| value.map(hundredths_quantity);
    let mut state = MarketState {
        market_id: market.identity.market_id.clone(),
        opportunity_id: market.identity.opportunity_id.clone(),
        venue: market.identity.venue.clone(),
        source: "kalshi".to_owned(),
        event_ticker: market.identity.event_ticker.clone(),
        series_ticker: market.identity.series_ticker.clone(),
        event_date: event_date.to_owned(),
        strike_type: market.identity.strike_type.clone(),
        fee_type: market.identity.fee_type.clone(),
        fee_multiplier: market
            .identity
            .fee_multiplier_millionths
            .map(|value| value as f64 / 1_000_000.0),
        fee_multiplier_millionths: market.identity.fee_multiplier_millionths,
        floor_strike: market
            .identity
            .floor_strike_milli_f
            .map(|value| value as f64 / 1_000.0)
            .or_else(|| {
                market
                    .identity
                    .floor_strike_milli_c
                    .map(|value| value as f64 / 1_000.0 * 9.0 / 5.0 + 32.0)
            }),
        cap_strike: cap_strike_milli_f
            .map(|value| value as f64 / 1_000.0)
            .or_else(|| {
                market
                    .identity
                    .cap_strike_milli_c
                    .map(|value| value as f64 / 1_000.0 * 9.0 / 5.0 + 32.0)
            }),
        cap_strike_milli_f,
        floor_strike_milli_c: market.identity.floor_strike_milli_c,
        floor_strike_milli_f: market.identity.floor_strike_milli_f,
        cap_strike_milli_c: market.identity.cap_strike_milli_c,
        close_time: millis(market.identity.close_at_unix_ms)?,
        expiration_time: millis(market.identity.expiration_at_unix_ms)?,
        components: MarketComponents {
            lifecycle: market_meta(&market.lifecycle_meta)?,
            ticker: market_meta(&market.ticker_meta)?,
            book: market_meta(&market.book_meta)?,
            last_trade: market_meta(&market.last_trade_meta)?,
            final_fact: market_meta(&market.final_fact_meta)?,
        },
        lifecycle: market
            .lifecycle
            .as_ref()
            .map(|lifecycle| {
                Ok::<_, KernelTransactionError>(MarketLifecycle {
                    status: lifecycle.status.clone(),
                    result: lifecycle.result.clone(),
                    connection_epoch: lifecycle.connection_epoch,
                    received_frame_ordinal: lifecycle.received_frame_ordinal,
                    open_at: millis(lifecycle.open_at_unix_ms)?,
                    close_at: millis(lifecycle.close_at_unix_ms)?,
                    settled_at: millis(lifecycle.settled_at_unix_ms)?,
                    updated_at: millis(lifecycle.updated_at_unix_ms)?,
                })
            })
            .transpose()?,
        ticker: ticker
            .map(|ticker| {
                Ok::<_, KernelTransactionError>(TickerQuote {
                    yes_bid: ticker.yes_bid_micros.map(price),
                    yes_ask: ticker.yes_ask_micros.map(price),
                    no_bid: ticker.no_bid_micros.map(price),
                    no_ask: ticker.no_ask_micros.map(price),
                    yes_bid_quantity: quantity(ticker.yes_bid_quantity_hundredths),
                    yes_ask_quantity: quantity(ticker.yes_ask_quantity_hundredths),
                    no_bid_quantity: quantity(ticker.no_bid_quantity_hundredths),
                    no_ask_quantity: quantity(ticker.no_ask_quantity_hundredths),
                    last_price: ticker.last_price_micros.map(price),
                    last_trade_quantity: quantity(ticker.last_trade_quantity_hundredths),
                    volume: quantity(ticker.volume_hundredths),
                    volume_24h: quantity(ticker.volume_24h_hundredths),
                    open_interest: quantity(ticker.open_interest_hundredths),
                    provider_at: millis(ticker.provider_at_unix_ms)?,
                })
            })
            .transpose()?,
        book: market
            .book
            .as_ref()
            .map(|book| {
                let levels = |levels: &[BookLevelV4]| {
                    levels
                        .iter()
                        .map(|level| BookLevel {
                            price: price(level.price_micros),
                            quantity: hundredths_quantity(level.quantity_hundredths),
                        })
                        .collect::<Vec<_>>()
                };
                Ok::<_, KernelTransactionError>(Book {
                    connection_epoch: book.connection_epoch,
                    sid: book.sid,
                    sequence: book.sequence,
                    snapshot_at: millis(book.snapshot_at_unix_ms)?,
                    resync_required: book.resync_required,
                    yes_bids: levels(&book.yes_bids),
                    no_bids: levels(&book.no_bids),
                })
            })
            .transpose()?,
        last_trade: market
            .last_trade
            .as_ref()
            .map(|trade| {
                Ok::<_, KernelTransactionError>(LastTrade {
                    trade_id: trade.trade_id.clone(),
                    yes_price: price(trade.yes_price_micros),
                    no_price: price(trade.no_price_micros),
                    quantity: hundredths_quantity(trade.quantity_hundredths),
                    taker_side: trade.taker_side.clone(),
                    traded_at: millis(Some(trade.traded_at_unix_ms))?,
                })
            })
            .transpose()?,
        final_fact: market
            .final_fact
            .as_ref()
            .map(|fact| {
                Ok::<_, KernelTransactionError>(FinalFact {
                    status: fact.status.clone(),
                    result: fact.result.clone(),
                    settlement_value: micros(fact.settlement_value_micros),
                    settled_price: fact.settled_price_micros.map(price),
                    provider_at: millis(fact.provider_at_unix_ms)?,
                })
            })
            .transpose()?,
        uncertain_fields: market.uncertain_fields.clone(),
        peak_yes_ask: None,
        last_update: ticker
            .and_then(|value| value.provider_at_unix_ms)
            .and_then(|value| millis(Some(value)).ok().flatten()),
        yes_bid_levels: Vec::new(),
        yes_ask_levels: Vec::new(),
        no_bid_levels: Vec::new(),
        no_ask_levels: Vec::new(),
    };
    state.derive_levels();
    Ok(state)
}

pub(super) fn hundredths_quantity(value: u64) -> ContractQuantity {
    ContractQuantity::from_hundredths(i64::try_from(value).unwrap_or(i64::MAX))
}
pub(super) fn millis(value: Option<i64>) -> Result<Option<DateTime<Utc>>, KernelTransactionError> {
    value
        .map(|value| {
            Utc.timestamp_millis_opt(value)
                .single()
                .ok_or(KernelTransactionError::InvalidTime)
        })
        .transpose()
}
pub(super) fn milli_c(value: Option<i32>) -> Option<f64> {
    value.map(|value| f64::from(value) / 1_000.0)
}
pub(super) fn milli_c_to_f(value: Option<i32>) -> Option<f64> {
    milli_c(value).map(|value| value * 9.0 / 5.0 + 32.0)
}
pub(super) fn micros(value: Option<i64>) -> Option<f64> {
    value.map(|value| value as f64 / 1_000_000.0)
}
pub(super) fn millionths(value: Option<i64>) -> Option<f64> {
    micros(value)
}
pub(super) fn price(value: u64) -> f64 {
    value as f64 / 1_000_000.0
}
pub(super) fn price_micros(value: f64) -> Result<u64, KernelTransactionError> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(KernelTransactionError::InvalidQuantity);
    }
    Ok((value * 1_000_000.0).round() as u64)
}
