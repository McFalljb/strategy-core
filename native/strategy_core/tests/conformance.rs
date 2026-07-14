use std::{collections::BTreeMap, fs, path::PathBuf, str::FromStr};

use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use strategy_core::*;

fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/conformance")
        .join(format!("{name}.json"));
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn round_trip<T>(wire: &Value) -> Result<Value, String>
where
    T: DeserializeOwned + Serialize,
{
    let decoded: T = serde_json::from_value(wire.clone()).map_err(|error| error.to_string())?;
    serde_json::to_value(decoded).map_err(|error| error.to_string())
}

fn dispatch(rust_type: &str, wire: &Value) -> Result<Value, String> {
    match rust_type {
        "Action" => round_trip::<Action>(wire),
        "BrokerOrderUpdate" => round_trip::<BrokerOrderUpdate>(wire),
        "BrokerUpdateStatus" => round_trip::<BrokerUpdateStatus>(wire),
        "ContractSide" => round_trip::<ContractSide>(wire),
        "CityInfo" => round_trip::<CityInfo>(wire),
        "CursorPage" => round_trip::<CursorPage>(wire),
        "DataResolution" => round_trip::<DataResolution>(wire),
        "EffectiveLimits" => round_trip::<EffectiveLimits>(wire),
        "EventDelivery" => round_trip::<EventDelivery>(wire),
        "FeeType" => round_trip::<FeeType>(wire),
        "ForecastBundle" => round_trip::<ForecastBundle>(wire),
        "ForecastBundleRun" => round_trip::<ForecastBundleRun>(wire),
        "ForecastHourly" => round_trip::<ForecastHourly>(wire),
        "ForecastQuery" => round_trip::<ForecastQuery>(wire),
        "ForecastRunQuery" => round_trip::<ForecastRunQuery>(wire),
        "ForecastRunData" => round_trip::<ForecastRunData>(wire),
        "ForecastRunSummary" => round_trip::<ForecastRunSummary>(wire),
        "ForecastRunsPage" => round_trip::<ForecastRunsPage>(wire),
        "ForecastRunsQuery" => round_trip::<ForecastRunsQuery>(wire),
        "ForecastUpdated" => round_trip::<ForecastUpdated>(wire),
        "ForecastVersions" => round_trip::<ForecastVersions>(wire),
        "FreshnessDomain" => round_trip::<FreshnessDomain>(wire),
        "FreshnessDomainSummary" => round_trip::<FreshnessDomainSummary>(wire),
        "FreshnessSnapshot" => round_trip::<FreshnessSnapshot>(wire),
        "FreshnessStatus" => round_trip::<FreshnessStatus>(wire),
        "FreshnessSummary" => round_trip::<FreshnessSummary>(wire),
        "LatestObservationQuery" => round_trip::<LatestObservationQuery>(wire),
        "LatestObservationData" => round_trip::<LatestObservationData>(wire),
        "LatestReportsData" => round_trip::<LatestReportsData>(wire),
        "LatestReportsQuery" => round_trip::<LatestReportsQuery>(wire),
        "LimitsQuery" => round_trip::<LimitsQuery>(wire),
        "HttpMethod" => round_trip::<HttpMethod>(wire),
        "HttpRequest" => round_trip::<HttpRequest>(wire),
        "HttpResponse" => round_trip::<HttpResponse>(wire),
        "HourlyForecastRecord" => round_trip::<HourlyForecastRecord>(wire),
        "IpGuardLimits" => round_trip::<IpGuardLimits>(wire),
        "KalshiCollateralReturnType" => round_trip::<KalshiCollateralReturnType>(wire),
        "KalshiCreateOrderResponse" => round_trip::<KalshiCreateOrderResponse>(wire),
        "KalshiEventLifecycleMessage" => round_trip::<KalshiEventLifecycleMessage>(wire),
        "KalshiGetMarketResponse" => round_trip::<KalshiGetMarketResponse>(wire),
        "KalshiGetOrderResponse" => round_trip::<KalshiGetOrderResponse>(wire),
        "KalshiGetOrderbookResponse" => round_trip::<KalshiGetOrderbookResponse>(wire),
        "KalshiGetOrderbooksResponse" => round_trip::<KalshiGetOrderbooksResponse>(wire),
        "KalshiGetOrdersResponse" => round_trip::<KalshiGetOrdersResponse>(wire),
        "KalshiImmediateTimeInForce" => round_trip::<KalshiImmediateTimeInForce>(wire),
        "KalshiListSubscriptionsCommand" => round_trip::<KalshiListSubscriptionsCommand>(wire),
        "KalshiMarket" => round_trip::<KalshiMarket>(wire),
        "KalshiMarketLifecycleEventType" => round_trip::<KalshiMarketLifecycleEventType>(wire),
        "KalshiMarketLifecycleMessage" => round_trip::<KalshiMarketLifecycleMessage>(wire),
        "KalshiMarketLifecycleMetadata" => round_trip::<KalshiMarketLifecycleMetadata>(wire),
        "KalshiMarketOrderbook" => round_trip::<KalshiMarketOrderbook>(wire),
        "KalshiMarketPositionMessage" => round_trip::<KalshiMarketPositionMessage>(wire),
        "KalshiMarketResult" => round_trip::<KalshiMarketResult>(wire),
        "KalshiMarketSide" => round_trip::<KalshiMarketSide>(wire),
        "KalshiMarketStatus" => round_trip::<KalshiMarketStatus>(wire),
        "KalshiMarketsPage" => round_trip::<KalshiMarketsPage>(wire),
        "KalshiMveSelectedLeg" => round_trip::<KalshiMveSelectedLeg>(wire),
        "KalshiOrder" => round_trip::<KalshiOrder>(wire),
        "KalshiOrderAction" => round_trip::<KalshiOrderAction>(wire),
        "KalshiOrderCreateRequest" => round_trip::<KalshiOrderCreateRequest>(wire),
        "KalshiOrderStatus" => round_trip::<KalshiOrderStatus>(wire),
        "KalshiOrderType" => round_trip::<KalshiOrderType>(wire),
        "KalshiOrderbook" => round_trip::<KalshiOrderbook>(wire),
        "KalshiOrderbookDeltaMessage" => round_trip::<KalshiOrderbookDeltaMessage>(wire),
        "KalshiOrderbookSnapshotMessage" => round_trip::<KalshiOrderbookSnapshotMessage>(wire),
        "KalshiOrderbookLevel" => round_trip::<KalshiOrderbookLevel>(wire),
        "KalshiPriceLevelStructure" => round_trip::<KalshiPriceLevelStructure>(wire),
        "KalshiPriceRange" => round_trip::<KalshiPriceRange>(wire),
        "KalshiSelfTradePreventionType" => round_trip::<KalshiSelfTradePreventionType>(wire),
        "KalshiSubscribeCommand" => round_trip::<KalshiSubscribeCommand>(wire),
        "KalshiSubscriptionUpdateAction" => round_trip::<KalshiSubscriptionUpdateAction>(wire),
        "KalshiTickerMessage" => round_trip::<KalshiTickerMessage>(wire),
        "KalshiTimeInForce" => round_trip::<KalshiTimeInForce>(wire),
        "KalshiTradeMessage" => round_trip::<KalshiTradeMessage>(wire),
        "KalshiUnsubscribeCommand" => round_trip::<KalshiUnsubscribeCommand>(wire),
        "KalshiUpdateSubscriptionCommand" => round_trip::<KalshiUpdateSubscriptionCommand>(wire),
        "KalshiUserFillMessage" => round_trip::<KalshiUserFillMessage>(wire),
        "KalshiUserOrderMessage" => round_trip::<KalshiUserOrderMessage>(wire),
        "KalshiWsChannel" => round_trip::<KalshiWsChannel>(wire),
        "KalshiWsMessage" => round_trip::<KalshiWsMessage>(wire),
        "MarketType" => round_trip::<MarketType>(wire),
        "MarketBracket" => round_trip::<MarketBracket>(wire),
        "ModelForecast" => round_trip::<ModelForecast>(wire),
        "NativeKernelResult" => round_trip::<NativeKernelResult>(wire),
        "NativeKernelStatus" => round_trip::<NativeKernelStatus>(wire),
        "NewHigh" => round_trip::<NewHigh>(wire),
        "NewLow" => round_trip::<NewLow>(wire),
        "Observation" => round_trip::<Observation>(wire),
        "ObservationRecord" => round_trip::<ObservationRecord>(wire),
        "OracleModelScore" => round_trip::<OracleModelScore>(wire),
        "OracleModelScoreRecord" => round_trip::<OracleModelScoreRecord>(wire),
        "OracleRankBy" => round_trip::<OracleRankBy>(wire),
        "OracleScoreData" => round_trip::<OracleScoreData>(wire),
        "OracleScoreMode" => round_trip::<OracleScoreMode>(wire),
        "OracleScoreRow" => round_trip::<OracleScoreRow>(wire),
        "OracleScoreTable" => round_trip::<OracleScoreTable>(wire),
        "OracleScoresQuery" => round_trip::<OracleScoresQuery>(wire),
        "OracleScoresUpdated" => round_trip::<OracleScoresUpdated>(wire),
        "OrderExecutionStyle" => round_trip::<OrderExecutionStyle>(wire),
        "OrderIntent" => round_trip::<OrderIntent>(wire),
        "OrderResult" => round_trip::<OrderResult>(wire),
        "OrderStatus" => round_trip::<OrderStatus>(wire),
        "OrderTimePolicy" => round_trip::<OrderTimePolicy>(wire),
        "OrderType" => round_trip::<OrderType>(wire),
        "PersistenceStatus" => round_trip::<PersistenceStatus>(wire),
        "PendingOrder" => round_trip::<PendingOrder>(wire),
        "Position" => round_trip::<Position>(wire),
        "PriceUpdate" => round_trip::<PriceUpdate>(wire),
        "PlanTier" => round_trip::<PlanTier>(wire),
        "ReportHistoryQuery" => round_trip::<ReportHistoryQuery>(wire),
        "ReportClockSchedule" => round_trip::<ReportClockSchedule>(wire),
        "ReportIntervalSchedule" => round_trip::<ReportIntervalSchedule>(wire),
        "ReportMultiHourSchedule" => round_trip::<ReportMultiHourSchedule>(wire),
        "ReportScheduleBasis" => round_trip::<ReportScheduleBasis>(wire),
        "ReportType" => round_trip::<ReportType>(wire),
        "ReportsQuery" => round_trip::<ReportsQuery>(wire),
        "RuntimeCapabilities" => round_trip::<RuntimeCapabilities>(wire),
        "RuntimeMode" => round_trip::<RuntimeMode>(wire),
        "ShutdownEvent" => round_trip::<ShutdownEvent>(wire),
        "StationForecast" => round_trip::<StationForecast>(wire),
        "StationForecastData" => round_trip::<StationForecastData>(wire),
        "StationInfo" => round_trip::<StationInfo>(wire),
        "StationOracleScores" => round_trip::<StationOracleScores>(wire),
        "StationReportHistoryPage" => round_trip::<StationReportHistoryPage>(wire),
        "StationReport" => round_trip::<StationReport>(wire),
        "StationReportRecord" => round_trip::<StationReportRecord>(wire),
        "StationReportsData" => round_trip::<StationReportsData>(wire),
        "StationWeather" => round_trip::<StationWeather>(wire),
        "StrategyConfig" => round_trip::<StrategyConfig>(wire),
        "StrategyEvent" => round_trip::<StrategyEvent>(wire),
        "StrategyScope" => round_trip::<StrategyScope>(wire),
        "TelemetryFields" => round_trip::<TelemetryFields>(wire),
        "TemperatureDayMode" => round_trip::<TemperatureDayMode>(wire),
        "TemperatureUnit" => round_trip::<TemperatureUnit>(wire),
        "TickerPrices" => round_trip::<TickerPrices>(wire),
        "TimerWake" => round_trip::<TimerWake>(wire),
        "WeatherEvent" => round_trip::<WeatherEvent>(wire),
        "WeatherEventSource" => round_trip::<WeatherEventSource>(wire),
        "WuDayMode" => round_trip::<WuDayMode>(wire),
        other => Err(format!(
            "conformance fixture names unknown Rust type {other:?}"
        )),
    }
}

