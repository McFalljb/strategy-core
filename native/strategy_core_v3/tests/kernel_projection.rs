//! Kernel projection and transaction runner contract: the exact typed originating event and
//! the supplied original-precision state reach a real `NativeKernel` through Core-owned views.
#![cfg(feature = "kernel")]

#[path = "kernel_projection/decisions.rs"]
mod decisions;
#[path = "kernel_projection/market_strikes.rs"]
mod market_strikes;

use std::cell::RefCell;
use std::rc::Rc;

use chrono::{DateTime, TimeZone, Utc};
use strategy_core_kernel::{
    ContractQuantity, ContractSide, KernelAction, KernelResult, LogAction, NativeKernel,
    OrderAction, OrderType, PlaceOrderRequest, StrategyEventView, StrategyKernelContext,
    StrategyKernelState, ValueOrigin,
};
use strategy_core_v3::decision_v4::{
    BrokerV4, ConfigV4, DecisionContextV4, ExtremeV4, FenceV4, MarketComparisonV4,
    MarketIdentityV4, MarketV4, OpportunityV4, ProvenanceV4, ReportV4, StationIdentityV4,
    StationV4, SupervisorV4, TickerV4, TriggerV4, WeatherEventV4,
};
use strategy_core_v3::decision_v6::{
    BrokerDetailV6, CapabilityGrantV6, DecisionContextV6, DecisionDispositionV6, DeploymentModeV6,
    OwnerTriggerV6, StrategyCommandV6, StrategyScopeV6, TelemetryEntryV6, TriggerV6,
    decode_decision_context_v6, derive_sleeve_identity_v6, encode_decision_context_v6,
};
use strategy_core_v3::kernel_v6::{
    KernelCheckpointCodec, KernelEvent, KernelSnapshot, KernelTransactionError, TransactionKernel,
    TransactionKernelFactory, run_transaction,
};
use strategy_core_v3::supplied_v6::{
    DecimalV6, EventEnvelopeV6, ExtremeKindV6, SUPPLIED_INPUTS_CONTRACT_VERSION,
    SuppliedDailyExtremesV6, SuppliedEventV6, SuppliedExtremeV6, SuppliedForecastModelV6,
    SuppliedForecastPointV6, SuppliedForecastV6, SuppliedInputsV6, SuppliedObservationV6,
    SuppliedOracleScoreV6, SuppliedOracleTableV6, SuppliedReportV6, SuppliedStationV6,
    SuppliedWeatherEventV6,
};

#[path = "kernel_projection/host_services.rs"]
mod host_services;

const STATION: &str = "KSEA";
const MARKET: &str = "KXHIGHTSEA-26AUG30-T80";
const OPPORTUNITY: &str = "KXHIGHTSEA-26AUG30";
const DIGEST: &str = "profile-calculator-digest";
const EMITTED_NS: i64 = 1_788_062_345_123_456_789;
const OBSERVED_NS: i64 = 1_788_062_340_000_000_000;
const RECEIVED_NS: i64 = 1_788_062_345_200_000_000;
const DECISION_MS: i64 = 1_788_062_400_000;

fn decimal(text: &str) -> Option<DecimalV6> {
    Some(DecimalV6::parse(text).unwrap())
}

fn ns(value: i64) -> DateTime<Utc> {
    Utc.timestamp_nanos(value)
}

fn envelope(event_id: &str, sequence: u64) -> EventEnvelopeV6 {
    EventEnvelopeV6 {
        event_id: event_id.to_owned(),
        sequence,
        city_sequence: Some(9),
        slug: Some("sea".to_owned()),
        emitted_at_unix_ns: EMITTED_NS,
        event_key: Some("KSEA|metar|2026-08-30T20:05:00Z".to_owned()),
        source_timestamp_unix_ns: Some(OBSERVED_NS),
        wmo_emit_time_unix_ns: Some(OBSERVED_NS + 60_000_000_000),
        producer_received_at_unix_ns: Some(EMITTED_NS - 700_000_000),
        live_published_at_unix_ns: Some(EMITTED_NS - 200_000_000),
        persistence_status: Some("uncommitted".to_owned()),
        producer_sequence: Some(1_001),
        received_at_unix_ns: RECEIVED_NS,
    }
}

fn provenance(event_id: &str, sequence: u64) -> ProvenanceV4 {
    ProvenanceV4 {
        provider: "minutetemp".to_owned(),
        source: "minutetemp.websocket.v1".to_owned(),
        event_id: Some(event_id.to_owned()),
        connection_epoch: Some(3),
        sequence: Some(sequence),
        city_sequence: Some(9),
        producer_sequence: Some(1_001),
        provider_at_unix_ms: Some(EMITTED_NS / 1_000_000),
        received_at_unix_ms: RECEIVED_NS / 1_000_000,
        ..Default::default()
    }
}

/// The id of the fixture Sleeve's command at `ordinal` in delivery `delivery.daily.<delivery>`.
fn cid(delivery: u32, ordinal: u32) -> String {
    strategy_core_v3::decision_v6::command_id_v6(
        &derive_sleeve_identity_v6("fixture", "binding.daily", "kalshi", OPPORTUNITY),
        1,
        &format!("delivery.daily.{delivery}"),
        ordinal,
    )
    .unwrap()
}

