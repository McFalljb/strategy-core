use sha2::{Digest, Sha256};
use strategy_core_v3::decision_v4::*;

fn text(length: usize) -> String {
    "x".repeat(length)
}
fn provenance() -> ProvenanceV4 {
    ProvenanceV4 {
        provider: text(128),
        source: text(128),
        received_at_unix_ms: 1,
        ..Default::default()
    }
}
fn meta() -> ComponentMetaV4 {
    ComponentMetaV4 {
        authority: AuthorityV4::Current,
        revision: 1,
        generation: 1,
        provenance: vec![provenance(); MAX_PROVENANCE_PER_COMPONENT],
        ..Default::default()
    }
}
fn evidence() -> EvidenceV4 {
    EvidenceV4 {
        kind: text(128),
        reference: text(512),
        byte_count: u64::MAX,
        ..Default::default()
    }
}
fn station(index: usize) -> StationV4 {
    let id = format!("S{index}");
    let point = ForecastPointV4 {
        at_unix_ms: 1,
        temperature_milli_c: Some(i32::MAX),
        apparent_temperature_milli_c: Some(i32::MAX),
        humidity_millionths: Some(i64::MAX),
        dew_point_milli_c: Some(i32::MAX),
        pressure_msl_micros: Some(i64::MAX),
        wind_speed_micros: Some(i64::MAX),
        wind_direction_micros: Some(i64::MAX),
        wind_gust_micros: Some(i64::MAX),
        cloud_cover_millionths: Some(i64::MAX),
        precipitation_probability_millionths: Some(i64::MAX),
        weather_code: Some(i32::MAX),
    };
    let models = (0..MAX_MODELS_PER_STATION)
        .map(|model| ForecastModelV4 {
            model_id: format!("m{model:02}"),
            version: text(128),
            run_id: Some(text(128)),
            fetched_at_unix_ms: Some(i64::MAX),
            issued_at_unix_ms: Some(i64::MAX),
            timezone: Some(text(128)),
            utc_offset_seconds: Some(i32::MAX),
            hourly: (0..MAX_POINTS_PER_MODEL)
                .map(|offset| ForecastPointV4 {
                    at_unix_ms: 1 + offset as i64,
                    ..point.clone()
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    let report = ReportV4 {
        report_id: text(128),
        report_type: text(128),
        report_date: text(32),
        authority: AuthorityV4::Current,
        provider: text(128),
        source_url: text(2048),
        provenance: provenance(),
        ..Default::default()
    };
    let event = WeatherEventV4 {
        event_id: text(128),
        event_type: text(128),
        tier: text(128),
        state: text(128),
        authority: AuthorityV4::Current,
        name: text(256),
        badge: text(256),
        detail: text(2048),
        summary: text(2048),
        source: Some(WeatherEventSourceV4 {
            metar_type: Some(text(2048)),
            flight_category: Some(text(2048)),
            wx_string: Some(text(2048)),
            wx_token: Some(text(2048)),
            cb_location: Some(text(2048)),
            ..Default::default()
        }),
        provenance: provenance(),
        ..Default::default()
    };
    let row = OracleRowV4 {
        rank: 1,
        model_id: text(128),
        model_name: text(256),
        is_public: Some(true),
        high_mae_millionths: Some(i64::MAX),
        low_mae_millionths: Some(i64::MAX),
        combined_mae_millionths: Some(i64::MAX),
        high_bias_millionths: Some(i64::MAX),
        low_bias_millionths: Some(i64::MAX),
        day_count: Some(u16::MAX),
    };
    StationV4 {
        contract_version: "1.0.0".into(),
        revision: 1,
        climate_event_date: "2026-08-26".into(),
        climate_day_start_utc_unix_ms: 0,
        climate_day_end_utc_unix_ms: 1000,
        identity: StationIdentityV4 {
            station_id: id.clone(),
            logical_location: text(128),
            timezone: "America/New_York".into(),
            name: Some(text(2048)),
            ..Default::default()
        },
        observation_meta: meta(),
        observation: ObservationV4 {
            station_id: id.clone(),
            observed_at_unix_ms: 1,
            provenance: provenance(),
            text_description: Some(text(2048)),
            ..Default::default()
        },
        weather_meta: meta(),
        weather: WeatherV4 {
            station_id: id,
            text_description: Some(text(2048)),
            ..Default::default()
        },
        extrema_meta: meta(),
        reports_meta: meta(),
        reports: vec![report; MAX_REPORTS],
        weather_events_meta: meta(),
        weather_events: vec![event; MAX_WEATHER_EVENTS],
        forecast_meta: meta(),
        forecast: ForecastV4 {
            advertised_versions: models
                .iter()
                .map(|m| (m.model_id.clone(), m.version.clone()))
                .collect(),
            models,
        },
        oracle_meta: meta(),
        oracle: OracleTableV4 {
            query: OracleQueryV4 {
                station_id: format!("S{index}"),
                mode: "day_of".into(),
                rank_by: RankByV4::High,
                days: 7,
            },
            range_start: text(32),
            range_end: text(32),
            rows: vec![row; MAX_ORACLE_ROWS],
            provenance: provenance(),
            ..Default::default()
        },
        provider_cursor: CursorV4 {
            connection_generation: 1,
            event_id: text(128),
            sequence: 1,
            snapshot_complete: true,
            ..Default::default()
        },
        evidence: vec![evidence(); MAX_EVIDENCE_REFERENCES],
        ..Default::default()
    }
}
fn market(index: usize) -> MarketV4 {
    let level = BookLevelV4 {
        price_micros: u64::MAX,
        quantity_hundredths: u64::MAX,
    };
    MarketV4 {
        contract_version: "1.0.0".into(),
        revision: 1,
        identity: MarketIdentityV4 {
            market_id: format!("M{index:03}"),
            opportunity_id: "O".into(),
            venue: text(128),
            event_ticker: text(128),
            series_ticker: text(128),
            strike_type: text(128),
            ..Default::default()
        },
        lifecycle_meta: MarketMetaV4 {
            authority: AuthorityV4::Current,
            provenance: vec![provenance(); MAX_PROVENANCE_PER_COMPONENT],
            ..Default::default()
        },
        ticker_meta: MarketMetaV4 {
            authority: AuthorityV4::Current,
            provenance: vec![provenance(); MAX_PROVENANCE_PER_COMPONENT],
            ..Default::default()
        },
        book_meta: MarketMetaV4 {
            authority: AuthorityV4::Current,
            provenance: vec![provenance(); MAX_PROVENANCE_PER_COMPONENT],
            ..Default::default()
        },
        book: Some(BookV4 {
            yes_bids: vec![level.clone(); MAX_BOOK_LEVELS_PER_SIDE],
            no_bids: vec![level; MAX_BOOK_LEVELS_PER_SIDE],
            ..Default::default()
        }),
        last_trade_meta: MarketMetaV4 {
            provenance: vec![provenance(); MAX_PROVENANCE_PER_COMPONENT],
            ..Default::default()
        },
        final_fact_meta: MarketMetaV4 {
            provenance: vec![provenance(); MAX_PROVENANCE_PER_COMPONENT],
            ..Default::default()
        },
        field_provenance: (0..32).map(|i| (format!("f{i}"), provenance())).collect(),
        minutetemp_comparison: Some(MarketComparisonV4 {
            source: text(128),
            event_id: text(128),
            event_date: "2026-08-26".into(),
            evidence_sha256: [7; 32],
            ..Default::default()
        }),
        evidence: vec![evidence(); MAX_EVIDENCE_REFERENCES],
        uncertain_fields: (0..32).map(|i| format!("f{i}")).collect(),
        ..Default::default()
    }
}
fn main() {
    let stations = (0..MAX_STATIONS).map(station).collect::<Vec<_>>();
    let markets = (0..MAX_MARKETS).map(market).collect::<Vec<_>>();
    let context = DecisionContextV4 {
        delivery_id: "schema-max".into(),
        sleeve: SupervisorV4 {
            sleeve_id: "schema-max".into(),
            incarnation: 1,
            process_attempt: 1,
            route_epoch: 1,
        },
        trigger: TriggerV4::Bootstrap,
        fence: FenceV4 {
            sleeve_incarnation: 1,
            config_revision: 1,
            profile_and_calculator_digest: text(128),
            route_epoch: 1,
            route_plan_sha256: [8; 32],
            source_revision: 1,
            source_generation: 1,
            catalog_revision: 1,
            market_authority_generation: 1,
            price_revision: 1,
            broker_revision: 1,
            station_resources: (0..45)
                .map(|i| ResourceRevisionV4 {
                    identity: format!("s{i}"),
                    revision: 1,
                    generation: 1,
                })
                .collect(),
            market_revisions: (0..MAX_MARKETS)
                .map(|i| ResourceRevisionV4 {
                    identity: format!("M{i:03}"),
                    revision: 1,
                    generation: 1,
                })
                .collect(),
        },
        config: ConfigV4 {
            revision: 1,
            profile_and_calculator_digest: text(128),
        },
        opportunity: OpportunityV4 {
            opportunity_id: "O".into(),
            venue_id: text(128),
            match_profile: text(128),
            resolved_scope: text(128),
            lifecycle: LifecycleV4::Eligible,
            close_time_unix_seconds: 1,
            market_ids: markets
                .iter()
                .map(|m| m.identity.market_id.clone())
                .collect(),
            contributor_stations: stations
                .iter()
                .map(|s| s.identity.station_id.clone())
                .collect(),
            authority: MarketAuthorityV4::Current,
        },
        stations,
        markets,
        broker: BrokerV4 {
            revision: 1,
            venue_account_id: text(128),
            sleeve_id: text(128),
            ..Default::default()
        },
        delivered_at_monotonic_ns: 1,
        hard_expires_at_monotonic_ns: 2,
        ..Default::default()
    };
    let encoded = encode_decision_context_v4(&context).expect("schema maximum fits V4 bound");
    assert_eq!(decode_decision_context_v4(&encoded).unwrap(), context);
    println!("{} {:x}", encoded.len(), Sha256::digest(&encoded));
}
