//! Frozen supplied-inputs/2 containers. S and C share the observation/daily positional
//! shapes, as do their oracle leaves. Unchanged C forecast/report leaves remain shared until
//! their layouts change.

use bincode::{Decode, Encode};

use crate::{decision_v5::DecisionV5Error, supplied_s, supplied_v5::*};

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
struct ExcludedObservationFields {
    day_mode: Option<String>,
    day_date: Option<String>,
    current_f: Option<DecimalV5>,
    current_c: Option<DecimalV5>,
    high_f: Option<DecimalV5>,
    low_f: Option<DecimalV5>,
    high_c: Option<DecimalV5>,
    low_c: Option<DecimalV5>,
    observed_at: Option<i64>,
    fetched_at: Option<i64>,
}

impl ExcludedObservationFields {
    fn observation(value: &supplied_s::SuppliedObservationV5) -> Self {
        Self {
            day_mode: value.wu_day_mode.clone(),
            day_date: value.wu_day_date.clone(),
            current_f: value.wu_current_temp_f,
            current_c: value.wu_current_temp_c,
            high_f: value.wu_daily_high_f,
            low_f: value.wu_daily_low_f,
            high_c: value.wu_daily_high_c,
            low_c: value.wu_daily_low_c,
            observed_at: value.wu_observation_time_unix_ns,
            fetched_at: value.wu_fetched_at_unix_ns,
        }
    }
    fn daily(value: &supplied_s::SuppliedDailyExtremesV5) -> Self {
        Self {
            day_mode: value.wu_day_mode.clone(),
            day_date: value.wu_day_date.clone(),
            current_f: value.wu_current_temp_f,
            current_c: value.wu_current_temp_c,
            high_f: value.wu_daily_high_f,
            low_f: value.wu_daily_low_f,
            high_c: value.wu_daily_high_c,
            low_c: value.wu_daily_low_c,
            observed_at: value.wu_observation_time_unix_ns,
            fetched_at: value.wu_fetched_at_unix_ns,
        }
    }
    fn validate(&self) -> Result<(), DecisionV5Error> {
        if [&self.day_mode, &self.day_date].into_iter().any(|value| {
            value
                .as_ref()
                .is_some_and(|text| text.len() > MAX_SUPPLIED_TEXT_BYTES)
        }) {
            return Err(DecisionV5Error::BoundExceeded);
        }
        if [
            self.current_f,
            self.current_c,
            self.high_f,
            self.low_f,
            self.high_c,
            self.low_c,
        ]
        .into_iter()
        .flatten()
        .any(|value| !value.is_canonical())
        {
            return Err(DecisionV5Error::InvalidContract);
        }
        Ok(())
    }
    fn restore_observation(&self, value: &mut supplied_s::SuppliedObservationV5) {
        value.wu_day_mode = self.day_mode.clone();
        value.wu_day_date = self.day_date.clone();
        value.wu_current_temp_f = self.current_f;
        value.wu_current_temp_c = self.current_c;
        value.wu_daily_high_f = self.high_f;
        value.wu_daily_low_f = self.low_f;
        value.wu_daily_high_c = self.high_c;
        value.wu_daily_low_c = self.low_c;
        value.wu_observation_time_unix_ns = self.observed_at;
        value.wu_fetched_at_unix_ns = self.fetched_at;
    }
    fn restore_daily(&self, value: &mut supplied_s::SuppliedDailyExtremesV5) {
        value.wu_day_mode = self.day_mode.clone();
        value.wu_day_date = self.day_date.clone();
        value.wu_current_temp_f = self.current_f;
        value.wu_current_temp_c = self.current_c;
        value.wu_daily_high_f = self.high_f;
        value.wu_daily_low_f = self.low_f;
        value.wu_daily_high_c = self.high_c;
        value.wu_daily_low_c = self.low_c;
        value.wu_observation_time_unix_ns = self.observed_at;
        value.wu_fetched_at_unix_ns = self.fetched_at;
    }
}

/// Opaque codec evidence for historical encoding identity and excluded fields. Only decoding can populate it;
/// neither kernel models nor their supplied originals carry this evidence.
#[doc(hidden)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RetainedSuppliedEncodingV5 {
    pub(crate) canonical_c: bool,
    pub(crate) canonical_d: bool,
    pub(crate) canonical_e: bool,
    stations: Vec<(String, ExcludedObservationFields, ExcludedObservationFields)>,
    originating: ExcludedObservationFields,
}

