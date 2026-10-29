//! A city's nowcast from the weather forecast runner, as host state.
//!
//! The runner issues, every five minutes, a quantile forecast of each member station of a
//! MinuteTemp index city at leads of 5 to 120 minutes, and publishes the versioned model
//! artefacts (blend coefficients, copula, bias table) a kernel combines them with. The host
//! (traderd) folds the runner's `nowcast-ws/1` feed into one [`SuppliedNowcast`] per city and
//! delivers it beside the hourly index; the kernel values it. Combining stations, choosing a
//! usable issue and gating orders are Strategy policy.
//!
//! The `Supplied*` structs are the originals at the runner's precision (`Decimal`, times as
//! Unix nanoseconds) with the host's receipt time of each. [`NowcastState`] adds the host's
//! authority of the feed. The host keeps the newest [`MAX_NOWCAST_ISSUES`] complete issues of
//! each station (oldest first), so a kernel can pick the newest issue that was usable at its
//! decision time; the runner's own time stamps (`issue_time`, `issued_at`) and the host's
//! receipt (`received_at`) are both kept.

use bincode::{Decode, Encode};

use crate::decimal::Decimal;
use crate::state::ComponentMeta;

/// Complete issues kept per station, oldest first.
pub const MAX_NOWCAST_ISSUES: usize = 3;

/// Where a sequenced runner event came from and when the host received it.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedNowcastEnvelope {
    /// The runner's `event_id`: globally unique and stable across replays.
    pub event_id: String,
    /// The city's `seq` of the event (of the snapshot, for state the snapshot carried).
    pub seq: u64,
    /// The runner's `emitted_at`.
    pub emitted_at_unix_ns: Option<i64>,
    /// When the host read the frame off the socket.
    pub received_at_unix_ns: i64,
}

/// One lead of a station forecast: `q[i]` is the forecast temperature (°F, as issued: after
/// the runner's rolling layer, before any bias correction) at quantile `levels[i]`.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedNowcastLead {
    pub lead_min: u32,
    /// `issue_time + lead_min`.
    pub target_time_unix_ns: i64,
    pub median: Decimal,
    /// One value per level of the forecast's `levels`.
    pub q: Vec<Decimal>,
    /// The bundle's median before the rolling layer.
    pub median_model: Option<Decimal>,
    /// The rolling layer's correction (identity: shift 0, scale 1).
    pub rc_shift: Option<Decimal>,
    pub rc_scale: Option<Decimal>,
}

/// One station's forecast of one issue (the runner's `station_forecast` payload).
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedStationForecast {
    pub station_id: String,
    /// The 5-minute grid time the forecast is for; it uses inputs known by about this time.
    pub issue_time_unix_ns: i64,
    /// When the runner produced the payload.
    pub issued_at_unix_ns: Option<i64>,
    pub envelope: SuppliedNowcastEnvelope,
    pub bundle_name: String,
    pub bundle_id: String,
    pub code_commit: String,
    pub degraded: bool,
    /// Sorted, as the runner sent them.
    pub degraded_reasons: Vec<String>,
    pub stale_sources: Vec<String>,
    /// The fallback bundle that issued, when the primary could not.
    pub fallback: Option<String>,
    pub fallback_reason: Option<String>,
    /// `input_ages.own`: minutes since the station's own reading the model used.
    pub own_age_min: Option<Decimal>,
    /// The station's own reading (°F) the model used.
    pub ref_f: Option<Decimal>,
    /// Quantile levels, strictly increasing in (0, 1).
    pub levels: Vec<Decimal>,
    /// Strictly by lead.
    pub leads: Vec<SuppliedNowcastLead>,
}

/// The complete issues the host keeps of one station, oldest first.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedNowcastStation {
    pub station_id: String,
    /// Strictly by issue time, at most [`MAX_NOWCAST_ISSUES`].
    pub issues: Vec<SuppliedStationForecast>,
}

