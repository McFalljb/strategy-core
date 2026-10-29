//! The forecast runner's nowcast of one city inside a Decision Context V6.
//!
//! The supplied originals are owned by `strategy_core_kernel::nowcast`; this module names them
//! with the V6 suffix, adds the host's metadata of the feed, and owns the bounds and structural
//! validation. A context holding an input that fails is invalid, so the host checks its input
//! with [`NowcastInputV6::validate`] (dropping what fails) before it builds the context.

pub use strategy_core_kernel::nowcast::{
    MAX_NOWCAST_ISSUES, NowcastUnavailableReason as NowcastUnavailableReasonV6,
    SuppliedMemberMetar as SuppliedMemberMetarV6, SuppliedNowcast as SuppliedNowcastV6,
    SuppliedNowcastArtifact as SuppliedNowcastArtifactV6,
    SuppliedNowcastEnvelope as SuppliedNowcastEnvelopeV6,
    SuppliedNowcastHeartbeat as SuppliedNowcastHeartbeatV6,
    SuppliedNowcastIssue as SuppliedNowcastIssueV6, SuppliedNowcastLead as SuppliedNowcastLeadV6,
    SuppliedNowcastStation as SuppliedNowcastStationV6,
    SuppliedStationForecast as SuppliedStationForecastV6,
};

use bincode::{Decode, Encode};
use sha2::{Digest, Sha256};

use crate::current_v6::validate_meta;
use crate::decision_v4::ComponentMetaV4;
use crate::decision_v6::DecisionV6Error;
use crate::hourly_index_v6::{
    MAX_INDEX_FORECAST_LEAD_NS, MAX_INDEX_OBSERVED_LEAD_NS, MIN_INDEX_TIME_UNIX_NS,
};
use crate::supplied_v6::{
    DecimalV6, identifier, optional_decimal, optional_text, strictly_sorted, text,
};

/// Member stations of one city.
pub const MAX_NOWCAST_STATIONS: usize = 16;
/// Leads of one station forecast (the runner issues 5 to 120 minutes in steps of 5: 24).
pub const MAX_NOWCAST_LEADS: usize = 32;
/// Quantile levels of one station forecast (the runner issues 19).
pub const MAX_NOWCAST_LEVELS: usize = 32;
/// Reasons and sources listed on one station forecast.
pub const MAX_NOWCAST_REASONS: usize = 32;
/// Artefacts a city keeps: each kind and model's version in force plus a next one.
pub const MAX_NOWCAST_ARTIFACTS: usize = 8;
/// Bytes of one artefact's canonical body.
pub const MAX_NOWCAST_ARTIFACT_BYTES: usize = 64 * 1024;
/// Member METARs a city keeps, across its stations.
pub const MAX_NOWCAST_METARS: usize = 64;

/// The host's metadata of the city's runner feed: its authority (see
/// [`strategy_core_kernel::NowcastState`]), the last change and its provenance.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct NowcastComponentsV6 {
    pub feed: ComponentMetaV4,
}

/// One city's nowcast and the host's metadata of its feed.
///
/// Fail closed: the feed is `Current` with no `unavailable` reason, or `Unavailable` with one;
/// no other authority is valid. A host delivers one to every Strategy that asks.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct NowcastInputV6 {
    pub supplied: SuppliedNowcastV6,
    pub components: NowcastComponentsV6,
    pub unavailable: Option<NowcastUnavailableReasonV6>,
}

