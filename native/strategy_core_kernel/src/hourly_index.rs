//! MinuteTemp's hourly Kalshi Weather Index of one city, as host state.
//!
//! MinuteTemp computes the index with Kalshi's calibration and quorum; a kernel reads it here
//! and never rebuilds it from member observations. The host folds the city's
//! `hourly_index_snapshot` and its per-city `seq` events (`index_minute`, `hour_update`,
//! `forecast_index`, `forecast_bias`, `hour_settled`, `hour_reconciled`, `calibration`) into
//! one [`SuppliedHourlyIndex`] and delivers it with one [`ComponentMeta`] per stream. The
//! state has no event of its own: a kernel reads it when it wakes (on its timer or any other
//! event) through [`crate::StrategyKernelState::hourly_index`].
//!
//! The `Supplied*` structs are the originals at the provider's precision (`Decimal`, times as
//! `*_unix_ns`, `Option` for absent-or-null); bounds, ordering and validation of a complete
//! block belong to the codec that carries it (`strategy_core_v3::hourly_index_v6`). The
//! kernel types beside them carry `f64` and [`DateTime`] conveniences, the supplied original
//! and a [`ValueOrigin`]. Excluded on purpose: the ladder and `market_update` (prices come from
//! the Kalshi feed), per-member forecast series, the derivable `adjusted_f` curve, and match
//! rates.

use bincode::{Decode, Encode};
use chrono::{DateTime, Utc};

use crate::decimal::Decimal;
use crate::events::ValueOrigin;
use crate::state::{ComponentMeta, EventProvenance, f64_of, nanos};

/// Phase of an index minute.
#[derive(Clone, Copy, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub enum IndexPhase {
    /// MinuteTemp's own value from the members' readings, before Kalshi publishes.
    #[default]
    Provisional,
    /// Kalshi's published value.
    Official,
    /// Declared missing: no value, nothing carried forward.
    Missing,
}

/// Status of the hour an [`IndexHour`] describes.
#[derive(Clone, Copy, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub enum IndexHourStatus {
    #[default]
    Open,
    /// Closed but not yet determined.
    Pending,
}

/// Outcome of a settled hour.
#[derive(Clone, Copy, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub enum IndexSettlementStatus {
    #[default]
    Determined,
    /// No official minute in the lookback: the hour has no value.
    NoValue,
}

/// Kind of a feed condition. A kind this contract does not know yet is `Other` with the
/// provider's text, never one of the known names.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub enum FeedConditionKind {
    #[default]
    PersistentFallback,
    IntermittentFallback,
    MemberExcluded,
    QcSubstitution,
    QuorumRisk,
    UnavailableMinutes,
    Other(String),
}

impl FeedConditionKind {
    /// The provider's name of a known kind.
    pub const KNOWN: [(&'static str, FeedConditionKind); 6] = [
        ("persistent_fallback", Self::PersistentFallback),
        ("intermittent_fallback", Self::IntermittentFallback),
        ("member_excluded", Self::MemberExcluded),
        ("qc_substitution", Self::QcSubstitution),
        ("quorum_risk", Self::QuorumRisk),
        ("unavailable_minutes", Self::UnavailableMinutes),
    ];

    /// The kind the provider's name stands for: a known kind, else `Other`.
    pub fn from_name(name: &str) -> Self {
        Self::KNOWN
            .into_iter()
            .find(|(known, _)| *known == name)
            .map_or_else(|| Self::Other(name.to_owned()), |(_, kind)| kind)
    }

    /// The provider's name.
    pub fn name(&self) -> &str {
        match self {
            Self::Other(name) => name.as_str(),
            known => Self::KNOWN
                .iter()
                .find(|(_, kind)| kind == known)
                .map_or("", |(name, _)| *name),
        }
    }
}

/// Severity of a feed condition (`critical` for `quorum_risk` and `unavailable_minutes`).
#[derive(Clone, Copy, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub enum FeedConditionSeverity {
    #[default]
    Warning,
    Critical,
}

