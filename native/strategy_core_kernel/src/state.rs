//! The canonical owned strategy-facing model: complete scoped station and market state.
//!
//! Hosts build these values from their own authority (Trader from the durable Decision Context,
//! Backtester from replay records) and kernels read them through [`crate::StrategyKernelState`].
//! Every component keeps its convenience `f64` fields beside the supplied original it was
//! projected from and an explicit [`ValueOrigin`]; component authority, revisions and update
//! times travel with the component. Derived summaries (`StationState::weather`, whole-contract
//! depths, `ForecastModel::value`) are named as such and never replace the originals.

use std::borrow::Cow;

use chrono::{DateTime, TimeZone, Utc};

use crate::actions::ContractQuantity;
use crate::decimal::Decimal;
use crate::events::{
    EventProvenanceView, ForecastHourlySnapshot, ForecastInputSnapshot, ForecastModelSnapshot,
    HighLowView, MarketBracketView, ObservationView, OracleInputSnapshot, OracleModelScoreSnapshot,
    PriceLevelView, StationReportView, StationWeatherView, TickerPriceView, ValueOrigin,
    WeatherEventSourceView, WeatherEventView,
};
use crate::supplied::{
    EventEnvelope, ExtremeKind, SuppliedDailyExtremes, SuppliedExtreme, SuppliedForecast,
    SuppliedForecastModel, SuppliedForecastPoint, SuppliedObservation, SuppliedOracleScore,
    SuppliedOracleTable, SuppliedReport, SuppliedWeatherEvent, SuppliedWeatherEventSource,
};

/// Host authority of one state component at delivery time.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ComponentAuthority {
    Warming,
    #[default]
    Current,
    RefreshPending,
    Uncertain,
    Unavailable,
}

/// Authority, revision and freshness of one state component.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ComponentMeta {
    pub authority: ComponentAuthority,
    pub revision: u64,
    pub generation: u64,
    pub updated_at: Option<DateTime<Utc>>,
    pub expected_version: Option<String>,
    pub refresh_error: Option<String>,
}

/// Provider event identity and producer metadata of one fact.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EventProvenance {
    pub provider: String,
    pub source: String,
    pub event_id: Option<String>,
    pub sequence: Option<i64>,
    pub city_sequence: Option<i64>,
    pub producer_sequence: Option<i64>,
    /// The provider's publication time; never the decision clock.
    pub emitted_at: Option<DateTime<Utc>>,
    pub slug: String,
    pub event_key: Option<String>,
    pub source_timestamp: Option<DateTime<Utc>>,
    pub wmo_emit_time: Option<DateTime<Utc>>,
    pub producer_received_at: Option<DateTime<Utc>>,
    pub live_published_at: Option<DateTime<Utc>>,
    pub persistence_status: Option<String>,
    /// Host receipt time; host evidence, never provider data.
    pub received_at: Option<DateTime<Utc>>,
    pub connection_epoch: Option<u64>,
    pub sid: Option<u64>,
    pub received_frame_ordinal: Option<u64>,
}

impl EventProvenance {
    /// Provenance of a supplied stream event; `station_id` stands in for an absent slug.
    pub fn from_envelope(envelope: Option<&EventEnvelope>, source: &str, station_id: &str) -> Self {
        let Some(envelope) = envelope else {
            return Self {
                source: source.to_owned(),
                slug: station_id.to_owned(),
                ..Self::default()
            };
        };
        Self {
            provider: String::new(),
            source: source.to_owned(),
            event_id: Some(envelope.event_id.clone()),
            sequence: i64::try_from(envelope.sequence).ok(),
            city_sequence: envelope
                .city_sequence
                .and_then(|value| i64::try_from(value).ok()),
            producer_sequence: envelope
                .producer_sequence
                .and_then(|value| i64::try_from(value).ok()),
            emitted_at: Some(nanos(envelope.emitted_at_unix_ns)),
            slug: envelope
                .slug
                .clone()
                .unwrap_or_else(|| station_id.to_owned()),
            event_key: envelope.event_key.clone(),
            source_timestamp: envelope.source_timestamp_unix_ns.map(nanos),
            wmo_emit_time: envelope.wmo_emit_time_unix_ns.map(nanos),
            producer_received_at: envelope.producer_received_at_unix_ns.map(nanos),
            live_published_at: envelope.live_published_at_unix_ns.map(nanos),
            persistence_status: envelope.persistence_status.clone(),
            received_at: Some(nanos(envelope.received_at_unix_ns)),
            connection_epoch: None,
            sid: None,
            received_frame_ordinal: None,
        }
    }

    pub fn view(&self) -> EventProvenanceView<'_> {
        EventProvenanceView {
            provider: &self.provider,
            source: &self.source,
            event_key: self.event_key.as_deref(),
            source_timestamp: self.source_timestamp,
            wmo_emit_time: self.wmo_emit_time,
            producer_received_at: self.producer_received_at,
            live_published_at: self.live_published_at,
            persistence_status: self.persistence_status.as_deref(),
            producer_sequence: self.producer_sequence,
            received_at: self.received_at,
            connection_epoch: self.connection_epoch,
            sid: self.sid,
            received_frame_ordinal: self.received_frame_ordinal,
        }
    }
}

