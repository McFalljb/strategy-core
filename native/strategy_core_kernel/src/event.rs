//! The canonical owned event model: the exact typed event that triggered one invocation.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

use crate::events::{
    ForecastUpdatedView, ForecastVersionsView, MarketBracketView, OracleScoresUpdatedView,
    PriceUpdateView, ShutdownView, StrategyEventView, TimerWakeView,
};
use crate::state::{
    EventProvenance, Extreme, MarketState, Observation, OracleTable, Report, WeatherEvent,
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PriceUpdate {
    pub provenance: EventProvenance,
    pub station_id: String,
    pub city_id: String,
    pub timestamp: Option<DateTime<Utc>>,
    pub markets: Vec<MarketState>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ForecastUpdated {
    pub provenance: EventProvenance,
    pub station_id: String,
    pub model_id: String,
    pub version: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ForecastVersions {
    pub provenance: EventProvenance,
    pub station_id: String,
    pub versions: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OracleScoresUpdated {
    pub provenance: EventProvenance,
    pub station_id: String,
    pub modes: Vec<String>,
    pub updated_at: Option<DateTime<Utc>>,
    pub overall: Option<OracleTable>,
    pub day_ahead: Option<OracleTable>,
    pub day_of: Option<OracleTable>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimerWake {
    pub scheduled_for: DateTime<Utc>,
    pub fired_at: Option<DateTime<Utc>>,
    pub name: String,
}

/// The exact typed event of one invocation, owned by the host for the invocation's lifetime.
#[derive(Clone, Debug, PartialEq)]
pub enum StrategyEvent {
    PriceUpdate(Box<PriceUpdate>),
    Observation(Box<Observation>),
    ForecastUpdated(Box<ForecastUpdated>),
    ForecastVersions(Box<ForecastVersions>),
    OracleScoresUpdated(Box<OracleScoresUpdated>),
    StationReport(Box<Report>),
    WeatherEvent(Box<WeatherEvent>),
    NewHigh(Box<Extreme>),
    NewLow(Box<Extreme>),
    TimerWake(TimerWake),
    Shutdown {
        reason: String,
    },
    Unknown {
        event_type: String,
        emitted_at: Option<DateTime<Utc>>,
    },
}

impl StrategyEvent {
    /// Presents the event to a kernel. Price updates borrow per-market bracket views, so the
    /// view only exists inside `f`.
    pub fn with_view<R>(&self, f: impl FnOnce(StrategyEventView<'_>) -> R) -> R {
        match self {
            Self::PriceUpdate(event) => {
                let markets = event
                    .markets
                    .iter()
                    .map(MarketState::bracket_view)
                    .collect::<Vec<_>>();
                f(StrategyEventView::PriceUpdate(PriceUpdateView {
                    event_id: event.provenance.event_id.as_deref(),
                    sequence: event.provenance.sequence,
                    city_sequence: event.provenance.city_sequence,
                    emitted_at: event.provenance.emitted_at,
                    source: &event.provenance.source,
                    slug: &event.provenance.slug,
                    station_id: &event.station_id,
                    city_id: &event.city_id,
                    timestamp: event.timestamp,
                    markets: &markets,
                }))
            }
            other => f(other
                .view()
                .expect("every non-price event has a borrowed view")),
        }
    }

    /// The borrowed view of every event except a price update (see [`Self::with_view`]).
    pub fn view(&self) -> Option<StrategyEventView<'_>> {
        Some(match self {
            Self::PriceUpdate(_) => return None,
            Self::Observation(event) => StrategyEventView::Observation(event.view()),
            Self::ForecastUpdated(event) => {
                StrategyEventView::ForecastUpdated(ForecastUpdatedView {
                    event_id: event.provenance.event_id.as_deref(),
                    sequence: event.provenance.sequence,
                    emitted_at: event.provenance.emitted_at,
                    slug: &event.provenance.slug,
                    station_id: &event.station_id,
                    model_id: &event.model_id,
                    version: &event.version,
                })
            }
            Self::ForecastVersions(event) => {
                StrategyEventView::ForecastVersions(ForecastVersionsView {
                    event_id: event.provenance.event_id.as_deref(),
                    sequence: event.provenance.sequence,
                    emitted_at: event.provenance.emitted_at,
                    slug: &event.provenance.slug,
                    station_id: &event.station_id,
                    versions: &event.versions,
                })
            }
            Self::OracleScoresUpdated(event) => {
                StrategyEventView::OracleScoresUpdated(OracleScoresUpdatedView {
                    event_id: event.provenance.event_id.as_deref(),
                    sequence: event.provenance.sequence,
                    emitted_at: event.provenance.emitted_at,
                    slug: &event.provenance.slug,
                    station_id: &event.station_id,
                    modes: &event.modes,
                    updated_at: event.updated_at,
                    overall: event.overall.as_ref().map(OracleTable::snapshot),
                    day_ahead: event.day_ahead.as_ref().map(OracleTable::snapshot),
                    day_of: event.day_of.as_ref().map(OracleTable::snapshot),
                })
            }
            Self::StationReport(event) => StrategyEventView::StationReport(event.view()),
            Self::WeatherEvent(event) => StrategyEventView::WeatherEvent(event.view()),
            Self::NewHigh(event) => StrategyEventView::NewHigh(event.view()),
            Self::NewLow(event) => StrategyEventView::NewLow(event.view()),
            Self::TimerWake(event) => StrategyEventView::TimerWake(TimerWakeView {
                scheduled_for: event.scheduled_for,
                fired_at: event.fired_at,
                name: &event.name,
            }),
            Self::Shutdown { reason } => StrategyEventView::Shutdown(ShutdownView { reason }),
            Self::Unknown {
                event_type,
                emitted_at,
            } => StrategyEventView::Unknown {
                event_type,
                emitted_at: *emitted_at,
            },
        })
    }

    /// Bracket views of a price update's markets, for hosts that assemble the view themselves.
    pub fn bracket_views(markets: &[MarketState]) -> Vec<MarketBracketView<'_>> {
        markets.iter().map(MarketState::bracket_view).collect()
    }
}
