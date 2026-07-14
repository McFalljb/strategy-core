use std::str::FromStr;

use serde_json::json;
use strategy_core::{
    Action, ContractSide, EventDelivery, FeeType, ForecastHourly, FreshnessDomain,
    FreshnessDomainSummary, FreshnessSnapshot, FreshnessStatus, FreshnessSummary,
    ICAO_TO_CITY_CODES, MARKET_TYPE_PREFIX, ModelForecast, NewHigh, Observation, OracleModelScore,
    OracleScoresUpdated, OrderExecutionStyle, OrderIntent, OrderTimePolicy, OrderType, PriceUpdate,
    RuntimeCapabilities, STATION_TIMEZONES, StationForecast, StationOracleScores, StationWeather,
    StrategyEvent, TICKER_PREFIXES, TickerPrices, WeatherEvent, apply_fee_rounding,
    calculate_fill_fee, calculate_trade_fee, city_codes_for_market_type,
    primary_city_code_for_market_type, primary_city_code_for_series, station_from_event_ticker,
    ticker_prefixes_for_station,
};

#[test]
fn broker_enums_serialize_to_python_literal_values() {
    assert_eq!(serde_json::to_string(&Action::Buy).unwrap(), r#""buy""#);
    assert_eq!(serde_json::to_string(&Action::Sell).unwrap(), r#""sell""#);
    assert_eq!(
        serde_json::to_string(&ContractSide::Yes).unwrap(),
        r#""yes""#
    );
}

#[test]
fn order_intent_carries_optional_expiry_duration() {
    let intent = OrderIntent {
        ticker: "KXHIGHMIA-26APR08-B70.5".to_owned(),
        action: Action::Buy,
        contract_side: ContractSide::Yes,
        order_type: OrderType::Limit,
        quantity: 3,
        limit_price: Some(0.61),
        max_price: None,
        max_cost: None,
        execution_style: Some(OrderExecutionStyle::RestingLimit),
        time_policy: Some(OrderTimePolicy::GoodTillCanceled),
        expires_after_ms: Some(30_000),
        reduce_only: false,
        post_only: false,
        signal_type: Some("demo".to_owned()),
        signal_metadata: None,
        client_order_id: Some("client-1".to_owned()),
    };

    let encoded = serde_json::to_value(&intent).unwrap();
    assert_eq!(encoded["expires_after_ms"], 30_000);
    assert_eq!(encoded["execution_style"], "resting_limit");
}

#[test]
fn runtime_capabilities_default_matches_python_dataclass_defaults() {
    let capabilities = RuntimeCapabilities::default();

    assert!(!capabilities.supports_http);
    assert!(capabilities.supports_data_queries);
    assert!(!capabilities.supports_one_shot_timers);
    assert!(!capabilities.supports_recurring_timers);
    assert!(!capabilities.supports_native_kernels);
    assert!(!capabilities.queue_is_durable);
    assert!(!capabilities.replay_controls_event_progression);
    assert_eq!(capabilities.event_delivery, EventDelivery::Wake);
}

#[test]
fn station_mappings_match_python_contract_examples() {
    assert_eq!(MARKET_TYPE_PREFIX["high"], "KXHIGH");
    assert_eq!(MARKET_TYPE_PREFIX["low"], "KXLOWT");
    assert!(TICKER_PREFIXES.contains(&"KXHIGH"));
    assert!(TICKER_PREFIXES.contains(&"KXLOWT"));

    assert_eq!(
        ticker_prefixes_for_station("KMIA", "high").unwrap(),
        vec!["KXHIGHMIA", "KXHIGHMI"]
    );
    assert_eq!(
        ticker_prefixes_for_station("KMIA", "low").unwrap(),
        vec!["KXLOWTMIA", "KXLOWTMI"]
    );
    assert_eq!(
        ticker_prefixes_for_station("KAUS", "low").unwrap(),
        vec!["KXLOWTAUS", "KXLOWTAU"]
    );
    assert_eq!(
        ticker_prefixes_for_station("KMDW", "high").unwrap(),
        vec!["KXHIGHCHI", "KXHIGHMDW", "KXHIGHMW"]
    );
    assert_eq!(
        ticker_prefixes_for_station("KDFW", "low").unwrap(),
        vec!["KXLOWTDAL", "KXLOWTDFW"]
    );

    assert_eq!(primary_city_code_for_series("KNYC"), "NY");
    assert_eq!(primary_city_code_for_series("KMIA"), "MIA");
    assert_eq!(primary_city_code_for_series("KMDW"), "CHI");
    assert_eq!(primary_city_code_for_series("KDFW"), "TDAL");
    assert_eq!(primary_city_code_for_market_type("KMIA", "low"), "MIA");
    assert_eq!(
        city_codes_for_market_type("UNKNOWN", "high"),
        vec!["UNKNOWN"]
    );
    assert_eq!(station_from_event_ticker("KXHIGHMI-260403"), Some("KMIA"));
    assert_eq!(station_from_event_ticker("KXLOWTDAL-260403"), Some("KDFW"));
    assert_eq!(station_from_event_ticker("OTHER-260403"), None);

    assert!(ticker_prefixes_for_station("KMIA", "invalid").is_err());
    for icao in ICAO_TO_CITY_CODES.keys() {
        assert!(STATION_TIMEZONES.contains_key(icao));
    }
}

#[test]
fn general_taker_fee_matches_python_fee_schedule_examples() {
    assert_eq!(
        calculate_fill_fee(
            Action::Buy,
            0.30,
            1,
            strategy_core::LiquidityRole::Taker,
            0.0,
            None,
            None,
        )
        .unwrap()
        .net_fee,
        0.02
    );
    assert_eq!(
        calculate_fill_fee(
            Action::Buy,
            0.50,
            100,
            strategy_core::LiquidityRole::Taker,
            0.0,
            None,
            None,
        )
        .unwrap()
        .net_fee,
        1.75
    );
    assert_eq!(
        calculate_fill_fee(
            Action::Buy,
            0.90,
            1,
            strategy_core::LiquidityRole::Taker,
            0.0,
            None,
            None,
        )
        .unwrap()
        .net_fee,
        0.01
    );
}

#[test]
fn fee_rounding_accumulator_applies_rebate_once_whole_cent_is_reached() {
    let first = apply_fee_rounding(-0.055, 0.0085, 0.0).unwrap();
    let second = apply_fee_rounding(-0.055, 0.0085, first.fee_accumulator).unwrap();
    let third = apply_fee_rounding(-0.055, 0.0085, second.fee_accumulator).unwrap();

    assert_eq!(first.rounding_fee, 0.0065);
    assert_eq!(first.rebate, 0.0);
    assert_eq!(first.net_fee, 0.015);
    assert_eq!(first.posted_balance_change, -0.07);

    assert_eq!(second.rounding_fee, 0.0065);
    assert_eq!(second.rebate, 0.01);
    assert_eq!(second.net_fee, 0.005);
    assert_eq!(second.posted_balance_change, -0.06);

    assert_eq!(third.rounding_fee, 0.0065);
    assert_eq!(third.rebate, 0.0);
    assert_eq!(third.net_fee, 0.015);
    assert_eq!(third.posted_balance_change, -0.07);
}

#[test]
fn signed_fee_inputs_round_toward_positive_infinity() {
    assert_eq!(
        calculate_trade_fee(0.25, -1, strategy_core::LiquidityRole::Taker, None, None,).unwrap(),
        -0.0131
    );
    assert_eq!(
        calculate_trade_fee(
            0.25,
            1,
            strategy_core::LiquidityRole::Taker,
            None,
            Some(-1.0),
        )
        .unwrap(),
        -0.0131
    );
    assert_eq!(
        apply_fee_rounding(-0.055, -0.00851, -0.0065)
            .unwrap()
            .trade_fee,
        -0.0085
    );
}

#[test]
fn fee_multiplier_and_flat_schedule_match_python_examples() {
    let default_fee = calculate_fill_fee(
        Action::Buy,
        0.30,
        10,
        strategy_core::LiquidityRole::Taker,
        0.0,
        Some(FeeType::Quadratic),
        Some(1.0),
    )
    .unwrap();
    let doubled_fee = calculate_fill_fee(
        Action::Buy,
        0.30,
        10,
        strategy_core::LiquidityRole::Taker,
        0.0,
        Some(FeeType::Quadratic),
        Some(2.0),
    )
    .unwrap();

    assert_eq!(doubled_fee.trade_fee, default_fee.trade_fee * 2.0);
    assert!(doubled_fee.net_fee > default_fee.net_fee);

    assert_eq!(
        calculate_fill_fee(
            Action::Buy,
            0.30,
            100,
            strategy_core::LiquidityRole::Taker,
            0.0,
            Some(FeeType::Flat),
            Some(1.0),
        )
        .unwrap()
        .net_fee,
        0.74
    );
    assert_eq!(
        calculate_fill_fee(
            Action::Buy,
            0.50,
            100,
            strategy_core::LiquidityRole::Taker,
            0.0,
            Some(FeeType::Flat),
            Some(1.0),
        )
        .unwrap()
        .net_fee,
        0.88
    );
}

#[test]
fn maker_fee_exemptions_and_unknown_fee_type_match_python_behavior() {
    let default_maker = calculate_fill_fee(
        Action::Buy,
        0.25,
        10,
        strategy_core::LiquidityRole::Maker,
        0.0,
        None,
        None,
    )
    .unwrap();
    let explicit_maker = calculate_fill_fee(
        Action::Buy,
        0.25,
        10,
        strategy_core::LiquidityRole::Maker,
        0.0,
        Some(FeeType::QuadraticWithMakerFees),
        None,
    )
    .unwrap();

    assert_eq!(default_maker.trade_fee, 0.0329);
    assert_eq!(default_maker.rounding_fee, 0.0071);
    assert_eq!(default_maker.net_fee, 0.04);
    assert_eq!(default_maker.posted_balance_change, -2.54);
    assert_eq!(default_maker.fee_accumulator, 0.0071);
    assert_eq!(explicit_maker, default_maker);

    assert_eq!(
        calculate_fill_fee(
            Action::Buy,
            0.25,
            10,
            strategy_core::LiquidityRole::Maker,
            0.0,
            Some(FeeType::Quadratic),
            None,
        )
        .unwrap()
        .net_fee,
        0.0
    );
    assert_eq!(
        calculate_fill_fee(
            Action::Buy,
            0.25,
            10,
            strategy_core::LiquidityRole::Maker,
            0.0,
            Some(FeeType::Flat),
            Some(1.0),
        )
        .unwrap()
        .net_fee,
        0.0
    );
    assert!(FeeType::from_str("unknown").is_err());
}

#[test]
fn strategy_event_union_deserializes_representative_python_payloads() {
    let observation: StrategyEvent = serde_json::from_value(json!({
        "type": "observation",
        "station_id": "KMIA",
        "temperature_f": 81.2,
        "temperature_day_mode": "nws_climate_day",
        "temperature_day_date": "2026-05-22",
        "wu_day_mode": "calendar_day",
        "wu_day_date": "2026-05-22"
    }))
    .unwrap();
    assert!(matches!(observation, StrategyEvent::Observation(_)));

    let price_update: StrategyEvent = serde_json::from_value(json!({
        "type": "price_update",
        "station_id": "KMIA",
        "source": "kalshi",
        "markets": [{"ticker": "KXHIGHMIA-26APR08-B70.5", "yes_price": 0.42}]
    }))
    .unwrap();
    let StrategyEvent::PriceUpdate(price_update) = price_update else {
        panic!("expected price update");
    };
    assert_eq!(price_update.markets[0].ticker, "KXHIGHMIA-26APR08-B70.5");
    assert_eq!(price_update.markets[0].yes_price, 0.42);

    let forecast_updated: StrategyEvent = serde_json::from_value(json!({
        "type": "forecast_updated",
        "station_id": "KMIA",
        "model_id": "ncep_hrrr_conus",
        "version": "2026-04-08T12:00:00Z"
    }))
    .unwrap();
    assert!(matches!(
        forecast_updated,
        StrategyEvent::ForecastUpdated(_)
    ));

    let forecast_versions: StrategyEvent = serde_json::from_value(json!({
        "type": "forecast_versions",
        "station_id": "KMIA",
        "versions": {"ncep_hrrr_conus": "2026-04-08T12:00:00Z"}
    }))
    .unwrap();
    assert!(matches!(
        forecast_versions,
        StrategyEvent::ForecastVersions(_)
    ));

    let oracle_scores: StrategyEvent = serde_json::from_value(json!({
        "type": "oracle_scores_updated",
        "station_id": "KMIA",
        "modes": ["overall", "day_of"],
        "day_of": {
            "station_id": "KMIA",
            "score_mode": "day_of",
            "scores": [{"model_id": "ncep_hrrr_conus"}]
        }
    }))
    .unwrap();
    assert!(matches!(
        oracle_scores,
        StrategyEvent::OracleScoresUpdated(_)
    ));

    let station_report: StrategyEvent = serde_json::from_value(json!({
        "type": "station_report",
        "station_id": "KMIA",
        "report_id": "report-1"
    }))
    .unwrap();
    assert!(matches!(station_report, StrategyEvent::StationReport(_)));

    let weather_event: StrategyEvent = serde_json::from_value(json!({
        "type": "weather_event",
        "station_id": "KMIA",
        "id": "storm-1",
        "event_type": "thunderstorm"
    }))
    .unwrap();
    let StrategyEvent::WeatherEvent(weather_event) = weather_event else {
        panic!("expected weather event");
    };
    assert_eq!(weather_event.event_type_name, "thunderstorm");

    let new_high: StrategyEvent = serde_json::from_value(json!({
        "type": "new_high",
        "station_id": "KMIA",
        "value_f": 92.1,
        "value_c": 33.4,
        "temperature_day_mode": "calendar_day",
        "temperature_day_date": "2026-05-22",
        "persistence_status": "uncommitted",
        "event_key": "obs-1",
        "producer_sequence": 42
    }))
    .unwrap();
    assert!(matches!(new_high, StrategyEvent::NewHigh(_)));

    let new_low: StrategyEvent = serde_json::from_value(json!({
        "type": "new_low",
        "station_id": "KMIA",
        "value_f": 61.3,
        "value_c": 16.3
    }))
    .unwrap();
    assert!(matches!(new_low, StrategyEvent::NewLow(_)));

    let timer_wake: StrategyEvent = serde_json::from_value(json!({
        "type": "timer_wake",
        "scheduled_for": "2026-04-08T12:30:00Z"
    }))
    .unwrap();
    assert!(matches!(timer_wake, StrategyEvent::TimerWake(_)));

    let shutdown: StrategyEvent = serde_json::from_value(json!({
        "type": "shutdown",
        "reason": "done"
    }))
    .unwrap();
    assert!(matches!(shutdown, StrategyEvent::ShutdownEvent(_)));
}

#[test]
fn event_models_reject_invalid_or_blank_required_fields() {
    assert!(
        serde_json::from_value::<StrategyEvent>(json!({
            "type": "price_update"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<StrategyEvent>(json!({
            "type": "price_update",
            "station_id": "KMIA",
            "source": ""
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<StrategyEvent>(json!({
            "type": "observation",
            "station_id": "KMIA",
            "temperature_day_mode": "invalid_mode"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<StrategyEvent>(json!({
            "type": "new_high",
            "station_id": "KMIA"
        }))
        .is_err()
    );
}

#[test]
fn event_structs_serialize_with_python_field_names() {
    let observation: Observation = serde_json::from_value(json!({
        "type": "observation",
        "station_id": "KMIA",
        "temperature_f": 80.0
    }))
    .unwrap();
    let encoded = serde_json::to_value(observation).unwrap();
    assert_eq!(encoded["type"], "observation");
    assert_eq!(encoded["station_id"], "KMIA");

    let price_update: PriceUpdate = serde_json::from_value(json!({
        "type": "price_update",
        "station_id": "KMIA",
        "source": "kalshi",
        "markets": [{"ticker": "KXHIGHMIA-26APR08-B70.5"}]
    }))
    .unwrap();
    let encoded = serde_json::to_value(price_update).unwrap();
    assert_eq!(encoded["type"], "price_update");
    assert_eq!(encoded["markets"][0]["ticker"], "KXHIGHMIA-26APR08-B70.5");

    let weather_event: WeatherEvent = serde_json::from_value(json!({
        "type": "weather_event",
        "station_id": "KMIA",
        "id": "storm-1",
        "event_type": "thunderstorm"
    }))
    .unwrap();
    let encoded = serde_json::to_value(weather_event).unwrap();
    assert_eq!(encoded["type"], "weather_event");
    assert_eq!(encoded["event_type"], "thunderstorm");

    let new_high: NewHigh = serde_json::from_value(json!({
        "type": "new_high",
        "station_id": "KMIA",
        "value_f": 92.1,
        "value_c": 33.4
    }))
    .unwrap();
    let encoded = serde_json::to_value(new_high).unwrap();
    assert_eq!(encoded["type"], "new_high");
    assert_eq!(encoded["is_from_report"], false);

    let oracle: OracleScoresUpdated = serde_json::from_value(json!({
        "type": "oracle_scores_updated",
        "station_id": "KMIA"
    }))
    .unwrap();
    let encoded = serde_json::to_value(oracle).unwrap();
    assert_eq!(encoded["type"], "oracle_scores_updated");
    assert_eq!(encoded["modes"], json!([]));
}

#[test]
fn state_freshness_helpers_match_python_properties() {
    let stale = FreshnessSnapshot {
        domain: FreshnessDomain::Price,
        key: "KXHIGHMIA-26APR08-B70.5".to_string(),
        status: FreshnessStatus::Stale,
        source: Some("kalshi".to_string()),
        updated_at: None,
        observed_at: None,
        stale_after_seconds: Some(30.0),
        age_seconds: Some(45.0),
        invalidation_reason: None,
        detail: None,
    };
    let missing = FreshnessSnapshot {
        status: FreshnessStatus::Missing,
        ..stale.clone()
    };

    assert!(stale.is_stale());
    assert!(!stale.is_missing());
    assert!(!missing.is_stale());
    assert!(missing.is_missing());

    let summary = FreshnessSummary {
        as_of: "2026-05-22T12:00:00Z".parse().unwrap(),
        domains: vec![
            FreshnessDomainSummary {
                domain: FreshnessDomain::Price,
                tracked_count: 3,
                fresh_count: 2,
                stale_count: 1,
                stalest_age_seconds: Some(45.0),
            },
            FreshnessDomainSummary {
                domain: FreshnessDomain::Weather,
                tracked_count: 2,
                fresh_count: 1,
                stale_count: 1,
                stalest_age_seconds: Some(60.0),
            },
        ],
    };
    assert_eq!(summary.tracked_count(), 5);
    assert_eq!(summary.stale_count(), 2);
    assert_eq!(
        serde_json::to_value(FreshnessDomain::Oracle).unwrap(),
        "oracle"
    );
    assert_eq!(
        serde_json::to_value(FreshnessStatus::Fresh).unwrap(),
        "fresh"
    );
}

#[test]
fn state_value_objects_preserve_python_json_shape_and_defaults() {
    let weather: StationWeather = serde_json::from_value(json!({})).unwrap();
    assert_eq!(weather.current_temp, None);
    assert!(!weather.preliminary);

    let prices: TickerPrices = serde_json::from_value(json!({
        "ticker": "KXHIGHMIA-26APR08-B70.5",
        "yes_bid_levels": [[0.41, 12]],
        "yes_ask_levels": [[0.42, 8]],
        "yes_price": 0.42
    }))
    .unwrap();
    assert_eq!(prices.ticker, "KXHIGHMIA-26APR08-B70.5");
    assert_eq!(prices.yes_bid_levels, vec![(0.41, 12)]);
    assert_eq!(prices.yes_ask_levels, vec![(0.42, 8)]);
    assert_eq!(prices.no_price, 0.0);

    let encoded = serde_json::to_value(prices).unwrap();
    assert_eq!(encoded["yes_bid_levels"], json!([[0.41, 12]]));
}

#[test]
fn forecast_and_oracle_state_models_match_python_field_names() {
    let forecast = StationForecast {
        model_forecasts: [(
            "ncep_hrrr_conus".to_string(),
            ModelForecast {
                model_id: "ncep_hrrr_conus".to_string(),
                value: 91.2,
                version: "2026-05-22T12:00:00Z".to_string(),
                updated_at: None,
                run_issued_at: None,
                hourly: vec![ForecastHourly {
                    time: "2026-05-22T18:00:00Z".to_string(),
                    temperature_2m_f: Some(90.0),
                    ..ForecastHourly::default()
                }],
            },
        )]
        .into_iter()
        .collect(),
        updated_at: None,
    };
    let encoded = serde_json::to_value(forecast).unwrap();
    assert_eq!(
        encoded["model_forecasts"]["ncep_hrrr_conus"]["hourly"][0]["temperature_2m_f"],
        90.0
    );

    let oracle = StationOracleScores {
        station_id: "KMIA".to_string(),
        scores: vec![OracleModelScore {
            model_id: "ncep_hrrr_conus".to_string(),
            model_name: String::new(),
            combined_mae: Some(1.1),
            high_mae: None,
            low_mae: None,
            high_bias: None,
            low_bias: None,
            day_count: Some(30),
            is_public: Some(true),
        }],
        rank_by: "combined_mae".to_string(),
        score_mode: "overall".to_string(),
        days_requested: "30".to_string(),
        range_start: "2026-04-22".to_string(),
        range_end: "2026-05-22".to_string(),
        updated_at: None,
    };
    let encoded = serde_json::to_value(oracle).unwrap();
    assert_eq!(encoded["station_id"], "KMIA");
    assert_eq!(encoded["scores"][0]["model_id"], "ncep_hrrr_conus");
    assert_eq!(encoded["scores"][0]["is_public"], true);
}