fn normalized_error_category(error: &str) -> &'static str {
    if error.contains("must not be blank") {
        return "range";
    }
    if error.contains("missing field") || error.contains("type is required") {
        return "required_field";
    }
    if error.contains("unknown variant")
        || error.contains("unknown strategy event type")
        || error.contains("expected event type")
    {
        return "enum";
    }
    if error.contains("invalid characters") || error.contains("premature end") {
        return "format";
    }
    "type"
}

fn helper_ok<T: Serialize>(value: T) -> Value {
    json!({"ok": serde_json::to_value(value).unwrap()})
}

fn helper_error(category: &str) -> Value {
    json!({"error": category})
}

fn input_f64(value: &Value) -> f64 {
    match value["non_finite"].as_str() {
        Some("nan") => f64::NAN,
        Some("inf") => f64::INFINITY,
        Some("-inf") => f64::NEG_INFINITY,
        _ => value.as_f64().unwrap(),
    }
}

fn input_finite_f64(value: &Value) -> Result<f64, Value> {
    let value = input_f64(value);
    if value.is_finite() {
        Ok(value)
    } else {
        Err(helper_error("invalid_decimal"))
    }
}

fn input_optional_f64(input: &Value, name: &str) -> Result<Option<f64>, Value> {
    match input.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => input_finite_f64(value).map(Some),
    }
}

