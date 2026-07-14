use std::{
    collections::BTreeMap,
    future::{Future, ready},
};

use chrono::{NaiveDate, TimeZone, Utc};
use serde_json::{Value, json};
use strategy_core::{
    Action, Broker, BrokerOrderUpdate, BrokerUpdateStatus, ContractSide, EngineClock,
    ForecastRunLookup, FreshnessDomain, FreshnessDomainSummary, FreshnessSnapshot, FreshnessStatus,
    FreshnessSummary, HttpClient, HttpMethod, HttpRequest, HttpResponse, LatestObservationQuery,
    MarketStateView, OracleModelScore, OracleScoreDays, OrderExecutionStyle, OrderIntent,
    OrderResult, OrderStatus, OrderTimePolicy, OrderType, PendingOrder, Position, ReportType,
    RuntimeCapabilities, RuntimeMode, SIGNAL_DSM_REACTION, SIGNAL_METAR_6HR_LOW,
    SIGNAL_METAR_6HR_NEW_LOW, StationOracleScores, StrategyContext, StrategyDataClient,
    StrategyEvent, StrategyLogger, StrategyRuntime, StrategyScope, Telemetry, TelemetryField,
    TickerPrices, TimerHandle, WorkHandle, climate_day_date, climate_day_end,
    climate_day_has_ended, parse_climate_date, station_timezone,
};

#[test]
fn query_objects_match_python_defaults_and_json_names() {
    let oracle = strategy_core::OracleScoresQuery::default();
    assert_eq!(oracle.days, "7");
    assert_eq!(oracle.mode, "day_ahead");
    assert_eq!(oracle.rank_by, "high");
    assert!(!oracle.refresh);

    let latest: LatestObservationQuery = serde_json::from_value(json!({
        "day_mode": "nws_climate_day",
        "refresh": true
    }))
    .unwrap();
    assert_eq!(latest.day_mode, Some("nws_climate_day".to_string()));
    assert!(latest.refresh);

    let reports: strategy_core::ReportsQuery = serde_json::from_value(json!({
        "report_type": "cli",
        "date": "2026-04-08",
        "refresh": true
    }))
    .unwrap();
    assert_eq!(reports.report_type, Some(ReportType::Cli));
    assert!(reports.refresh);
}

#[test]
fn http_models_accept_non_object_json_values() {
    let request: HttpRequest = serde_json::from_value(json!({
        "method": "POST",
        "url": "https://example.com/list",
        "json_body": [{"station": "KMIA"}],
        "params": {"station": "KMIA", "refresh": true, "limit": 5}
    }))
    .unwrap();
    assert_eq!(request.method, HttpMethod::Post);
    assert_eq!(request.json_body, Some(json!([{"station": "KMIA"}])));
    assert_eq!(request.params["limit"], json!(5));

    let response = HttpResponse {
        status_code: 200,
        headers: BTreeMap::new(),
        text: None,
        json_body: Some(json!([1, 2, 3])),
    };
    assert_eq!(
        serde_json::to_value(response).unwrap()["json_body"],
        json!([1, 2, 3])
    );
}

#[test]
fn signal_constants_match_python_contract() {
    assert_eq!(SIGNAL_DSM_REACTION, "dsm_reaction");
    assert_eq!(SIGNAL_METAR_6HR_LOW, "metar_6hr_low");
    assert_eq!(SIGNAL_METAR_6HR_NEW_LOW, "metar_6hr_new_low");
}

#[test]
fn climate_day_helpers_match_python_contract_examples() {
    assert_eq!(
        station_timezone(Some("KMIA"), None).unwrap().to_string(),
        "America/New_York"
    );
    assert_eq!(
        station_timezone(Some("KPHX"), None).unwrap().to_string(),
        "America/Phoenix"
    );

    let before_midnight_utc = Utc.with_ymd_and_hms(2026, 4, 2, 4, 55, 0).unwrap();
    let after_midnight_utc = Utc.with_ymd_and_hms(2026, 4, 2, 5, 5, 0).unwrap();
    assert_eq!(
        climate_day_date(Some("KMIA"), before_midnight_utc, None).unwrap(),
        NaiveDate::from_ymd_opt(2026, 4, 1).unwrap()
    );
    assert_eq!(
        climate_day_date(Some("KMIA"), after_midnight_utc, None).unwrap(),
        NaiveDate::from_ymd_opt(2026, 4, 2).unwrap()
    );

    assert_eq!(
        parse_climate_date(Some("2026-04-03")),
        Some(NaiveDate::from_ymd_opt(2026, 4, 3).unwrap())
    );
    assert_eq!(
        parse_climate_date(Some("20260403")),
        Some(NaiveDate::from_ymd_opt(2026, 4, 3).unwrap())
    );
    assert_eq!(
        parse_climate_date(Some("260403")),
        Some(NaiveDate::from_ymd_opt(2026, 4, 3).unwrap())
    );
    assert_eq!(parse_climate_date(Some("bad")), None);
    assert_eq!(parse_climate_date(Some("2026-99-99")), None);

    let event_date = NaiveDate::from_ymd_opt(2026, 7, 4).unwrap();
    assert_eq!(
        climate_day_end(Some("KMIA"), event_date, None).unwrap(),
        Utc.with_ymd_and_hms(2026, 7, 5, 5, 0, 0).unwrap()
    );
    assert!(
        !climate_day_has_ended(
            Some("KMIA"),
            event_date,
            Utc.with_ymd_and_hms(2026, 7, 5, 4, 59, 0).unwrap(),
            None,
        )
        .unwrap()
    );
    assert!(
        climate_day_has_ended(
            Some("KMIA"),
            event_date,
            Utc.with_ymd_and_hms(2026, 7, 5, 5, 0, 0).unwrap(),
            None,
        )
        .unwrap()
    );
}