/// One station observation: the current state component and the `observation` event.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Observation {
    pub provenance: EventProvenance,
    pub station_id: String,
    pub observed_at: Option<DateTime<Utc>>,
    pub lag_seconds: Option<i64>,
    pub preliminary: bool,
    pub temperature_f: Option<f64>,
    pub temperature_c: Option<f64>,
    pub temp_min_f: Option<f64>,
    pub temp_max_f: Option<f64>,
    pub temp_min_c: Option<f64>,
    pub temp_max_c: Option<f64>,
    pub is_from_report: bool,
    pub report_type: Option<String>,
    pub source_report_id: Option<String>,
    pub temperature_day_mode: Option<String>,
    pub temperature_day_date: Option<String>,
    pub dewpoint: Option<f64>,
    pub heat_index: Option<f64>,
    pub wind_chill: Option<f64>,
    pub relative_humidity: Option<f64>,
    pub wind_speed: Option<f64>,
    pub wind_direction: Option<f64>,
    pub wind_gust: Option<f64>,
    pub text_description: Option<String>,
    pub barometric_pressure: Option<f64>,
    pub sea_level_pressure: Option<f64>,
    pub precipitation_1h: Option<f64>,
    pub precipitation_3h: Option<f64>,
    pub precipitation_6h: Option<f64>,
    pub is_locf: Option<bool>,
    pub origin: ValueOrigin,
    pub supplied: Option<SuppliedObservation>,
}

impl Observation {
    /// Projects a supplied original: each unit is presented only when supplied; no unit is
    /// converted from the other.
    pub fn from_supplied(event: &SuppliedObservation) -> Self {
        Self {
            provenance: EventProvenance::from_envelope(
                event.envelope.as_ref(),
                &event.source,
                &event.station_id,
            ),
            station_id: event.station_id.clone(),
            observed_at: Some(nanos(event.observed_at_unix_ns)),
            lag_seconds: event.lag_seconds,
            preliminary: event.preliminary,
            temperature_f: f64_of(event.temperature_f),
            temperature_c: f64_of(event.temperature_c),
            temp_min_f: f64_of(event.temp_min_f),
            temp_max_f: f64_of(event.temp_max_f),
            temp_min_c: f64_of(event.temp_min_c),
            temp_max_c: f64_of(event.temp_max_c),
            is_from_report: event.is_from_report,
            report_type: event.report_type.clone(),
            source_report_id: event.source_report_id.clone(),
            temperature_day_mode: event.temperature_day_mode.clone(),
            temperature_day_date: event.temperature_day_date.clone(),
            dewpoint: f64_of(event.dewpoint),
            heat_index: f64_of(event.heat_index),
            wind_chill: f64_of(event.wind_chill),
            relative_humidity: f64_of(event.relative_humidity),
            wind_speed: f64_of(event.wind_speed),
            wind_direction: f64_of(event.wind_direction),
            wind_gust: f64_of(event.wind_gust),
            text_description: event.text_description.clone(),
            barometric_pressure: f64_of(event.barometric_pressure),
            sea_level_pressure: f64_of(event.sea_level_pressure),
            precipitation_1h: f64_of(event.precipitation_1h),
            precipitation_3h: f64_of(event.precipitation_3h),
            precipitation_6h: f64_of(event.precipitation_6h),
            is_locf: event.is_locf,
            origin: ValueOrigin::Supplied,
            supplied: Some(event.clone()),
        }
    }

    pub fn view(&self) -> ObservationView<'_> {
        ObservationView {
            event_id: self.provenance.event_id.as_deref(),
            sequence: self.provenance.sequence,
            city_sequence: self.provenance.city_sequence,
            emitted_at: self.provenance.emitted_at,
            slug: &self.provenance.slug,
            station_id: &self.station_id,
            observed_at: self.observed_at,
            lag_seconds: self.lag_seconds,
            preliminary: self.preliminary,
            temperature_f: self.temperature_f,
            temperature_c: self.temperature_c,
            temp_min_f: self.temp_min_f,
            temp_max_f: self.temp_max_f,
            temp_min_c: self.temp_min_c,
            temp_max_c: self.temp_max_c,
            is_from_report: self.is_from_report,
            report_type: self.report_type.as_deref(),
            source_report_id: self.source_report_id.as_deref(),
            temperature_day_mode: self.temperature_day_mode.as_deref(),
            temperature_day_date: self.temperature_day_date.as_deref(),
            dewpoint: self.dewpoint,
            heat_index: self.heat_index,
            wind_chill: self.wind_chill,
            relative_humidity: self.relative_humidity,
            wind_speed: self.wind_speed,
            wind_direction: self.wind_direction,
            wind_gust: self.wind_gust,
            text_description: self.text_description.as_deref(),
            barometric_pressure: self.barometric_pressure,
            sea_level_pressure: self.sea_level_pressure,
            precipitation_1h: self.precipitation_1h,
            precipitation_3h: self.precipitation_3h,
            precipitation_6h: self.precipitation_6h,
            is_locf: self.is_locf,
            provenance: self.provenance.view(),
            origin: self.origin,
            supplied: self.supplied.as_ref(),
        }
    }
}

/// REST daily-extremes evidence captured at `received_at`; retained separately from the
/// current summaries it once seeded.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DailyExtremes {
    pub source: String,
    pub received_at: Option<DateTime<Utc>>,
    pub daily_high_f: Option<f64>,
    pub daily_low_f: Option<f64>,
    pub daily_high_c: Option<f64>,
    pub daily_low_c: Option<f64>,
    pub asos_daily_high_f: Option<f64>,
    pub asos_daily_low_f: Option<f64>,
    pub asos_daily_high_c: Option<f64>,
    pub asos_daily_low_c: Option<f64>,
    pub temperature_day_mode: Option<String>,
    pub temperature_day_date: Option<String>,
    pub temperature_unit: Option<String>,
    pub uses_nws_climate_day: Option<bool>,
    pub supplied: SuppliedDailyExtremes,
}

impl DailyExtremes {
    pub fn from_supplied(daily: &SuppliedDailyExtremes) -> Self {
        Self {
            source: daily.source.clone(),
            received_at: Some(nanos(daily.received_at_unix_ns)),
            daily_high_f: f64_of(daily.daily_high_f),
            daily_low_f: f64_of(daily.daily_low_f),
            daily_high_c: f64_of(daily.daily_high_c),
            daily_low_c: f64_of(daily.daily_low_c),
            asos_daily_high_f: f64_of(daily.asos_daily_high_f),
            asos_daily_low_f: f64_of(daily.asos_daily_low_f),
            asos_daily_high_c: f64_of(daily.asos_daily_high_c),
            asos_daily_low_c: f64_of(daily.asos_daily_low_c),
            temperature_day_mode: daily.temperature_day_mode.clone(),
            temperature_day_date: daily.temperature_day_date.clone(),
            temperature_unit: daily.temperature_unit.clone(),
            uses_nws_climate_day: daily.uses_nws_climate_day,
            supplied: daily.clone(),
        }
    }
}