fn input_fee_type(input: &Value) -> Result<Option<FeeType>, Value> {
    input["fee_type"]
        .as_str()
        .map(FeeType::from_str)
        .transpose()
        .map_err(|_| helper_error("unknown_fee_type"))
}

fn input_action(input: &Value) -> Result<Action, Value> {
    serde_json::from_value(input["action"].clone()).map_err(|_| helper_error("unknown_action"))
}

fn input_liquidity_role(input: &Value) -> Result<strategy_core::LiquidityRole, Value> {
    serde_json::from_value(input["liquidity_role"].clone())
        .map_err(|_| helper_error("unknown_liquidity_role"))
}

fn fee_result<T: Serialize>(result: Result<T, FeeError>) -> Value {
    match result {
        Ok(value) => helper_ok(value),
        Err(FeeError::UnknownFeeType(_)) => helper_error("unknown_fee_type"),
        Err(FeeError::InvalidDecimal(_)) => helper_error("invalid_decimal"),
    }
}

fn custom_timezones(input: &Value) -> Option<BTreeMap<String, String>> {
    input
        .get("station_timezones")
        .map(|value| serde_json::from_value(value.clone()).unwrap())
}

fn input_datetime(input: &Value, name: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(input[name].as_str().unwrap())
        .unwrap()
        .with_timezone(&Utc)
}