// ---------------------------------------------------------------------------------------------
// Supplied originals
// ---------------------------------------------------------------------------------------------

/// The hourly-index envelope of the event (or snapshot) that last set a minute.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedHourlyIndexEnvelope {
    pub event_id: Option<String>,
    /// The per-city `seq` the collector assigned.
    pub seq: u64,
    pub emitted_at_unix_ns: Option<i64>,
    /// Host socket receipt time; host evidence rather than provider data.
    pub received_at_unix_ns: i64,
}

/// One member's reading inside a city-minute, as supplied.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedIndexStationReading {
    pub station_id: String,
    pub temp_f: Option<Decimal>,
    /// Kalshi's reading code (`ok`, `missing`, `late`, `range`, `rate_spatial`, `extreme`,
    /// `pending`).
    pub code: String,
    /// `hf_asos` (1-minute ASOS) or `metar` (fallback).
    pub source: Option<String>,
    pub contributes: bool,
    pub weight: Decimal,
    pub offset_c: Option<Decimal>,
    /// Leave-one-out pull on the index, °F.
    pub pull_f: Option<Decimal>,
    pub change_5m_f: Option<Decimal>,
    pub hour_high_f: Option<Decimal>,
    pub hour_low_f: Option<Decimal>,
    /// Kalshi's receipt time of the reading.
    pub received_at_unix_ns: Option<i64>,
}

/// One city-minute, as supplied.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedIndexMinute {
    /// Absent for a minute from a REST baseline rather than a stream event or snapshot.
    pub envelope: Option<SuppliedHourlyIndexEnvelope>,
    pub source: String,
    pub minute_unix_ns: i64,
    pub phase: IndexPhase,
    /// The provider's `final`: the minute never goes back to provisional.
    pub is_final: bool,
    pub revision: u64,
    /// `official_f` when `phase` is official, else `provisional_f`.
    pub value_f: Option<Decimal>,
    pub official_f: Option<Decimal>,
    pub provisional_f: Option<Decimal>,
    /// `normal`, `degraded` or `incomplete`.
    pub kalshi_status: Option<String>,
    pub config_version: String,
    pub contributors: u32,
    pub weight_share: Decimal,
    pub quorum_met: bool,
    pub calibration_pending: bool,
    pub provisional_at_unix_ns: Option<i64>,
    pub official_at_unix_ns: Option<i64>,
    /// Member readings, strictly sorted by station id; empty when the provider sent none.
    pub stations: Vec<SuppliedIndexStationReading>,
}

/// The value the hour would settle at now: the last official minute in the lookback.
#[derive(Clone, Copy, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedIndexMinuteValue {
    pub minute_unix_ns: i64,
    pub value_f: Decimal,
}

/// One degradation of the city's index feed, as supplied.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedFeedCondition {
    pub kind: FeedConditionKind,
    pub severity: FeedConditionSeverity,
    /// The member station; `None` for `quorum_risk` and `unavailable_minutes`.
    pub station_id: Option<String>,
    pub minutes: u32,
    pub window_minutes: u32,
    pub since_unix_ns: i64,
    pub last_seen_unix_ns: i64,
}

/// The open or closed-but-undetermined hour, as supplied (`hour_update`, the snapshot's
/// `current_hour`).
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedIndexHour {
    pub hour_end_unix_ns: i64,
    pub status: IndexHourStatus,
    pub event_ticker: Option<String>,
    pub settles_now_as: Option<SuppliedIndexMinuteValue>,
    pub high_f: Option<Decimal>,
    pub low_f: Option<Decimal>,
    /// The HRRR index forecast at `hour_end`, not bias-adjusted.
    pub forecast_settle_f: Option<Decimal>,
    pub forecast_bias_f: Option<Decimal>,
    pub forecast_adjusted_settle_f: Option<Decimal>,
    /// `hour_end` + 5 min, when the top-of-hour minute is official.
    pub final_at_unix_ns: i64,
    /// `hour_end` + 8 min, the latest time the settle is determined.
    pub determine_by_unix_ns: i64,
    /// Empty when the feed is clean.
    pub feed_conditions: Vec<SuppliedFeedCondition>,
}