impl NowcastInputV6 {
    /// Bounds, canonical order and structure at the context's decision time. Unsorted lists
    /// are `NonCanonicalOrder`, lists over their bound `BoundExceeded`, anything else
    /// malformed `InvalidContract`.
    pub fn validate(&self, decision_time_unix_ms: i64) -> Result<(), DecisionV6Error> {
        let clock = Clock::at(decision_time_unix_ms);
        let meta = &self.components.feed;
        validate_meta(meta)?;
        match (&meta.authority, self.unavailable) {
            (crate::decision_v4::AuthorityV4::Current, None)
            | (crate::decision_v4::AuthorityV4::Unavailable, Some(_)) => {}
            _ => return Err(DecisionV6Error::InvalidContract),
        }
        let observed_ms = clock.observed.div_euclid(1_000_000);
        for at in meta
            .updated_at_unix_ms
            .into_iter()
            .chain(meta.provenance.iter().flat_map(|provenance| {
                provenance
                    .provider_at_unix_ms
                    .into_iter()
                    .chain([provenance.received_at_unix_ms])
            }))
        {
            if !(MIN_INDEX_TIME_UNIX_NS / 1_000_000..=observed_ms).contains(&at) {
                return Err(DecisionV6Error::InvalidContract);
            }
        }

        let nowcast = &self.supplied;
        identifier(&nowcast.city)?;
        if nowcast.stations.len() > MAX_NOWCAST_STATIONS
            || nowcast.forecasts.len() > MAX_NOWCAST_STATIONS
            || nowcast.artifacts.len() > MAX_NOWCAST_ARTIFACTS
            || nowcast.member_metars.len() > MAX_NOWCAST_METARS
        {
            return Err(DecisionV6Error::BoundExceeded);
        }
        for station_id in &nowcast.stations {
            identifier(station_id)?;
        }
        strictly_sorted(nowcast.stations.iter())?;
        strictly_sorted(
            nowcast
                .forecasts
                .iter()
                .map(|station| station.station_id.as_str()),
        )?;
        // Every forecast and METAR is of a member station the runner listed for the city.
        let member = |station_id: &str| {
            nowcast
                .stations
                .binary_search_by(|member| member.as_str().cmp(station_id))
                .is_ok()
        };
        for station in &nowcast.forecasts {
            if !member(&station.station_id) {
                return Err(DecisionV6Error::InvalidContract);
            }
            validate_station(station, &clock)?;
        }
        strictly_sorted(nowcast.artifacts.iter().map(|artifact| {
            (
                artifact.kind.as_str(),
                artifact.model_id.as_str(),
                artifact.valid_from_unix_ns,
            )
        }))?;
        for artifact in &nowcast.artifacts {
            validate_artifact(artifact, &clock)?;
        }
        strictly_sorted(
            nowcast
                .member_metars
                .iter()
                .map(|metar| (metar.station_id.as_str(), metar.valid_time_unix_ns)),
        )?;
        for metar in &nowcast.member_metars {
            identifier(&metar.station_id)?;
            if !member(&metar.station_id) {
                return Err(DecisionV6Error::InvalidContract);
            }
            time(metar.valid_time_unix_ns, clock.observed)?;
            optional_time(metar.first_seen_unix_ns, clock.observed)?;
            optional_decimal(metar.temp_f)?;
            validate_envelope(&metar.envelope, &clock)?;
        }
        if let Some(issue) = &nowcast.last_issue {
            time(issue.issue_time_unix_ns, clock.observed)?;
            validate_station_list(&issue.stations)?;
            validate_station_list(&issue.failed)?;
            optional_decimal(issue.seconds)?;
            validate_envelope(&issue.envelope, &clock)?;
        }
        if let Some(heartbeat) = &nowcast.heartbeat {
            time(heartbeat.received_at_unix_ns, clock.observed)?;
            optional_time(heartbeat.generated_at_unix_ns, clock.observed)?;
            optional_time(heartbeat.last_issue_time_unix_ns, clock.observed)?;
            optional_text(&heartbeat.code_commit)?;
        }
        Ok(())
    }
}

/// The latest time each kind of supplied time may have.
struct Clock {
    observed: i64,
    forecast: i64,
}

impl Clock {
    fn at(decision_time_unix_ms: i64) -> Self {
        let now = decision_time_unix_ms.saturating_mul(1_000_000);
        Self {
            observed: now.saturating_add(MAX_INDEX_OBSERVED_LEAD_NS),
            forecast: now.saturating_add(MAX_INDEX_FORECAST_LEAD_NS),
        }
    }
}