#[test]
fn strategy_context_traits_are_implementable() {
    let mut ctx = FakeContext::default();

    assert_eq!(ctx.runtime().mode(), RuntimeMode::Replay);
    assert_eq!(
        ctx.capabilities().event_delivery,
        strategy_core::EventDelivery::Wake
    );
    assert_eq!(ctx.config()["strategy"], json!("demo"));
    assert!(ctx.state().get_prices("missing").is_none());
    assert_eq!(ctx.telemetry().logger().info_count, 0);

    let order = ctx.broker().place_order(
        "KXHIGHMIA-26APR08-B70.5",
        Action::Buy,
        ContractSide::Yes,
        OrderType::Limit,
        1,
        Some(0.42),
        Some("dsm_reaction"),
        Some("{\"source\":\"test\"}"),
        Some("client-1"),
    );
    assert_eq!(order.unwrap().status, OrderStatus::Filled);
    let pending = ctx.broker().get_pending_orders();
    assert_eq!(pending[0].signal_type.as_deref(), Some("dsm_reaction"));
    assert_eq!(
        pending[0].signal_metadata.as_deref(),
        Some("{\"source\":\"test\"}")
    );
    assert_eq!(pending[0].client_order_id.as_deref(), Some("client-1"));
    assert!(
        ctx.broker()
            .get_positions()
            .contains_key("KXHIGHMIA-26APR08-B70.5:yes")
    );
}

#[test]
fn market_state_oracle_selectors_match_python_contract() {
    let scores = StationOracleScores {
        station_id: "KMIA".to_string(),
        scores: vec![OracleModelScore {
            model_id: "ncep_hrrr_conus".to_string(),
            model_name: String::new(),
            combined_mae: None,
            high_mae: Some(1.2),
            low_mae: None,
            high_bias: None,
            low_bias: None,
            day_count: Some(7),
            is_public: Some(true),
        }],
        rank_by: "high".to_string(),
        score_mode: "day_of".to_string(),
        days_requested: "7".to_string(),
        range_start: String::new(),
        range_end: String::new(),
        updated_at: None,
    };
    let state = FakeState {
        oracle_scores: [("KMIA".to_string(), scores)].into_iter().collect(),
    };

    assert!(state.get_oracle_scores("KMIA").is_some());
    assert!(state.get_oracle_scores("KORD").is_none());

    for (case_id, station, days, mode, rank_by, expected) in [
        ("omitted", "KMIA", None, None, None, true),
        (
            "string days",
            "KMIA",
            Some(OracleScoreDays::from("7")),
            Some("day_of"),
            Some("high"),
            true,
        ),
        (
            "integer days",
            "KMIA",
            Some(OracleScoreDays::from(7_i64)),
            Some("day_of"),
            Some("high"),
            true,
        ),
        (
            "mismatched days",
            "KMIA",
            Some(OracleScoreDays::from("30")),
            Some("day_of"),
            Some("high"),
            false,
        ),
        (
            "mismatched mode",
            "KMIA",
            Some(OracleScoreDays::from("7")),
            Some("day_ahead"),
            Some("high"),
            false,
        ),
        (
            "mismatched rank",
            "KMIA",
            Some(OracleScoreDays::from("7")),
            Some("day_of"),
            Some("combined"),
            false,
        ),
        (
            "missing station",
            "KORD",
            Some(OracleScoreDays::from(7_i64)),
            Some("day_of"),
            Some("high"),
            false,
        ),
    ] {
        assert_eq!(
            state
                .get_oracle_scores_matching(station, days, mode, rank_by)
                .is_some(),
            expected,
            "{case_id}",
        );
    }
}

#[test]
fn broker_place_order_with_intent_preserves_legacy_trait_method() {
    let mut broker = FakeBroker::default();
    let result = broker.place_order_with_intent(OrderIntent {
        ticker: "KXHIGHMIA-26APR08-B70.5".to_string(),
        action: Action::Buy,
        contract_side: ContractSide::Yes,
        order_type: OrderType::Limit,
        quantity: 1,
        limit_price: Some(0.42),
        max_price: Some(0.43),
        max_cost: Some(0.43),
        execution_style: Some(OrderExecutionStyle::RestingLimit),
        time_policy: Some(OrderTimePolicy::GoodTillCanceled),
        reduce_only: false,
        post_only: false,
        signal_type: Some("dsm_reaction".to_string()),
        signal_metadata: Some("{\"source\":\"test\"}".to_string()),
        client_order_id: Some("client-1".to_string()),
        expires_after_ms: Some(30_000),
    });

    assert_eq!(result.unwrap().status, OrderStatus::Filled);
    assert_eq!(
        broker.pending[0].client_order_id.as_deref(),
        Some("client-1")
    );
}