/// Forecast settle of an upcoming hour (the step at its top of hour), as supplied.
#[derive(Clone, Copy, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedIndexForecastSettle {
    pub hour_end_unix_ns: i64,
    pub value_f: Option<Decimal>,
    pub quorum_met: bool,
    pub partial: bool,
}

/// One member's bias at the bias minute.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedMemberBias {
    pub station_id: String,
    pub bias_f: Option<Decimal>,
}

/// The forecast's bias (`forecast_bias`, or a forecast's own bias fields). Absent when the
/// provider's `bias_f` is null.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedIndexForecastBias {
    /// Newest valued index minute minus the forecast at that minute, °F.
    pub bias_f: Decimal,
    pub as_of_minute_unix_ns: i64,
    /// Strictly sorted by station id.
    pub member_bias: Vec<SuppliedMemberBias>,
}

/// One model's 15-minute index forecast, as supplied (`forecast_index`, the snapshot's
/// `forecast`). The per-step arrays are the provider's and are aligned to `steps_unix_ns`.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedIndexForecast {
    pub model_id: String,
    pub run_time_unix_ns: i64,
    pub fetched_at_unix_ns: i64,
    pub config_version: String,
    /// Members with no forecast in this run, strictly sorted.
    pub missing_members: Vec<String>,
    pub steps_unix_ns: Vec<i64>,
    pub value_f: Vec<Option<Decimal>>,
    pub quorum_met: Vec<bool>,
    pub partial: Vec<bool>,
    /// The next top-of-hour steps after the refresh.
    pub settles: Vec<SuppliedIndexForecastSettle>,
    pub bias: Option<SuppliedIndexForecastBias>,
}

/// MinuteTemp's settlement of one hour, as supplied (`hour_settled`, `hour_reconciled`).
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedIndexSettlement {
    pub hour_end_unix_ns: i64,
    pub status: IndexSettlementStatus,
    pub event_ticker: Option<String>,
    pub settle_minute_unix_ns: Option<i64>,
    pub settle_value_f: Option<Decimal>,
    pub winning_floor_strike: Option<Decimal>,
    /// The city has no published quorum rule.
    pub informational: bool,
    pub determined_at_unix_ns: i64,
    /// Null until Kalshi finalizes the market.
    pub kalshi_expiration_value: Option<Decimal>,
    pub kalshi_finalized_at_unix_ns: Option<i64>,
    /// The provider's `match`: our settle equals Kalshi's; null until Kalshi finalizes.
    pub matched: Option<bool>,
    pub revision: u64,
}

/// One calibrated member.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedIndexMember {
    pub station_id: String,
    pub weight: Decimal,
    pub offset_c: Decimal,
}

/// The city's quorum rule.
#[derive(Clone, Copy, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedIndexQuorum {
    pub min_members: u32,
    pub min_weight: Decimal,
    /// False where Kalshi has not published a quorum rule; settlements are informational.
    pub published: bool,
}

/// The calibration in force, as supplied (the snapshot's `members`/`quorum`, `calibration`).
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedIndexCalibration {
    pub config_version: String,
    /// Absent when only the snapshot (which carries no effective time) supplied it.
    pub effective_at_unix_ns: Option<i64>,
    /// Strictly sorted by station id.
    pub members: Vec<SuppliedIndexMember>,
    pub quorum: SuppliedIndexQuorum,
}