fn input_date(input: &Value, name: &str) -> NaiveDate {
    NaiveDate::parse_from_str(input[name].as_str().unwrap(), "%Y-%m-%d").unwrap()
}

fn evaluate_helper(helper: &str, input: &Value) -> Value {
    match helper {
        "calculate_trade_fee" => {
            let liquidity_role = match input_liquidity_role(input) {
                Ok(value) => value,
                Err(error) => return error,
            };
            let fee_multiplier = match input_optional_f64(input, "fee_multiplier") {
                Ok(value) => value,
                Err(error) => return error,
            };
            let fee_type = match input_fee_type(input) {
                Ok(value) => value,
                Err(error) => return error,
            };
            let price = match input_finite_f64(&input["price"]) {
                Ok(value) => value,
                Err(error) => return error,
            };
            fee_result(calculate_trade_fee(
                price,
                input["quantity"].as_i64().unwrap(),
                liquidity_role,
                fee_type,
                fee_multiplier,
            ))
        }
        "apply_fee_rounding" => fee_result(apply_fee_rounding(
            input_f64(&input["revenue"]),
            input_f64(&input["trade_fee"]),
            input_f64(&input["fee_accumulator"]),
        )),
        "calculate_fill_fee" => {
            let action = match input_action(input) {
                Ok(value) => value,
                Err(error) => return error,
            };
            let price = match input_finite_f64(&input["price"]) {
                Ok(value) => value,
                Err(error) => return error,
            };
            let liquidity_role = match input_liquidity_role(input) {
                Ok(value) => value,
                Err(error) => return error,
            };
            let fee_multiplier = match input_optional_f64(input, "fee_multiplier") {
                Ok(value) => value,
                Err(error) => return error,
            };
            let fee_type = match input_fee_type(input) {
                Ok(value) => value,
                Err(error) => return error,
            };
            let fee_accumulator = match input_finite_f64(&input["fee_accumulator"]) {
                Ok(value) => value,
                Err(error) => return error,
            };
            fee_result(calculate_fill_fee(
                action,
                price,
                input["quantity"].as_i64().unwrap(),
                liquidity_role,
                fee_accumulator,
                fee_type,
                fee_multiplier,
            ))
        }
        "station_constants" => helper_ok(json!({
            "icao_to_city_codes": serde_json::to_value(&*ICAO_TO_CITY_CODES).unwrap(),
            "city_to_icao": serde_json::to_value(&*CITY_TO_ICAO).unwrap(),
            "station_timezones": serde_json::to_value(&*STATION_TIMEZONES).unwrap(),
            "market_type_prefix": serde_json::to_value(&*MARKET_TYPE_PREFIX).unwrap(),
            "ticker_prefixes": TICKER_PREFIXES,
        })),
        "signal_constants" => helper_ok(json!({
            "dsm_reaction": SIGNAL_DSM_REACTION,
            "metar_6hr_low": SIGNAL_METAR_6HR_LOW,
            "metar_6hr_new_low": SIGNAL_METAR_6HR_NEW_LOW,
        })),
        "primary_city_code_for_series" => helper_ok(primary_city_code_for_series(
            input["station"].as_str().unwrap(),
        )),
        "city_codes_for_market_type" => helper_ok(city_codes_for_market_type(
            input["station"].as_str().unwrap(),
            input["market_type"].as_str().unwrap(),
        )),
        "primary_city_code_for_market_type" => helper_ok(primary_city_code_for_market_type(
            input["station"].as_str().unwrap(),
            input["market_type"].as_str().unwrap(),
        )),
        "ticker_prefixes_for_station" => match ticker_prefixes_for_station(
            input["station"].as_str().unwrap(),
            input["market_type"].as_str().unwrap(),
        ) {
            Ok(value) => helper_ok(value),
            Err(_) => helper_error("unknown_market_type"),
        },
        "station_from_event_ticker" => helper_ok(station_from_event_ticker(
            input["event_ticker"].as_str().unwrap(),
        )),
        "station_timezone" => {
            let timezones = custom_timezones(input);
            match station_timezone(input["station"].as_str(), timezones.as_ref()) {
                Ok(value) => helper_ok(value.to_string()),
                Err(_) => helper_error("timezone"),
            }
        }
        "parse_climate_date" => {
            helper_ok(parse_climate_date(input["raw"].as_str()).map(|value| value.to_string()))
        }
        "climate_day_date" => {
            let timezones = custom_timezones(input);
            match climate_day_date(
                input["station"].as_str(),
                input_datetime(input, "now"),
                timezones.as_ref(),
            ) {
                Ok(value) => helper_ok(value.to_string()),
                Err(_) => helper_error("timezone"),
            }
        }
        "climate_day_end" => {
            let timezones = custom_timezones(input);
            match climate_day_end(
                input["station"].as_str(),
                input_date(input, "event_date"),
                timezones.as_ref(),
            ) {
                Ok(value) => helper_ok(value.to_rfc3339_opts(SecondsFormat::AutoSi, true)),
                Err(_) => helper_error("timezone"),
            }
        }
        "climate_day_has_ended" => {
            let timezones = custom_timezones(input);
            match climate_day_has_ended(
                input["station"].as_str(),
                input_date(input, "event_date"),
                input_datetime(input, "now"),
                timezones.as_ref(),
            ) {
                Ok(value) => helper_ok(value),
                Err(_) => helper_error("timezone"),
            }
        }
        other => panic!("unknown helper vector {other:?}"),
    }
}

