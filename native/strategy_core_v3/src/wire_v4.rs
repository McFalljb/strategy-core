//! Frozen positional observation/weather encodings, shared by standalone V4 and embedded V5.
//!
//! Excluded historical WU values belong only to this codec. Encoding always takes ordinary
//! facts from the active owner value, so retained evidence cannot mask changed ordinary data.

use bincode::{Decode, Encode};

use crate::decision_v4::{ObservationV4, ProvenanceV4, WeatherV4};

/// Opaque historical encoding evidence, never a strategy weather component.
#[doc(hidden)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RetainedObservationEncodingV4 {
    current: Option<i32>,
    high: Option<i32>,
    low: Option<i32>,
    observed_at: Option<i64>,
    fetched_at: Option<i64>,
    day_mode: Option<String>,
    day_date: Option<String>,
}

/// Opaque historical encoding evidence, never a strategy weather component.
#[doc(hidden)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RetainedWeatherEncodingV4 {
    current: Option<i32>,
    high: Option<i32>,
    low: Option<i32>,
}

// Keep these field orders: V4 uses fixed integers, V5 uses variable integers. The enclosing
// encoder supplies that configuration; serializing a standalone V4 blob inside V5 is invalid.
#[derive(Encode, Decode)]
struct FrozenObservationV4 {
    station_id: String,
    observed_at_unix_ms: i64,
    source_timestamp_unix_ms: Option<i64>,
    producer_received_at_unix_ms: Option<i64>,
    live_published_at_unix_ms: Option<i64>,
    lag_ms: Option<i64>,
    preliminary: bool,
    persistence_status: Option<String>,
    temperature_milli_c: Option<i32>,
    temperature_min_milli_c: Option<i32>,
    temperature_max_milli_c: Option<i32>,
    wu_current_temperature_milli_c: Option<i32>,
    wu_daily_high_milli_c: Option<i32>,
    wu_daily_low_milli_c: Option<i32>,
    wu_observation_at_unix_ms: Option<i64>,
    wu_fetched_at_unix_ms: Option<i64>,
    temperature_day_mode: Option<String>,
    temperature_day_date: Option<String>,
    wu_day_mode: Option<String>,
    wu_day_date: Option<String>,
    is_from_report: bool,
    report_type: Option<String>,
    source_report_id: Option<String>,
    dewpoint_micros: Option<i64>,
    heat_index_micros: Option<i64>,
    wind_chill_micros: Option<i64>,
    relative_humidity_micros: Option<i64>,
    wind_speed_micros: Option<i64>,
    wind_direction_micros: Option<i64>,
    wind_gust_micros: Option<i64>,
    text_description: Option<String>,
    provenance: ProvenanceV4,
}

impl From<&ObservationV4> for FrozenObservationV4 {
    fn from(value: &ObservationV4) -> Self {
        Self {
            station_id: value.station_id.clone(),
            observed_at_unix_ms: value.observed_at_unix_ms,
            source_timestamp_unix_ms: value.source_timestamp_unix_ms,
            producer_received_at_unix_ms: value.producer_received_at_unix_ms,
            live_published_at_unix_ms: value.live_published_at_unix_ms,
            lag_ms: value.lag_ms,
            preliminary: value.preliminary,
            persistence_status: value.persistence_status.clone(),
            temperature_milli_c: value.temperature_milli_c,
            temperature_min_milli_c: value.temperature_min_milli_c,
            temperature_max_milli_c: value.temperature_max_milli_c,
            wu_current_temperature_milli_c: value.retained_encoding.current,
            wu_daily_high_milli_c: value.retained_encoding.high,
            wu_daily_low_milli_c: value.retained_encoding.low,
            wu_observation_at_unix_ms: value.retained_encoding.observed_at,
            wu_fetched_at_unix_ms: value.retained_encoding.fetched_at,
            temperature_day_mode: value.temperature_day_mode.clone(),
            temperature_day_date: value.temperature_day_date.clone(),
            wu_day_mode: value.retained_encoding.day_mode.clone(),
            wu_day_date: value.retained_encoding.day_date.clone(),
            is_from_report: value.is_from_report,
            report_type: value.report_type.clone(),
            source_report_id: value.source_report_id.clone(),
            dewpoint_micros: value.dewpoint_micros,
            heat_index_micros: value.heat_index_micros,
            wind_chill_micros: value.wind_chill_micros,
            relative_humidity_micros: value.relative_humidity_micros,
            wind_speed_micros: value.wind_speed_micros,
            wind_direction_micros: value.wind_direction_micros,
            wind_gust_micros: value.wind_gust_micros,
            text_description: value.text_description.clone(),
            provenance: value.provenance.clone(),
        }
    }
}