/// One city's hourly index as the host folded it from the snapshot and the `seq` events.
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SuppliedHourlyIndex {
    /// MinuteTemp's index city id (`miami`, `nyc`, `chicago`, `la-coastal`).
    pub city: String,
    /// Empty until the first snapshot.
    pub timezone: String,
    /// `None` for an index-only city.
    pub hourly_series_ticker: Option<String>,
    /// Empty until the first snapshot.
    pub config_version: String,
    pub calibration_pending: bool,
    /// The last provider `seq` folded in; 0 before the first snapshot.
    pub seq: u64,
    pub latest: Option<SuppliedIndexMinute>,
    /// The newest minute with a value.
    pub latest_valued: Option<SuppliedIndexMinute>,
    /// Newest first, strictly by minute.
    pub recent_minutes: Vec<SuppliedIndexMinute>,
    pub current_hour: Option<SuppliedIndexHour>,
    /// The default (HRRR) forecast, matching the provider's top-level fields.
    pub forecast: Option<SuppliedIndexForecast>,
    /// Other models' active forecasts, strictly sorted by model id. Never contains HRRR.
    pub forecasts: Vec<SuppliedIndexForecast>,
    /// Newest first, strictly by hour end.
    pub recent_settlements: Vec<SuppliedIndexSettlement>,
    pub calibration: Option<SuppliedIndexCalibration>,
}

// ---------------------------------------------------------------------------------------------
// Kernel state
// ---------------------------------------------------------------------------------------------

/// The provider's default model; top-level forecast and bias fields always belong to it.
pub const DEFAULT_INDEX_FORECAST_MODEL: &str = "ncep_hrrr_conus_15min";

/// Authority and freshness of one non-default model, including a removed forecast.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IndexModelComponents {
    pub model_id: String,
    pub forecast: ComponentMeta,
    pub bias: ComponentMeta,
}

/// Authority and freshness of each hourly-index stream.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HourlyIndexComponents {
    /// `index_minute` (and the snapshot's minutes): `latest`, `latest_valued`,
    /// `recent_minutes`.
    pub minutes: ComponentMeta,
    /// `hour_update`: `current_hour`.
    pub hour: ComponentMeta,
    /// `forecast_index`: `forecast` without its bias.
    pub forecast: ComponentMeta,
    /// `forecast_bias`: `forecast.bias`.
    pub bias: ComponentMeta,
    /// `hour_settled` and `hour_reconciled`: `recent_settlements`.
    pub settlements: ComponentMeta,
    /// `calibration` (and the snapshot's members and quorum): `calibration`.
    pub calibration: ComponentMeta,
    /// Other models' metadata, strictly sorted by model id. Removal keeps its metadata.
    pub models: Vec<IndexModelComponents>,
}

/// One member's reading inside a city-minute.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IndexStationReading {
    pub station_id: String,
    pub temp_f: Option<f64>,
    pub code: String,
    pub source: Option<String>,
    pub contributes: bool,
    pub weight: f64,
    pub offset_c: Option<f64>,
    pub pull_f: Option<f64>,
    pub change_5m_f: Option<f64>,
    pub hour_high_f: Option<f64>,
    pub hour_low_f: Option<f64>,
    pub received_at: Option<DateTime<Utc>>,
}

impl IndexStationReading {
    pub fn from_supplied(reading: &SuppliedIndexStationReading) -> Self {
        Self {
            station_id: reading.station_id.clone(),
            temp_f: f64_of(reading.temp_f),
            code: reading.code.clone(),
            source: reading.source.clone(),
            contributes: reading.contributes,
            weight: reading.weight.to_f64(),
            offset_c: f64_of(reading.offset_c),
            pull_f: f64_of(reading.pull_f),
            change_5m_f: f64_of(reading.change_5m_f),
            hour_high_f: f64_of(reading.hour_high_f),
            hour_low_f: f64_of(reading.hour_low_f),
            received_at: reading.received_at_unix_ns.map(nanos),
        }
    }
}