#[test]
fn broker_intent_and_update_models_preserve_explicit_execution_semantics() {
    let intent = OrderIntent {
        ticker: "KXHIGHMIA-26APR08-B70.5".to_string(),
        action: Action::Buy,
        contract_side: ContractSide::Yes,
        order_type: OrderType::Market,
        quantity: 5,
        limit_price: None,
        max_price: Some(0.61),
        max_cost: Some(305.0),
        execution_style: Some(OrderExecutionStyle::Sweep),
        time_policy: Some(OrderTimePolicy::ImmediateOrCancel),
        expires_after_ms: None,
        reduce_only: false,
        post_only: false,
        signal_type: Some("demo".to_string()),
        signal_metadata: None,
        client_order_id: Some("client-1".to_string()),
    };

    let encoded = serde_json::to_value(&intent).unwrap();
    assert_eq!(encoded["execution_style"], json!("sweep"));
    assert_eq!(encoded["time_policy"], json!("immediate_or_cancel"));

    let update = BrokerOrderUpdate {
        order_id: "order-1".to_string(),
        sleeve_id: "demo:KMIA".to_string(),
        ticker: intent.ticker,
        status: BrokerUpdateStatus::PartiallyFilled,
        action: intent.action,
        contract_side: intent.contract_side,
        requested_quantity: 5,
        filled_quantity: 3,
        remaining_quantity: 2,
        fill_price: 0.0,
        average_fill_price: 0.59,
        fee_cost: 0.0,
        reason: String::new(),
        client_order_id: intent.client_order_id,
        provider_order_id: Some("provider-order-1".to_string()),
        provider_sequence: Some("sid=13:seq=42".to_string()),
        updated_at: "2026-06-17T12:00:00Z".to_string(),
        expires_at: Some("2026-06-17T12:00:30Z".to_string()),
    };

    let encoded_update = serde_json::to_value(update).unwrap();
    assert_eq!(encoded_update["status"], json!("partially_filled"));
    assert_eq!(encoded_update["provider_sequence"], json!("sid=13:seq=42"));
    assert_eq!(encoded_update["expires_at"], json!("2026-06-17T12:00:30Z"));
    assert_eq!(
        serde_json::to_value(BrokerUpdateStatus::Expired).unwrap(),
        json!("expired")
    );
    assert_eq!(
        serde_json::to_value(BrokerUpdateStatus::Closed).unwrap(),
        json!("closed")
    );
}

#[test]
fn pending_order_preserves_expiry_deadline() {
    let pending = PendingOrder {
        order_id: "order-1".to_string(),
        sleeve_id: "demo:KMIA".to_string(),
        ticker: "KXHIGHMIA-26APR08-B70.5".to_string(),
        action: Action::Buy,
        contract_side: ContractSide::Yes,
        limit_price: 0.61,
        requested_quantity: 3,
        filled_quantity: 0,
        reserved_global: 0.0,
        reserved_sleeve: 0.0,
        fee_type: String::new(),
        fee_multiplier: None,
        fee_accumulator: 0.0,
        signal_type: None,
        signal_metadata: None,
        created_at: String::new(),
        client_order_id: None,
        expires_at: Some("2026-06-17T12:00:30Z".to_string()),
    };

    let encoded = serde_json::to_value(pending).unwrap();
    assert_eq!(encoded["expires_at"], json!("2026-06-17T12:00:30Z"));
}

#[test]
fn minutetemp_models_preserve_python_defaults_and_day_bucketing_fields() {
    let station = strategy_core::StationInfo {
        station_id: "KMDW".to_string(),
        uses_nws_climate_day: Some(true),
        ..strategy_core::StationInfo::default()
    };
    assert_eq!(station.temperature_unit, "F");

    let payload = strategy_core::LatestObservationData {
        station: Some(station),
        temperature_day_mode: Some("nws_climate_day".to_string()),
        temperature_day_date: NaiveDate::from_ymd_opt(2026, 5, 22),
        wu_day_mode: Some("calendar_day".to_string()),
        wu_day_date: NaiveDate::from_ymd_opt(2026, 5, 22),
        ..strategy_core::LatestObservationData::default()
    };
    assert_eq!(
        payload.temperature_day_mode,
        Some("nws_climate_day".to_string())
    );
    assert_eq!(payload.wu_day_mode, Some("calendar_day".to_string()));
    assert_eq!(
        payload
            .station
            .as_ref()
            .and_then(|station| station.uses_nws_climate_day),
        Some(true)
    );

    let limits = strategy_core::EffectiveLimits::default();
    assert_eq!(limits.tier, "starter");

    let oracle = strategy_core::OracleScoreData::default();
    assert_eq!(oracle.score_mode, "overall");
    assert_eq!(oracle.rank_by, "combined");
}