fn base_context() -> DecisionContextV6 {
    let owner_state = DecisionContextV4 {
        delivery_id: "delivery.daily.1".to_owned(),
        sleeve: SupervisorV4 {
            sleeve_id: derive_sleeve_identity_v6("fixture", "binding.daily", "kalshi", OPPORTUNITY),
            incarnation: 1,
            process_attempt: 1,
            route_epoch: 1,
        },
        trigger: TriggerV4::Recovery,
        fence: FenceV4 {
            profile_and_calculator_digest: DIGEST.to_owned(),
            route_plan_sha256: [7; 32],
            ..Default::default()
        },
        config: ConfigV4 {
            profile_and_calculator_digest: DIGEST.to_owned(),
            ..Default::default()
        },
        stations: vec![StationV4 {
            climate_event_date: "2026-08-30".to_owned(),
            climate_day_start_utc_unix_ms: 1_788_019_200_000,
            climate_day_end_utc_unix_ms: 1_788_105_600_000,
            identity: StationIdentityV4 {
                station_id: STATION.to_owned(),
                logical_location: STATION.to_owned(),
                timezone: "America/Los_Angeles".to_owned(),
                ..Default::default()
            },
            ..Default::default()
        }],
        opportunity: OpportunityV4 {
            opportunity_id: OPPORTUNITY.to_owned(),
            venue_id: "kalshi".to_owned(),
            match_profile: "daily-high".to_owned(),
            market_ids: vec![MARKET.to_owned()],
            contributor_stations: vec![STATION.to_owned()],
            ..Default::default()
        },
        markets: vec![MarketV4 {
            identity: MarketIdentityV4 {
                market_id: MARKET.to_owned(),
                opportunity_id: OPPORTUNITY.to_owned(),
                event_ticker: OPPORTUNITY.to_owned(),
                fee_type: "quadratic".to_owned(),
                fee_multiplier_millionths: Some(1_000_000),
                floor_strike_milli_f: Some(79_500),
                ..Default::default()
            },
            revision: 3,
            ticker: Some(TickerV4 {
                yes_bid_micros: Some(400_000),
                yes_ask_micros: Some(420_000),
                no_bid_micros: Some(580_000),
                no_ask_micros: Some(600_000),
                yes_ask_quantity_hundredths: Some(1_250),
                volume_hundredths: Some(10_050),
                open_interest_hundredths: Some(5_000),
                provider_at_unix_ms: Some(DECISION_MS - 1_000),
                ..Default::default()
            }),
            minutetemp_comparison: Some(MarketComparisonV4 {
                event_date: "2026-08-30".to_owned(),
                ..Default::default()
            }),
            ..Default::default()
        }],
        broker: BrokerV4 {
            provider_available_balance: 100_000_000,
            allowance_limit: 100_000_000,
            ..Default::default()
        },
        delivered_at_monotonic_ns: 1,
        hard_expires_at_monotonic_ns: 2,
        ..Default::default()
    };
    DecisionContextV6 {
        market_strikes: None,
        current_weather: None,
        forecast_issuance: None,
        current_inputs: None,
        deployment_mode: DeploymentModeV6::Paper,
        capabilities: CapabilityGrantV6 {
            timers: true,
            external_requests: Vec::new(),
        },
        command_receipts: Vec::new(),
        owner_state,
        strategy: StrategyScopeV6 {
            strategy_id: "fixture".to_owned(),
            binding_id: "binding.daily".to_owned(),
            profile: "daily-high".to_owned(),
            parameters: Vec::new(),
            station_id: STATION.to_owned(),
            event_ticker: OPPORTUNITY.to_owned(),
            event_date: "2026-08-30".to_owned(),
            market_ids: vec![MARKET.to_owned()],
            profile_and_calculator_digest: DIGEST.to_owned(),
        },
        broker: BrokerDetailV6 {
            revision: 0,
            reserved_cash_micros: 0,
            positions: Vec::new(),
            orders: Vec::new(),
        },
        trigger: TriggerV6::Owner(OwnerTriggerV6::Recovery),
        kernel_checkpoint: None,
        decision_time_unix_ms: DECISION_MS,
        supplied: SuppliedInputsV6::default(),
    }
}

fn observation(temperature_c: Option<&str>, temperature_f: Option<&str>) -> SuppliedObservationV6 {
    SuppliedObservationV6 {
        envelope: Some(envelope("evt-obs-44", 44)),
        source: "minutetemp.websocket.v1".to_owned(),
        station_id: STATION.to_owned(),
        observed_at_unix_ns: OBSERVED_NS,
        lag_seconds: Some(45),
        preliminary: true,
        temperature_c: temperature_c.and_then(decimal),
        temperature_f: temperature_f.and_then(decimal),
        temp_min_f: decimal("72.5"),
        temp_max_f: decimal("74.3"),
        temp_min_c: decimal("22.5"),
        temp_max_c: decimal("23.5"),
        is_from_report: true,
        report_type: Some("metar_tgroup".to_owned()),
        source_report_id: Some("report.tgroup.9".to_owned()),
        dewpoint: decimal("12.8"),
        relative_humidity: decimal("53.4"),
        wind_direction: decimal("230"),
        barometric_pressure: decimal("1013.25"),
        text_description: Some("Partly Cloudy".to_owned()),
        temperature_day_mode: Some("nws_climate_day".to_owned()),
        temperature_day_date: Some("2026-08-30".to_owned()),
        ..Default::default()
    }
}

fn report(revision: u64, max_temp_f: &str) -> SuppliedReportV6 {
    SuppliedReportV6 {
        envelope: Some(envelope(&format!("evt-dsm-{revision}"), 40 + revision)),
        source: "minutetemp.websocket.v1".to_owned(),
        station_id: STATION.to_owned(),
        report_id: "report.dsm.1".to_owned(),
        report_fingerprint: Some(format!("fp-{revision}")),
        report_revision: Some(revision),
        report_updated_at_unix_ns: Some(1_788_062_300_000_000_000 + revision as i64),
        report_type: "dsm".to_owned(),
        report_date: "2026-08-30".to_owned(),
        issuance_time_unix_ns: Some(1_788_062_280_000_000_000),
        fetched_at_unix_ns: Some(1_788_062_290_000_000_000),
        source_url: Some("https://weather.example/dsm".to_owned()),
        max_temp_f: decimal(max_temp_f),
        max_temp_c: decimal("26.7"),
        max_temp_time_unix_ns: Some(1_788_051_000_000_000_000),
        min_temp_f: decimal("58"),
        min_temp_c: decimal("14.4"),
        min_temp_time_unix_ns: Some(1_788_020_000_000_000_000),
        temp_f: None,
        temp_c: None,
        provider: Some("dsm".to_owned()),
    }
}