/// One city-minute.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IndexMinute {
    pub minute: DateTime<Utc>,
    pub phase: IndexPhase,
    pub is_final: bool,
    pub revision: u64,
    pub value_f: Option<f64>,
    pub official_f: Option<f64>,
    pub provisional_f: Option<f64>,
    pub kalshi_status: Option<String>,
    pub config_version: String,
    pub contributors: u32,
    pub weight_share: f64,
    pub quorum_met: bool,
    pub calibration_pending: bool,
    pub provisional_at: Option<DateTime<Utc>>,
    pub official_at: Option<DateTime<Utc>>,
    /// Strictly sorted by station id.
    pub stations: Vec<IndexStationReading>,
    /// The event (or snapshot) that last set the minute: `city_sequence` is its `seq` and
    /// `slug` the city.
    pub provenance: EventProvenance,
    pub origin: ValueOrigin,
    pub supplied: Option<SuppliedIndexMinute>,
}

impl IndexMinute {
    pub fn from_supplied(minute: &SuppliedIndexMinute, city: &str) -> Self {
        Self {
            minute: nanos(minute.minute_unix_ns),
            phase: minute.phase,
            is_final: minute.is_final,
            revision: minute.revision,
            value_f: f64_of(minute.value_f),
            official_f: f64_of(minute.official_f),
            provisional_f: f64_of(minute.provisional_f),
            kalshi_status: minute.kalshi_status.clone(),
            config_version: minute.config_version.clone(),
            contributors: minute.contributors,
            weight_share: minute.weight_share.to_f64(),
            quorum_met: minute.quorum_met,
            calibration_pending: minute.calibration_pending,
            provisional_at: minute.provisional_at_unix_ns.map(nanos),
            official_at: minute.official_at_unix_ns.map(nanos),
            stations: minute
                .stations
                .iter()
                .map(IndexStationReading::from_supplied)
                .collect(),
            provenance: provenance(minute.envelope.as_ref(), &minute.source, city),
            origin: ValueOrigin::Supplied,
            supplied: Some(minute.clone()),
        }
    }

    /// The member's reading, if the minute carries one.
    pub fn station(&self, station_id: &str) -> Option<&IndexStationReading> {
        self.stations
            .iter()
            .find(|reading| reading.station_id == station_id)
    }
}

fn provenance(
    envelope: Option<&SuppliedHourlyIndexEnvelope>,
    source: &str,
    city: &str,
) -> EventProvenance {
    let mut provenance = EventProvenance {
        source: source.to_owned(),
        slug: city.to_owned(),
        ..EventProvenance::default()
    };
    if let Some(envelope) = envelope {
        provenance.event_id = envelope.event_id.clone();
        provenance.city_sequence = i64::try_from(envelope.seq).ok();
        provenance.emitted_at = envelope.emitted_at_unix_ns.map(nanos);
        provenance.received_at = Some(nanos(envelope.received_at_unix_ns));
    }
    provenance
}

/// A minute and its value.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IndexMinuteValue {
    pub minute: DateTime<Utc>,
    pub value_f: f64,
}

/// One degradation of the city's index feed over the final minutes of the 30 minutes ending
/// at the newest final minute.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FeedCondition {
    pub kind: FeedConditionKind,
    pub severity: FeedConditionSeverity,
    pub station_id: Option<String>,
    pub minutes: u32,
    pub window_minutes: u32,
    pub since: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
}

impl FeedCondition {
    pub fn from_supplied(condition: &SuppliedFeedCondition) -> Self {
        Self {
            kind: condition.kind.clone(),
            severity: condition.severity,
            station_id: condition.station_id.clone(),
            minutes: condition.minutes,
            window_minutes: condition.window_minutes,
            since: nanos(condition.since_unix_ns),
            last_seen: nanos(condition.last_seen_unix_ns),
        }
    }
}