#[test]
fn minutetemp_report_schedules_support_single_and_multiple_entries() {
    let schedule = strategy_core::ReportSchedule::Clock(strategy_core::ReportClockSchedule {
        hour: Some(1),
        ..strategy_core::ReportClockSchedule::default()
    });
    let payload = strategy_core::LatestReportsData {
        report_schedules: Some(
            [(
                "cli".to_string(),
                strategy_core::ReportScheduleEntry::Multiple(vec![schedule.clone()]),
            )]
            .into_iter()
            .collect(),
        ),
        ..strategy_core::LatestReportsData::default()
    };

    let encoded = serde_json::to_value(payload).unwrap();
    assert_eq!(encoded["report_schedules"]["cli"][0]["hour"], 1);

    let decoded: strategy_core::LatestReportsData = serde_json::from_value(json!({
        "report_schedules": {
            "cli": [{"basis": "utc", "hour": 1}, {"basis": "utc", "hour": 13}]
        }
    }))
    .unwrap();
    let schedules = decoded.report_schedules.unwrap();
    let strategy_core::ReportScheduleEntry::Multiple(items) = &schedules["cli"] else {
        panic!("expected multiple schedule entry");
    };
    assert_eq!(items.len(), 2);

    let interval: strategy_core::LatestReportsData = serde_json::from_value(json!({
        "report_schedules": {
            "metar_6hr": {"interval_minutes": 360, "utc_minute": 55}
        }
    }))
    .unwrap();
    let schedules = interval.report_schedules.unwrap();
    let strategy_core::ReportScheduleEntry::Single(strategy_core::ReportSchedule::Interval(item)) =
        &schedules["metar_6hr"]
    else {
        panic!("expected interval schedule");
    };
    assert_eq!(item.interval_minutes, 360);

    let multi_hour: strategy_core::LatestReportsData = serde_json::from_value(json!({
        "report_schedules": {
            "dsm": [{"utc_hours": [1, 13], "utc_minute": 0}]
        }
    }))
    .unwrap();
    let schedules = multi_hour.report_schedules.unwrap();
    let strategy_core::ReportScheduleEntry::Multiple(items) = &schedules["dsm"] else {
        panic!("expected multiple schedule entry");
    };
    let strategy_core::ReportSchedule::MultiHour(item) = &items[0] else {
        panic!("expected multi-hour schedule");
    };
    assert_eq!(item.utc_hours, vec![1, 13]);
}

#[test]
fn kalshi_models_preserve_nested_orderbook_and_custom_strike_shapes() {
    let response = strategy_core::KalshiGetOrderbookResponse {
        orderbook_fp: strategy_core::KalshiOrderbook {
            yes_dollars: vec![strategy_core::KalshiOrderbookLevel {
                price_dollars: "0.1500".to_string(),
                count_fp: "100.00".to_string(),
            }],
            no_dollars: vec![strategy_core::KalshiOrderbookLevel {
                price_dollars: "0.8500".to_string(),
                count_fp: "25.00".to_string(),
            }],
        },
    };
    let encoded = serde_json::to_value(response).unwrap();
    assert_eq!(
        encoded["orderbook_fp"]["yes_dollars"][0]["price_dollars"],
        "0.1500"
    );
    assert_eq!(
        encoded["orderbook_fp"]["no_dollars"][0]["count_fp"],
        "25.00"
    );

    let multi = strategy_core::KalshiGetOrderbooksResponse {
        orderbooks: vec![strategy_core::KalshiMarketOrderbook {
            ticker: "FED-24DEC-T3.00".to_string(),
            orderbook_fp: strategy_core::KalshiOrderbook::default(),
        }],
    };
    assert_eq!(
        serde_json::to_value(multi).unwrap()["orderbooks"][0]["ticker"],
        "FED-24DEC-T3.00"
    );

    let market: strategy_core::KalshiMarket = serde_json::from_value(json!({
        "custom_strike": {"threshold": 53.5}
    }))
    .unwrap();
    assert_eq!(market.market_type, "binary");
    assert_eq!(market.status, "open");
    assert_eq!(market.custom_strike["threshold"], json!(53.5));

    let metadata: strategy_core::KalshiMarketLifecycleMetadata = serde_json::from_value(json!({
        "custom_strike": {"threshold": 54.0}
    }))
    .unwrap();
    assert_eq!(metadata.custom_strike["threshold"], json!(54.0));
}

#[test]
fn kalshi_order_and_ws_payloads_use_python_field_names() {
    let order = strategy_core::KalshiOrder {
        ticker: "FED-24DEC-T3.00".to_string(),
        ..strategy_core::KalshiOrder::default()
    };
    let encoded = serde_json::to_value(order).unwrap();
    assert_eq!(encoded["type"], "limit");
    assert_eq!(encoded["side"], "yes");
    assert_eq!(encoded["action"], "buy");
    assert_eq!(encoded["status"], "resting");

    let message: strategy_core::KalshiWsMessage = serde_json::from_value(json!({
        "sid": 2,
        "seq": 3,
        "market_ticker": "FED-24DEC-T3.00",
        "market_id": "market-123"
    }))
    .unwrap();
    let strategy_core::KalshiWsMessage::OrderbookSnapshot(snapshot) = message else {
        panic!("expected orderbook snapshot");
    };
    assert_eq!(snapshot.sid, 2);
    assert_eq!(snapshot.seq, 3);
    assert!(snapshot.yes_dollars_fp.is_empty());

    let delta: strategy_core::KalshiWsMessage = serde_json::from_value(json!({
        "sid": 2,
        "seq": 4,
        "market_ticker": "FED-24DEC-T3.00",
        "market_id": "market-123",
        "price_dollars": "0.4200",
        "delta_fp": "10.00",
        "side": "yes"
    }))
    .unwrap();
    let strategy_core::KalshiWsMessage::OrderbookDelta(delta) = delta else {
        panic!("expected orderbook delta");
    };
    assert_eq!(delta.delta_fp, "10.00");

    let fill: strategy_core::KalshiWsMessage = serde_json::from_value(json!({
        "sid": 7,
        "trade_id": "trade-1",
        "order_id": "order-1",
        "market_ticker": "FED-24DEC-T3.00",
        "is_taker": true,
        "side": "yes",
        "count_fp": "2.00",
        "fee_cost": "0.0100"
    }))
    .unwrap();
    let strategy_core::KalshiWsMessage::UserFill(fill) = fill else {
        panic!("expected user fill");
    };
    assert_eq!(fill.order_id, "order-1");
}