// Wire payloads carry only retained historical evidence. Encoding selectors are never
// serialized; only decoding an outer historical header can populate them.
impl Encode for RetainedSuppliedEncodingV5 {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        self.stations.encode(encoder)?;
        self.originating.encode(encoder)
    }
}
impl<Context> Decode<Context> for RetainedSuppliedEncodingV5 {
    fn decode<D: bincode::de::Decoder<Context = Context>>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        Ok(Self {
            canonical_c: false,
            canonical_d: false,
            canonical_e: false,
            stations: Decode::decode(decoder)?,
            originating: Decode::decode(decoder)?,
        })
    }
}
bincode::impl_borrow_decode!(RetainedSuppliedEncodingV5);

impl RetainedSuppliedEncodingV5 {
    pub(crate) fn validate(&self, inputs: &SuppliedInputsV5) -> Result<(), DecisionV5Error> {
        if self.stations.len() > crate::decision_v4::MAX_STATIONS {
            return Err(DecisionV5Error::BoundExceeded);
        }
        let empty = ExcludedObservationFields::default();
        let mut previous = None;
        for (id, observation, daily) in &self.stations {
            if previous.is_some_and(|previous: &str| previous >= id.as_str()) {
                return Err(DecisionV5Error::InvalidContract);
            }
            previous = Some(id.as_str());
            let station = inputs
                .stations
                .iter()
                .find(|station| station.station_id == *id)
                .ok_or(DecisionV5Error::InvalidContract)?;
            if (observation != &empty && station.observation.is_none())
                || (daily != &empty && station.daily_extremes.is_none())
                || (observation == &empty && daily == &empty)
            {
                return Err(DecisionV5Error::InvalidContract);
            }
            observation.validate()?;
            daily.validate()?;
        }
        self.originating.validate()?;
        if self.originating != empty
            && !matches!(
                inputs.originating_event,
                Some(SuppliedEventV5::Observation(_))
            )
        {
            return Err(DecisionV5Error::InvalidContract);
        }
        Ok(())
    }

    fn station(
        &mut self,
        id: &str,
        observation: Option<&supplied_s::SuppliedObservationV5>,
        daily: Option<&supplied_s::SuppliedDailyExtremesV5>,
    ) -> Result<(), DecisionV5Error> {
        let observation = observation
            .map(ExcludedObservationFields::observation)
            .unwrap_or_default();
        let daily = daily
            .map(ExcludedObservationFields::daily)
            .unwrap_or_default();
        observation.validate()?;
        daily.validate()?;
        if observation != ExcludedObservationFields::default()
            || daily != ExcludedObservationFields::default()
        {
            self.stations.push((id.to_owned(), observation, daily));
        }
        Ok(())
    }
    pub(crate) fn from_s(value: &supplied_s::SuppliedInputsV5) -> Result<Self, DecisionV5Error> {
        let absent = value.contract_version.is_empty()
            && value.stations.is_empty()
            && value.originating_event.is_none();
        if !absent && value.contract_version != supplied_s::CONTRACT_VERSION {
            return Err(DecisionV5Error::InvalidContract);
        }
        let mut retained = Self::default();
        for station in &value.stations {
            retained.station(
                &station.station_id,
                station.observation.as_ref(),
                station.daily_extremes.as_ref(),
            )?;
        }
        if let Some(supplied_s::SuppliedEventV5::Observation(event)) = &value.originating_event {
            retained.originating = ExcludedObservationFields::observation(event);
            retained.originating.validate()?;
        }
        Ok(retained)
    }
    pub(crate) fn restore_s(&self, value: &mut supplied_s::SuppliedInputsV5) {
        for station in &mut value.stations {
            self.restore_station(
                &station.station_id,
                station.observation.as_mut(),
                station.daily_extremes.as_mut(),
            );
        }
        if let Some(supplied_s::SuppliedEventV5::Observation(event)) = &mut value.originating_event
        {
            self.originating.restore_observation(event);
        }
    }
    fn restore_station(
        &self,
        id: &str,
        observation: Option<&mut supplied_s::SuppliedObservationV5>,
        daily: Option<&mut supplied_s::SuppliedDailyExtremesV5>,
    ) {
        if let Some((_, old_observation, old_daily)) =
            self.stations.iter().find(|(station, _, _)| station == id)
        {
            if let Some(observation) = observation {
                old_observation.restore_observation(observation);
            }
            if let Some(daily) = daily {
                old_daily.restore_daily(daily);
            }
        }
    }
}