/// The open or closed-but-undetermined hour.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IndexHour {
    pub hour_end: DateTime<Utc>,
    pub status: IndexHourStatus,
    pub event_ticker: Option<String>,
    pub settles_now_as: Option<IndexMinuteValue>,
    pub high_f: Option<f64>,
    pub low_f: Option<f64>,
    pub forecast_settle_f: Option<f64>,
    pub forecast_bias_f: Option<f64>,
    pub forecast_adjusted_settle_f: Option<f64>,
    pub final_at: DateTime<Utc>,
    pub determine_by: DateTime<Utc>,
    pub feed_conditions: Vec<FeedCondition>,
    pub origin: ValueOrigin,
    pub supplied: Option<SuppliedIndexHour>,
}

impl IndexHour {
    pub fn from_supplied(hour: &SuppliedIndexHour) -> Self {
        Self {
            hour_end: nanos(hour.hour_end_unix_ns),
            status: hour.status,
            event_ticker: hour.event_ticker.clone(),
            settles_now_as: hour.settles_now_as.map(|value| IndexMinuteValue {
                minute: nanos(value.minute_unix_ns),
                value_f: value.value_f.to_f64(),
            }),
            high_f: f64_of(hour.high_f),
            low_f: f64_of(hour.low_f),
            forecast_settle_f: f64_of(hour.forecast_settle_f),
            forecast_bias_f: f64_of(hour.forecast_bias_f),
            forecast_adjusted_settle_f: f64_of(hour.forecast_adjusted_settle_f),
            final_at: nanos(hour.final_at_unix_ns),
            determine_by: nanos(hour.determine_by_unix_ns),
            feed_conditions: hour
                .feed_conditions
                .iter()
                .map(FeedCondition::from_supplied)
                .collect(),
            origin: ValueOrigin::Supplied,
            supplied: Some(hour.clone()),
        }
    }
}

/// One 15-minute forecast step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IndexForecastStep {
    pub at: DateTime<Utc>,
    /// `None` below quorum.
    pub value_f: Option<f64>,
    pub quorum_met: bool,
    /// Some members had no forecast at this step.
    pub partial: bool,
}

/// Forecast settle of an upcoming hour.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IndexForecastSettle {
    pub hour_end: DateTime<Utc>,
    pub value_f: Option<f64>,
    pub quorum_met: bool,
    pub partial: bool,
}

/// The forecast's bias; add `bias_f` to a step's or settle's `value_f` for the adjusted value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IndexForecastBias {
    pub bias_f: f64,
    pub as_of_minute: DateTime<Utc>,
    /// Per member, sorted by station id: its reading at `as_of_minute` minus its HRRR
    /// temperature there, °F.
    pub member_bias: Vec<(String, Option<f64>)>,
}

/// The HRRR 15-minute index forecast.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IndexForecast {
    pub model_id: String,
    pub run_time: DateTime<Utc>,
    pub fetched_at: DateTime<Utc>,
    pub config_version: String,
    pub missing_members: Vec<String>,
    /// In time order.
    pub steps: Vec<IndexForecastStep>,
    /// In time order.
    pub settles: Vec<IndexForecastSettle>,
    pub bias: Option<IndexForecastBias>,
    pub origin: ValueOrigin,
    pub supplied: Option<SuppliedIndexForecast>,
}