#[test]
fn native_kernel_runner_respects_capability_flag() {
    let result = strategy_core::NativeKernelResult::default();
    assert_eq!(result.status, strategy_core::NativeKernelStatus::Completed);
    assert_eq!(serde_json::to_value(result).unwrap()["status"], "completed");

    let mut unsupported = FakeContext::default();
    assert!(strategy_core::get_native_kernel_runner(&mut unsupported).is_none());

    let mut supported = FakeContext {
        capabilities: RuntimeCapabilities {
            supports_native_kernels: true,
            ..RuntimeCapabilities::default()
        },
        native_runner: Some(FakeNativeRunner::default()),
        ..FakeContext::default()
    };
    assert!(strategy_core::get_native_kernel_runner(&mut supported).is_some());
}

#[test]
fn run_native_or_fallback_calls_native_runner_when_available() {
    let mut ctx = FakeContext {
        capabilities: RuntimeCapabilities {
            supports_native_kernels: true,
            ..RuntimeCapabilities::default()
        },
        native_runner: Some(FakeNativeRunner::default()),
        ..FakeContext::default()
    };
    let mut kernel = FakeKernel;

    let result = block_on(strategy_core::run_native_or_fallback(
        &mut ctx,
        &mut kernel,
        Some(|| ready(())),
        false,
    ))
    .unwrap();

    assert_eq!(result.status, strategy_core::NativeKernelStatus::Completed);
    assert_eq!(ctx.native_runner.as_ref().unwrap().calls, 1);
}

#[test]
fn run_native_or_fallback_uses_fallback_when_native_is_unavailable() {
    let mut ctx = FakeContext::default();
    let mut kernel = FakeKernel;
    let mut fallback_calls = 0;

    let result = block_on(strategy_core::run_native_or_fallback(
        &mut ctx,
        &mut kernel,
        Some(|| {
            fallback_calls += 1;
            ready(())
        }),
        false,
    ))
    .unwrap();

    assert_eq!(
        result.status,
        strategy_core::NativeKernelStatus::FallbackCompleted
    );
    assert!(result.fallback_used);
    assert_eq!(fallback_calls, 1);
}

#[test]
fn run_native_or_fallback_distinguishes_unavailable_and_runner_errors() {
    let mut no_runner = FakeContext::default();
    let mut kernel = FakeKernel;

    let unavailable = block_on(strategy_core::run_native_or_fallback(
        &mut no_runner,
        &mut kernel,
        None::<fn() -> std::future::Ready<()>>,
        true,
    ))
    .unwrap_err();
    assert!(matches!(
        unavailable,
        strategy_core::NativeKernelRunError::Unavailable(_)
    ));

    let mut failing = FakeContext {
        capabilities: RuntimeCapabilities {
            supports_native_kernels: true,
            ..RuntimeCapabilities::default()
        },
        native_runner: Some(FakeNativeRunner {
            error: Some("runner failed".to_string()),
            ..FakeNativeRunner::default()
        }),
        ..FakeContext::default()
    };
    let runner_error = block_on(strategy_core::run_native_or_fallback(
        &mut failing,
        &mut kernel,
        Some(|| ready(())),
        false,
    ))
    .unwrap_err();
    assert_eq!(
        runner_error,
        strategy_core::NativeKernelRunError::Runner("runner failed".to_string())
    );
}

#[derive(Default)]
struct FakeState {
    oracle_scores: BTreeMap<String, StationOracleScores>,
}

impl MarketStateView for FakeState {
    fn get_weather(&self, _station: &str) -> Option<&strategy_core::StationWeather> {
        None
    }

    fn get_forecast(&self, _station: &str) -> Option<&strategy_core::StationForecast> {
        None
    }

    fn get_oracle_scores(&self, station: &str) -> Option<&strategy_core::StationOracleScores> {
        self.oracle_scores.get(station)
    }

    fn get_prices(&self, _ticker: &str) -> Option<&TickerPrices> {
        None
    }

    fn get_weather_freshness(&self, station: &str) -> FreshnessSnapshot {
        missing_freshness(FreshnessDomain::Weather, station)
    }

    fn get_forecast_freshness(&self, station: &str) -> FreshnessSnapshot {
        missing_freshness(FreshnessDomain::Forecast, station)
    }

    fn get_oracle_scores_freshness(&self, station: &str) -> FreshnessSnapshot {
        missing_freshness(FreshnessDomain::Oracle, station)
    }

    fn get_price_freshness(&self, ticker: &str) -> FreshnessSnapshot {
        missing_freshness(FreshnessDomain::Price, ticker)
    }