/// `MIN_INDEX_TIME_UNIX_NS ≤ at ≤ latest`.
fn time(at: i64, latest: i64) -> Result<(), DecisionV6Error> {
    if (MIN_INDEX_TIME_UNIX_NS..=latest).contains(&at) {
        Ok(())
    } else {
        Err(DecisionV6Error::InvalidContract)
    }
}

fn optional_time(at: Option<i64>, latest: i64) -> Result<(), DecisionV6Error> {
    at.map_or(Ok(()), |at| time(at, latest))
}

fn decimal(value: DecimalV6) -> Result<(), DecisionV6Error> {
    optional_decimal(Some(value))
}

fn validate_envelope(
    envelope: &SuppliedNowcastEnvelopeV6,
    clock: &Clock,
) -> Result<(), DecisionV6Error> {
    text(&envelope.event_id)?;
    optional_time(envelope.emitted_at_unix_ns, clock.observed)?;
    time(envelope.received_at_unix_ns, clock.observed)
}

fn validate_station_list(stations: &[String]) -> Result<(), DecisionV6Error> {
    if stations.len() > MAX_NOWCAST_STATIONS {
        return Err(DecisionV6Error::BoundExceeded);
    }
    for station_id in stations {
        identifier(station_id)?;
    }
    strictly_sorted(stations.iter())
}

fn validate_station(
    station: &SuppliedNowcastStationV6,
    clock: &Clock,
) -> Result<(), DecisionV6Error> {
    identifier(&station.station_id)?;
    if station.issues.is_empty() {
        return Err(DecisionV6Error::InvalidContract);
    }
    if station.issues.len() > MAX_NOWCAST_ISSUES {
        return Err(DecisionV6Error::BoundExceeded);
    }
    // Oldest first.
    strictly_sorted(station.issues.iter().map(|issue| issue.issue_time_unix_ns))?;
    for issue in &station.issues {
        if issue.station_id != station.station_id {
            return Err(DecisionV6Error::InvalidContract);
        }
        validate_forecast(issue, clock)?;
    }
    Ok(())
}

fn validate_forecast(
    forecast: &SuppliedStationForecastV6,
    clock: &Clock,
) -> Result<(), DecisionV6Error> {
    time(forecast.issue_time_unix_ns, clock.observed)?;
    optional_time(forecast.issued_at_unix_ns, clock.observed)?;
    validate_envelope(&forecast.envelope, clock)?;
    for value in [
        &forecast.bundle_name,
        &forecast.bundle_id,
        &forecast.code_commit,
    ] {
        optional_text(&Some(value.clone()))?;
    }
    optional_text(&forecast.fallback)?;
    optional_text(&forecast.fallback_reason)?;
    if forecast.degraded_reasons.len() > MAX_NOWCAST_REASONS
        || forecast.stale_sources.len() > MAX_NOWCAST_REASONS
        || forecast.levels.len() > MAX_NOWCAST_LEVELS
        || forecast.leads.len() > MAX_NOWCAST_LEADS
    {
        return Err(DecisionV6Error::BoundExceeded);
    }
    for reason in forecast
        .degraded_reasons
        .iter()
        .chain(&forecast.stale_sources)
    {
        text(reason)?;
    }
    for value in [forecast.own_age_min, forecast.ref_f] {
        optional_decimal(value)?;
    }
    // Levels: within (0, 1), strictly increasing (by value: `Decimal`'s own order is not
    // numeric).
    if forecast.levels.is_empty()
        || forecast
            .levels
            .iter()
            .any(|level| !(level.is_canonical() && level.to_f64() > 0.0 && level.to_f64() < 1.0))
    {
        return Err(DecisionV6Error::InvalidContract);
    }
    if forecast
        .levels
        .windows(2)
        .any(|pair| pair[0].to_f64() >= pair[1].to_f64())
    {
        return Err(DecisionV6Error::NonCanonicalOrder);
    }
    strictly_sorted(forecast.leads.iter().map(|lead| lead.lead_min))?;
    for lead in &forecast.leads {
        if lead.lead_min == 0 || lead.q.len() != forecast.levels.len() {
            return Err(DecisionV6Error::InvalidContract);
        }
        time(lead.target_time_unix_ns, clock.forecast)?;
        decimal(lead.median)?;
        for value in &lead.q {
            decimal(*value)?;
        }
        for value in [lead.median_model, lead.rc_shift, lead.rc_scale] {
            optional_decimal(value)?;
        }
    }
    Ok(())
}

