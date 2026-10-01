//! The hourly Kalshi Weather Index of one city inside a Decision Context V6.
//!
//! The supplied originals are owned by `strategy_core_kernel::hourly_index` (the same types a
//! kernel reads beside its `f64` conveniences); this module names them with the V6 suffix,
//! adds the host's component metadata, and owns the bounds and structural validation. A
//! context holding an input that fails is invalid, so the host checks its input with
//! [`HourlyIndexInputV6::validate`] (dropping or repairing what fails) before it builds the
//! context.

pub use strategy_core_kernel::hourly_index::{
    FeedConditionKind as FeedConditionKindV6, FeedConditionSeverity as FeedConditionSeverityV6,
    IndexHourStatus as IndexHourStatusV6, IndexPhase as IndexPhaseV6,
    IndexSettlementStatus as IndexSettlementStatusV6,
    SuppliedFeedCondition as SuppliedFeedConditionV6, SuppliedHourlyIndex as SuppliedHourlyIndexV6,
    SuppliedHourlyIndexEnvelope as SuppliedHourlyIndexEnvelopeV6,
    SuppliedIndexCalibration as SuppliedIndexCalibrationV6,
    SuppliedIndexForecast as SuppliedIndexForecastV6,
    SuppliedIndexForecastBias as SuppliedIndexForecastBiasV6,
    SuppliedIndexForecastSettle as SuppliedIndexForecastSettleV6,
    SuppliedIndexHour as SuppliedIndexHourV6, SuppliedIndexMember as SuppliedIndexMemberV6,
    SuppliedIndexMinute as SuppliedIndexMinuteV6,
    SuppliedIndexMinuteValue as SuppliedIndexMinuteValueV6,
    SuppliedIndexQuorum as SuppliedIndexQuorumV6,
    SuppliedIndexSettlement as SuppliedIndexSettlementV6,
    SuppliedIndexStationReading as SuppliedIndexStationReadingV6,
    SuppliedMemberBias as SuppliedMemberBiasV6,
};

use bincode::{Decode, Encode};

use crate::current_v6::validate_meta;
use crate::decision_v4::ComponentMetaV4;
use crate::decision_v6::DecisionV6Error;
use crate::supplied_v6::{
    DecimalV6, MAX_SUPPLIED_TEXT_BYTES, identifier, optional_decimal, optional_text,
    strictly_sorted, text,
};

/// The minutes a city keeps: the snapshot's 75 (an hour and its lookback).
pub const MAX_INDEX_MINUTES: usize = 75;
/// Member stations of one city (readings, calibration members, member biases, missing
/// forecast members).
pub const MAX_INDEX_STATIONS: usize = 16;
/// 15-minute forecast steps (the provider runs to +18 h: 73).
pub const MAX_INDEX_FORECAST_STEPS: usize = 96;
/// Upcoming top-of-hour forecast settles.
pub const MAX_INDEX_FORECAST_SETTLES: usize = 6;
/// Settlements a city keeps, newest first.
pub const MAX_INDEX_SETTLEMENTS: usize = 3;
/// Feed conditions of the current hour (a few kinds per member, plus the city's own).
pub const MAX_INDEX_FEED_CONDITIONS: usize = 64;

/// Host authority and freshness of each hourly-index stream; see
/// [`strategy_core_kernel::HourlyIndexComponents`] for which fields each covers.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct HourlyIndexComponentsV6 {
    pub minutes: ComponentMetaV4,
    pub hour: ComponentMetaV4,
    pub forecast: ComponentMetaV4,
    pub bias: ComponentMetaV4,
    pub settlements: ComponentMetaV4,
    pub calibration: ComponentMetaV4,
}

/// One city's hourly index and the host's metadata of its streams.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct HourlyIndexInputV6 {
    pub supplied: SuppliedHourlyIndexV6,
    pub components: HourlyIndexComponentsV6,
}