    fn freshness_summary(&self) -> FreshnessSummary {
        FreshnessSummary {
            as_of: Utc.with_ymd_and_hms(2026, 4, 8, 12, 0, 0).unwrap(),
            domains: vec![FreshnessDomainSummary {
                domain: FreshnessDomain::Price,
                tracked_count: 0,
                fresh_count: 0,
                stale_count: 0,
                stalest_age_seconds: None,
            }],
        }
    }
}

fn missing_freshness(domain: FreshnessDomain, key: &str) -> FreshnessSnapshot {
    FreshnessSnapshot {
        domain,
        key: key.to_string(),
        status: FreshnessStatus::Missing,
        source: None,
        updated_at: None,
        observed_at: None,
        stale_after_seconds: None,
        age_seconds: None,
        invalidation_reason: None,
        detail: None,
    }
}

#[derive(Default)]
struct FakeData;

impl StrategyDataClient for FakeData {
    type Error = String;

    fn fetch_limits(
        &self,
        _query: Option<strategy_core::LimitsQuery>,
        _refresh: bool,
    ) -> impl Future<Output = Result<strategy_core::EffectiveLimits, Self::Error>> + Send {
        ready(Ok(strategy_core::EffectiveLimits {
            tier: "demo".to_string(),
            ..strategy_core::EffectiveLimits::default()
        }))
    }

    fn fetch_forecast(
        &self,
        _query: Option<strategy_core::ForecastQuery>,
        _model_id: Option<&str>,
        _refresh: bool,
    ) -> impl Future<Output = Result<Option<strategy_core::StationForecastData>, Self::Error>> + Send
    {
        ready(Ok(None))
    }

    fn fetch_oracle_scores(
        &self,
        _query: Option<strategy_core::OracleScoresQuery>,
        _days: &str,
        _mode: &str,
        _rank_by: &str,
        _refresh: bool,
    ) -> impl Future<Output = Result<Option<strategy_core::OracleScoreData>, Self::Error>> + Send
    {
        ready(Ok(None))
    }

    fn fetch_forecast_runs(
        &self,
        _query: Option<strategy_core::ForecastRunsQuery>,
        _model_id: Option<&str>,
        _start: Option<strategy_core::DateLike>,
        _end: Option<strategy_core::DateLike>,
        _limit: Option<i64>,
        _cursor: Option<&str>,
        _refresh: bool,
    ) -> impl Future<Output = Result<strategy_core::ForecastRunsPage, Self::Error>> + Send {
        ready(Ok(strategy_core::ForecastRunsPage::default()))
    }

    fn fetch_forecast_run(
        &self,
        _run_id_or_query: ForecastRunLookup,
        _refresh: bool,
    ) -> impl Future<Output = Result<Option<strategy_core::ForecastRunData>, Self::Error>> + Send
    {
        ready(Ok(None))
    }

    fn fetch_latest_reports(
        &self,
        _query: Option<strategy_core::LatestReportsQuery>,
        _refresh: bool,
    ) -> impl Future<Output = Result<strategy_core::LatestReportsData, Self::Error>> + Send {
        ready(Ok(strategy_core::LatestReportsData::default()))
    }

    fn fetch_reports(
        &self,
        _query: Option<strategy_core::ReportsQuery>,
        _report_type: Option<&str>,
        _date: Option<strategy_core::LocalDateLike>,
        _refresh: bool,
    ) -> impl Future<Output = Result<strategy_core::StationReportsData, Self::Error>> + Send {
        ready(Ok(strategy_core::StationReportsData::default()))
    }

    fn fetch_report_history(
        &self,
        _query: Option<strategy_core::ReportHistoryQuery>,
        _report_type: Option<&str>,
        _start: Option<strategy_core::LocalDateLike>,
        _end: Option<strategy_core::LocalDateLike>,
        _limit: Option<i64>,
        _cursor: Option<&str>,
        _refresh: bool,
    ) -> impl Future<Output = Result<strategy_core::StationReportHistoryPage, Self::Error>> + Send
    {
        ready(Ok(strategy_core::StationReportHistoryPage::default()))
    }

    fn fetch_latest_observation(
        &self,
        _query: Option<LatestObservationQuery>,
        _refresh: bool,
    ) -> impl Future<Output = Result<strategy_core::LatestObservationData, Self::Error>> + Send
    {
        ready(Ok(strategy_core::LatestObservationData::default()))
    }
}

#[derive(Default)]
struct FakeBroker {
    positions: Vec<Position>,
    pending: Vec<PendingOrder>,
}

impl Broker for FakeBroker {
    type Error = String;