impl From<FrozenObservationV4> for ObservationV4 {
    fn from(value: FrozenObservationV4) -> Self {
        Self {
            station_id: value.station_id,
            observed_at_unix_ms: value.observed_at_unix_ms,
            source_timestamp_unix_ms: value.source_timestamp_unix_ms,
            producer_received_at_unix_ms: value.producer_received_at_unix_ms,
            live_published_at_unix_ms: value.live_published_at_unix_ms,
            lag_ms: value.lag_ms,
            preliminary: value.preliminary,
            persistence_status: value.persistence_status,
            temperature_milli_c: value.temperature_milli_c,
            temperature_min_milli_c: value.temperature_min_milli_c,
            temperature_max_milli_c: value.temperature_max_milli_c,
            temperature_day_mode: value.temperature_day_mode,
            temperature_day_date: value.temperature_day_date,
            is_from_report: value.is_from_report,
            report_type: value.report_type,
            source_report_id: value.source_report_id,
            dewpoint_micros: value.dewpoint_micros,
            heat_index_micros: value.heat_index_micros,
            wind_chill_micros: value.wind_chill_micros,
            relative_humidity_micros: value.relative_humidity_micros,
            wind_speed_micros: value.wind_speed_micros,
            wind_direction_micros: value.wind_direction_micros,
            wind_gust_micros: value.wind_gust_micros,
            text_description: value.text_description,
            provenance: value.provenance,
            retained_encoding: RetainedObservationEncodingV4 {
                current: value.wu_current_temperature_milli_c,
                high: value.wu_daily_high_milli_c,
                low: value.wu_daily_low_milli_c,
                observed_at: value.wu_observation_at_unix_ms,
                fetched_at: value.wu_fetched_at_unix_ms,
                day_mode: value.wu_day_mode,
                day_date: value.wu_day_date,
            },
        }
    }
}

impl Encode for ObservationV4 {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        FrozenObservationV4::from(self).encode(encoder)
    }
}
impl<Context> Decode<Context> for ObservationV4 {
    fn decode<D: bincode::de::Decoder<Context = Context>>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        FrozenObservationV4::decode(decoder).map(Into::into)
    }
}
bincode::impl_borrow_decode!(ObservationV4);

#[derive(Encode, Decode)]
struct FrozenWeatherV4 {
    station_id: String,
    current_temperature_milli_c: Option<i32>,
    running_high_milli_c: Option<i32>,
    running_low_milli_c: Option<i32>,
    last_metar_at_unix_ms: Option<i64>,
    dsm_high_milli_c: Option<i32>,
    dsm_low_milli_c: Option<i32>,
    dsm_high_at_unix_ms: Option<i64>,
    dsm_low_at_unix_ms: Option<i64>,
    six_hour_high_milli_c: Option<i32>,
    six_hour_low_milli_c: Option<i32>,
    asos_daily_high_milli_c: Option<i32>,
    asos_daily_low_milli_c: Option<i32>,
    wu_current_temperature_milli_c: Option<i32>,
    wu_daily_high_milli_c: Option<i32>,
    wu_daily_low_milli_c: Option<i32>,
    dewpoint_micros: Option<i64>,
    heat_index_micros: Option<i64>,
    wind_chill_micros: Option<i64>,
    relative_humidity_micros: Option<i64>,
    wind_speed_micros: Option<i64>,
    wind_direction_micros: Option<i64>,
    wind_gust_micros: Option<i64>,
    text_description: Option<String>,
    preliminary: bool,
}

