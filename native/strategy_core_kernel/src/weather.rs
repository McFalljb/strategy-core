//! Bounded current weather facts selected by the host at acceptance time.
//!
//! Unlike the last observation or REST response, these values survive unrelated partial
//! updates. Each occupied field has one winner. Copies of superseded REST/report evidence
//! live on the station, not in this map. Getters never arbitrate winners by value or age.

use std::collections::BTreeMap;

use bincode::{Decode, Encode};

use crate::{decimal::Decimal, supplied::EventEnvelope};

#[derive(Clone, Copy, Debug, Encode, Decode, Eq, PartialEq, Ord, PartialOrd)]
pub enum WeatherField {
    Temperature,
    Minimum,
    Maximum,
    RunningHigh,
    RunningLow,
    AsosHigh,
    AsosLow,
    DsmHigh,
    DsmLow,
    SixHourHigh,
    SixHourLow,
    Dewpoint,
    HeatIndex,
    WindChill,
    RelativeHumidity,
    WindSpeed,
    WindDirection,
    WindGust,
    BarometricPressure,
    SeaLevelPressure,
    Precipitation1h,
    Precipitation3h,
    Precipitation6h,
    LastMetarTime,
    DsmHighTime,
    DsmLowTime,
    Description,
    LagSeconds,
    Preliminary,
    ExtremeHigh,
    ExtremeLow,
}

/// Native C and F are independent originals. Conversion is a convenience, never an original.
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum WeatherValue {
    Temperature {
        c: Option<Decimal>,
        f: Option<Decimal>,
    },
    Decimal(Decimal),
    TimeUnixNs(i64),
    Text(String),
    Integer(i64),
    Boolean(bool),
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct WeatherFactProvenance {
    pub source: String,
    /// Identity retained even for derived-only legacy inputs that have no supplied envelope.
    pub source_event_id: Option<String>,
    pub source_sequence: Option<u64>,
    pub envelope: Option<EventEnvelope>,
    pub observed_at_unix_ns: Option<i64>,
    pub received_at_unix_ns: Option<i64>,
    pub report_type: Option<String>,
    pub report_id: Option<String>,
    pub temperature_day_mode: Option<String>,
    pub temperature_day_date: Option<String>,
    /// Acceptance identity assigned by the host, not inferred from value or provider age.
    pub owner_generation: u64,
    pub owner_revision: u64,
    /// False for legacy/controlled inputs available only at the owner's derived precision.
    pub supplied: bool,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct WeatherFact {
    pub value: WeatherValue,
    pub provenance: WeatherFactProvenance,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WeatherFacts {
    pub fields: BTreeMap<WeatherField, WeatherFact>,
}

impl Encode for WeatherFacts {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        self.fields.iter().collect::<Vec<_>>().encode(encoder)
    }
}

impl<Context> Decode<Context> for WeatherFacts {
    fn decode<D: bincode::de::Decoder<Context = Context>>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        let fields = Vec::<(WeatherField, WeatherFact)>::decode(decoder)?;
        if fields.len() > 31 || fields.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
            return Err(bincode::error::DecodeError::Other(
                "noncanonical weather fields",
            ));
        }
        Ok(Self {
            fields: fields.into_iter().collect(),
        })
    }
}
bincode::impl_borrow_decode!(WeatherFacts);

impl WeatherFacts {
    /// Accept the fields present in a partial update; unrelated winners remain untouched.
    pub fn accept(&mut self, mut update: Self, generation: u64, revision: u64) {
        for fact in update.fields.values_mut() {
            fact.provenance.owner_generation = generation;
            fact.provenance.owner_revision = revision;
        }
        self.fields.extend(update.fields);
    }

    pub fn insert(
        &mut self,
        field: WeatherField,
        value: WeatherValue,
        provenance: &WeatherFactProvenance,
    ) {
        self.fields.insert(
            field,
            WeatherFact {
                value,
                provenance: provenance.clone(),
            },
        );
    }

    pub fn temperature_f(&self, field: WeatherField) -> Option<f64> {
        match &self.fields.get(&field)?.value {
            WeatherValue::Temperature { c, f } => f
                .map(Decimal::to_f64)
                .or_else(|| c.map(|value| crate::state::celsius_to_fahrenheit(value.to_f64()))),
            _ => None,
        }
    }

    pub fn temperature_c(&self, field: WeatherField) -> Option<f64> {
        match &self.fields.get(&field)?.value {
            WeatherValue::Temperature { c, f } => c
                .map(Decimal::to_f64)
                .or_else(|| f.map(|value| crate::state::fahrenheit_to_celsius(value.to_f64()))),
            _ => None,
        }
    }

    pub fn decimal(&self, field: WeatherField) -> Option<f64> {
        match self.fields.get(&field)?.value {
            WeatherValue::Decimal(value) => Some(value.to_f64()),
            _ => None,
        }
    }