    fn place_order(
        &mut self,
        ticker: &str,
        _action: Action,
        contract_side: ContractSide,
        _order_type: OrderType,
        quantity: i64,
        limit_price: Option<f64>,
        signal_type: Option<&str>,
        signal_metadata: Option<&str>,
        client_order_id: Option<&str>,
    ) -> Result<OrderResult, Self::Error> {
        self.positions.push(Position {
            ticker: ticker.to_string(),
            side: contract_side,
            quantity,
            avg_price: limit_price.unwrap_or_default(),
        });
        self.pending.push(PendingOrder {
            order_id: client_order_id.unwrap_or("order-1").to_string(),
            sleeve_id: "demo:KMIA".to_string(),
            ticker: ticker.to_string(),
            action: _action,
            contract_side,
            limit_price: limit_price.unwrap_or_default(),
            requested_quantity: quantity,
            filled_quantity: quantity,
            reserved_global: 0.0,
            reserved_sleeve: 0.0,
            fee_type: String::new(),
            fee_multiplier: None,
            fee_accumulator: 0.0,
            signal_type: signal_type.map(str::to_string),
            signal_metadata: signal_metadata.map(str::to_string),
            created_at: String::new(),
            client_order_id: client_order_id.map(str::to_string),
            expires_at: None,
        });
        Ok(OrderResult {
            order_id: client_order_id.unwrap_or("order-1").to_string(),
            sleeve_id: "demo:KMIA".to_string(),
            status: OrderStatus::Filled,
            filled_quantity: quantity,
            fill_price: limit_price.unwrap_or_default(),
            fee_cost: 0.0,
            reason: String::new(),
        })
    }

    fn cancel_order(&mut self, _order_id: &str) -> Result<bool, Self::Error> {
        Ok(false)
    }

    fn cancel_all_orders(&mut self) -> Result<usize, Self::Error> {
        Ok(0)
    }

    fn get_position(&self, ticker: &str, side: ContractSide) -> Option<&Position> {
        self.positions
            .iter()
            .find(|position| position.ticker == ticker && position.side == side)
    }

    fn get_positions(&self) -> BTreeMap<String, &Position> {
        self.positions
            .iter()
            .map(|position| {
                let side = match position.side {
                    ContractSide::Yes => "yes",
                    ContractSide::No => "no",
                };
                (format!("{}:{side}", position.ticker), position)
            })
            .collect()
    }

    fn get_pending_orders(&self) -> Vec<&PendingOrder> {
        self.pending.iter().collect()
    }

    fn get_sleeve_buying_power(&self) -> f64 {
        100.0
    }
}

#[derive(Default)]
struct FakeHttp;

impl HttpClient for FakeHttp {
    type Error = String;

    fn request(
        &self,
        request: HttpRequest,
    ) -> impl Future<Output = Result<HttpResponse, Self::Error>> + Send {
        ready(Ok(HttpResponse {
            status_code: 200,
            headers: BTreeMap::new(),
            text: None,
            json_body: Some(json!({"method": request.method, "url": request.url})),
        }))
    }

    fn get(
        &self,
        url: &str,
        _headers: Option<strategy_core::http::HttpHeaders>,
        _params: Option<strategy_core::http::HttpParams>,
        _timeout_seconds: Option<f64>,
    ) -> impl Future<Output = Result<HttpResponse, Self::Error>> + Send {
        self.request(HttpRequest {
            method: HttpMethod::Get,
            url: url.to_string(),
            headers: BTreeMap::new(),
            params: BTreeMap::new(),
            json_body: None,
            text_body: None,
            timeout_seconds: None,
        })
    }

    fn post(
        &self,
        url: &str,
        _headers: Option<strategy_core::http::HttpHeaders>,
        _params: Option<strategy_core::http::HttpParams>,
        json_body: Option<Value>,
        text_body: Option<String>,
        _timeout_seconds: Option<f64>,
    ) -> impl Future<Output = Result<HttpResponse, Self::Error>> + Send {
        self.request(HttpRequest {
            method: HttpMethod::Post,
            url: url.to_string(),
            headers: BTreeMap::new(),
            params: BTreeMap::new(),
            json_body,
            text_body,
            timeout_seconds: None,
        })
    }
}

#[derive(Default)]
struct FakeLogger {
    info_count: usize,
}

impl StrategyLogger for FakeLogger {
    fn debug(&self, _message: &str) {}
    fn info(&self, _message: &str) {}
    fn warning(&self, _message: &str) {}
    fn error(&self, _message: &str) {}
    fn exception(&self, _message: &str) {}
}

#[derive(Default)]
struct FakeTelemetry {
    logger: FakeLogger,
}

impl Telemetry for FakeTelemetry {
    type Logger = FakeLogger;

    fn logger(&self) -> &Self::Logger {
        &self.logger
    }

    fn counter(
        &mut self,
        _name: &str,
        _value: f64,
        _fields: Option<&strategy_core::TelemetryFields>,
    ) {
    }
    fn gauge(
        &mut self,
        _name: &str,
        _value: f64,
        _fields: Option<&strategy_core::TelemetryFields>,
    ) {
    }

    fn annotate(
        &mut self,
        _name: &str,
        _value: TelemetryField,
        _fields: Option<&strategy_core::TelemetryFields>,
    ) {
    }
}

#[derive(Default)]
struct FakeTimer {
    cancelled: bool,
}

impl TimerHandle for FakeTimer {
    fn cancelled(&self) -> bool {
        self.cancelled
    }

    fn cancel(&mut self) {
        self.cancelled = true;
    }
}

#[derive(Default)]
struct FakeWork {
    cancelled: bool,
    done: bool,
}

impl WorkHandle for FakeWork {
    type Error = String;

    fn cancelled(&self) -> bool {
        self.cancelled
    }

    fn done(&self) -> bool {
        self.done
    }

    fn exception(&self) -> Option<&Self::Error> {
        None
    }

    fn cancel(&mut self) {
        self.cancelled = true;
    }
}

#[derive(Default)]
struct FakeClock;

