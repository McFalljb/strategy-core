//! The exact typed event of one decision, projected from its trigger.

use chrono::{TimeZone, Utc};
use strategy_core_kernel::{
    EventProvenance, Extreme, ForecastUpdated, KernelResult, Observation, OracleScoresUpdated,
    PriceUpdate, Report, StrategyEvent, StrategyEventView, StrategyKernelContext, TimerWake,
    WeatherEvent,
};

use super::projection::{
    component_meta, derived_extreme, derived_observation, derived_report, derived_weather_event,
    market_state, millis, station_state,
};
use super::{KernelTransactionError, TransactionKernel};
use crate::decision_v4::{RankByV4, StationV4};
use crate::decision_v6::{DecisionContextV6, DecisionV6Error, OwnerTriggerV6, TriggerV6};
use crate::supplied_v6::{ExtremeKindV6, SuppliedEventV6};

/// The station an event is projected from: the trigger's station when it names one, else the
/// Sleeve's primary station.
fn event_station<'a>(
    context: &'a DecisionContextV6,
    station_id: Option<&str>,
) -> Result<&'a StationV4, KernelTransactionError> {
    let station_id = station_id.unwrap_or(&context.strategy.station_id);
    context
        .owner_state
        .stations
        .iter()
        .find(|station| station.identity.station_id == station_id)
        .ok_or_else(|| KernelTransactionError::Kernel(format!("station {station_id} missing")))
}

fn captured_weather_event(
    originating: &crate::current_v6::OriginatingWeatherV6,
    event_date: &str,
) -> Result<StrategyEvent, KernelTransactionError> {
    use crate::current_v6::WeatherDataV6;
    let supplied = originating.supplied.as_ref();
    let station = originating.station_id.as_str();
    let mut event = match &originating.data {
        WeatherDataV6::Observation(value) => StrategyEvent::Observation(Box::new(match supplied {
            Some(SuppliedEventV6::Observation(event)) => Observation::from_supplied(event),
            _ => derived_observation(value, station)?,
        })),
        WeatherDataV6::Report(value) => StrategyEvent::StationReport(Box::new(match supplied {
            Some(SuppliedEventV6::Report(event)) => Report::from_supplied(event),
            _ => derived_report(value, station),
        })),
        WeatherDataV6::WeatherEvent(value) => {
            StrategyEvent::WeatherEvent(Box::new(match supplied {
                Some(SuppliedEventV6::WeatherEvent(event)) => WeatherEvent::from_supplied(event),
                _ => derived_weather_event(value, station)?,
            }))
        }
        WeatherDataV6::Extreme { high, value } => {
            let extreme = Box::new(match supplied {
                Some(SuppliedEventV6::Extreme(event)) => Extreme::from_supplied(event, event_date),
                _ => derived_extreme(
                    value,
                    station,
                    if *high {
                        ExtremeKindV6::High
                    } else {
                        ExtremeKindV6::Low
                    },
                    event_date,
                )?,
            });
            if *high {
                StrategyEvent::NewHigh(extreme)
            } else {
                StrategyEvent::NewLow(extreme)
            }
        }
    };
    let provenance = match &mut event {
        StrategyEvent::Observation(event) => &mut event.provenance,
        StrategyEvent::StationReport(event) => {
            event.meta = component_meta(&originating.meta)?;
            &mut event.provenance
        }
        StrategyEvent::WeatherEvent(event) => {
            event.meta = component_meta(&originating.meta)?;
            &mut event.provenance
        }
        StrategyEvent::NewHigh(event) | StrategyEvent::NewLow(event) => &mut event.provenance,
        _ => unreachable!(),
    };
    provenance.connection_epoch = Some(originating.cursor.connection_generation);
    provenance.acceptance = Some(strategy_core_kernel::state::EventAcceptance {
        station_generation: originating.cursor.connection_generation,
        station_revision: originating.station_revision,
        component: component_meta(&originating.meta)?,
        weather: originating.facts.clone(),
    });
    Ok(event)
}

/// The exact typed event for one transaction. `None` for bootstrap/recovery, which invoke
/// `on_start` instead of `on_event`.
pub struct KernelEvent {
    event: Option<StrategyEvent>,
}