impl HourlyIndexInputV6 {
    /// Bounds, canonical order and structure. Unsorted lists are `NonCanonicalOrder`, lists
    /// over their bound `BoundExceeded`, anything else malformed `InvalidContract`.
    pub fn validate(&self) -> Result<(), DecisionV6Error> {
        let components = &self.components;
        for meta in [
            &components.minutes,
            &components.hour,
            &components.forecast,
            &components.bias,
            &components.settlements,
            &components.calibration,
        ] {
            validate_meta(meta)?;
        }
        validate_supplied(&self.supplied)
    }
}

fn validate_supplied(index: &SuppliedHourlyIndexV6) -> Result<(), DecisionV6Error> {
    identifier(&index.city)?;
    bounded(&index.timezone)?;
    bounded(&index.config_version)?;
    optional_text(&index.hourly_series_ticker)?;

    if index.recent_minutes.len() > MAX_INDEX_MINUTES
        || index.recent_settlements.len() > MAX_INDEX_SETTLEMENTS
    {
        return Err(DecisionV6Error::BoundExceeded);
    }
    for minute in index
        .latest
        .iter()
        .chain(&index.latest_valued)
        .chain(&index.recent_minutes)
    {
        validate_minute(minute)?;
    }
    // Newest first.
    strictly_sorted(
        index
            .recent_minutes
            .iter()
            .rev()
            .map(|minute| minute.minute_unix_ns),
    )?;
    if let Some(valued) = &index.latest_valued {
        if valued.value_f.is_none()
            || index
                .latest
                .as_ref()
                .is_none_or(|latest| latest.minute_unix_ns < valued.minute_unix_ns)
        {
            return Err(DecisionV6Error::InvalidContract);
        }
    }

    if let Some(hour) = &index.current_hour {
        validate_hour(hour)?;
    }
    if let Some(forecast) = &index.forecast {
        validate_forecast(forecast)?;
    }
    for settlement in &index.recent_settlements {
        validate_settlement(settlement)?;
    }
    strictly_sorted(
        index
            .recent_settlements
            .iter()
            .rev()
            .map(|settlement| settlement.hour_end_unix_ns),
    )?;
    if let Some(calibration) = &index.calibration {
        validate_calibration(calibration)?;
    }
    Ok(())
}

fn validate_minute(minute: &SuppliedIndexMinuteV6) -> Result<(), DecisionV6Error> {
    if let Some(envelope) = &minute.envelope {
        optional_text(&envelope.event_id)?;
    }
    text(&minute.source)?;
    optional_text(&minute.kalshi_status)?;
    bounded(&minute.config_version)?;
    for value in [minute.value_f, minute.official_f, minute.provisional_f] {
        optional_decimal(value)?;
    }
    decimal(minute.weight_share)?;
    if minute.stations.len() > MAX_INDEX_STATIONS {
        return Err(DecisionV6Error::BoundExceeded);
    }
    for reading in &minute.stations {
        identifier(&reading.station_id)?;
        text(&reading.code)?;
        optional_text(&reading.source)?;
        for value in [
            reading.temp_f,
            Some(reading.weight),
            reading.offset_c,
            reading.pull_f,
            reading.change_5m_f,
            reading.hour_high_f,
            reading.hour_low_f,
        ] {
            optional_decimal(value)?;
        }
    }
    strictly_sorted(
        minute
            .stations
            .iter()
            .map(|reading| reading.station_id.as_str()),
    )
}

fn validate_hour(hour: &SuppliedIndexHourV6) -> Result<(), DecisionV6Error> {
    optional_text(&hour.event_ticker)?;
    optional_decimal(hour.settles_now_as.map(|value| value.value_f))?;
    for value in [
        hour.high_f,
        hour.low_f,
        hour.forecast_settle_f,
        hour.forecast_bias_f,
        hour.forecast_adjusted_settle_f,
    ] {
        optional_decimal(value)?;
    }
    if hour.feed_conditions.len() > MAX_INDEX_FEED_CONDITIONS {
        return Err(DecisionV6Error::BoundExceeded);
    }
    for condition in &hour.feed_conditions {
        if let FeedConditionKindV6::Other(name) = &condition.kind {
            text(name)?;
            // A known kind is never `Other`.
            if !matches!(
                FeedConditionKindV6::from_name(name),
                FeedConditionKindV6::Other(_)
            ) {
                return Err(DecisionV6Error::InvalidContract);
            }
        }
        if let Some(station_id) = &condition.station_id {
            identifier(station_id)?;
        }
        if condition.since_unix_ns > condition.last_seen_unix_ns {
            return Err(DecisionV6Error::InvalidContract);
        }
    }
    Ok(())
}