impl EngineClock for FakeClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 4, 8, 12, 0, 0).unwrap()
    }

    fn sleep(&self, _seconds: f64) -> impl Future<Output = ()> + Send {
        ready(())
    }

    fn sleep_until(&self, _when: chrono::DateTime<Utc>) -> impl Future<Output = ()> + Send {
        ready(())
    }
}

struct FakeRuntime {
    clock: FakeClock,
    scope: StrategyScope,
    identity: Value,
}

impl Default for FakeRuntime {
    fn default() -> Self {
        Self {
            clock: FakeClock,
            scope: StrategyScope {
                sleeve_id: "demo:KMIA".to_string(),
                strategy_name: "demo".to_string(),
                station_id: Some("KMIA".to_string()),
                tickers: Vec::new(),
                market_type: None,
                event_ticker: None,
                event_date: None,
            },
            identity: json!({"engine": "test"}),
        }
    }
}

impl StrategyRuntime for FakeRuntime {
    type Clock = FakeClock;
    type Timer = FakeTimer;
    type Work = FakeWork;

    fn mode(&self) -> RuntimeMode {
        RuntimeMode::Replay
    }

    fn run_id(&self) -> &str {
        "run-1"
    }

    fn scope(&self) -> &StrategyScope {
        &self.scope
    }

    fn clock(&self) -> &Self::Clock {
        &self.clock
    }

    fn runtime_identity(&self) -> &Value {
        &self.identity
    }

    fn wake_at(&mut self, _when: chrono::DateTime<Utc>, _name: Option<&str>) -> Self::Timer {
        FakeTimer::default()
    }

    fn start_work<F>(&mut self, _work: F, _name: Option<&str>) -> Self::Work
    where
        F: Future<Output = ()> + Send + 'static,
    {
        FakeWork {
            cancelled: false,
            done: true,
        }
    }
}

struct FakeContext {
    state: FakeState,
    data: FakeData,
    broker: FakeBroker,
    http: FakeHttp,
    runtime: FakeRuntime,
    capabilities: RuntimeCapabilities,
    config: strategy_core::StrategyConfig,
    telemetry: FakeTelemetry,
    native_runner: Option<FakeNativeRunner>,
}

impl Default for FakeContext {
    fn default() -> Self {
        Self {
            state: FakeState::default(),
            data: FakeData,
            broker: FakeBroker::default(),
            http: FakeHttp,
            runtime: FakeRuntime::default(),
            capabilities: RuntimeCapabilities::default(),
            config: [("strategy".to_string(), json!("demo"))]
                .into_iter()
                .collect(),
            telemetry: FakeTelemetry::default(),
            native_runner: None,
        }
    }
}

impl StrategyContext for FakeContext {
    type State = FakeState;
    type Data = FakeData;
    type Broker = FakeBroker;
    type Http = FakeHttp;
    type Runtime = FakeRuntime;
    type Telemetry = FakeTelemetry;

    fn state(&self) -> &Self::State {
        &self.state
    }

    fn data(&self) -> &Self::Data {
        &self.data
    }

    fn broker(&mut self) -> &mut Self::Broker {
        &mut self.broker
    }

    fn http(&self) -> &Self::Http {
        &self.http
    }

    fn runtime(&mut self) -> &mut Self::Runtime {
        &mut self.runtime
    }

    fn capabilities(&self) -> &RuntimeCapabilities {
        &self.capabilities
    }

    fn config(&self) -> &strategy_core::StrategyConfig {
        &self.config
    }

    fn telemetry(&mut self) -> &mut Self::Telemetry {
        &mut self.telemetry
    }

    fn next_event(&mut self) -> impl Future<Output = Option<StrategyEvent>> + Send {
        ready(None)
    }
}

impl strategy_core::NativeStrategyContext for FakeContext {
    type NativeRunner = FakeNativeRunner;

    fn native_kernel_runner(&mut self) -> Option<&mut Self::NativeRunner> {
        self.native_runner.as_mut()
    }
}

#[derive(Default)]
struct FakeNativeRunner {
    calls: usize,
    error: Option<String>,
}

struct FakeKernel;

impl strategy_core::kernel::NativeKernel for FakeKernel {
    fn name(&self) -> &str {
        "fake"
    }

    fn on_event(
        &mut self,
        _event: strategy_core::kernel::StrategyEventView<'_>,
        _ctx: &mut dyn strategy_core::kernel::StrategyKernelContext,
    ) -> strategy_core::kernel::KernelResult<()> {
        Ok(())
    }
}

impl strategy_core::NativeKernelRunner<FakeKernel> for FakeNativeRunner {
    type Error = String;

    fn run_native_kernel(
        &mut self,
        _kernel: &mut FakeKernel,
    ) -> impl Future<Output = Result<strategy_core::NativeKernelResult, Self::Error>> + Send {
        self.calls += 1;
        ready(match self.error.clone() {
            Some(error) => Err(error),
            None => Ok(strategy_core::NativeKernelResult::default()),
        })
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    struct NoopWake;

    impl std::task::Wake for NoopWake {
        fn wake(self: std::sync::Arc<Self>) {}
    }

    let waker = std::task::Waker::from(std::sync::Arc::new(NoopWake));
    let mut context = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);

    loop {
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(output) => return output,
            std::task::Poll::Pending => std::thread::yield_now(),
        }
    }
}