impl IndexForecast {
    /// Zips the provider's aligned per-step arrays; a codec has checked their alignment.
    pub fn from_supplied(forecast: &SuppliedIndexForecast) -> Self {
        Self {
            model_id: forecast.model_id.clone(),
            run_time: nanos(forecast.run_time_unix_ns),
            fetched_at: nanos(forecast.fetched_at_unix_ns),
            config_version: forecast.config_version.clone(),
            missing_members: forecast.missing_members.clone(),
            steps: forecast
                .steps_unix_ns
                .iter()
                .zip(&forecast.value_f)
                .zip(forecast.quorum_met.iter().zip(&forecast.partial))
                .map(|((at, value_f), (quorum_met, partial))| IndexForecastStep {
                    at: nanos(*at),
                    value_f: f64_of(*value_f),
                    quorum_met: *quorum_met,
                    partial: *partial,
                })
                .collect(),
            settles: forecast
                .settles
                .iter()
                .map(|settle| IndexForecastSettle {
                    hour_end: nanos(settle.hour_end_unix_ns),
                    value_f: f64_of(settle.value_f),
                    quorum_met: settle.quorum_met,
                    partial: settle.partial,
                })
                .collect(),
            bias: forecast.bias.as_ref().map(|bias| IndexForecastBias {
                bias_f: bias.bias_f.to_f64(),
                as_of_minute: nanos(bias.as_of_minute_unix_ns),
                member_bias: bias
                    .member_bias
                    .iter()
                    .map(|member| (member.station_id.clone(), f64_of(member.bias_f)))
                    .collect(),
            }),
            origin: ValueOrigin::Supplied,
            supplied: Some(forecast.clone()),
        }
    }

    /// The forecast settle of the hour ending at `hour_end`.
    pub fn settle(&self, hour_end: DateTime<Utc>) -> Option<&IndexForecastSettle> {
        self.settles
            .iter()
            .find(|settle| settle.hour_end == hour_end)
    }
}

/// MinuteTemp's settlement of one hour.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IndexSettlement {
    pub hour_end: DateTime<Utc>,
    pub status: IndexSettlementStatus,
    pub event_ticker: Option<String>,
    pub settle_minute: Option<DateTime<Utc>>,
    pub settle_value_f: Option<f64>,
    pub winning_floor_strike: Option<f64>,
    pub informational: bool,
    pub determined_at: DateTime<Utc>,
    pub kalshi_expiration_value: Option<f64>,
    pub kalshi_finalized_at: Option<DateTime<Utc>>,
    pub matched: Option<bool>,
    pub revision: u64,
    pub origin: ValueOrigin,
    pub supplied: Option<SuppliedIndexSettlement>,
}

impl IndexSettlement {
    pub fn from_supplied(settlement: &SuppliedIndexSettlement) -> Self {
        Self {
            hour_end: nanos(settlement.hour_end_unix_ns),
            status: settlement.status,
            event_ticker: settlement.event_ticker.clone(),
            settle_minute: settlement.settle_minute_unix_ns.map(nanos),
            settle_value_f: f64_of(settlement.settle_value_f),
            winning_floor_strike: f64_of(settlement.winning_floor_strike),
            informational: settlement.informational,
            determined_at: nanos(settlement.determined_at_unix_ns),
            kalshi_expiration_value: f64_of(settlement.kalshi_expiration_value),
            kalshi_finalized_at: settlement.kalshi_finalized_at_unix_ns.map(nanos),
            matched: settlement.matched,
            revision: settlement.revision,
            origin: ValueOrigin::Supplied,
            supplied: Some(settlement.clone()),
        }
    }
}

/// One calibrated member.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IndexMember {
    pub station_id: String,
    pub weight: f64,
    pub offset_c: f64,
}

/// The city's quorum rule.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IndexQuorum {
    pub min_members: u32,
    pub min_weight: f64,
    pub published: bool,
}

/// The calibration in force.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IndexCalibration {
    pub config_version: String,
    pub effective_at: Option<DateTime<Utc>>,
    /// Sorted by station id.
    pub members: Vec<IndexMember>,
    pub quorum: IndexQuorum,
    pub origin: ValueOrigin,
    pub supplied: Option<SuppliedIndexCalibration>,
}