fn validate_forecast(forecast: &SuppliedIndexForecastV6) -> Result<(), DecisionV6Error> {
    text(&forecast.model_id)?;
    bounded(&forecast.config_version)?;
    let steps = forecast.steps_unix_ns.len();
    if steps > MAX_INDEX_FORECAST_STEPS
        || forecast.settles.len() > MAX_INDEX_FORECAST_SETTLES
        || forecast.missing_members.len() > MAX_INDEX_STATIONS
        || forecast
            .bias
            .as_ref()
            .is_some_and(|bias| bias.member_bias.len() > MAX_INDEX_STATIONS)
    {
        return Err(DecisionV6Error::BoundExceeded);
    }
    if forecast.value_f.len() != steps
        || forecast.quorum_met.len() != steps
        || forecast.partial.len() != steps
    {
        return Err(DecisionV6Error::InvalidContract);
    }
    for value in &forecast.value_f {
        optional_decimal(*value)?;
    }
    for station_id in &forecast.missing_members {
        identifier(station_id)?;
    }
    strictly_sorted(forecast.missing_members.iter())?;
    strictly_sorted(forecast.steps_unix_ns.iter())?;
    for settle in &forecast.settles {
        optional_decimal(settle.value_f)?;
    }
    strictly_sorted(
        forecast
            .settles
            .iter()
            .map(|settle| settle.hour_end_unix_ns),
    )?;
    if let Some(bias) = &forecast.bias {
        decimal(bias.bias_f)?;
        for member in &bias.member_bias {
            identifier(&member.station_id)?;
            optional_decimal(member.bias_f)?;
        }
        strictly_sorted(
            bias.member_bias
                .iter()
                .map(|member| member.station_id.as_str()),
        )?;
    }
    Ok(())
}

fn validate_settlement(settlement: &SuppliedIndexSettlementV6) -> Result<(), DecisionV6Error> {
    optional_text(&settlement.event_ticker)?;
    for value in [
        settlement.settle_value_f,
        settlement.winning_floor_strike,
        settlement.kalshi_expiration_value,
    ] {
        optional_decimal(value)?;
    }
    Ok(())
}

fn validate_calibration(calibration: &SuppliedIndexCalibrationV6) -> Result<(), DecisionV6Error> {
    text(&calibration.config_version)?;
    if calibration.members.len() > MAX_INDEX_STATIONS {
        return Err(DecisionV6Error::BoundExceeded);
    }
    for member in &calibration.members {
        identifier(&member.station_id)?;
        decimal(member.weight)?;
        decimal(member.offset_c)?;
    }
    strictly_sorted(
        calibration
            .members
            .iter()
            .map(|member| member.station_id.as_str()),
    )?;
    decimal(calibration.quorum.min_weight)
}

/// A string that may be empty (before the first snapshot); only the byte bound applies.
fn bounded(value: &str) -> Result<(), DecisionV6Error> {
    if value.len() > MAX_SUPPLIED_TEXT_BYTES {
        return Err(DecisionV6Error::BoundExceeded);
    }
    Ok(())
}

fn decimal(value: DecimalV6) -> Result<(), DecisionV6Error> {
    optional_decimal(Some(value))
}