    pub fn time(&self, field: WeatherField) -> Option<chrono::DateTime<chrono::Utc>> {
        match self.fields.get(&field)?.value {
            WeatherValue::TimeUnixNs(value) => Some(chrono::DateTime::from_timestamp_nanos(value)),
            _ => None,
        }
    }

    pub fn summary(&self, station_id: &str) -> crate::StationWeatherView {
        use WeatherField::*;
        crate::StationWeatherView {
            station_id: station_id.to_owned(),
            current_temp: self.temperature_f(Temperature),
            running_high: self.temperature_f(RunningHigh),
            running_low: self.temperature_f(RunningLow),
            last_metar_time: self.time(LastMetarTime),
            temp_min_f: self.temperature_f(Minimum),
            temp_max_f: self.temperature_f(Maximum),
            temp_min_c: self.temperature_c(Minimum),
            temp_max_c: self.temperature_c(Maximum),
            preliminary: self
                .fields
                .get(&Preliminary)
                .is_some_and(|fact| matches!(fact.value, WeatherValue::Boolean(true))),
            dsm_high: self.temperature_f(DsmHigh),
            dsm_low: self.temperature_f(DsmLow),
            dsm_high_time: self.time(DsmHighTime),
            dsm_low_time: self.time(DsmLowTime),
            six_hr_high: self.temperature_f(SixHourHigh),
            six_hr_low: self.temperature_f(SixHourLow),
            last_dsm_time: self.time(DsmHighTime).or_else(|| self.time(DsmLowTime)),
            last_six_hr_time: None,
            asos_daily_high_f: self.temperature_f(AsosHigh),
            asos_daily_low_f: self.temperature_f(AsosLow),
            dewpoint: self.decimal(Dewpoint),
            heat_index: self.decimal(HeatIndex),
            wind_chill: self.decimal(WindChill),
            relative_humidity: self.decimal(RelativeHumidity),
            wind_speed: self.decimal(WindSpeed),
            wind_direction: self.decimal(WindDirection),
            wind_gust: self.decimal(WindGust),
            text_description: self
                .fields
                .get(&Description)
                .and_then(|fact| match &fact.value {
                    WeatherValue::Text(value) => Some(value.clone()),
                    _ => None,
                }),
            lag_seconds: self
                .fields
                .get(&LagSeconds)
                .and_then(|fact| match fact.value {
                    WeatherValue::Integer(value) => Some(value),
                    _ => None,
                }),
        }
    }

    pub fn text_values(&self) -> impl Iterator<Item = &str> {
        self.fields.values().flat_map(|fact| {
            let provenance = &fact.provenance;
            let envelope = provenance.envelope.as_ref();
            [
                Some(provenance.source.as_str()),
                provenance.source_event_id.as_deref(),
                provenance.report_type.as_deref(),
                provenance.report_id.as_deref(),
                provenance.temperature_day_mode.as_deref(),
                provenance.temperature_day_date.as_deref(),
                envelope.map(|event| event.event_id.as_str()),
                envelope.and_then(|event| event.slug.as_deref()),
                envelope.and_then(|event| event.event_key.as_deref()),
                envelope.and_then(|event| event.persistence_status.as_deref()),
                match &fact.value {
                    WeatherValue::Text(value) => Some(value.as_str()),
                    _ => None,
                },
            ]
            .into_iter()
            .flatten()
        })
    }

    /// Fixed field identities bound the number of winners. The enclosing codec validates
    /// provenance text/envelopes and its total byte bound before admitting the context.
    pub fn values_are_valid(&self) -> bool {
        use WeatherField::*;
        self.fields
            .iter()
            .all(|(field, fact)| match (field, &fact.value) {
                (
                    Temperature | Minimum | Maximum | RunningHigh | RunningLow | AsosHigh | AsosLow
                    | DsmHigh | DsmLow | SixHourHigh | SixHourLow | ExtremeHigh | ExtremeLow,
                    WeatherValue::Temperature { c, f },
                ) => {
                    (c.is_some() || f.is_some())
                        && c.iter().chain(f.iter()).all(|value| value.is_canonical())
                }
                (
                    Dewpoint | HeatIndex | WindChill | RelativeHumidity | WindSpeed | WindDirection
                    | WindGust | BarometricPressure | SeaLevelPressure | Precipitation1h
                    | Precipitation3h | Precipitation6h,
                    WeatherValue::Decimal(value),
                ) => value.is_canonical(),
                (LastMetarTime | DsmHighTime | DsmLowTime, WeatherValue::TimeUnixNs(_))
                | (Description, WeatherValue::Text(_))
                | (LagSeconds, WeatherValue::Integer(_))
                | (Preliminary, WeatherValue::Boolean(_)) => true,
                _ => false,
            })
    }
}