fn validate_artifact(
    artifact: &SuppliedNowcastArtifactV6,
    clock: &Clock,
) -> Result<(), DecisionV6Error> {
    identifier(&artifact.kind)?;
    identifier(&artifact.model_id)?;
    text(&artifact.version)?;
    // Unbounded above: an artefact may be published ahead of the time it applies from.
    time(artifact.valid_from_unix_ns, i64::MAX)?;
    optional_time(artifact.published_at_unix_ns, clock.observed)?;
    validate_envelope(&artifact.envelope, clock)?;
    if artifact.body_json.len() > MAX_NOWCAST_ARTIFACT_BYTES {
        return Err(DecisionV6Error::BoundExceeded);
    }
    if artifact.sha256 != sha256_hex(artifact.body_json.as_bytes()) {
        return Err(DecisionV6Error::InvalidContract);
    }
    Ok(())
}

/// Lowercase hex SHA-256: an artefact's `sha256` of its `body_json`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut text = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// A city with three issues of two stations, every artefact kind, member METARs, the last
/// issue and a heartbeat, for the corpus and the tests.
#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;
    use crate::decision_v4::{AuthorityV4, ProvenanceV4};
    use crate::hourly_index_v6::fixtures::{T0_S, d, ns};

    pub(crate) const SEQ: u64 = 9_120;
    pub(crate) const MEMBERS: [&str; 5] = ["KFLL", "KFXE", "KMIA", "KOPF", "KPMP"];
    /// The canonical body of the blend artefact (keys sorted, no whitespace).
    pub(crate) const BLEND_BODY: &str = r#"{"cities":["chicago","la-coastal","miami","nyc"],"coef":[1.1383442675728548,-0.2922350385190219],"intercept":-0.3241092365517112}"#;

    fn envelope(seq: u64, at_s: i64) -> SuppliedNowcastEnvelopeV6 {
        SuppliedNowcastEnvelopeV6 {
            event_id: format!("nowcast-{seq}"),
            seq,
            emitted_at_unix_ns: Some(ns(at_s)),
            received_at_unix_ns: ns(at_s) + 2_000_000,
        }
    }

    fn levels() -> Vec<DecimalV6> {
        (1..=19).map(|k| d(&format!("0.{:02}", 5 * k))).collect()
    }

    pub(crate) fn forecast(station_id: &str, issue_s: i64, seq: u64) -> SuppliedStationForecastV6 {
        SuppliedStationForecastV6 {
            station_id: station_id.to_owned(),
            issue_time_unix_ns: ns(issue_s),
            issued_at_unix_ns: Some(ns(issue_s + 35)),
            envelope: envelope(seq, issue_s + 36),
            bundle_name: "full".to_owned(),
            bundle_id: "full-s0_base-1y-20261021".to_owned(),
            code_commit: "daccc4a".to_owned(),
            degraded: station_id == "KOPF",
            degraded_reasons: if station_id == "KOPF" {
                vec!["own_reading_stale:31min".to_owned()]
            } else {
                Vec::new()
            },
            stale_sources: Vec::new(),
            fallback: None,
            fallback_reason: None,
            own_age_min: Some(d("3")),
            ref_f: Some(d("84.2")),
            levels: levels(),
            leads: (1..=24)
                .map(|k| {
                    let lead_min = 5 * k;
                    SuppliedNowcastLeadV6 {
                        lead_min,
                        target_time_unix_ns: ns(issue_s + 60 * i64::from(lead_min)),
                        median: d("84.31"),
                        q: (0..19)
                            .map(|i| d(&format!("{:.3}", 83.0 + 0.137 * i as f64)))
                            .collect(),
                        median_model: Some(d("84.31")),
                        rc_shift: Some(d("0")),
                        rc_scale: Some(d("1")),
                    }
                })
                .collect(),
        }
    }

    pub(crate) fn artifact(
        kind: &str,
        model_id: &str,
        valid_from_s: i64,
        body: &str,
        seq: u64,
    ) -> SuppliedNowcastArtifactV6 {
        SuppliedNowcastArtifactV6 {
            kind: kind.to_owned(),
            model_id: model_id.to_owned(),
            version: format!("{model_id}-{valid_from_s}"),
            sha256: sha256_hex(body.as_bytes()),
            valid_from_unix_ns: ns(valid_from_s),
            published_at_unix_ns: Some(ns(T0_S - 7_200)),
            envelope: envelope(seq, T0_S - 7_000),
            body_json: body.to_owned(),
        }
    }

    pub(crate) fn snapshot() -> NowcastInputV6 {
        let issues = [T0_S - 900, T0_S - 600, T0_S - 300];
        NowcastInputV6 {
            supplied: SuppliedNowcastV6 {
                city: "miami".to_owned(),
                seq: SEQ,
                stations: MEMBERS
                    .iter()
                    .map(|station| (*station).to_owned())
                    .collect(),
                forecasts: ["KMIA", "KOPF"]
                    .iter()
                    .map(|station| SuppliedNowcastStationV6 {
                        station_id: (*station).to_owned(),
                        issues: issues
                            .iter()
                            .enumerate()
                            .map(|(k, issue)| forecast(station, *issue, SEQ - 10 + k as u64))
                            .collect(),
                    })
                    .collect(),
                artifacts: vec![
                    artifact(
                        "bias_table",
                        "biascorr_roll7d",
                        T0_S - 3_600,
                        r#"{"b":{"KMIA":[0.1,0.2,0.3,0.4]},"buckets":[[5,15],[20,30],[35,60],[65,120]]}"#,
                        SEQ - 30,
                    ),
                    artifact("blend", "lr_full", T0_S - 86_400, BLEND_BODY, SEQ - 40),
                    // The next version, not yet valid.
                    artifact(
                        "blend",
                        "lr_full",
                        T0_S + 3_600,
                        r#"{"coef":[1.0]}"#,
                        SEQ - 20,
                    ),
                    artifact(
                        "copula",
                        "copula_biascorr",
                        T0_S - 86_400,
                        r#"{"R|miami|0-15":{"matrix":[[1.0,0.0946],[0.0946,1.0]],"stations":["KMIA","KOPF"]}}"#,
                        SEQ - 41,
                    ),
                ],
                member_metars: vec![
                    SuppliedMemberMetarV6 {
                        station_id: "KMIA".to_owned(),
                        valid_time_unix_ns: ns(T0_S - 540),
                        first_seen_unix_ns: Some(ns(T0_S - 360)),
                        temp_f: Some(d("84.2")),
                        t_ok: true,
                        envelope: envelope(SEQ - 5, T0_S - 359),
                    },
                    SuppliedMemberMetarV6 {
                        station_id: "KOPF".to_owned(),
                        valid_time_unix_ns: ns(T0_S - 540),
                        first_seen_unix_ns: None,
                        temp_f: None,
                        t_ok: false,
                        envelope: envelope(SEQ - 4, T0_S - 358),
                    },
                ],
                last_issue: Some(SuppliedNowcastIssueV6 {
                    issue_time_unix_ns: ns(T0_S - 300),
                    stations: vec!["KMIA".to_owned(), "KOPF".to_owned()],
                    failed: vec!["KFXE".to_owned()],
                    seconds: Some(d("35.2")),
                    envelope: envelope(SEQ, T0_S - 260),
                }),
                heartbeat: Some(SuppliedNowcastHeartbeatV6 {
                    received_at_unix_ns: ns(T0_S - 5),
                    generated_at_unix_ns: Some(ns(T0_S - 5)),
                    last_issue_time_unix_ns: Some(ns(T0_S - 300)),
                    last_issue_ok: Some(true),
                    code_commit: Some("daccc4a".to_owned()),
                }),
            },
            components: NowcastComponentsV6 {
                feed: ComponentMetaV4 {
                    authority: AuthorityV4::Current,
                    revision: 41,
                    generation: 1,
                    updated_at_unix_ms: Some((T0_S - 260) * 1_000),
                    expected_version: None,
                    provenance: vec![ProvenanceV4 {
                        provider: "nowcast_runner".to_owned(),
                        source: "nowcast-ws/1".to_owned(),
                        event_id: Some("nowcast-9120".to_owned()),
                        connection_epoch: Some(1),
                        city_sequence: Some(SEQ),
                        provider_at_unix_ms: Some((T0_S - 260) * 1_000),
                        received_at_unix_ms: (T0_S - 260) * 1_000,
                        ..ProvenanceV4::default()
                    }],
                    refresh_error: None,
                },
            },
            unavailable: None,
        }
    }

    /// The runner is down: the connection was lost, the state held over until a snapshot.
    pub(crate) fn disconnected() -> NowcastInputV6 {
        let mut input = snapshot();
        input.components.feed.authority = AuthorityV4::Unavailable;
        input.components.feed.refresh_error =
            Some("the connection was lost; awaiting a snapshot".to_owned());
        input.unavailable = Some(NowcastUnavailableReasonV6::Disconnected);
        input
    }

    /// A subscribed city before its first snapshot.
    pub(crate) fn no_snapshot() -> NowcastInputV6 {
        NowcastInputV6 {
            supplied: SuppliedNowcastV6 {
                city: "miami".to_owned(),
                ..Default::default()
            },
            components: NowcastComponentsV6 {
                feed: ComponentMetaV4 {
                    authority: AuthorityV4::Unavailable,
                    revision: 1,
                    generation: 1,
                    ..ComponentMetaV4::default()
                },
            },
            unavailable: Some(NowcastUnavailableReasonV6::NoSnapshot),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use crate::decision_v4::AuthorityV4;
    use crate::hourly_index_v6::fixtures::{DECISION_MS, T0_S, d, ns};

    fn rejects(mutate: impl FnOnce(&mut NowcastInputV6), error: DecisionV6Error) {
        let mut input = snapshot();
        mutate(&mut input);
        assert_eq!(input.validate(DECISION_MS), Err(error));
    }

    #[test]
    fn the_fixtures_are_valid() {
        snapshot().validate(DECISION_MS).unwrap();
        disconnected().validate(DECISION_MS).unwrap();
        no_snapshot().validate(DECISION_MS).unwrap();
    }

    /// Every rule of the contract, one mutation each.
    #[test]
    fn malformed_nowcasts_are_rejected_with_their_category() {
        type Mutation = fn(&mut NowcastInputV6);
        let cases: &[(&str, Mutation, DecisionV6Error)] = &[
            (
                "a forecast of a station that is not a member",
                |input| input.supplied.stations.retain(|station| station != "KMIA"),
                DecisionV6Error::InvalidContract,
            ),
            (
                "a METAR of a station that is not a member",
                |input| {
                    input.supplied.member_metars[1].station_id = "KZZZ".to_owned();
                },
                DecisionV6Error::InvalidContract,
            ),
            (
                "current with a reason",
                |input| input.unavailable = Some(NowcastUnavailableReasonV6::Stale),
                DecisionV6Error::InvalidContract,
            ),
            (
                "unavailable without a reason",
                |input| input.components.feed.authority = AuthorityV4::Unavailable,
                DecisionV6Error::InvalidContract,
            ),
            (
                "uncertain is not a delivered authority",
                |input| {
                    input.components.feed.authority = AuthorityV4::Uncertain;
                    input.unavailable = Some(NowcastUnavailableReasonV6::Disconnected);
                },
                DecisionV6Error::InvalidContract,
            ),
            (
                "city id",
                |input| input.supplied.city = "mi ami".to_owned(),
                DecisionV6Error::InvalidContract,
            ),
            (
                "stations unsorted",
                |input| input.supplied.stations.swap(0, 1),
                DecisionV6Error::NonCanonicalOrder,
            ),
            (
                "forecast stations unsorted",
                |input| input.supplied.forecasts.swap(0, 1),
                DecisionV6Error::NonCanonicalOrder,
            ),
            (
                "issues newest first",
                |input| input.supplied.forecasts[0].issues.reverse(),
                DecisionV6Error::NonCanonicalOrder,
            ),
            (
                "too many issues",
                |input| {
                    let extra = forecast("KMIA", T0_S - 1_200, 1);
                    input.supplied.forecasts[0].issues.insert(0, extra);
                },
                DecisionV6Error::BoundExceeded,
            ),
            (
                "a station without issues",
                |input| input.supplied.forecasts[0].issues.clear(),
                DecisionV6Error::InvalidContract,
            ),
            (
                "an issue of another station",
                |input| input.supplied.forecasts[0].issues[0].station_id = "KOPF".to_owned(),
                DecisionV6Error::InvalidContract,
            ),
            (
                "quantiles not aligned with levels",
                |input| {
                    input.supplied.forecasts[0].issues[0].leads[3].q.pop();
                },
                DecisionV6Error::InvalidContract,
            ),
            (
                "a level outside (0, 1)",
                |input| input.supplied.forecasts[0].issues[0].levels[18] = d("1"),
                DecisionV6Error::InvalidContract,
            ),
            (
                "levels not increasing",
                |input| input.supplied.forecasts[0].issues[0].levels.swap(0, 1),
                DecisionV6Error::NonCanonicalOrder,
            ),
            (
                "leads not increasing",
                |input| input.supplied.forecasts[0].issues[0].leads.swap(0, 1),
                DecisionV6Error::NonCanonicalOrder,
            ),
            (
                "an issue in the future",
                |input| input.supplied.forecasts[0].issues[2].issue_time_unix_ns = ns(T0_S + 3_600),
                DecisionV6Error::InvalidContract,
            ),
            (
                "artefact hash",
                |input| input.supplied.artifacts[0].body_json.push(' '),
                DecisionV6Error::InvalidContract,
            ),
            (
                "artefacts unsorted",
                |input| input.supplied.artifacts.swap(1, 2),
                DecisionV6Error::NonCanonicalOrder,
            ),
            (
                "artefact body over its bound",
                |input| {
                    let body = format!("\"{}\"", "x".repeat(MAX_NOWCAST_ARTIFACT_BYTES));
                    input.supplied.artifacts[0].sha256 = sha256_hex(body.as_bytes());
                    input.supplied.artifacts[0].body_json = body;
                },
                DecisionV6Error::BoundExceeded,
            ),
            (
                "metars unsorted",
                |input| input.supplied.member_metars.swap(0, 1),
                DecisionV6Error::NonCanonicalOrder,
            ),
            (
                "feed metadata revision 0",
                |input| input.components.feed.revision = 0,
                DecisionV6Error::InvalidContract,
            ),
            (
                "feed updated in the future",
                |input| input.components.feed.updated_at_unix_ms = Some((T0_S + 3_600) * 1_000),
                DecisionV6Error::InvalidContract,
            ),
        ];
        for (name, mutate, error) in cases {
            let mut input = snapshot();
            mutate(&mut input);
            assert_eq!(input.validate(DECISION_MS), Err(error.clone()), "{name}");
        }
        rejects(
            |input| {
                input.supplied.stations = (0..=MAX_NOWCAST_STATIONS)
                    .map(|k| format!("K{k:03}"))
                    .collect()
            },
            DecisionV6Error::BoundExceeded,
        );
    }
}