/// One station report: a current state component and the `station_report` event.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub provenance: EventProvenance,
    pub station_id: String,
    pub report_id: String,
    pub report_type: String,
    pub report_date: String,
    /// Provider revision; `0` when the provider supplied none.
    pub report_revision: i64,
    pub report_fingerprint: Option<String>,
    pub report_updated_at: Option<DateTime<Utc>>,
    pub issuance_time: Option<DateTime<Utc>>,
    pub fetched_at: Option<DateTime<Utc>>,
    pub source_url: String,
    pub provider: String,
    pub max_temp_f: Option<f64>,
    pub max_temp_c: Option<f64>,
    pub min_temp_f: Option<f64>,
    pub min_temp_c: Option<f64>,
    pub temp_f: Option<f64>,
    pub temp_c: Option<f64>,
    pub max_temp_time_utc: Option<DateTime<Utc>>,
    pub min_temp_time_utc: Option<DateTime<Utc>>,
    pub meta: ComponentMeta,
    pub origin: ValueOrigin,
    pub supplied: Option<SuppliedReport>,
}

impl Report {
    pub fn from_supplied(event: &SuppliedReport) -> Self {
        Self {
            provenance: EventProvenance::from_envelope(
                event.envelope.as_ref(),
                &event.source,
                &event.station_id,
            ),
            station_id: event.station_id.clone(),
            report_id: event.report_id.clone(),
            report_type: event.report_type.clone(),
            report_date: event.report_date.clone(),
            report_revision: event
                .report_revision
                .and_then(|value| i64::try_from(value).ok())
                .unwrap_or(0),
            report_fingerprint: event.report_fingerprint.clone(),
            report_updated_at: event.report_updated_at_unix_ns.map(nanos),
            issuance_time: event.issuance_time_unix_ns.map(nanos),
            fetched_at: event.fetched_at_unix_ns.map(nanos),
            source_url: event.source_url.clone().unwrap_or_default(),
            provider: event.provider.clone().unwrap_or_default(),
            max_temp_f: f64_of(event.max_temp_f),
            max_temp_c: f64_of(event.max_temp_c),
            min_temp_f: f64_of(event.min_temp_f),
            min_temp_c: f64_of(event.min_temp_c),
            temp_f: f64_of(event.temp_f),
            temp_c: f64_of(event.temp_c),
            max_temp_time_utc: event.max_temp_time_unix_ns.map(nanos),
            min_temp_time_utc: event.min_temp_time_unix_ns.map(nanos),
            meta: ComponentMeta::default(),
            origin: ValueOrigin::Supplied,
            supplied: Some(event.clone()),
        }
    }

    pub fn view(&self) -> StationReportView<'_> {
        StationReportView {
            event_id: self.provenance.event_id.as_deref(),
            sequence: self.provenance.sequence,
            city_sequence: self.provenance.city_sequence,
            emitted_at: self.provenance.emitted_at,
            slug: &self.provenance.slug,
            station_id: &self.station_id,
            report_id: &self.report_id,
            report_type: &self.report_type,
            report_date: &self.report_date,
            report_revision: self.report_revision,
            report_updated_at: self.report_updated_at,
            issuance_time: self.issuance_time,
            fetched_at: self.fetched_at,
            source_url: &self.source_url,
            provider: &self.provider,
            max_temp_f: self.max_temp_f,
            max_temp_c: self.max_temp_c,
            min_temp_f: self.min_temp_f,
            min_temp_c: self.min_temp_c,
            temp_f: self.temp_f,
            temp_c: self.temp_c,
            max_temp_time_utc: self.max_temp_time_utc,
            min_temp_time_utc: self.min_temp_time_utc,
            report_fingerprint: self.report_fingerprint.as_deref(),
            provenance: self.provenance.view(),
            origin: self.origin,
            supplied: self.supplied.as_ref(),
        }
    }
}

/// A daily extreme: a current state component and the `new_high` / `new_low` event.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Extreme {
    pub provenance: EventProvenance,
    pub station_id: String,
    pub kind: ExtremeKind,
    pub event_key: String,
    /// Each unit is the supplied original; only an absent unit is converted from the other.
    pub value_f: f64,
    pub value_c: f64,
    pub prev_value_f: Option<f64>,
    pub observed_at: Option<DateTime<Utc>>,
    pub temperature_day_mode: Option<String>,
    pub temperature_day_date: Option<String>,
    pub is_from_report: bool,
    pub report_type: Option<String>,
    pub source_report_id: Option<String>,
    pub origin: ValueOrigin,
    pub supplied: Option<SuppliedExtreme>,
}

impl Extreme {
    pub fn from_supplied(event: &SuppliedExtreme, event_date: &str) -> Self {
        let value_f = f64_of(event.value_f);
        let value_c = f64_of(event.value_c);
        Self {
            provenance: EventProvenance::from_envelope(
                event.envelope.as_ref(),
                &event.source,
                &event.station_id,
            ),
            station_id: event.station_id.clone(),
            kind: event.kind,
            event_key: event
                .temperature_day_date
                .clone()
                .unwrap_or_else(|| event_date.to_owned()),
            value_f: value_f
                .or_else(|| value_c.map(celsius_to_fahrenheit))
                .unwrap_or(f64::NAN),
            value_c: value_c
                .or_else(|| value_f.map(fahrenheit_to_celsius))
                .unwrap_or(f64::NAN),
            prev_value_f: f64_of(event.prev_value_f),
            observed_at: event.observed_at_unix_ns.map(nanos),
            temperature_day_mode: event.temperature_day_mode.clone(),
            temperature_day_date: event.temperature_day_date.clone(),
            is_from_report: event.is_from_report,
            report_type: event.report_type.clone(),
            source_report_id: event.source_report_id.clone(),
            origin: ValueOrigin::Supplied,
            supplied: Some(event.clone()),
        }
    }