impl IndexCalibration {
    pub fn from_supplied(calibration: &SuppliedIndexCalibration) -> Self {
        Self {
            config_version: calibration.config_version.clone(),
            effective_at: calibration.effective_at_unix_ns.map(nanos),
            members: calibration
                .members
                .iter()
                .map(|member| IndexMember {
                    station_id: member.station_id.clone(),
                    weight: member.weight.to_f64(),
                    offset_c: member.offset_c.to_f64(),
                })
                .collect(),
            quorum: IndexQuorum {
                min_members: calibration.quorum.min_members,
                min_weight: calibration.quorum.min_weight.to_f64(),
                published: calibration.quorum.published,
            },
            origin: ValueOrigin::Supplied,
            supplied: Some(calibration.clone()),
        }
    }
}

/// One city's hourly index as the host delivered it for this invocation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HourlyIndexState {
    pub city: String,
    pub timezone: String,
    pub hourly_series_ticker: Option<String>,
    pub config_version: String,
    pub calibration_pending: bool,
    /// The last provider `seq` folded in; 0 before the first snapshot.
    pub seq: u64,
    pub components: HourlyIndexComponents,
    pub latest: Option<IndexMinute>,
    pub latest_valued: Option<IndexMinute>,
    /// Newest first.
    pub recent_minutes: Vec<IndexMinute>,
    pub current_hour: Option<IndexHour>,
    pub forecast: Option<IndexForecast>,
    /// Other models' active forecasts, strictly sorted by model id.
    pub forecasts: Vec<IndexForecast>,
    /// Newest first.
    pub recent_settlements: Vec<IndexSettlement>,
    pub calibration: Option<IndexCalibration>,
}

impl HourlyIndexState {
    /// Projects a supplied city with the host's component metadata.
    pub fn from_supplied(index: &SuppliedHourlyIndex, components: HourlyIndexComponents) -> Self {
        let minute = |minute: &SuppliedIndexMinute| IndexMinute::from_supplied(minute, &index.city);
        Self {
            city: index.city.clone(),
            timezone: index.timezone.clone(),
            hourly_series_ticker: index.hourly_series_ticker.clone(),
            config_version: index.config_version.clone(),
            calibration_pending: index.calibration_pending,
            seq: index.seq,
            components,
            latest: index.latest.as_ref().map(minute),
            latest_valued: index.latest_valued.as_ref().map(minute),
            recent_minutes: index.recent_minutes.iter().map(minute).collect(),
            current_hour: index.current_hour.as_ref().map(IndexHour::from_supplied),
            forecast: index.forecast.as_ref().map(IndexForecast::from_supplied),
            forecasts: index
                .forecasts
                .iter()
                .map(IndexForecast::from_supplied)
                .collect(),
            recent_settlements: index
                .recent_settlements
                .iter()
                .map(IndexSettlement::from_supplied)
                .collect(),
            calibration: index
                .calibration
                .as_ref()
                .map(IndexCalibration::from_supplied),
        }
    }

    /// An active forecast by exact model id. No fallback to the default model.
    pub fn forecast_for(&self, model_id: &str) -> Option<&IndexForecast> {
        self.forecast
            .iter()
            .chain(&self.forecasts)
            .find(|forecast| forecast.model_id == model_id)
    }

    /// Forecast and bias metadata by exact model id, even after a removal.
    pub fn forecast_components(&self, model_id: &str) -> Option<(&ComponentMeta, &ComponentMeta)> {
        if model_id == DEFAULT_INDEX_FORECAST_MODEL {
            Some((&self.components.forecast, &self.components.bias))
        } else {
            self.components
                .models
                .iter()
                .find(|model| model.model_id == model_id)
                .map(|model| (&model.forecast, &model.bias))
        }
    }

    /// The retained minute at `minute`.
    pub fn minute(&self, minute: DateTime<Utc>) -> Option<&IndexMinute> {
        self.recent_minutes
            .iter()
            .find(|candidate| candidate.minute == minute)
    }

    /// The retained settlement of the hour ending at `hour_end`.
    pub fn settlement(&self, hour_end: DateTime<Utc>) -> Option<&IndexSettlement> {
        self.recent_settlements
            .iter()
            .find(|settlement| settlement.hour_end == hour_end)
    }
}