/// A versioned model artefact (`model_artifact`): blend coefficients, a copula, a bias table
/// or a taker's combined coefficient set. `body_json` is the artefact's body serialized
/// canonically (UTF-8 JSON, object keys sorted, no whitespace), the bytes `sha256` is the hex
/// SHA-256 of.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedNowcastArtifact {
    /// `blend`, `copula`, `bias_table` or `combined` (the open-window taker's coefficient
    /// sets). Any identifier is carried; a kernel reads the kinds it knows by exact name, so a
    /// `combined` set never stands in for a `blend`.
    pub kind: String,
    /// E.g. `lr_full`, `lr_open`, `copula_biascorr`, `biascorr_roll7d`.
    pub model_id: String,
    pub version: String,
    /// Lowercase hex.
    pub sha256: String,
    /// The artefact applies from this time; a host keeps a next version before it is valid.
    pub valid_from_unix_ns: i64,
    pub published_at_unix_ns: Option<i64>,
    pub envelope: SuppliedNowcastEnvelope,
    pub body_json: String,
}

/// The runner's decoded METAR of a member station (`member_metar`).
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedMemberMetar {
    pub station_id: String,
    pub valid_time_unix_ns: i64,
    /// When the runner first saw the report.
    pub first_seen_unix_ns: Option<i64>,
    pub temp_f: Option<Decimal>,
    /// The runner's quality flag on the temperature.
    pub t_ok: bool,
    pub envelope: SuppliedNowcastEnvelope,
}

/// The newest whole issue of the city (`issue_complete`, or the snapshot's `last_issue`).
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedNowcastIssue {
    pub issue_time_unix_ns: i64,
    /// Stations issued, sorted.
    pub stations: Vec<String>,
    /// Stations that failed, sorted.
    pub failed: Vec<String>,
    /// How long the issue took the runner.
    pub seconds: Option<Decimal>,
    pub envelope: SuppliedNowcastEnvelope,
}

/// The runner's latest unsequenced `heartbeat`, which says the connection is alive even when
/// the runner is not issuing.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedNowcastHeartbeat {
    pub received_at_unix_ns: i64,
    pub generated_at_unix_ns: Option<i64>,
    pub last_issue_time_unix_ns: Option<i64>,
    pub last_issue_ok: Option<bool>,
    pub code_commit: Option<String>,
}

/// One city's nowcast as the host folded it from the snapshot and the `seq` events.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedNowcast {
    /// MinuteTemp's index city id (`miami`, `nyc`, `chicago`, `la-coastal`).
    pub city: String,
    /// The last runner `seq` folded in; 0 before the first snapshot.
    pub seq: u64,
    /// The city's member stations as the runner lists them, sorted.
    pub stations: Vec<String>,
    /// Strictly by station id; only stations with at least one complete issue.
    pub forecasts: Vec<SuppliedNowcastStation>,
    /// Strictly by (`kind`, `model_id`, `valid_from`): each kind and model's version in force
    /// and any next one not yet valid.
    pub artifacts: Vec<SuppliedNowcastArtifact>,
    /// Strictly by (`station_id`, `valid_time`).
    pub member_metars: Vec<SuppliedMemberMetar>,
    pub last_issue: Option<SuppliedNowcastIssue>,
    pub heartbeat: Option<SuppliedNowcastHeartbeat>,
}

impl SuppliedNowcast {
    /// The issues held of `station_id`, oldest first; empty when it has none.
    pub fn issues(&self, station_id: &str) -> &[SuppliedStationForecast] {
        self.forecasts
            .iter()
            .find(|station| station.station_id == station_id)
            .map_or(&[], |station| station.issues.as_slice())
    }

    /// The artefacts of `kind` and `model_id`, oldest `valid_from` first.
    pub fn artifacts_of<'a>(
        &'a self,
        kind: &'a str,
        model_id: &'a str,
    ) -> impl Iterator<Item = &'a SuppliedNowcastArtifact> + 'a {
        self.artifacts
            .iter()
            .filter(move |artifact| artifact.kind == kind && artifact.model_id == model_id)
    }
}

// ---------------------------------------------------------------------------------------------
// Kernel state
// ---------------------------------------------------------------------------------------------