fn episode(state: &str, sequence: u64) -> SuppliedWeatherEventV6 {
    SuppliedWeatherEventV6 {
        envelope: Some(envelope(&format!("evt-wx-{sequence}"), sequence)),
        source: "minutetemp.websocket.v1".to_owned(),
        station_id: STATION.to_owned(),
        episode_id: "01a03d6a-f462-7153-9133-dbd2a26af5b4".to_owned(),
        event_type: "thunderstorm".to_owned(),
        tier: "tier1".to_owned(),
        state: state.to_owned(),
        name: "Thunderstorm".to_owned(),
        badge: Some("TS".to_owned()),
        detail: None,
        summary: Some("Thunderstorm near KSEA".to_owned()),
        started_at_unix_ns: Some(1_788_060_000_000_000_000),
        last_confirmed_at_unix_ns: Some(1_788_062_000_000_000_000),
        ended_at_unix_ns: (state == "ended").then_some(1_788_062_400_000_000_000),
        source_snapshot: None,
    }
}

fn supplied_station(observation: SuppliedObservationV6) -> SuppliedStationV6 {
    SuppliedStationV6 {
        station_id: STATION.to_owned(),
        observation: Some(observation),
        daily_extremes: Some(SuppliedDailyExtremesV6 {
            source: "minutetemp.rest.latest".to_owned(),
            received_at_unix_ns: 1_788_000_000_000_000_000,
            daily_high_f: decimal("80.1"),
            daily_low_f: decimal("58"),
            asos_daily_high_f: decimal("79.5"),
            ..Default::default()
        }),
        reports: vec![report(2, "80")],
        extreme_high: None,
        extreme_low: None,
        weather_events: vec![episode("active", 42)],
        forecast: Some(SuppliedForecastV6 {
            source: "minutetemp.rest.forecast".to_owned(),
            received_at_unix_ns: 1_788_000_000_000_000_000,
            advertised_versions: vec![("hrrr".to_owned(), "2026-08-30T18:00:00Z".to_owned())],
            models: vec![SuppliedForecastModelV6 {
                model_id: "hrrr".to_owned(),
                fetched_at: Some("2026-08-30T18:00:00Z".to_owned()),
                fetched_at_unix_ns: Some(1_788_055_200_000_000_000),
                hourly: vec![SuppliedForecastPointV6 {
                    time: "2026-08-30T19:00:00Z".to_owned(),
                    time_unix_ns: 1_788_058_800_000_000_000,
                    temperature_2m_f: decimal("78.6"),
                    temperature_2m_c: decimal("25.88888888888889"),
                    apparent_temperature_f: decimal("77.9"),
                    ..Default::default()
                }],
                ..Default::default()
            }],
        }),
        oracle_tables: vec![SuppliedOracleTableV6 {
            updated_at_unix_ns: Some(1_788_000_000_000_000_123),
            notification_updated_at_unix_ns: Some(1_788_000_000_000_000_987),
            source: "minutetemp.rest.oracle".to_owned(),
            received_at_unix_ns: 1_788_000_000_000_000_000,
            station_id: STATION.to_owned(),
            range_start: "2026-08-23".to_owned(),
            range_end: "2026-08-29".to_owned(),
            days_requested: Some(7),
            score_mode: Some("day_of".to_owned()),
            rank_by: Some("high".to_owned()),
            scores: vec![SuppliedOracleScoreV6 {
                rank: Some(1),
                model_id: "hrrr".to_owned(),
                model_name: "HRRR".to_owned(),
                high_mae: decimal("0.123456789012345678"),
                day_count: Some(7),
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
}

fn with_supplied(
    mut context: DecisionContextV6,
    station: SuppliedStationV6,
    event: SuppliedEventV6,
) -> DecisionContextV6 {
    context.supplied = SuppliedInputsV6 {
        contract_version: SUPPLIED_INPUTS_CONTRACT_VERSION.to_owned(),
        stations: vec![station],
        originating_event: Some(event),
    };
    context
}

fn observation_context(
    temperature_c: Option<&str>,
    temperature_f: Option<&str>,
) -> DecisionContextV6 {
    let mut context = base_context();
    context.owner_state.trigger = TriggerV4::Weather {
        station_id: STATION.to_owned(),
        source_generation: 3,
        source_sequence: 44,
    };
    let station = &mut context.owner_state.stations[0];
    station.observation_meta.revision = 7;
    station.observation.observed_at_unix_ms = OBSERVED_NS / 1_000_000;
    station.observation.temperature_milli_c = Some(22_778);
    station.observation.preliminary = true;
    station.observation.provenance = provenance("evt-obs-44", 44);
    station.weather.current_temperature_milli_c = Some(22_778);
    station.weather.preliminary = true;
    station.weather_events_meta.revision = 5;
    station.weather_events.push(WeatherEventV4 {
        event_id: "01a03d6a-f462-7153-9133-dbd2a26af5b4".to_owned(),
        state: "active".to_owned(),
        ..Default::default()
    });
    context.trigger = TriggerV6::Owner(OwnerTriggerV6::Observation {
        station_id: STATION.to_owned(),
        observed_at_unix_ms: OBSERVED_NS / 1_000_000,
        component_revision: 7,
        source_generation: 3,
        source_sequence: 44,
    });
    let observation = observation(temperature_c, temperature_f);
    with_supplied(
        context,
        supplied_station(observation.clone()),
        SuppliedEventV6::Observation(observation),
    )
}

fn report_context() -> DecisionContextV6 {
    let mut context = base_context();
    context.owner_state.trigger = TriggerV4::StationReport {
        station_id: STATION.to_owned(),
        report_id: "report.dsm.1".to_owned(),
        report_type: "dsm".to_owned(),
        report_revision: 2,
        provider: "dsm".to_owned(),
        source_generation: 3,
        source_sequence: 42,
    };
    context.owner_state.stations[0].reports.push(ReportV4 {
        report_id: "report.dsm.1".to_owned(),
        report_type: "dsm".to_owned(),
        report_date: "2026-08-30".to_owned(),
        revision: 2,
        provider: "dsm".to_owned(),
        max_temperature_milli_c: Some(26_700),
        provenance: provenance("evt-dsm-2", 42),
        ..Default::default()
    });
    context.trigger = TriggerV6::Owner(OwnerTriggerV6::StationReport {
        station_id: STATION.to_owned(),
        report_id: "report.dsm.1".to_owned(),
        report_type: "dsm".to_owned(),
        report_revision: 2,
        provider: "dsm".to_owned(),
        source_generation: 3,
        source_sequence: 42,
    });
    with_supplied(
        context,
        supplied_station(observation(Some("22.8"), Some("73"))),
        SuppliedEventV6::Report(report(2, "80")),
    )
}

fn new_low_context() -> DecisionContextV6 {
    let mut context = base_context();
    context.owner_state.trigger = TriggerV4::Weather {
        station_id: STATION.to_owned(),
        source_generation: 3,
        source_sequence: 50,
    };
    let station = &mut context.owner_state.stations[0];
    station.extrema_meta.revision = 4;
    station.extrema.low = Some(ExtremeV4 {
        value_milli_c: 14_444,
        observed_at_unix_ms: Some(1_788_020_000_000),
        ..Default::default()
    });
    context.trigger = TriggerV6::Owner(OwnerTriggerV6::NewLow {
        station_id: STATION.to_owned(),
        event_date: Some("2026-08-30".to_owned()),
        temperature_milli_c: Some(14_444),
        observed_at_unix_ms: 1_788_020_000_000,
        component_revision: 4,
        source_generation: 3,
        source_sequence: 50,
    });
    let extreme = SuppliedExtremeV6 {
        envelope: Some(envelope("evt-low-50", 50)),
        source: "minutetemp.websocket.v1".to_owned(),
        kind: ExtremeKindV6::Low,
        station_id: STATION.to_owned(),
        value_f: decimal("58"),
        value_c: decimal("14.44444444444444"),
        prev_value_f: decimal("58.4"),
        observed_at_unix_ns: Some(1_788_020_000_000_000_000),
        temperature_day_mode: Some("nws_climate_day".to_owned()),
        temperature_day_date: Some("2026-08-30".to_owned()),
        is_from_report: false,
        report_type: None,
        source_report_id: None,
    };
    let mut station = supplied_station(observation(Some("22.8"), Some("73")));
    station.extreme_low = Some(extreme.clone());
    with_supplied(context, station, SuppliedEventV6::Extreme(extreme))
}

#[test]
fn accepted_near_equal_extreme_does_not_select_the_older_rest_original() {
    let mut context = new_low_context();
    let station = &mut context.supplied.stations[0];
    station.extreme_low.as_mut().unwrap().value_f = decimal("57.999999999");
    station.extreme_low.as_mut().unwrap().value_c = decimal("14.44444444388889");
    context.supplied.originating_event = Some(SuppliedEventV6::Extreme(
        station.extreme_low.clone().unwrap(),
    ));
    context.owner_state.stations[0].weather.running_low_milli_c = Some(14_444);
    context.owner_state.stations[0].revision = 4;
    context.owner_state.stations[0]
        .provider_cursor
        .connection_generation = 3;
    let mut facts = strategy_core_kernel::WeatherFacts::default();
    facts.insert(
        strategy_core_kernel::WeatherField::RunningLow,
        strategy_core_kernel::WeatherValue::Temperature {
            c: decimal("14.44444444388889"),
            f: decimal("57.999999999"),
        },
        &strategy_core_kernel::WeatherFactProvenance {
            source: "minutetemp.websocket.v1".to_owned(),
            envelope: Some(envelope("evt-low-50", 50)),
            owner_generation: 3,
            owner_revision: 4,
            supplied: true,
            ..Default::default()
        },
    );
    context.current_weather = Some(vec![strategy_core_v3::decision_v6::StationWeatherV6 {
        station_id: STATION.to_owned(),
        facts,
    }]);
    let encoded = encode_decision_context_v6(&context).unwrap();
    assert!(encoded.starts_with(b"SDCTXV6A"));
    assert_eq!(decode_decision_context_v6(&encoded).unwrap(), context);
    let snapshot =
        KernelSnapshot::from_context(&decode_decision_context_v6(&encoded).unwrap()).unwrap();
    let current = snapshot.station(STATION).unwrap();
    assert_eq!(
        current.daily_extremes.as_ref().unwrap().daily_low_f,
        Some(58.0)
    );
    assert_eq!(current.extreme_low.as_ref().unwrap().value_f, 57.999999999);
    assert_eq!(
        current.weather.running_low,
        Some(57.999999999),
        "the accepted extreme owns the summary even when its rounded milli-C equals REST"
    );
}

fn ended_episode_context() -> DecisionContextV6 {
    let mut context = base_context();
    context.owner_state.trigger = TriggerV4::Weather {
        station_id: STATION.to_owned(),
        source_generation: 3,
        source_sequence: 45,
    };
    context.owner_state.stations[0].weather_events_meta.revision = 6;
    context.trigger = TriggerV6::Owner(OwnerTriggerV6::WeatherEvent {
        station_id: STATION.to_owned(),
        episode_id: "01a03d6a-f462-7153-9133-dbd2a26af5b4".to_owned(),
        state: "ended".to_owned(),
        component_revision: 6,
        source_generation: 3,
        source_sequence: 45,
    });
    let mut station = supplied_station(observation(Some("22.8"), Some("73")));
    station.weather_events.clear();
    with_supplied(
        context,
        station,
        SuppliedEventV6::WeatherEvent(episode("ended", 45)),
    )
}

/// Records every view field a frozen kernel could read.
#[derive(Clone, Default)]
struct RecordingKernel {
    seen: Rc<RefCell<Vec<String>>>,
    place: bool,
}

impl NativeKernel for RecordingKernel {
    fn name(&self) -> &str {
        "recording"
    }

    fn on_start(&mut self, _context: &mut dyn StrategyKernelContext) -> KernelResult<()> {
        self.seen.borrow_mut().push("start".to_owned());
        Ok(())
    }

    fn on_event(
        &mut self,
        event: StrategyEventView<'_>,
        context: &mut dyn StrategyKernelContext,
    ) -> KernelResult<()> {
        self.seen.borrow_mut().push(format!("{event:?}"));
        let weather = context.state().get_weather(STATION).unwrap();
        self.seen.borrow_mut().push(format!("weather={weather:?}"));
        let forecast = context.state().latest_forecast(STATION).unwrap();
        self.seen
            .borrow_mut()
            .push(format!("forecast={forecast:?}"));
        let price = context.state().get_price(MARKET).unwrap();
        self.seen.borrow_mut().push(format!("price={price:?}"));
        let station = context.state().station(STATION).unwrap();
        assert_eq!(weather, station.weather);
        assert_eq!(forecast, station.forecast.as_ref().unwrap().snapshot());
        assert_eq!(price, context.state().market(MARKET).unwrap().ticker_view());
        let oracle = context
            .state()
            .latest_oracle_scores(STATION, Some("day_of"), Some("high"), Some("7"))
            .unwrap();
        assert_eq!(
            oracle,
            station
                .oracle_table(Some("day_of"), Some("high"), Some("7"))
                .unwrap()
                .snapshot()
        );
        assert_eq!(
            oracle.supplied.unwrap().scores[0].high_mae,
            decimal("0.123456789012345678")
        );
        assert_eq!(
            oracle.updated_at.unwrap().timestamp_nanos_opt(),
            Some(1_788_000_000_000_000_123),
            "table freshness must not borrow the notification timestamp"
        );
        assert_eq!(
            oracle.supplied.unwrap().notification_updated_at_unix_ns,
            Some(1_788_000_000_000_000_987)
        );
        assert_eq!(oracle.supplied.unwrap().scores[0].rank, Some(1));
        assert!(
            context
                .state()
                .latest_oracle_scores(STATION, Some("day_ahead"), None, None)
                .is_none()
        );
        assert!(context.state().station("UNKNOWN").is_none());
        assert!(context.state().get_weather("UNKNOWN").is_none());
        assert!(context.state().latest_forecast("UNKNOWN").is_none());
        assert!(context.state().market("UNKNOWN").is_none());
        assert!(context.state().get_price("UNKNOWN").is_none());
        self.seen.borrow_mut().push(format!(
            "station={} origin={:?} pressure={:?} open_interest={:?}",
            station.station_id(),
            station
                .observation
                .as_ref()
                .map(|observation| observation.origin),
            station
                .observation
                .as_ref()
                .and_then(|observation| observation.barometric_pressure),
            context.state().market(MARKET).and_then(|market| market
                .ticker
                .as_ref()
                .and_then(|ticker| ticker.open_interest))
        ));
        if let Some(secondary) = context.state().station("KSFO") {
            self.seen.borrow_mut().push(format!(
                "secondary={} temperature_f={:?}",
                secondary.station_id(),
                secondary
                    .observation
                    .as_ref()
                    .and_then(|observation| observation.temperature_f),
            ));
        }
        context
            .telemetry()
            .counter("fixture_counter", 1.0, &[("k", "v")])?;
        context.emit(KernelAction::Log(LogAction {
            level: "info".to_owned(),
            message: serde_json::json!({"code": "gate", "reason": "fixture"}).to_string(),
        }))?;
        if self.place {
            let result = context.broker().place_order(PlaceOrderRequest {
                ticker: MARKET.to_owned(),
                action: OrderAction::Buy,
                contract_side: ContractSide::Yes,
                order_type: OrderType::Limit,
                quantity: ContractQuantity::from_hundredths(300),
                limit_price: Some(0.4),
                expires_after_ms: Some(30_000),
                reduce_only: false,
                signal_type: Some("fixture".to_owned()),
                signal_metadata: None,
                client_order_id: Some("client.fixture.1".to_owned()),
            })?;
            self.seen.borrow_mut().push(format!("placed={result:?}"));
        }
        Ok(())
    }
}

impl TransactionKernel for RecordingKernel {
    fn encode_checkpoint_state(&self) -> Result<Vec<u8>, KernelTransactionError> {
        Ok(format!("events={}", self.seen.borrow().len()).into_bytes())
    }
}

struct Factory {
    seen: Rc<RefCell<Vec<String>>>,
    place: bool,
}

impl TransactionKernelFactory for Factory {
    type Kernel = RecordingKernel;

    fn checkpoint_codec(
        &self,
        strategy_id: &str,
    ) -> Result<KernelCheckpointCodec, KernelTransactionError> {
        (strategy_id == "fixture")
            .then(|| KernelCheckpointCodec {
                profile: "fixture.checkpoint.v1".to_owned(),
                version: 1,
            })
            .ok_or_else(|| KernelTransactionError::UnsupportedStrategy(strategy_id.to_owned()))
    }
    fn create(&self, _context: &DecisionContextV6) -> Result<Self::Kernel, KernelTransactionError> {
        Ok(RecordingKernel {
            seen: Rc::clone(&self.seen),
            place: self.place,
        })
    }
    fn restore(
        &self,
        context: &DecisionContextV6,
        _checkpoint: &strategy_core_v3::decision_v6::KernelCheckpointV6,
    ) -> Result<Self::Kernel, KernelTransactionError> {
        self.create(context)
    }
}

fn run(
    context: &DecisionContextV6,
    place: bool,
) -> (Vec<String>, strategy_core_v3::decision_v6::DecisionResultV6) {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let factory = Factory {
        seen: Rc::clone(&seen),
        place,
    };
    let result = run_transaction(&factory, context).unwrap();
    let seen = seen.borrow().clone();
    (seen, result)
}

fn view_of(context: &DecisionContextV6) -> String {
    let event = KernelEvent::from_context(context).unwrap();
    format!("{:?}", event.view().unwrap())
}

#[test]
fn supplied_temperatures_reach_views_per_unit_without_conversion() {
    for (name, celsius, fahrenheit, expect_f, expect_c) in [
        (
            "both-distinct",
            Some("22.77777777777778"),
            Some("73"),
            Some(73.0),
            Some(22.77777777777778),
        ),
        ("f-only", None, Some("72.5"), Some(72.5), None),
        ("c-only", Some("22.8"), None, None, Some(22.8)),
        ("absent", None, None, None, None),
        ("zero", Some("0"), Some("32"), Some(32.0), Some(0.0)),
        (
            "beyond-milli",
            Some("22.777777777777779"),
            Some("73.000000000000001"),
            Some(73.000000000000001),
            Some(22.777_777_777_777_78),
        ),
    ] {
        let context = observation_context(celsius, fahrenheit);
        context.validate().unwrap();
        let event = KernelEvent::from_context(&context).unwrap();
        let StrategyEventView::Observation(view) = event.view().unwrap() else {
            panic!("{name}: expected observation view");
        };
        assert_eq!(view.temperature_f, expect_f, "{name}");
        assert_eq!(view.temperature_c, expect_c, "{name}");
        assert_eq!(view.temp_min_f, Some(72.5), "{name}");
        assert_eq!(view.temp_max_f, Some(74.3), "{name}");
        assert_eq!(view.temp_min_c, Some(22.5), "{name}");
        assert_eq!(view.temp_max_c, Some(23.5), "{name}");
        assert!(view.preliminary && view.is_from_report, "{name}");
        assert_eq!(view.report_type, Some("metar_tgroup"), "{name}");
        assert_eq!(view.source_report_id, Some("report.tgroup.9"), "{name}");
        assert_eq!(view.lag_seconds, Some(45), "{name}");
        assert_eq!(view.dewpoint, Some(12.8), "{name}");
        assert_eq!(view.relative_humidity, Some(53.4), "{name}");
        assert_eq!(view.wind_direction, Some(230.0), "{name}");
        assert_eq!(view.text_description, Some("Partly Cloudy"), "{name}");
        assert_eq!(view.temperature_day_mode, Some("nws_climate_day"), "{name}");
        assert_eq!(view.temperature_day_date, Some("2026-08-30"), "{name}");
        assert_eq!(view.event_id, Some("evt-obs-44"), "{name}");
        assert_eq!(view.sequence, Some(44), "{name}");
        assert_eq!(view.city_sequence, Some(9), "{name}");
        assert_eq!(view.slug, "sea", "{name}");

        // Publication, observation, receipt and decision times are all distinct.
        assert_eq!(view.emitted_at, Some(ns(EMITTED_NS)), "{name}");
        assert_eq!(view.observed_at, Some(ns(OBSERVED_NS)), "{name}");
        assert_ne!(
            view.emitted_at.unwrap().timestamp_millis(),
            DECISION_MS,
            "{name}: emitted_at is never the decision clock"
        );
        assert_ne!(view.emitted_at, view.observed_at, "{name}");
        let received = context
            .supplied
            .originating_event
            .as_ref()
            .and_then(SuppliedEventV6::envelope)
            .unwrap()
            .received_at_unix_ns;
        assert!(received > EMITTED_NS, "{name}");

        // The station weather view reads the same originals and the REST daily extremes.
        let snapshot = KernelSnapshot::from_context(&context).unwrap();
        let weather = (&snapshot as &dyn StrategyKernelState)
            .get_weather(STATION)
            .unwrap();
        let expected_current = expect_f.or_else(|| Some(22.778 * 9.0 / 5.0 + 32.0));
        assert_eq!(weather.current_temp, expected_current, "{name}");
        assert_eq!(weather.temp_max_f, Some(74.3), "{name}");
        assert_eq!(weather.running_high, Some(80.1), "{name}");
        assert_eq!(weather.asos_daily_high_f, Some(79.5), "{name}");
        assert!(weather.preliminary, "{name}");
    }
}

#[test]
fn supplied_report_view_carries_originals_publication_time_and_correction_identity() {
    let context = report_context();
    context.validate().unwrap();
    let event = KernelEvent::from_context(&context).unwrap();
    let StrategyEventView::StationReport(view) = event.view().unwrap() else {
        panic!("expected report view");
    };
    assert_eq!(view.max_temp_f, Some(80.0));
    assert_eq!(view.max_temp_c, Some(26.7));
    assert_eq!(view.min_temp_f, Some(58.0));
    assert_eq!(view.temp_f, None);
    assert_eq!(view.report_revision, 2);
    assert_eq!(view.report_id, "report.dsm.1");
    assert_eq!(
        view.event_id,
        Some("evt-dsm-2"),
        "provider event identity, not the report id"
    );
    assert_eq!(
        view.sequence,
        Some(42),
        "provider sequence, not the revision"
    );
    assert_eq!(view.emitted_at, Some(ns(EMITTED_NS)));
    assert_eq!(view.issuance_time, Some(ns(1_788_062_280_000_000_000)));
    assert_eq!(view.fetched_at, Some(ns(1_788_062_290_000_000_000)));
    assert_eq!(view.report_updated_at, Some(ns(1_788_062_300_000_000_002)));
    assert_eq!(view.max_temp_time_utc, Some(ns(1_788_051_000_000_000_000)));
    assert_eq!(view.provider, "dsm");
    assert_eq!(view.source_url, "https://weather.example/dsm");

    // A superseded revision in the supplied station state is not the originating event.
    let mut corrected = report_context();
    corrected.supplied.stations[0].reports = vec![report(3, "81")];
    corrected.owner_state.stations[0].reports[0].revision = 3;
    corrected.owner_state.trigger = TriggerV4::StationReport {
        station_id: STATION.to_owned(),
        report_id: "report.dsm.1".to_owned(),
        report_type: "dsm".to_owned(),
        report_revision: 3,
        provider: "dsm".to_owned(),
        source_generation: 3,
        source_sequence: 43,
    };
    corrected.trigger = TriggerV6::Owner(OwnerTriggerV6::StationReport {
        station_id: STATION.to_owned(),
        report_id: "report.dsm.1".to_owned(),
        report_type: "dsm".to_owned(),
        report_revision: 3,
        provider: "dsm".to_owned(),
        source_generation: 3,
        source_sequence: 43,
    });
    assert!(
        corrected.validate().is_err(),
        "an originating revision 2 event cannot accompany a revision 3 trigger"
    );
    corrected.supplied.originating_event = Some(SuppliedEventV6::Report(report(3, "81")));
    corrected.validate().unwrap();
    assert!(view_of(&corrected).contains("max_temp_f: Some(81.0)"));
}

#[test]
fn derived_fallback_projects_the_v4_projection_when_nothing_was_supplied() {
    let mut context = report_context();
    context.supplied = SuppliedInputsV6::default();
    context.owner_state.stations[0].reports[0].temperature_milli_f = Some(73_400);
    context.validate().unwrap();
    let event = KernelEvent::from_context(&context).unwrap();
    let StrategyEventView::StationReport(view) = event.view().unwrap() else {
        panic!("expected report view");
    };
    assert_eq!(
        view.max_temp_f,
        Some(26.7 * 9.0 / 5.0 + 32.0),
        "derived from milli-C"
    );
    assert_eq!(
        view.temp_f,
        Some(73.4),
        "V4 carries the supplied point milli-F"
    );
    assert_eq!(view.event_id, Some("evt-dsm-2"));
    assert_eq!(
        view.emitted_at,
        Some(Utc.timestamp_millis_opt(EMITTED_NS / 1_000_000).unwrap())
    );
    assert_eq!(view.source_url, "");

    let mut observation = observation_context(Some("22.8"), Some("73"));
    observation.supplied = SuppliedInputsV6::default();
    observation.validate().unwrap();
    let event = KernelEvent::from_context(&observation).unwrap();
    let StrategyEventView::Observation(view) = event.view().unwrap() else {
        panic!("expected observation view");
    };
    assert_eq!(view.temperature_c, Some(22.778));
    assert_eq!(view.temperature_f, Some(22.778 * 9.0 / 5.0 + 32.0));
    assert!(view.preliminary);
    assert_eq!(
        view.emitted_at,
        Some(Utc.timestamp_millis_opt(EMITTED_NS / 1_000_000).unwrap())
    );
}

#[test]
fn new_low_and_episode_events_are_typed_with_their_own_identities() {
    let low = new_low_context();
    low.validate().unwrap();
    let event = KernelEvent::from_context(&low).unwrap();
    let StrategyEventView::NewLow(view) = event.view().unwrap() else {
        panic!("expected new low");
    };
    assert_eq!(view.value_f, 58.0);
    assert_eq!(view.value_c, 14.44444444444444);
    assert_eq!(view.prev_value_f, Some(58.4));
    assert_eq!(view.event_id, Some("evt-low-50"));
    assert_eq!(view.event_key, "2026-08-30");
    assert_eq!(view.persistence_status, Some("uncommitted"));
    assert_eq!(view.wmo_emit_time, Some(ns(OBSERVED_NS + 60_000_000_000)));
    assert_eq!(view.observed_at, Some(ns(1_788_020_000_000_000_000)));

    let ended = ended_episode_context();
    ended.validate().unwrap();
    let event = KernelEvent::from_context(&ended).unwrap();
    let StrategyEventView::WeatherEvent(view) = event.view().unwrap() else {
        panic!("expected weather event");
    };
    assert_eq!(view.id, "01a03d6a-f462-7153-9133-dbd2a26af5b4");
    assert_eq!(
        view.event_id,
        Some("evt-wx-45"),
        "envelope identity is distinct from the episode"
    );
    assert_eq!(view.state, "ended");
    assert_eq!(view.ended_at, Some(ns(1_788_062_400_000_000_000)));
    assert_eq!(view.badge, "TS");
    assert_eq!(view.detail, "");

    let active = observation_context(Some("22.8"), Some("73"));
    let snapshot = KernelSnapshot::from_context(&active).unwrap();
    let station = snapshot.station(STATION).unwrap();
    assert_eq!(station.weather_events[0].state, "active");
    assert_eq!(station.weather_events[0].origin, ValueOrigin::Supplied);
    let observation = station.observation.as_ref().unwrap();
    assert_eq!(
        observation.barometric_pressure,
        Some(1013.25),
        "every supplied field is an ordinary typed field of the canonical state"
    );
    assert_eq!(
        observation.supplied.as_ref().unwrap().barometric_pressure,
        decimal("1013.25"),
        "and the exact original travels with it"
    );
    let event = KernelEvent::from_context(&active).unwrap();
    let StrategyEventView::Observation(view) = event.view().unwrap() else {
        panic!("expected observation view");
    };
    assert_eq!(view.origin, ValueOrigin::Supplied);
    assert_eq!(
        view.supplied.unwrap().temperature_c,
        decimal("22.8"),
        "the event view carries the supplied original beside its conveniences"
    );

    // An ended episode without its supplied event cannot be reconstructed from state.
    let mut unsupplied = ended_episode_context();
    unsupplied.supplied = SuppliedInputsV6::default();
    let event = KernelEvent::from_context(&unsupplied).unwrap();
    assert!(matches!(
        event.view().unwrap(),
        StrategyEventView::Unknown {
            event_type: "weather_event",
            ..
        }
    ));
}

#[test]
fn transaction_runner_presents_the_event_over_supplied_state_and_bridges_the_broker() {
    let mut context = observation_context(Some("22.77777777777778"), Some("73"));
    context
        .owner_state
        .opportunity
        .contributor_stations
        .push("KSFO".to_owned());
    let primary = &context.owner_state.stations[0];
    context.owner_state.stations.push(StationV4 {
        identity: StationIdentityV4 {
            station_id: "KSFO".to_owned(),
            logical_location: "sfo".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
            ..Default::default()
        },
        climate_event_date: primary.climate_event_date.clone(),
        climate_day_start_utc_unix_ms: primary.climate_day_start_utc_unix_ms,
        climate_day_end_utc_unix_ms: primary.climate_day_end_utc_unix_ms,
        ..Default::default()
    });
    context.supplied.stations.push(SuppliedStationV6 {
        station_id: "KSFO".to_owned(),
        observation: Some(SuppliedObservationV6 {
            source: "minutetemp.rest.latest".to_owned(),
            station_id: "KSFO".to_owned(),
            observed_at_unix_ns: OBSERVED_NS,
            temperature_c: decimal("17"),
            temperature_f: decimal("62.6"),
            ..Default::default()
        }),
        ..Default::default()
    });
    let (seen, result) = run(&context, false);
    assert!(
        seen.iter()
            .any(|row| row == "secondary=KSFO temperature_f=Some(62.6)"),
        "the invocation must expose every delivered contributor, not only the primary station"
    );
    assert_eq!(result.disposition, DecisionDispositionV6::Completed);
    assert_eq!(result.kernel_checkpoint.as_ref().unwrap().sequence, 1);
    assert!(seen[0].starts_with("Observation("), "{}", seen[0]);
    assert!(seen[0].contains("temperature_f: Some(73.0)"), "{}", seen[0]);
    assert!(seen[1].contains("running_high: Some(80.1)"), "{}", seen[1]);
    assert!(
        seen[2].contains("temperature_2m_c: Some(25.88888888888889)"),
        "{}",
        seen[2]
    );
    assert!(
        seen[2].contains("time: \"2026-08-30T19:00:00Z\""),
        "original time text"
    );
    assert!(
        seen[3].contains("yes_ask_depth: Some(12)"),
        "whole-contract depth is the explicit floor: {}",
        seen[3]
    );
    assert!(
        seen[3].contains("yes_ask_quantity: Some(ContractQuantity(1250))"),
        "the exact hundredths are beside it: {}",
        seen[3]
    );
    assert!(seen[3].contains("volume: Some(100.5)"), "{}", seen[3]);
    assert!(
        seen[3].contains("open_interest: Some(ContractQuantity(5000))"),
        "{}",
        seen[3]
    );
    assert_eq!(
        seen[4],
        "station=KSEA origin=Some(Supplied) pressure=Some(1013.25) open_interest=Some(ContractQuantity(5000))"
    );
    assert!(
        result
            .telemetry
            .iter()
            .any(|entry| matches!(entry, TelemetryEntryV6::Counter { name, .. } if name == "fixture_counter")),
        "counters survive as typed telemetry"
    );
    assert!(
        result.diagnostics.iter().any(|d| d.code == "kernel_log"),
        "logs are diagnostics"
    );
    assert!(
        result.commands.is_empty(),
        "logs and telemetry are not commands"
    );

    // A Broker call returns its ticket at once; the run completes with the command.
    let (seen, placed) = run(&context, true);
    assert_eq!(placed.disposition, DecisionDispositionV6::Completed);
    assert_eq!(placed.kernel_checkpoint.as_ref().unwrap().sequence, 1);
    let StrategyCommandV6::PlaceOrder(command) = &placed.commands[0] else {
        panic!("expected place order");
    };
    assert_eq!(command.command_id, cid(1, 0));
    assert_eq!(command.provider_client_id, "client.fixture.1");
    assert_eq!(command.quantity_hundredths, 300);
    assert_eq!(command.limit_price_micros, Some(400_000));
    assert!(
        seen.iter().any(|line| line
            == &format!(
                r#"placed=OrderTicket {{ command_id: "{}", client_order_id: "client.fixture.1" }}"#,
                cid(1, 0)
            )),
        "{seen:?}"
    );
    let runner = &placed.kernel_checkpoint.as_ref().unwrap().runner;
    assert_eq!(
        runner.entries.len(),
        1,
        "the place is tracked until its outcome is seen"
    );
    assert_eq!(runner.entries[0].command_id, command.command_id);
    let stored =
        decode_decision_context_v6(&encode_decision_context_v6(&context).unwrap()).unwrap();
    assert_eq!(
        run(&stored, true).1,
        placed,
        "one run per event, deterministic"
    );
}
