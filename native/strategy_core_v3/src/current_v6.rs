//! Current inputs beside the V4 owner projection: per-station oracle tables with their
//! supplied originals, and the complete accepted originating weather packet.
use crate::decision_v4::{ComponentMetaV4, OracleTableV4, RankByV4};
use crate::decision_v6::{DecisionContextV6, DecisionV6Error};
use crate::supplied_v6::SuppliedOracleTableV6;
use bincode::{Decode, Encode};

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct CurrentInputsV6 {
    pub stations: Vec<StationInputsV6>,
    pub originating: Option<OriginatingWeatherV6>,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct OriginatingWeatherV6 {
    pub station_id: String,
    pub source_generation: u64,
    pub source_sequence: u64,
    pub station_revision: u64,
    pub meta: ComponentMetaV4,
    pub cursor: crate::decision_v4::CursorV4,
    pub data: WeatherDataV6,
    pub facts: strategy_core_kernel::WeatherFacts,
    pub supplied: Option<crate::supplied_v6::SuppliedEventV6>,
}
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum WeatherDataV6 {
    Observation(crate::decision_v4::ObservationV4),
    Report(crate::decision_v4::ReportV4),
    Extreme {
        high: bool,
        value: crate::decision_v4::ExtremeV4,
    },
    WeatherEvent(crate::decision_v4::WeatherEventV4),
}
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct StationInputsV6 {
    pub station_id: String,
    pub oracles: Vec<OracleInputV6>,
}
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct OracleInputV6 {
    pub meta: ComponentMetaV4,
    pub table: OracleTableV4,
    pub supplied: Option<SuppliedOracleTableV6>,
}

pub(crate) fn validate(context: &DecisionContextV6) -> Result<(), DecisionV6Error> {
    let Some(current) = &context.current_inputs else {
        return Ok(());
    };
    if let Some(originating) = &current.originating {
        originating.validate(context)?;
    }
    if current.stations.len() != context.owner_state.stations.len() {
        return Err(DecisionV6Error::InvalidContract);
    }
    for (inputs, station) in current.stations.iter().zip(&context.owner_state.stations) {
        if inputs.station_id != station.identity.station_id
            || inputs.oracles.len() > 2
            || context
                .supplied
                .station(&inputs.station_id)
                .is_some_and(|supplied| !supplied.oracle_tables.is_empty())
        {
            return Err(DecisionV6Error::InvalidContract);
        }
        let mut previous = None;
        for input in &inputs.oracles {
            let table = &input.table;
            let query = &table.query;
            let low = matches!(query.rank_by, RankByV4::Low);
            if query.station_id != inputs.station_id
                || query.mode != "day_of"
                || query.days != 7
                || previous.is_some_and(|prior| prior >= low)
                || table.rows.len() > 32
                || input.meta.revision > station.revision
            {
                return Err(DecisionV6Error::InvalidContract);
            }
            previous = Some(low);
            validate_meta(&input.meta)?;
            validate_provenance(&table.provenance)?;
            text(&table.range_start)?;
            text(&table.range_end)?;
            for (index, row) in table.rows.iter().enumerate() {
                if row.rank as usize != index + 1 {
                    return Err(DecisionV6Error::InvalidContract);
                }
                text(&row.model_id)?;
                text(&row.model_name)?;
            }
            if let Some(original) = &input.supplied {
                crate::supplied_v6::validate_oracle_table(original)?;
                if original.station_id != inputs.station_id
                    || original
                        .score_mode
                        .as_ref()
                        .is_some_and(|mode| mode != &query.mode)
                    || original
                        .rank_by
                        .as_deref()
                        .is_some_and(|rank| rank != if low { "low" } else { "high" })
                    || original
                        .days_requested
                        .is_some_and(|days| days != i64::from(query.days))
                {
                    return Err(DecisionV6Error::InvalidContract);
                }
            }
            if table.query == station.oracle.query
                && (table != &station.oracle || input.meta != station.oracle_meta)
            {
                return Err(DecisionV6Error::InvalidContract);
            }
        }
        if !inputs
            .oracles
            .iter()
            .any(|input| input.table.query == station.oracle.query)
        {
            return Err(DecisionV6Error::InvalidContract);
        }
    }
    Ok(())
}
impl OriginatingWeatherV6 {
    pub(crate) fn validate(&self, context: &DecisionContextV6) -> Result<(), DecisionV6Error> {
        use crate::{
            decision_v4::TriggerV4, decision_v6::OwnerTriggerV6, supplied_v6::SuppliedEventV6,
        };
        let invalid = DecisionV6Error::InvalidContract;
        let expected = OwnerTriggerV6::CapturedWeather {
            station_id: self.station_id.clone(),
            source_generation: self.source_generation,
            source_sequence: self.source_sequence,
        };
        if context.owner_trigger() != Some(&expected)
            || context.supplied.originating_event.is_some()
            || self.station_revision == 0
            || self.source_sequence == 0
            || self.cursor.sequence == 0
            || self.source_generation != self.cursor.connection_generation
            || !self.cursor.snapshot_complete
        {
            return Err(invalid);
        }
        let station = context
            .owner_state
            .stations
            .iter()
            .find(|station| station.identity.station_id == self.station_id)
            .ok_or(DecisionV6Error::InvalidContract)?;
        let header = match (&self.data, &context.owner_state.trigger) {
            (
                WeatherDataV6::Report(report),
                TriggerV4::StationReport {
                    station_id,
                    report_id,
                    report_type,
                    report_revision,
                    provider,
                    source_generation,
                    source_sequence,
                },
            ) => {
                report.report_id == *report_id
                    && report.report_type == *report_type
                    && report.revision == *report_revision
                    && report.provider == *provider
                    && *station_id == self.station_id
                    && *source_generation == self.source_generation
                    && *source_sequence == self.source_sequence
            }
            (
                WeatherDataV6::Observation(_)
                | WeatherDataV6::Extreme { .. }
                | WeatherDataV6::WeatherEvent(_),
                TriggerV4::Weather {
                    station_id,
                    source_generation,
                    source_sequence,
                },
            ) => {
                *station_id == self.station_id
                    && *source_generation == self.source_generation
                    && *source_sequence == self.source_sequence
            }
            _ => false,
        };
        let current_meta = match &self.data {
            WeatherDataV6::Observation(value) => {
                if value.station_id != self.station_id {
                    return Err(invalid);
                }
                &station.observation_meta
            }
            WeatherDataV6::Report(_) => &station.reports_meta,
            WeatherDataV6::Extreme { .. } => &station.extrema_meta,
            WeatherDataV6::WeatherEvent(_) => &station.weather_events_meta,
        };
        if !header
            || self.cursor.connection_generation > station.provider_cursor.connection_generation
            || (self.cursor.connection_generation == station.provider_cursor.connection_generation
                && (self.station_revision > station.revision
                    || self.meta.revision > current_meta.revision
                    || self.meta.generation > current_meta.generation))
        {
            return Err(invalid);
        }
        validate_meta(&self.meta)?;
        text(&self.cursor.event_id)?;
        self.data.validate()?;
        if !self.facts.values_are_valid()
            || self.facts.text_values().any(|text| text.len() > 2048)
            || self.facts.fields.values().any(|fact| {
                fact.provenance.owner_generation != self.source_generation
                    || fact.provenance.owner_revision != self.station_revision
            })
        {
            return Err(invalid);
        }
        for fact in self.facts.fields.values() {
            if let Some(envelope) = &fact.provenance.envelope {
                crate::supplied_v6::validate_envelope(envelope)?;
            }
        }
        if let Some(event) = &self.supplied {
            crate::supplied_v6::validate_event(event)?;
            let (matches, envelope) = match (&self.data, event) {
                (WeatherDataV6::Observation(value), SuppliedEventV6::Observation(event)) => (
                    event.station_id == self.station_id
                        && event.observed_at_unix_ns.div_euclid(1_000_000)
                            == value.observed_at_unix_ms,
                    &event.envelope,
                ),
                (WeatherDataV6::Report(value), SuppliedEventV6::Report(event)) => (
                    event.station_id == self.station_id
                        && event.report_id == value.report_id
                        && event.report_type == value.report_type
                        && event.report_revision.unwrap_or(0) == value.revision,
                    &event.envelope,
                ),
                (WeatherDataV6::Extreme { high, value }, SuppliedEventV6::Extreme(event)) => (
                    event.station_id == self.station_id
                        && *high == (event.kind == crate::supplied_v6::ExtremeKindV6::High)
                        && event.observed_at_unix_ns.map(|at| at.div_euclid(1_000_000))
                            == value.observed_at_unix_ms,
                    &event.envelope,
                ),
                (WeatherDataV6::WeatherEvent(value), SuppliedEventV6::WeatherEvent(event)) => (
                    event.station_id == self.station_id
                        && event.episode_id == value.event_id
                        && event.state == value.state,
                    &event.envelope,
                ),
                _ => return Err(invalid),
            };
            if !matches
                || envelope.as_ref().is_some_and(|envelope| {
                    envelope.event_id != self.cursor.event_id
                        || envelope.sequence != self.cursor.sequence
                })
            {
                return Err(invalid);
            }
        }
        if bincode::encode_to_vec(self, bincode::config::standard())
            .map_err(|_| DecisionV6Error::Encode)?
            .len()
            > 256 * 1024
        {
            return Err(DecisionV6Error::BoundExceeded);
        }
        Ok(())
    }
}

impl WeatherDataV6 {
    fn validate(&self) -> Result<(), DecisionV6Error> {
        let (provenance, optional) = match self {
            Self::Observation(value) => {
                text(&value.station_id)?;
                if let Some(description) = &value.text_description {
                    content(description)?;
                }
                (
                    &value.provenance,
                    vec![
                        &value.persistence_status,
                        &value.temperature_day_mode,
                        &value.temperature_day_date,
                        &value.report_type,
                        &value.source_report_id,
                    ],
                )
            }
            Self::Report(value) => {
                for value in [
                    &value.report_id,
                    &value.report_type,
                    &value.report_date,
                    &value.provider,
                ] {
                    text(value)?;
                }
                content(&value.source_url)?;
                (&value.provenance, vec![])
            }
            Self::Extreme { value, .. } => (
                &value.provenance,
                vec![
                    &value.temperature_day_mode,
                    &value.temperature_day_date,
                    &value.report_type,
                    &value.source_report_id,
                ],
            ),
            Self::WeatherEvent(value) => {
                for value in [
                    &value.event_id,
                    &value.event_type,
                    &value.tier,
                    &value.state,
                ] {
                    text(value)?;
                }
                if !matches!(value.state.as_str(), "active" | "updated" | "ended") {
                    return Err(DecisionV6Error::InvalidContract);
                }
                for value in [&value.name, &value.badge, &value.detail, &value.summary] {
                    content(value)?;
                }
                let optional = value.source.as_ref().map_or_else(Vec::new, |source| {
                    vec![
                        &source.metar_type,
                        &source.flight_category,
                        &source.wx_string,
                        &source.wx_token,
                        &source.cb_location,
                    ]
                });
                (&value.provenance, optional)
            }
        };
        for value in optional.into_iter().flatten() {
            text(value)?;
        }
        validate_provenance(provenance)
    }
}
fn content(value: &str) -> Result<(), DecisionV6Error> {
    if value.len() > 2048 {
        Err(DecisionV6Error::BoundExceeded)
    } else {
        Ok(())
    }
}
fn validate_provenance(value: &crate::decision_v4::ProvenanceV4) -> Result<(), DecisionV6Error> {
    text(&value.provider)?;
    text(&value.source)?;
    if let Some(id) = &value.event_id {
        text(id)?;
    }
    Ok(())
}

pub(crate) fn text(value: &str) -> Result<(), DecisionV6Error> {
    if value.is_empty() || value.len() > 2048 {
        Err(DecisionV6Error::BoundExceeded)
    } else {
        Ok(())
    }
}
pub(crate) fn validate_meta(meta: &ComponentMetaV4) -> Result<(), DecisionV6Error> {
    if meta.provenance.len() > 4 || meta.revision == 0 || meta.generation == 0 {
        return Err(DecisionV6Error::InvalidContract);
    }
    for value in [&meta.expected_version, &meta.refresh_error]
        .into_iter()
        .flatten()
    {
        text(value)?;
    }
    for provenance in &meta.provenance {
        validate_provenance(provenance)?;
    }
    Ok(())
}