impl KernelEvent {
    pub fn from_context(context: &DecisionContextV6) -> Result<Self, KernelTransactionError> {
        if let Some(originating) = context
            .current_inputs
            .as_ref()
            .and_then(|current| current.originating.as_ref())
        {
            originating
                .validate(context)
                .map_err(KernelTransactionError::Contract)?;
            let station = event_station(context, Some(&originating.station_id))?;
            return Ok(Self {
                event: Some(captured_weather_event(
                    originating,
                    &station.climate_event_date,
                )?),
            });
        }
        let decision_at = millis(Some(context.decision_time_unix_ms))?
            .ok_or(KernelTransactionError::InvalidTime)?;
        let trigger = match &context.trigger {
            TriggerV6::Owner(trigger) => trigger,
            // The order updates are the news; the trigger itself follows them as before.
            TriggerV6::BrokerState { .. } => {
                return Ok(Self {
                    event: Some(StrategyEvent::Unknown {
                        event_type: "broker_state".to_owned(),
                        emitted_at: Some(decision_at),
                    }),
                });
            }
        };
        // Weather, forecast and oracle events come from the station that triggered them, which
        // may be any contributor station; price and timer events from the primary station.
        let station = event_station(context, trigger.station_id())?;
        let station_id = station.identity.station_id.as_str();
        let supplied_event = context.supplied.originating_event.as_ref();
        let event_date = station.climate_event_date.as_str();
        let event = match trigger {
            OwnerTriggerV6::CapturedWeather { .. } => {
                return Err(KernelTransactionError::Contract(
                    DecisionV6Error::InvalidContract,
                ));
            }
            OwnerTriggerV6::Bootstrap | OwnerTriggerV6::Recovery => None,
            OwnerTriggerV6::Observation { .. } => {
                Some(StrategyEvent::Observation(Box::new(match supplied_event {
                    Some(SuppliedEventV6::Observation(event)) => Observation::from_supplied(event),
                    _ => derived_observation(&station.observation, station_id)?,
                })))
            }
            OwnerTriggerV6::StationReport { report_id, .. } => Some(StrategyEvent::StationReport(
                Box::new(match supplied_event {
                    Some(SuppliedEventV6::Report(event)) => Report::from_supplied(event),
                    _ => derived_report(
                        station
                            .reports
                            .iter()
                            .find(|report| report.report_id == *report_id)
                            .ok_or_else(|| {
                                KernelTransactionError::Kernel("report missing".to_owned())
                            })?,
                        station_id,
                    ),
                }),
            )),
            OwnerTriggerV6::NewHigh { .. } => {
                Some(StrategyEvent::NewHigh(Box::new(match supplied_event {
                    Some(SuppliedEventV6::Extreme(event)) => {
                        Extreme::from_supplied(event, event_date)
                    }
                    _ => derived_extreme(
                        station.extrema.high.as_ref().ok_or_else(|| {
                            KernelTransactionError::Kernel("new high missing".to_owned())
                        })?,
                        station_id,
                        ExtremeKindV6::High,
                        event_date,
                    )?,
                })))
            }
            OwnerTriggerV6::NewLow { .. } => {
                Some(StrategyEvent::NewLow(Box::new(match supplied_event {
                    Some(SuppliedEventV6::Extreme(event)) => {
                        Extreme::from_supplied(event, event_date)
                    }
                    _ => derived_extreme(
                        station.extrema.low.as_ref().ok_or_else(|| {
                            KernelTransactionError::Kernel("new low missing".to_owned())
                        })?,
                        station_id,
                        ExtremeKindV6::Low,
                        event_date,
                    )?,
                })))
            }
            OwnerTriggerV6::WeatherEvent { episode_id, .. } => {
                match supplied_event {
                    Some(SuppliedEventV6::WeatherEvent(event)) => Some(
                        StrategyEvent::WeatherEvent(Box::new(WeatherEvent::from_supplied(event))),
                    ),
                    _ => match station
                        .weather_events
                        .iter()
                        .find(|event| event.event_id == *episode_id)
                    {
                        Some(event) => Some(StrategyEvent::WeatherEvent(Box::new(
                            derived_weather_event(event, station_id)?,
                        ))),
                        // An ended episode is absent from current state and cannot be
                        // reconstructed without its supplied event.
                        None => Some(StrategyEvent::Unknown {
                            event_type: "weather_event".to_owned(),
                            emitted_at: Some(decision_at),
                        }),
                    },
                }
            }
            OwnerTriggerV6::MarketPrice {
                market_id,
                emitted_at_unix_ms,
                ..
            } => {
                let emitted_at = millis(Some(*emitted_at_unix_ms))?
                    .ok_or(KernelTransactionError::InvalidTime)?;
                Some(StrategyEvent::PriceUpdate(Box::new(PriceUpdate {
                    provenance: EventProvenance {
                        source: "kalshi".to_owned(),
                        event_id: Some(market_id.clone()),
                        emitted_at: Some(emitted_at),
                        slug: station_id.to_owned(),
                        ..EventProvenance::default()
                    },
                    station_id: station_id.to_owned(),
                    city_id: String::new(),
                    timestamp: Some(emitted_at),
                    markets: context
                        .owner_state
                        .markets
                        .iter()
                        .map(|market| market_state(market, &context.strategy.event_date, context))
                        .collect::<Result<_, _>>()?,
                })))
            }
            OwnerTriggerV6::ForecastUpdated {
                emitted_at_unix_ms, ..
            } => {
                let model = station.forecast.models.first().ok_or_else(|| {
                    KernelTransactionError::Kernel("forecast model missing".to_owned())
                })?;
                Some(StrategyEvent::ForecastUpdated(Box::new(ForecastUpdated {
                    provenance: EventProvenance {
                        source: "minutetemp".to_owned(),
                        emitted_at: millis(Some(*emitted_at_unix_ms))?,
                        slug: station_id.to_owned(),
                        ..EventProvenance::default()
                    },
                    station_id: station_id.to_owned(),
                    model_id: model.model_id.clone(),
                    version: model.version.clone(),
                })))
            }
            OwnerTriggerV6::OracleScoresUpdated {
                emitted_at_unix_ms, ..
            } => {
                let snapshot_station = station_state(
                    station,
                    context.supplied.station(station_id),
                    context
                        .current_weather
                        .as_ref()
                        .and_then(|stations| {
                            stations
                                .iter()
                                .find(|weather| weather.station_id == *station_id)
                        })
                        .map(|weather| &weather.facts),
                    context
                        .forecast_issuance
                        .as_ref()
                        .and_then(|stations| {
                            stations
                                .iter()
                                .find(|issued| issued.station_id == *station_id)
                        })
                        .map(|issued| issued.models.as_slice()),
                    context.current_inputs.as_ref().and_then(|current| {
                        current
                            .stations
                            .iter()
                            .find(|input| input.station_id == *station_id)
                    }),
                )?;
                let day_of = snapshot_station
                    .oracle_tables
                    .iter()
                    .find(|table| {
                        table.mode == "day_of"
                            && (context.current_inputs.is_none()
                                || table.rank_by
                                    == match station.oracle.query.rank_by {
                                        RankByV4::High => "high",
                                        RankByV4::Low => "low",
                                    })
                    })
                    .cloned();
                let modes = day_of
                    .as_ref()
                    .map(|table| table.modes.clone())
                    .filter(|modes| !modes.is_empty())
                    .unwrap_or_else(|| vec![station.oracle.query.mode.clone()]);
                let emitted_at = millis(Some(*emitted_at_unix_ms))?
                    .ok_or(KernelTransactionError::InvalidTime)?;
                Some(StrategyEvent::OracleScoresUpdated(Box::new(
                    OracleScoresUpdated {
                        provenance: EventProvenance {
                            source: "minutetemp".to_owned(),
                            emitted_at: Some(emitted_at),
                            slug: station_id.to_owned(),
                            ..EventProvenance::default()
                        },
                        station_id: station_id.to_owned(),
                        modes,
                        updated_at: day_of
                            .as_ref()
                            .and_then(|table| table.updated_at)
                            .or_else(|| context.current_inputs.is_none().then_some(emitted_at)),
                        overall: None,
                        day_ahead: None,
                        day_of,
                    },
                )))
            }
            OwnerTriggerV6::Timer {
                key,
                scheduled_at_epoch_ns,
                ..
            } => Some(StrategyEvent::TimerWake(TimerWake {
                scheduled_for: Utc.timestamp_nanos(
                    i64::try_from(*scheduled_at_epoch_ns)
                        .map_err(|_| KernelTransactionError::InvalidTime)?,
                ),
                fired_at: Some(decision_at),
                name: key.clone(),
            })),
        };
        Ok(Self { event })
    }

    /// The canonical owned event, or `None` for bootstrap/recovery.
    pub fn event(&self) -> Option<&StrategyEvent> {
        self.event.as_ref()
    }

    pub fn run<K: TransactionKernel>(
        &self,
        kernel: &mut K,
        context: &mut dyn StrategyKernelContext,
    ) -> KernelResult<()> {
        match &self.event {
            Some(event) => event.with_view(|view| kernel.on_event(view, context)),
            None => kernel.on_start(context),
        }
    }

    /// The projected event view; `None` for bootstrap/recovery and for price updates, which
    /// are only presented inside [`KernelEvent::run`] because they borrow per-market views.
    pub fn view(&self) -> Option<StrategyEventView<'_>> {
        self.event.as_ref().and_then(StrategyEvent::view)
    }
}