impl From<&WeatherV4> for FrozenWeatherV4 {
    fn from(value: &WeatherV4) -> Self {
        Self {
            station_id: value.station_id.clone(),
            current_temperature_milli_c: value.current_temperature_milli_c,
            running_high_milli_c: value.running_high_milli_c,
            running_low_milli_c: value.running_low_milli_c,
            last_metar_at_unix_ms: value.last_metar_at_unix_ms,
            dsm_high_milli_c: value.dsm_high_milli_c,
            dsm_low_milli_c: value.dsm_low_milli_c,
            dsm_high_at_unix_ms: value.dsm_high_at_unix_ms,
            dsm_low_at_unix_ms: value.dsm_low_at_unix_ms,
            six_hour_high_milli_c: value.six_hour_high_milli_c,
            six_hour_low_milli_c: value.six_hour_low_milli_c,
            asos_daily_high_milli_c: value.asos_daily_high_milli_c,
            asos_daily_low_milli_c: value.asos_daily_low_milli_c,
            wu_current_temperature_milli_c: value.retained_encoding.current,
            wu_daily_high_milli_c: value.retained_encoding.high,
            wu_daily_low_milli_c: value.retained_encoding.low,
            dewpoint_micros: value.dewpoint_micros,
            heat_index_micros: value.heat_index_micros,
            wind_chill_micros: value.wind_chill_micros,
            relative_humidity_micros: value.relative_humidity_micros,
            wind_speed_micros: value.wind_speed_micros,
            wind_direction_micros: value.wind_direction_micros,
            wind_gust_micros: value.wind_gust_micros,
            text_description: value.text_description.clone(),
            preliminary: value.preliminary,
        }
    }
}
impl From<FrozenWeatherV4> for WeatherV4 {
    fn from(value: FrozenWeatherV4) -> Self {
        Self {
            station_id: value.station_id,
            current_temperature_milli_c: value.current_temperature_milli_c,
            running_high_milli_c: value.running_high_milli_c,
            running_low_milli_c: value.running_low_milli_c,
            last_metar_at_unix_ms: value.last_metar_at_unix_ms,
            dsm_high_milli_c: value.dsm_high_milli_c,
            dsm_low_milli_c: value.dsm_low_milli_c,
            dsm_high_at_unix_ms: value.dsm_high_at_unix_ms,
            dsm_low_at_unix_ms: value.dsm_low_at_unix_ms,
            six_hour_high_milli_c: value.six_hour_high_milli_c,
            six_hour_low_milli_c: value.six_hour_low_milli_c,
            asos_daily_high_milli_c: value.asos_daily_high_milli_c,
            asos_daily_low_milli_c: value.asos_daily_low_milli_c,
            dewpoint_micros: value.dewpoint_micros,
            heat_index_micros: value.heat_index_micros,
            wind_chill_micros: value.wind_chill_micros,
            relative_humidity_micros: value.relative_humidity_micros,
            wind_speed_micros: value.wind_speed_micros,
            wind_direction_micros: value.wind_direction_micros,
            wind_gust_micros: value.wind_gust_micros,
            text_description: value.text_description,
            preliminary: value.preliminary,
            retained_encoding: RetainedWeatherEncodingV4 {
                current: value.wu_current_temperature_milli_c,
                high: value.wu_daily_high_milli_c,
                low: value.wu_daily_low_milli_c,
            },
        }
    }
}
impl Encode for WeatherV4 {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        FrozenWeatherV4::from(self).encode(encoder)
    }
}
impl<Context> Decode<Context> for WeatherV4 {
    fn decode<D: bincode::de::Decoder<Context = Context>>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        FrozenWeatherV4::decode(decoder).map(Into::into)
    }
}
bincode::impl_borrow_decode!(WeatherV4);