#[test]
fn python_authored_values_round_trip_structurally() {
    for family in [
        "events",
        "state",
        "broker",
        "runtime",
        "queries",
        "minutetemp",
        "kalshi",
        "http-data",
    ] {
        let document = fixture(family);
        assert_eq!(document["family"], family);
        for case in document["valid"].as_array().unwrap() {
            let case_id = case["id"].as_str().unwrap();
            let rust_type = case["rust_type"].as_str().unwrap();
            let actual = dispatch(rust_type, &case["wire"])
                .unwrap_or_else(|error| panic!("{family}/{case_id} failed to decode: {error}"));
            assert_eq!(actual, case["expected"], "{family}/{case_id}");
        }
    }
}

#[test]
fn python_authored_invalid_values_are_rejected() {
    for family in [
        "events",
        "state",
        "broker",
        "runtime",
        "queries",
        "minutetemp",
        "kalshi",
        "http-data",
    ] {
        let document = fixture(family);
        for case in document["invalid"].as_array().unwrap() {
            let case_id = case["id"].as_str().unwrap();
            let rust_type = case["rust_type"].as_str().unwrap();
            let error = dispatch(rust_type, &case["wire"])
                .expect_err(&format!("{family}/{case_id} unexpectedly decoded"));
            assert_eq!(
                normalized_error_category(&error),
                case["category"],
                "{family}/{case_id}: {error}"
            );
        }
    }
}

#[test]
fn python_authored_helper_vectors_match_rust_results() {
    let document = fixture("helpers");
    for case in document["cases"].as_array().unwrap() {
        let case_id = case["id"].as_str().unwrap();
        let actual = evaluate_helper(case["helper"].as_str().unwrap(), &case["input"]);
        assert_eq!(actual, case["expected"], "{case_id}");
    }
}