#[derive(Clone, Debug, Encode, Decode)]
pub(crate) struct FrozenSuppliedInputsV5 {
    contract_version: String,
    stations: Vec<FrozenStationV5>,
    originating_event: Option<FrozenEventV5>,
}
#[derive(Clone, Debug, Encode, Decode)]
struct FrozenStationV5 {
    station_id: String,
    observation: Option<supplied_s::SuppliedObservationV5>,
    daily_extremes: Option<supplied_s::SuppliedDailyExtremesV5>,
    reports: Vec<SuppliedReportV5>,
    extreme_high: Option<SuppliedExtremeV5>,
    extreme_low: Option<SuppliedExtremeV5>,
    weather_events: Vec<SuppliedWeatherEventV5>,
    forecast: Option<SuppliedForecastV5>,
    oracle_tables: Vec<supplied_s::SuppliedOracleTableV5>,
}
// Preserve the frozen event codec shape; this is not a Strategy-facing value.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Encode, Decode)]
enum FrozenEventV5 {
    Observation(supplied_s::SuppliedObservationV5),
    Report(SuppliedReportV5),
    Extreme(SuppliedExtremeV5),
    WeatherEvent(SuppliedWeatherEventV5),
}

impl FrozenSuppliedInputsV5 {
    pub(crate) fn from_current(
        value: &SuppliedInputsV5,
        retained: &RetainedSuppliedEncodingV5,
    ) -> Self {
        let stations = value
            .stations
            .iter()
            .map(|station| {
                let mut observation = station.observation.as_ref().map(observation_to_s);
                let mut daily_extremes = station.daily_extremes.as_ref().map(daily_to_s);
                retained.restore_station(
                    &station.station_id,
                    observation.as_mut(),
                    daily_extremes.as_mut(),
                );
                FrozenStationV5 {
                    station_id: station.station_id.clone(),
                    observation,
                    daily_extremes,
                    reports: station.reports.clone(),
                    extreme_high: station.extreme_high.clone(),
                    extreme_low: station.extreme_low.clone(),
                    weather_events: station.weather_events.clone(),
                    forecast: station.forecast.clone(),
                    oracle_tables: station.oracle_tables.iter().map(oracle_to_s).collect(),
                }
            })
            .collect();
        let originating_event = value.originating_event.as_ref().map(|event| match event {
            SuppliedEventV5::Observation(event) => {
                let mut event = observation_to_s(event);
                retained.originating.restore_observation(&mut event);
                FrozenEventV5::Observation(event)
            }
            SuppliedEventV5::Report(event) => FrozenEventV5::Report(event.clone()),
            SuppliedEventV5::Extreme(event) => FrozenEventV5::Extreme(event.clone()),
            SuppliedEventV5::WeatherEvent(event) => FrozenEventV5::WeatherEvent(event.clone()),
        });
        Self {
            contract_version: if value.is_absent() {
                String::new()
            } else {
                "supplied-inputs/2".to_owned()
            },
            stations,
            originating_event,
        }
    }
    pub(crate) fn into_current(
        self,
    ) -> Result<(SuppliedInputsV5, RetainedSuppliedEncodingV5), DecisionV5Error> {
        let absent = self.contract_version.is_empty()
            && self.stations.is_empty()
            && self.originating_event.is_none();
        if !absent && self.contract_version != "supplied-inputs/2" {
            return Err(DecisionV5Error::InvalidContract);
        }
        let mut retained = RetainedSuppliedEncodingV5::default();
        let mut stations = Vec::with_capacity(self.stations.len());
        for station in self.stations {
            retained.station(
                &station.station_id,
                station.observation.as_ref(),
                station.daily_extremes.as_ref(),
            )?;
            stations.push(SuppliedStationV5 {
                station_id: station.station_id,
                observation: station.observation.map(observation_from_s),
                daily_extremes: station.daily_extremes.map(daily_from_s),
                reports: station.reports,
                extreme_high: station.extreme_high,
                extreme_low: station.extreme_low,
                weather_events: station.weather_events,
                forecast: station.forecast,
                oracle_tables: station
                    .oracle_tables
                    .into_iter()
                    .map(oracle_from_s)
                    .collect(),
            });
        }
        let originating_event = self.originating_event.map(|event| match event {
            FrozenEventV5::Observation(event) => {
                retained.originating = ExcludedObservationFields::observation(&event);
                SuppliedEventV5::Observation(observation_from_s(event))
            }
            FrozenEventV5::Report(event) => SuppliedEventV5::Report(event),
            FrozenEventV5::Extreme(event) => SuppliedEventV5::Extreme(event),
            FrozenEventV5::WeatherEvent(event) => SuppliedEventV5::WeatherEvent(event),
        });
        retained.originating.validate()?;
        Ok((
            SuppliedInputsV5 {
                contract_version: if absent {
                    String::new()
                } else {
                    SUPPLIED_INPUTS_CONTRACT_VERSION.to_owned()
                },
                stations,
                originating_event,
            },
            retained,
        ))
    }
}