/// Why the host delivers a city's nowcast as unavailable. A Strategy gated on the nowcast
/// does not trade while it is unavailable (fail closed). New reasons are appended only.
#[derive(Clone, Copy, Debug, Encode, Decode, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NowcastUnavailableReason {
    /// A Strategy asked but the host configures no runner source.
    NotConfigured,
    /// The host has no API key for the runner (missing, unreadable or empty key file).
    NoKey,
    /// The runner refused the key.
    AuthFailed,
    /// The connection is down or was lost; held state awaits a fresh snapshot.
    Disconnected,
    /// Connected and subscribed, but no snapshot yet.
    NoSnapshot,
    /// A `seq` gap that has not (yet) been closed by a resume.
    Gap,
    /// The runner could not replay a gap; awaiting a fresh snapshot.
    ResumeFailed,
    /// No frame or heartbeat for too long, or the newest issue is too old.
    Stale,
    /// No model artefact is held.
    NoArtifact,
    /// An artefact body did not hash to its `sha256`.
    HashMismatch,
    /// The runner refused the city's subscription.
    Rejected,
    /// The runner speaks another protocol version.
    ProtocolMismatch,
    /// A frame for the city could not be read.
    Unreadable,
    /// The held state fails the contract at the decision's time.
    Invalid,
}

impl NowcastUnavailableReason {
    pub const ALL: [Self; 14] = [
        Self::NotConfigured,
        Self::NoKey,
        Self::AuthFailed,
        Self::Disconnected,
        Self::NoSnapshot,
        Self::Gap,
        Self::ResumeFailed,
        Self::Stale,
        Self::NoArtifact,
        Self::HashMismatch,
        Self::Rejected,
        Self::ProtocolMismatch,
        Self::Unreadable,
        Self::Invalid,
    ];

    /// The stable snake_case token, for logs and read models.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotConfigured => "not_configured",
            Self::NoKey => "no_key",
            Self::AuthFailed => "auth_failed",
            Self::Disconnected => "disconnected",
            Self::NoSnapshot => "no_snapshot",
            Self::Gap => "gap",
            Self::ResumeFailed => "resume_failed",
            Self::Stale => "stale",
            Self::NoArtifact => "no_artifact",
            Self::HashMismatch => "hash_mismatch",
            Self::Rejected => "rejected",
            Self::ProtocolMismatch => "protocol_mismatch",
            Self::Unreadable => "unreadable",
            Self::Invalid => "invalid",
        }
    }
}

impl std::fmt::Display for NowcastUnavailableReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One city's nowcast as the host delivered it for this invocation.
///
/// Fail closed: `feed.authority` is `Current` with `unavailable == None` when the host vouches
/// for the feed at the decision's time, and otherwise `Unavailable` with the reason. A host
/// delivers a nowcast to every Strategy that asks, so a Strategy that asked and reads none
/// treats it as unavailable too. Prior state stays resident in `supplied` whatever the
/// authority; whether an issue is still usable is also the kernel's rule (by its issue time).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NowcastState {
    pub city: String,
    pub feed: ComponentMeta,
    pub unavailable: Option<NowcastUnavailableReason>,
    pub supplied: SuppliedNowcast,
}

impl NowcastState {
    pub fn from_supplied(
        nowcast: &SuppliedNowcast,
        feed: ComponentMeta,
        unavailable: Option<NowcastUnavailableReason>,
    ) -> Self {
        Self {
            city: nowcast.city.clone(),
            feed,
            unavailable,
            supplied: nowcast.clone(),
        }
    }

    /// The nowcast when the host vouches for it, else why not.
    pub fn available(&self) -> Result<&SuppliedNowcast, NowcastUnavailableReason> {
        match self.unavailable {
            Some(reason) => Err(reason),
            None if self.feed.authority == crate::state::ComponentAuthority::Current => {
                Ok(&self.supplied)
            }
            None => Err(NowcastUnavailableReason::Invalid),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ComponentAuthority;

    /// Reason tokens are distinct and snake_case; only a current feed without a reason is
    /// available.
    #[test]
    fn reasons_have_stable_tokens_and_only_current_is_available() {
        let tokens = NowcastUnavailableReason::ALL
            .iter()
            .map(|reason| reason.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(tokens.len(), NowcastUnavailableReason::ALL.len());
        assert!(tokens.iter().all(|token| {
            token
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
        }));

        let mut state = NowcastState::default();
        state.feed.authority = ComponentAuthority::Current;
        assert!(state.available().is_ok());
        state.unavailable = Some(NowcastUnavailableReason::Stale);
        assert_eq!(state.available(), Err(NowcastUnavailableReason::Stale));
        state.unavailable = None;
        state.feed.authority = ComponentAuthority::Unavailable;
        assert_eq!(state.available(), Err(NowcastUnavailableReason::Invalid));
    }
}