    pub fn view(&self) -> HighLowView<'_> {
        HighLowView {
            event_id: self.provenance.event_id.as_deref(),
            sequence: self.provenance.sequence,
            city_sequence: self.provenance.city_sequence,
            emitted_at: self.provenance.emitted_at,
            event_key: &self.event_key,
            source_timestamp: self.provenance.source_timestamp.or(self.observed_at),
            wmo_emit_time: self.provenance.wmo_emit_time,
            producer_received_at: self.provenance.producer_received_at,
            live_published_at: self.provenance.live_published_at,
            persistence_status: self.provenance.persistence_status.as_deref(),
            producer_sequence: self.provenance.producer_sequence,
            slug: &self.provenance.slug,
            station_id: &self.station_id,
            value_f: self.value_f,
            value_c: self.value_c,
            prev_value_f: self.prev_value_f,
            observed_at: self.observed_at,
            temperature_day_mode: self.temperature_day_mode.as_deref(),
            temperature_day_date: self.temperature_day_date.as_deref(),
            is_from_report: self.is_from_report,
            report_type: self.report_type.as_deref(),
            source_report_id: self.source_report_id.as_deref(),
            provenance: self.provenance.view(),
            origin: self.origin,
            supplied: self.supplied.as_ref(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
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

impl WeatherEventSource {
    pub fn from_supplied(source: &SuppliedWeatherEventSource) -> Self {
        Self {
            metar_type: source.metar_type.clone(),
            flight_category: source.flight_category.clone(),
            wx_string: source.wx_string.clone(),
            wx_token: source.wx_token.clone(),
            wind_speed_kt: f64_of(source.wind_speed_kt),
            wind_gust_kt: f64_of(source.wind_gust_kt),
            peak_wind_kt: f64_of(source.peak_wind_kt),
            peak_wind_direction: source.peak_wind_direction,
            visibility_mi: f64_of(source.visibility_mi),
            cb_location: source.cb_location.clone(),
        }
    }

    pub fn view(&self) -> WeatherEventSourceView<'_> {
        WeatherEventSourceView {
            metar_type: self.metar_type.as_deref(),
            flight_category: self.flight_category.as_deref(),
            wx_string: self.wx_string.as_deref(),
            wx_token: self.wx_token.as_deref(),
            wind_speed_kt: self.wind_speed_kt,
            wind_gust_kt: self.wind_gust_kt,
            peak_wind_kt: self.peak_wind_kt,
            peak_wind_direction: self.peak_wind_direction,
            visibility_mi: self.visibility_mi,
            cb_location: self.cb_location.as_deref(),
        }
    }
}

/// One weather episode transition: a current state component and the `weather_event` event.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WeatherEvent {
    pub provenance: EventProvenance,
    pub station_id: String,
    /// The provider's stable episode identifier.
    pub id: String,
    pub event_type: String,
    pub tier: String,
    pub state: String,
    pub name: String,
    pub badge: String,
    pub detail: String,
    pub summary: String,
    pub started_at: Option<DateTime<Utc>>,
    pub last_confirmed_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub source: Option<WeatherEventSource>,
    pub meta: ComponentMeta,
    pub origin: ValueOrigin,
    pub supplied: Option<SuppliedWeatherEvent>,
}

impl WeatherEvent {
    pub fn from_supplied(event: &SuppliedWeatherEvent) -> Self {
        Self {
            provenance: EventProvenance::from_envelope(
                event.envelope.as_ref(),
                &event.source,
                &event.station_id,
            ),
            station_id: event.station_id.clone(),
            id: event.episode_id.clone(),
            event_type: event.event_type.clone(),
            tier: event.tier.clone(),
            state: event.state.clone(),
            name: event.name.clone(),
            badge: event.badge.clone().unwrap_or_default(),
            detail: event.detail.clone().unwrap_or_default(),
            summary: event.summary.clone().unwrap_or_default(),
            started_at: event.started_at_unix_ns.map(nanos),
            last_confirmed_at: event.last_confirmed_at_unix_ns.map(nanos),
            ended_at: event.ended_at_unix_ns.map(nanos),
            source: event
                .source_snapshot
                .as_ref()
                .map(WeatherEventSource::from_supplied),
            meta: ComponentMeta::default(),
            origin: ValueOrigin::Supplied,
            supplied: Some(event.clone()),
        }
    }