/// A Miami snapshot-like city and a warming one, for the corpus and the tests.
#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;
    use crate::decision_v4::{AuthorityV4, ProvenanceV4};

    /// The decision time of the corpus's base context: a top of hour.
    pub(crate) const T0_S: i64 = 1_788_062_400;
    pub(crate) const MEMBERS: [&str; 5] = ["KFLL", "KFXE", "KMIA", "KOPF", "KPMP"];
    pub(crate) const SEQ: u64 = 48_213;

    pub(crate) fn ns(seconds: i64) -> i64 {
        seconds * 1_000_000_000
    }

    pub(crate) fn d(text: &str) -> DecimalV6 {
        DecimalV6::parse(text).unwrap()
    }

    fn meta(authority: AuthorityV4, revision: u64, seq: Option<u64>) -> ComponentMetaV4 {
        ComponentMetaV4 {
            authority,
            revision,
            generation: 1,
            updated_at_unix_ms: seq.map(|_| (T0_S - 30) * 1_000),
            provenance: seq
                .map(|seq| ProvenanceV4 {
                    provider: "minutetemp".to_owned(),
                    source: "minutetemp.websocket.hourly_index".to_owned(),
                    event_id: Some(format!("evt-miami-{seq}")),
                    connection_epoch: Some(2),
                    city_sequence: Some(seq),
                    provider_at_unix_ms: Some((T0_S - 31) * 1_000),
                    received_at_unix_ms: (T0_S - 30) * 1_000,
                    ..Default::default()
                })
                .into_iter()
                .collect(),
            ..Default::default()
        }
    }

    /// Minute `k` (0 newest) of the snapshot: three provisional minutes, then final official
    /// ones, every one with all five members.
    fn minute(k: i64) -> SuppliedIndexMinuteV6 {
        let at = T0_S - 180 - 60 * k;
        let official = k >= 3;
        let value = format!("{:.2}", 84.3 - 0.01 * k as f64);
        SuppliedIndexMinuteV6 {
            envelope: Some(SuppliedHourlyIndexEnvelopeV6 {
                event_id: Some(format!("evt-miami-min-{k}")),
                seq: SEQ - k as u64,
                emitted_at_unix_ns: Some(ns(at + 150)),
                received_at_unix_ns: ns(at + 151),
            }),
            source: "minutetemp.websocket.hourly_index".to_owned(),
            minute_unix_ns: ns(at),
            phase: if official {
                IndexPhaseV6::Official
            } else {
                IndexPhaseV6::Provisional
            },
            is_final: official,
            revision: if official { 3 } else { 1 },
            value_f: Some(d(&value)),
            official_f: official.then(|| d(&value)),
            provisional_f: Some(d(&value)),
            kalshi_status: official.then(|| "normal".to_owned()),
            config_version: "miami-temperature-v1.0-cal-20260928".to_owned(),
            contributors: 5,
            weight_share: d("1"),
            quorum_met: true,
            calibration_pending: false,
            provisional_at_unix_ns: Some(ns(at + 150)),
            official_at_unix_ns: official.then(|| ns(at + 360)),
            stations: MEMBERS
                .iter()
                .enumerate()
                .map(|(index, station_id)| {
                    let metar = *station_id == "KOPF" && k % 4 == 0;
                    SuppliedIndexStationReadingV6 {
                        station_id: (*station_id).to_owned(),
                        temp_f: Some(d(&format!("{:.1}", 82.4 + 1.8 * index as f64))),
                        code: "ok".to_owned(),
                        source: Some(if metar { "metar" } else { "hf_asos" }.to_owned()),
                        contributes: true,
                        weight: d("0.2"),
                        offset_c: Some(d(if index == 2 { "0.1" } else { "0" })),
                        pull_f: Some(d(&format!("{:.2}", -0.36 + 0.18 * index as f64))),
                        change_5m_f: (k < 70).then(|| d("0.18")),
                        hour_high_f: Some(d("86")),
                        hour_low_f: Some(d("81.9")),
                        received_at_unix_ns: Some(ns(at + 95)),
                    }
                })
                .collect(),
        }
    }

    pub(crate) fn snapshot() -> HourlyIndexInputV6 {
        let minutes = (0..MAX_INDEX_MINUTES as i64)
            .map(minute)
            .collect::<Vec<_>>();
        let hour_end = T0_S + 3_600;
        let run_time = T0_S - 900;
        let steps = (0..73)
            .map(|step| ns(run_time + 900 * step))
            .collect::<Vec<_>>();
        let mut value_f = (0..73)
            .map(|step| Some(d(&format!("{:.1}", 84.0 + 0.1 * (step % 9) as f64))))
            .collect::<Vec<_>>();
        value_f[72] = None;
        let mut partial = vec![false; 73];
        partial[72] = true;
        let mut quorum_met = vec![true; 73];
        quorum_met[72] = false;
        HourlyIndexInputV6 {
            supplied: SuppliedHourlyIndexV6 {
                city: "miami".to_owned(),
                timezone: "America/New_York".to_owned(),
                hourly_series_ticker: Some("KXTEMPMIAH".to_owned()),
                config_version: "miami-temperature-v1.0-cal-20260928".to_owned(),
                calibration_pending: false,
                seq: SEQ,
                latest: Some(minutes[0].clone()),
                latest_valued: Some(minutes[0].clone()),
                recent_minutes: minutes,
                current_hour: Some(SuppliedIndexHourV6 {
                    hour_end_unix_ns: ns(hour_end),
                    status: IndexHourStatusV6::Open,
                    event_ticker: Some("KXTEMPMIAH-26AUG3001".to_owned()),
                    settles_now_as: Some(SuppliedIndexMinuteValueV6 {
                        minute_unix_ns: ns(T0_S - 360),
                        value_f: d("84.27"),
                    }),
                    high_f: Some(d("84.3")),
                    low_f: Some(d("84.27")),
                    forecast_settle_f: Some(d("84.4")),
                    forecast_bias_f: Some(d("0.62")),
                    forecast_adjusted_settle_f: Some(d("85.02")),
                    final_at_unix_ns: ns(hour_end + 300),
                    determine_by_unix_ns: ns(hour_end + 480),
                    feed_conditions: vec![
                        SuppliedFeedConditionV6 {
                            kind: FeedConditionKindV6::IntermittentFallback,
                            severity: FeedConditionSeverityV6::Warning,
                            station_id: Some("KOPF".to_owned()),
                            minutes: 8,
                            window_minutes: 30,
                            since_unix_ns: ns(T0_S - 1_980),
                            last_seen_unix_ns: ns(T0_S - 420),
                        },
                        SuppliedFeedConditionV6 {
                            kind: FeedConditionKindV6::Other("member_drift".to_owned()),
                            severity: FeedConditionSeverityV6::Critical,
                            station_id: None,
                            minutes: 1,
                            window_minutes: 1,
                            since_unix_ns: ns(T0_S - 360),
                            last_seen_unix_ns: ns(T0_S - 360),
                        },
                    ],
                }),
                forecast: Some(SuppliedIndexForecastV6 {
                    model_id: "ncep_hrrr_conus_15min".to_owned(),
                    run_time_unix_ns: ns(run_time),
                    fetched_at_unix_ns: ns(run_time + 420),
                    config_version: "miami-temperature-v1.0-cal-20260928".to_owned(),
                    missing_members: vec!["KPMP".to_owned()],
                    steps_unix_ns: steps,
                    value_f,
                    quorum_met,
                    partial,
                    settles: (1..=6)
                        .map(|hour| SuppliedIndexForecastSettleV6 {
                            hour_end_unix_ns: ns(T0_S + 3_600 * hour),
                            value_f: Some(d(&format!("{:.1}", 84.4 - 0.3 * (hour - 1) as f64))),
                            quorum_met: true,
                            partial: hour == 6,
                        })
                        .collect(),
                    bias: Some(SuppliedIndexForecastBiasV6 {
                        bias_f: d("0.62"),
                        as_of_minute_unix_ns: ns(T0_S - 180),
                        member_bias: MEMBERS
                            .iter()
                            .map(|station_id| SuppliedMemberBiasV6 {
                                station_id: (*station_id).to_owned(),
                                bias_f: (*station_id != "KPMP").then(|| d("0.55")),
                            })
                            .collect(),
                    }),
                }),
                recent_settlements: vec![
                    SuppliedIndexSettlementV6 {
                        hour_end_unix_ns: ns(T0_S),
                        status: IndexSettlementStatusV6::Determined,
                        event_ticker: Some("KXTEMPMIAH-26AUG3000".to_owned()),
                        settle_minute_unix_ns: Some(ns(T0_S)),
                        settle_value_f: Some(d("84.31")),
                        winning_floor_strike: Some(d("83.99")),
                        informational: false,
                        determined_at_unix_ns: ns(T0_S + 330),
                        kalshi_expiration_value: None,
                        kalshi_finalized_at_unix_ns: None,
                        matched: None,
                        revision: 1,
                    },
                    SuppliedIndexSettlementV6 {
                        hour_end_unix_ns: ns(T0_S - 3_600),
                        status: IndexSettlementStatusV6::Determined,
                        event_ticker: Some("KXTEMPMIAH-26AUG2923".to_owned()),
                        settle_minute_unix_ns: Some(ns(T0_S - 3_600)),
                        settle_value_f: Some(d("84.89")),
                        winning_floor_strike: Some(d("84.99")),
                        informational: false,
                        determined_at_unix_ns: ns(T0_S - 3_270),
                        kalshi_expiration_value: Some(d("84.89")),
                        kalshi_finalized_at_unix_ns: Some(ns(T0_S - 900)),
                        matched: Some(true),
                        revision: 2,
                    },
                    SuppliedIndexSettlementV6 {
                        hour_end_unix_ns: ns(T0_S - 7_200),
                        status: IndexSettlementStatusV6::NoValue,
                        event_ticker: Some("KXTEMPMIAH-26AUG2922".to_owned()),
                        determined_at_unix_ns: ns(T0_S - 6_720),
                        revision: 1,
                        ..Default::default()
                    },
                ],
                calibration: Some(SuppliedIndexCalibrationV6 {
                    config_version: "miami-temperature-v1.0-cal-20260928".to_owned(),
                    effective_at_unix_ns: Some(ns(1_790_553_960)),
                    members: MEMBERS
                        .iter()
                        .map(|station_id| SuppliedIndexMemberV6 {
                            station_id: (*station_id).to_owned(),
                            weight: d("0.2"),
                            offset_c: d(if *station_id == "KMIA" { "0.1" } else { "0" }),
                        })
                        .collect(),
                    quorum: SuppliedIndexQuorumV6 {
                        min_members: 4,
                        min_weight: d("0.8"),
                        published: true,
                    },
                }),
            },
            components: HourlyIndexComponentsV6 {
                minutes: meta(AuthorityV4::Current, 75, Some(SEQ)),
                hour: meta(AuthorityV4::Current, 12, Some(SEQ - 1)),
                forecast: meta(AuthorityV4::Current, 3, Some(SEQ - 40)),
                bias: meta(AuthorityV4::Current, 18, Some(SEQ - 2)),
                settlements: meta(AuthorityV4::Current, 4, Some(SEQ - 5)),
                calibration: meta(AuthorityV4::Current, 1, None),
            },
        }
    }

    /// A subscribed city before its first snapshot: only its id, every stream warming.
    pub(crate) fn warming() -> HourlyIndexInputV6 {
        let warming = meta(AuthorityV4::Warming, 1, None);
        HourlyIndexInputV6 {
            supplied: SuppliedHourlyIndexV6 {
                city: "miami".to_owned(),
                ..Default::default()
            },
            components: HourlyIndexComponentsV6 {
                minutes: warming.clone(),
                hour: warming.clone(),
                forecast: warming.clone(),
                bias: warming.clone(),
                settlements: warming.clone(),
                calibration: warming,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    fn rejects(mutate: impl FnOnce(&mut HourlyIndexInputV6), error: DecisionV6Error) {
        let mut input = snapshot();
        mutate(&mut input);
        assert_eq!(input.validate(), Err(error));
    }

    #[test]
    fn the_fixtures_are_valid_and_the_snapshot_is_at_its_bounds() {
        snapshot().validate().unwrap();
        warming().validate().unwrap();
        let snapshot = snapshot().supplied;
        assert_eq!(snapshot.recent_minutes.len(), MAX_INDEX_MINUTES);
        assert_eq!(snapshot.recent_settlements.len(), MAX_INDEX_SETTLEMENTS);
        assert_eq!(
            snapshot.forecast.unwrap().settles.len(),
            MAX_INDEX_FORECAST_SETTLES
        );
    }

    #[test]
    fn identity_and_metadata_are_checked() {
        use DecisionV6Error::*;
        rejects(|input| input.supplied.city.clear(), InvalidContract);
        rejects(
            |input| input.supplied.city = "new york".to_owned(),
            InvalidContract,
        );
        rejects(|input| input.components.bias.revision = 0, InvalidContract);
        rejects(
            |input| input.components.hour.generation = 0,
            InvalidContract,
        );
        rejects(
            |input| input.supplied.timezone = "x".repeat(2049),
            BoundExceeded,
        );
        // Empty before the first snapshot.
        let mut input = snapshot();
        input.supplied.timezone.clear();
        input.supplied.config_version.clear();
        input.validate().unwrap();
    }

    #[test]
    fn minutes_are_bounded_newest_first_with_sorted_members() {
        use DecisionV6Error::*;
        rejects(
            |input| input.supplied.recent_minutes.swap(0, 1),
            NonCanonicalOrder,
        );
        rejects(
            |input| {
                let duplicate = input.supplied.recent_minutes[0].clone();
                input.supplied.recent_minutes.insert(0, duplicate);
                input.supplied.recent_minutes.pop();
            },
            NonCanonicalOrder,
        );
        rejects(
            |input| {
                let mut older = input.supplied.recent_minutes[74].clone();
                older.minute_unix_ns -= ns(60);
                input.supplied.recent_minutes.push(older);
            },
            BoundExceeded,
        );
        rejects(
            |input| input.supplied.recent_minutes[3].stations.swap(0, 1),
            NonCanonicalOrder,
        );
        rejects(
            |input| {
                let stations = &mut input.supplied.recent_minutes[3].stations;
                for index in 0..12 {
                    let mut extra = stations[0].clone();
                    extra.station_id = format!("KZZ{index:02}");
                    stations.push(extra);
                }
            },
            BoundExceeded,
        );
        rejects(
            |input| input.supplied.recent_minutes[3].stations[0].code.clear(),
            BoundExceeded,
        );
        rejects(
            |input| {
                input.supplied.recent_minutes[9].stations[2].pull_f = Some(DecimalV6 {
                    coefficient: 1,
                    scale: 19,
                })
            },
            InvalidContract,
        );
        rejects(
            |input| {
                input.supplied.latest.as_mut().unwrap().weight_share = DecimalV6 {
                    coefficient: 10,
                    scale: 1,
                }
            },
            InvalidContract,
        );
    }

    #[test]
    fn latest_valued_has_a_value_and_is_not_newer_than_latest() {
        use DecisionV6Error::*;
        rejects(
            |input| input.supplied.latest_valued.as_mut().unwrap().value_f = None,
            InvalidContract,
        );
        rejects(|input| input.supplied.latest = None, InvalidContract);
        // A newest minute below quorum: latest has no value, latest_valued is older.
        let mut input = snapshot();
        let latest = input.supplied.latest.as_mut().unwrap();
        latest.minute_unix_ns += ns(60);
        latest.value_f = None;
        input.validate().unwrap();
    }

    #[test]
    fn the_hour_and_its_feed_conditions_are_checked() {
        use DecisionV6Error::*;
        fn hour(input: &mut HourlyIndexInputV6) -> &mut SuppliedIndexHourV6 {
            input.supplied.current_hour.as_mut().unwrap()
        }
        rejects(
            |input| {
                hour(input).feed_conditions[1].kind =
                    FeedConditionKindV6::Other("quorum_risk".to_owned())
            },
            InvalidContract,
        );
        rejects(
            |input| hour(input).feed_conditions[1].kind = FeedConditionKindV6::Other(String::new()),
            BoundExceeded,
        );
        rejects(
            |input| {
                let condition = &mut hour(input).feed_conditions[0];
                condition.since_unix_ns = condition.last_seen_unix_ns + 1;
            },
            InvalidContract,
        );
        rejects(
            |input| hour(input).feed_conditions[0].station_id = Some(String::new()),
            InvalidContract,
        );
        rejects(
            |input| {
                let conditions = &mut hour(input).feed_conditions;
                *conditions = vec![conditions[0].clone(); MAX_INDEX_FEED_CONDITIONS + 1];
            },
            BoundExceeded,
        );
        rejects(
            |input| {
                hour(input).settles_now_as.as_mut().unwrap().value_f = DecimalV6 {
                    coefficient: 0,
                    scale: 2,
                }
            },
            InvalidContract,
        );
    }

    #[test]
    fn forecast_arrays_are_aligned_ordered_and_bounded() {
        use DecisionV6Error::*;
        fn forecast(input: &mut HourlyIndexInputV6) -> &mut SuppliedIndexForecastV6 {
            input.supplied.forecast.as_mut().unwrap()
        }
        rejects(
            |input| forecast(input).value_f.truncate(72),
            InvalidContract,
        );
        rejects(
            |input| forecast(input).quorum_met.truncate(72),
            InvalidContract,
        );
        rejects(|input| forecast(input).partial.push(false), InvalidContract);
        rejects(
            |input| forecast(input).steps_unix_ns.swap(3, 4),
            NonCanonicalOrder,
        );
        rejects(
            |input| {
                let forecast = forecast(input);
                for _ in 0..MAX_INDEX_FORECAST_STEPS - 72 {
                    let last = *forecast.steps_unix_ns.last().unwrap();
                    forecast.steps_unix_ns.push(last + ns(900));
                    forecast.value_f.push(None);
                    forecast.quorum_met.push(false);
                    forecast.partial.push(true);
                }
            },
            BoundExceeded,
        );
        rejects(
            |input| {
                let settles = &mut forecast(input).settles;
                let mut seventh = settles[5];
                seventh.hour_end_unix_ns += ns(3_600);
                settles.push(seventh);
            },
            BoundExceeded,
        );
        rejects(
            |input| forecast(input).settles.swap(0, 1),
            NonCanonicalOrder,
        );
        rejects(
            |input| forecast(input).missing_members.push("KFLL".to_owned()),
            NonCanonicalOrder,
        );
        rejects(
            |input| forecast(input).bias.as_mut().unwrap().member_bias.reverse(),
            NonCanonicalOrder,
        );
        rejects(|input| forecast(input).model_id.clear(), BoundExceeded);
    }

    #[test]
    fn settlements_and_calibration_are_checked() {
        use DecisionV6Error::*;
        rejects(
            |input| input.supplied.recent_settlements.reverse(),
            NonCanonicalOrder,
        );
        rejects(
            |input| {
                let mut fourth = input.supplied.recent_settlements[2].clone();
                fourth.hour_end_unix_ns -= ns(3_600);
                input.supplied.recent_settlements.push(fourth);
            },
            BoundExceeded,
        );
        rejects(
            |input| {
                input
                    .supplied
                    .calibration
                    .as_mut()
                    .unwrap()
                    .members
                    .swap(1, 2);
            },
            NonCanonicalOrder,
        );
        rejects(
            |input| {
                input
                    .supplied
                    .calibration
                    .as_mut()
                    .unwrap()
                    .quorum
                    .min_weight = DecimalV6 {
                    coefficient: 80,
                    scale: 2,
                }
            },
            InvalidContract,
        );
    }
}