    pub fn view(&self) -> WeatherEventView<'_> {
        WeatherEventView {
            event_id: self.provenance.event_id.as_deref(),
            sequence: self.provenance.sequence,
            city_sequence: self.provenance.city_sequence,
            emitted_at: self.provenance.emitted_at,
            slug: &self.provenance.slug,
            station_id: &self.station_id,
            id: &self.id,
            event_type_name: &self.event_type,
            tier: &self.tier,
            state: &self.state,
            name: &self.name,
            badge: &self.badge,
            detail: &self.detail,
            summary: &self.summary,
            started_at: self.started_at,
            last_confirmed_at: self.last_confirmed_at,
            ended_at: self.ended_at,
            source: self.source.as_ref().map(WeatherEventSource::view),
            provenance: self.provenance.view(),
            origin: self.origin,
            supplied: self.supplied.as_ref(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ForecastPoint {
    pub time: String,
    pub at: Option<DateTime<Utc>>,
    pub temperature_f: Option<f64>,
    pub temperature_c: Option<f64>,
    pub apparent_f: Option<f64>,
    pub apparent_c: Option<f64>,
    pub humidity: Option<f64>,
    pub dew_point: Option<f64>,
    pub pressure: Option<f64>,
    pub wind_speed: Option<f64>,
    pub wind_direction: Option<f64>,
    pub wind_gust: Option<f64>,
    pub cloud_cover: Option<f64>,
    pub precipitation: Option<f64>,
    pub weather_code: Option<i64>,
}

impl ForecastPoint {
    pub fn from_supplied(point: &SuppliedForecastPoint) -> Self {
        Self {
            time: point.time.clone(),
            at: Some(nanos(point.time_unix_ns)),
            temperature_f: f64_of(point.temperature_2m_f),
            temperature_c: f64_of(point.temperature_2m_c),
            apparent_f: f64_of(point.apparent_temperature_f),
            apparent_c: f64_of(point.apparent_temperature_c),
            humidity: f64_of(point.relative_humidity_2m),
            dew_point: f64_of(point.dew_point_2m),
            pressure: f64_of(point.pressure_msl),
            wind_speed: f64_of(point.wind_speed_10m),
            wind_direction: f64_of(point.wind_direction_10m),
            wind_gust: f64_of(point.wind_gusts_10m),
            cloud_cover: f64_of(point.cloud_cover),
            precipitation: f64_of(point.precipitation_probability),
            weather_code: point.weather_code,
        }
    }

    pub fn snapshot(&self) -> ForecastHourlySnapshot<'_> {
        ForecastHourlySnapshot {
            time: &self.time,
            temperature_2m_f: self.temperature_f,
            temperature_2m_c: self.temperature_c,
            apparent_temperature_f: self.apparent_f,
            apparent_temperature_c: self.apparent_c,
            relative_humidity_2m: self.humidity,
            dew_point_2m: self.dew_point,
            pressure_msl: self.pressure,
            wind_speed_10m: self.wind_speed,
            wind_direction_10m: self.wind_direction,
            wind_gusts_10m: self.wind_gust,
            cloud_cover: self.cloud_cover,
            precipitation_probability: self.precipitation,
            weather_code: self.weather_code,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ForecastModel {
    pub id: String,
    /// The version the provider advertised for this model.
    pub version: String,
    pub run_id: Option<String>,
    pub fetched_at: Option<DateTime<Utc>>,
    pub issued_at: Option<DateTime<Utc>>,
    pub timezone: Option<String>,
    pub utc_offset_seconds: Option<i64>,
    pub hourly: Vec<ForecastPoint>,
    pub origin: ValueOrigin,
    pub supplied: Option<SuppliedForecastModel>,
}

impl ForecastModel {
    /// `advertised` is the provider's advertised version for this model id from the bundle,
    /// used when the model itself carries none.
    pub fn from_supplied(model: &SuppliedForecastModel, advertised: Option<&str>) -> Self {
        Self {
            id: model.model_id.clone(),
            version: model
                .version
                .clone()
                .or_else(|| advertised.map(str::to_owned))
                .or_else(|| model.fetched_at.clone())
                .unwrap_or_default(),
            run_id: model.run_id.clone(),
            fetched_at: model.fetched_at_unix_ns.map(nanos),
            issued_at: model.issued_at_unix_ns.map(nanos),
            timezone: model.timezone.clone(),
            utc_offset_seconds: model.utc_offset_seconds,
            hourly: model
                .hourly
                .iter()
                .map(ForecastPoint::from_supplied)
                .collect(),
            origin: ValueOrigin::Supplied,
            supplied: Some(model.clone()),
        }
    }

    /// Derived summary retained for the frozen kernel contract: the maximum point temperature.
    pub fn value(&self) -> f64 {
        self.hourly
            .iter()
            .filter_map(|point| point.temperature_f)
            .fold(f64::NEG_INFINITY, f64::max)
    }

    pub fn snapshot(&self) -> ForecastModelSnapshot<'_> {
        ForecastModelSnapshot {
            model_id: &self.id,
            value: self.value(),
            version: &self.version,
            updated_at: self.fetched_at,
            run_issued_at: self.issued_at,
            run_id: self.run_id.as_deref(),
            timezone: self.timezone.as_deref(),
            utc_offset_seconds: self.utc_offset_seconds,
            hourly: Cow::Owned(self.hourly.iter().map(ForecastPoint::snapshot).collect()),
            origin: self.origin,
            supplied: self.supplied.as_ref(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Forecast {
    pub station_id: String,
    pub source: String,
    pub received_at: Option<DateTime<Utc>>,
    pub advertised_versions: Vec<(String, String)>,
    pub models: Vec<ForecastModel>,
    pub meta: ComponentMeta,
    pub origin: ValueOrigin,
    pub supplied: Option<SuppliedForecast>,
}

impl Forecast {
    pub fn from_supplied(station_id: &str, forecast: &SuppliedForecast) -> Self {
        Self {
            station_id: station_id.to_owned(),
            source: forecast.source.clone(),
            received_at: Some(nanos(forecast.received_at_unix_ns)),
            advertised_versions: forecast.advertised_versions.clone(),
            models: forecast
                .models
                .iter()
                .map(|model| {
                    let advertised = forecast
                        .advertised_versions
                        .iter()
                        .find(|(id, _)| *id == model.model_id)
                        .map(|(_, version)| version.as_str());
                    ForecastModel::from_supplied(model, advertised)
                })
                .collect(),
            meta: ComponentMeta::default(),
            origin: ValueOrigin::Supplied,
            supplied: Some(forecast.clone()),
        }
    }

    pub fn snapshot(&self) -> ForecastInputSnapshot<'_> {
        ForecastInputSnapshot {
            station_id: &self.station_id,
            received_at: self.received_at,
            source: &self.source,
            models: Cow::Owned(self.models.iter().map(ForecastModel::snapshot).collect()),
            advertised_versions: Cow::Borrowed(&self.advertised_versions),
            origin: self.origin,
            supplied: self.supplied.as_ref(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OracleScore {
    pub rank: Option<u8>,
    pub model_id: String,
    pub model_name: String,
    pub is_public: Option<bool>,
    pub high_mae: Option<f64>,
    pub low_mae: Option<f64>,
    pub combined_mae: Option<f64>,
    pub high_bias: Option<f64>,
    pub low_bias: Option<f64>,
    pub day_count: Option<i64>,
}

impl OracleScore {
    pub fn from_supplied(score: &SuppliedOracleScore, rank: Option<u8>) -> Self {
        Self {
            rank,
            model_id: score.model_id.clone(),
            model_name: score.model_name.clone(),
            is_public: score.is_public,
            high_mae: f64_of(score.high_mae),
            low_mae: f64_of(score.low_mae),
            combined_mae: f64_of(score.combined_mae),
            high_bias: f64_of(score.high_bias),
            low_bias: f64_of(score.low_bias),
            day_count: score.day_count,
        }
    }

    pub fn snapshot(&self) -> OracleModelScoreSnapshot<'_> {
        OracleModelScoreSnapshot {
            model_id: &self.model_id,
            model_name: &self.model_name,
            is_public: self.is_public,
            high_mae: self.high_mae,
            low_mae: self.low_mae,
            combined_mae: self.combined_mae,
            high_bias: self.high_bias,
            low_bias: self.low_bias,
            day_count: self.day_count,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OracleTable {
    pub station_id: String,
    pub source: String,
    pub received_at: Option<DateTime<Utc>>,
    pub mode: String,
    pub rank_by: String,
    pub days: String,
    pub all_time: Option<bool>,
    pub range_start: String,
    pub range_end: String,
    pub updated_at: Option<DateTime<Utc>>,
    pub modes: Vec<String>,
    pub scores: Vec<OracleScore>,
    pub meta: ComponentMeta,
    pub origin: ValueOrigin,
    pub supplied: Option<SuppliedOracleTable>,
}

impl OracleTable {
    pub fn from_supplied(table: &SuppliedOracleTable) -> Self {
        Self {
            station_id: table.station_id.clone(),
            source: table.source.clone(),
            received_at: Some(nanos(table.received_at_unix_ns)),
            mode: table.score_mode.clone().unwrap_or_default(),
            rank_by: table.rank_by.clone().unwrap_or_default(),
            days: table
                .days_requested
                .map(|days| days.to_string())
                .unwrap_or_default(),
            all_time: table.all_time,
            range_start: table.range_start.clone(),
            range_end: table.range_end.clone(),
            updated_at: table.notification_updated_at_unix_ns.map(nanos),
            modes: table.notification_modes.clone(),
            scores: table
                .scores
                .iter()
                .enumerate()
                .map(|(index, score)| {
                    OracleScore::from_supplied(score, u8::try_from(index + 1).ok())
                })
                .collect(),
            meta: ComponentMeta::default(),
            origin: ValueOrigin::Supplied,
            supplied: Some(table.clone()),
        }
    }

    pub fn snapshot(&self) -> OracleInputSnapshot<'_> {
        OracleInputSnapshot {
            station_id: &self.station_id,
            received_at: self.received_at,
            source: &self.source,
            score_mode: &self.mode,
            rank_by: &self.rank_by,
            days_requested: &self.days,
            range_start: &self.range_start,
            range_end: &self.range_end,
            scores: Cow::Owned(self.scores.iter().map(OracleScore::snapshot).collect()),
            all_time: self.all_time,
            updated_at: self.updated_at,
            notification_modes: &self.modes,
            origin: self.origin,
            supplied: self.supplied.as_ref(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StationIdentity {
    pub station_id: String,
    pub city_id: Option<String>,
    pub city_slug: Option<String>,
    pub logical_location: String,
    pub name: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub timezone: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClimateDay {
    pub event_date: String,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
}

/// Authority and freshness of each station component.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StationComponents {
    pub observation: ComponentMeta,
    pub weather: ComponentMeta,
    pub extrema: ComponentMeta,
    pub reports: ComponentMeta,
    pub weather_events: ComponentMeta,
    pub forecast: ComponentMeta,
    pub oracle: ComponentMeta,
}

/// Complete scoped state of one station.
///
/// `weather` is the derived current summary the frozen kernels read through `get_weather`:
/// each of its facts is the freshest accepted value for that fact, projected from the supplied
/// original when the original is at least as fresh as the host's derived component and from
/// the derived component otherwise. The originals themselves live in `observation`,
/// `daily_extremes`, `extreme_high`/`extreme_low`, `reports`, `weather_events`, `forecast`
/// and `oracle_tables`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StationState {
    pub identity: StationIdentity,
    pub climate_day: ClimateDay,
    pub components: StationComponents,
    pub weather: StationWeatherView,
    pub observation: Option<Observation>,
    pub daily_extremes: Option<DailyExtremes>,
    pub extreme_high: Option<Extreme>,
    pub extreme_low: Option<Extreme>,
    /// Current report per type, sorted by report type.
    pub reports: Vec<Report>,
    /// Current (not ended) episodes.
    pub weather_events: Vec<WeatherEvent>,
    pub forecast: Option<Forecast>,
    pub oracle_tables: Vec<OracleTable>,
}

impl StationState {
    pub fn station_id(&self) -> &str {
        &self.identity.station_id
    }

    pub fn report(&self, report_id: &str) -> Option<&Report> {
        self.reports
            .iter()
            .find(|report| report.report_id == report_id)
    }

    pub fn weather_event(&self, id: &str) -> Option<&WeatherEvent> {
        self.weather_events.iter().find(|event| event.id == id)
    }

    /// The oracle table matching the query, or any table when no dimension is requested.
    pub fn oracle_table(
        &self,
        mode: Option<&str>,
        rank_by: Option<&str>,
        days: Option<&str>,
    ) -> Option<&OracleTable> {
        self.oracle_tables.iter().find(|table| {
            mode.is_none_or(|value| value == table.mode)
                && rank_by.is_none_or(|value| value == table.rank_by)
                && days.is_none_or(|value| value == table.days)
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MarketLifecycle {
    pub status: String,
    pub result: Option<String>,
    pub connection_epoch: Option<u64>,
    pub received_frame_ordinal: Option<u64>,
    pub open_at: Option<DateTime<Utc>>,
    pub close_at: Option<DateTime<Utc>>,
    pub settled_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// Top-of-book quote with exact quantities.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TickerQuote {
    pub yes_bid: Option<f64>,
    pub yes_ask: Option<f64>,
    pub no_bid: Option<f64>,
    pub no_ask: Option<f64>,
    pub yes_bid_quantity: Option<ContractQuantity>,
    pub yes_ask_quantity: Option<ContractQuantity>,
    pub no_bid_quantity: Option<ContractQuantity>,
    pub no_ask_quantity: Option<ContractQuantity>,
    pub last_price: Option<f64>,
    pub last_trade_quantity: Option<ContractQuantity>,
    pub volume: Option<ContractQuantity>,
    pub volume_24h: Option<ContractQuantity>,
    pub open_interest: Option<ContractQuantity>,
    pub provider_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BookLevel {
    pub price: f64,
    pub quantity: ContractQuantity,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Book {
    pub connection_epoch: Option<u64>,
    pub sid: Option<u64>,
    pub sequence: Option<u64>,
    pub snapshot_at: Option<DateTime<Utc>>,
    pub resync_required: bool,
    pub yes_bids: Vec<BookLevel>,
    pub no_bids: Vec<BookLevel>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LastTrade {
    pub trade_id: String,
    pub yes_price: f64,
    pub no_price: f64,
    pub quantity: ContractQuantity,
    pub taker_side: Option<String>,
    pub traded_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FinalFact {
    pub status: String,
    pub result: String,
    pub settlement_value: Option<f64>,
    pub settled_price: Option<f64>,
    pub provider_at: Option<DateTime<Utc>>,
}

/// Authority and freshness of each market component.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MarketComponents {
    pub lifecycle: ComponentMeta,
    pub ticker: ComponentMeta,
    pub book: ComponentMeta,
    pub last_trade: ComponentMeta,
    pub final_fact: ComponentMeta,
}

/// Complete scoped state of one market.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MarketState {
    pub market_id: String,
    pub opportunity_id: String,
    pub venue: String,
    pub source: String,
    pub event_ticker: String,
    pub series_ticker: String,
    pub event_date: String,
    pub strike_type: String,
    pub fee_type: String,
    pub fee_multiplier: Option<f64>,
    /// Strikes in Fahrenheit; the exact fixed-point identity values follow.
    pub floor_strike: Option<f64>,
    pub cap_strike: Option<f64>,
    pub floor_strike_milli_c: Option<i64>,
    pub floor_strike_milli_f: Option<i64>,
    pub cap_strike_milli_c: Option<i64>,
    pub close_time: Option<DateTime<Utc>>,
    pub expiration_time: Option<DateTime<Utc>>,
    pub components: MarketComponents,
    pub lifecycle: Option<MarketLifecycle>,
    pub ticker: Option<TickerQuote>,
    pub book: Option<Book>,
    pub last_trade: Option<LastTrade>,
    pub final_fact: Option<FinalFact>,
    pub uncertain_fields: Vec<String>,
    /// Historical peak ask, when the host tracks one.
    pub peak_yes_ask: Option<f64>,
    pub last_update: Option<DateTime<Utc>>,
    /// Frozen-view levels derived from `book`: yes asks and no asks are the inverted opposite
    /// bids. Hosts that receive levels directly fill these and leave `book` empty.
    pub yes_bid_levels: Vec<PriceLevelView>,
    pub yes_ask_levels: Vec<PriceLevelView>,
    pub no_bid_levels: Vec<PriceLevelView>,
    pub no_ask_levels: Vec<PriceLevelView>,
}

impl MarketState {
    /// Fills the frozen-view level slices from `book`.
    pub fn derive_levels(&mut self) {
        let Some(book) = &self.book else {
            return;
        };
        let levels = |levels: &[BookLevel], invert: bool| {
            levels
                .iter()
                .map(|level| {
                    PriceLevelView::exact(
                        if invert {
                            1.0 - level.price
                        } else {
                            level.price
                        },
                        level.quantity,
                    )
                })
                .collect::<Vec<_>>()
        };
        self.yes_bid_levels = levels(&book.yes_bids, false);
        self.yes_ask_levels = levels(&book.no_bids, true);
        self.no_bid_levels = levels(&book.no_bids, false);
        self.no_ask_levels = levels(&book.yes_bids, true);
    }

    fn yes_price(&self) -> f64 {
        let ticker = self.ticker.as_ref();
        ticker
            .and_then(|ticker| ticker.last_price)
            .or_else(|| ticker.and_then(|ticker| ticker.yes_ask))
            .or_else(|| ticker.and_then(|ticker| ticker.yes_bid))
            .unwrap_or(0.0)
    }

    fn no_price(&self) -> f64 {
        let ticker = self.ticker.as_ref();
        ticker
            .and_then(|ticker| ticker.last_price)
            .map(|value| 1.0 - value)
            .or_else(|| ticker.and_then(|ticker| ticker.no_ask))
            .or_else(|| ticker.and_then(|ticker| ticker.no_bid))
            .unwrap_or(0.0)
    }

    pub fn ticker_view(&self) -> TickerPriceView<'_> {
        let ticker = self.ticker.as_ref();
        TickerPriceView {
            ticker: &self.market_id,
            source: &self.source,
            event_ticker: &self.event_ticker,
            event_date: &self.event_date,
            series_ticker: &self.series_ticker,
            close_time: self.close_time,
            fee_type: &self.fee_type,
            fee_multiplier: self.fee_multiplier,
            strike_type: &self.strike_type,
            floor_strike: self.floor_strike,
            cap_strike: self.cap_strike,
            yes_price: self.yes_price(),
            no_price: self.no_price(),
            yes_bid: ticker.and_then(|ticker| ticker.yes_bid),
            yes_ask: ticker.and_then(|ticker| ticker.yes_ask),
            no_bid: ticker.and_then(|ticker| ticker.no_bid),
            no_ask: ticker.and_then(|ticker| ticker.no_ask),
            yes_bid_depth: ticker
                .and_then(|ticker| ticker.yes_bid_quantity)
                .map(whole_contracts),
            yes_ask_depth: ticker
                .and_then(|ticker| ticker.yes_ask_quantity)
                .map(whole_contracts),
            no_bid_depth: ticker
                .and_then(|ticker| ticker.no_bid_quantity)
                .map(whole_contracts),
            no_ask_depth: ticker
                .and_then(|ticker| ticker.no_ask_quantity)
                .map(whole_contracts),
            yes_bid_quantity: ticker.and_then(|ticker| ticker.yes_bid_quantity),
            yes_ask_quantity: ticker.and_then(|ticker| ticker.yes_ask_quantity),
            no_bid_quantity: ticker.and_then(|ticker| ticker.no_bid_quantity),
            no_ask_quantity: ticker.and_then(|ticker| ticker.no_ask_quantity),
            yes_bid_levels: &self.yes_bid_levels,
            yes_ask_levels: &self.yes_ask_levels,
            no_bid_levels: &self.no_bid_levels,
            no_ask_levels: &self.no_ask_levels,
            orderbook_depth: ticker
                .and_then(|ticker| ticker.yes_ask_quantity)
                .map(whole_contracts),
            volume: ticker
                .and_then(|ticker| ticker.volume)
                .map(|volume| volume.hundredths() as f64 / 100.0),
            volume_exact: ticker.and_then(|ticker| ticker.volume),
            volume_24h: ticker.and_then(|ticker| ticker.volume_24h),
            open_interest: ticker.and_then(|ticker| ticker.open_interest),
            last_price: ticker.and_then(|ticker| ticker.last_price),
            last_trade_quantity: ticker.and_then(|ticker| ticker.last_trade_quantity),
            expiration_time: self.expiration_time,
            lifecycle_status: self
                .lifecycle
                .as_ref()
                .map(|lifecycle| lifecycle.status.as_str()),
            lifecycle_result: self
                .lifecycle
                .as_ref()
                .and_then(|lifecycle| lifecycle.result.as_deref()),
            peak_yes_ask: self
                .peak_yes_ask
                .or_else(|| ticker.and_then(|ticker| ticker.yes_ask)),
            last_update: self.last_update,
        }
    }

    pub fn bracket_view(&self) -> MarketBracketView<'_> {
        let ticker = self.ticker.as_ref();
        MarketBracketView {
            market_id: &self.market_id,
            ticker: &self.market_id,
            yes_price: self.yes_price(),
            no_price: self.no_price(),
            event_ticker: &self.event_ticker,
            event_date: &self.event_date,
            close_time: self.close_time,
            strike_type: &self.strike_type,
            floor_strike: self.floor_strike,
            cap_strike: self.cap_strike,
            snapshot_time: self.last_update,
            yes_bid: ticker.and_then(|ticker| ticker.yes_bid),
            yes_ask: ticker.and_then(|ticker| ticker.yes_ask),
            no_bid: ticker.and_then(|ticker| ticker.no_bid),
            no_ask: ticker.and_then(|ticker| ticker.no_ask),
            yes_bid_depth: ticker
                .and_then(|ticker| ticker.yes_bid_quantity)
                .map(whole_contracts),
            yes_ask_depth: ticker
                .and_then(|ticker| ticker.yes_ask_quantity)
                .map(whole_contracts),
            no_bid_depth: ticker
                .and_then(|ticker| ticker.no_bid_quantity)
                .map(whole_contracts),
            no_ask_depth: ticker
                .and_then(|ticker| ticker.no_ask_quantity)
                .map(whole_contracts),
            yes_bid_quantity: ticker.and_then(|ticker| ticker.yes_bid_quantity),
            yes_ask_quantity: ticker.and_then(|ticker| ticker.yes_ask_quantity),
            no_bid_quantity: ticker.and_then(|ticker| ticker.no_bid_quantity),
            no_ask_quantity: ticker.and_then(|ticker| ticker.no_ask_quantity),
            yes_bid_levels: &self.yes_bid_levels,
            yes_ask_levels: &self.yes_ask_levels,
            no_bid_levels: &self.no_bid_levels,
            no_ask_levels: &self.no_ask_levels,
            orderbook_depth: ticker
                .and_then(|ticker| ticker.yes_ask_quantity)
                .map(whole_contracts),
            volume: ticker
                .and_then(|ticker| ticker.volume)
                .map(|volume| volume.hundredths() as f64 / 100.0),
            volume_exact: ticker.and_then(|ticker| ticker.volume),
            volume_24h: ticker.and_then(|ticker| ticker.volume_24h),
            open_interest: ticker.and_then(|ticker| ticker.open_interest),
        }
    }
}

/// Whole-contract floor of an exact quantity.
pub fn whole_contracts(quantity: ContractQuantity) -> i64 {
    quantity.hundredths().div_euclid(100)
}

pub fn nanos(value: i64) -> DateTime<Utc> {
    Utc.timestamp_nanos(value)
}

pub fn f64_of(value: Option<Decimal>) -> Option<f64> {
    value.map(Decimal::to_f64)
}

pub fn celsius_to_fahrenheit(value: f64) -> f64 {
    value * 9.0 / 5.0 + 32.0
}

pub fn fahrenheit_to_celsius(value: f64) -> f64 {
    (value - 32.0) * 5.0 / 9.0
}
