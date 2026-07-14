use std::{fs, path::PathBuf};

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use strategy_core::{
    Action, BrokerOrderUpdate, BrokerUpdateStatus, ContractSide, EventDelivery, FeeType,
    ForecastQuery, ForecastRunQuery, ForecastRunsQuery, FreshnessDomain, FreshnessSnapshot,
    FreshnessStatus, FreshnessSummary, LatestObservationQuery, LatestReportsQuery, LimitsQuery,
    MarketType, NativeKernelResult, NativeKernelStatus, OracleScoresQuery, OrderExecutionStyle,
    OrderIntent, OrderResult, OrderStatus, OrderTimePolicy, OrderType, PendingOrder, Position,
    ReportHistoryQuery, ReportsQuery, RuntimeCapabilities, RuntimeMode, StationForecast,
    StationOracleScores, StationWeather, StrategyConfig, StrategyEvent, StrategyScope,
    TelemetryFields, TickerPrices,
};

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
        "EventDelivery" => round_trip::<EventDelivery>(wire),
        "FeeType" => round_trip::<FeeType>(wire),
        "ForecastQuery" => round_trip::<ForecastQuery>(wire),
        "ForecastRunQuery" => round_trip::<ForecastRunQuery>(wire),
        "ForecastRunsQuery" => round_trip::<ForecastRunsQuery>(wire),
        "FreshnessDomain" => round_trip::<FreshnessDomain>(wire),
        "FreshnessSnapshot" => round_trip::<FreshnessSnapshot>(wire),
        "FreshnessStatus" => round_trip::<FreshnessStatus>(wire),
        "FreshnessSummary" => round_trip::<FreshnessSummary>(wire),
        "LatestObservationQuery" => round_trip::<LatestObservationQuery>(wire),
        "LatestReportsQuery" => round_trip::<LatestReportsQuery>(wire),
        "LimitsQuery" => round_trip::<LimitsQuery>(wire),
        "MarketType" => round_trip::<MarketType>(wire),
        "NativeKernelResult" => round_trip::<NativeKernelResult>(wire),
        "NativeKernelStatus" => round_trip::<NativeKernelStatus>(wire),
        "OracleScoresQuery" => round_trip::<OracleScoresQuery>(wire),
        "OrderExecutionStyle" => round_trip::<OrderExecutionStyle>(wire),
        "OrderIntent" => round_trip::<OrderIntent>(wire),
        "OrderResult" => round_trip::<OrderResult>(wire),
        "OrderStatus" => round_trip::<OrderStatus>(wire),
        "OrderTimePolicy" => round_trip::<OrderTimePolicy>(wire),
        "OrderType" => round_trip::<OrderType>(wire),
        "PendingOrder" => round_trip::<PendingOrder>(wire),
        "Position" => round_trip::<Position>(wire),
        "ReportHistoryQuery" => round_trip::<ReportHistoryQuery>(wire),
        "ReportsQuery" => round_trip::<ReportsQuery>(wire),
        "RuntimeCapabilities" => round_trip::<RuntimeCapabilities>(wire),
        "RuntimeMode" => round_trip::<RuntimeMode>(wire),
        "StationForecast" => round_trip::<StationForecast>(wire),
        "StationOracleScores" => round_trip::<StationOracleScores>(wire),
        "StationWeather" => round_trip::<StationWeather>(wire),
        "StrategyConfig" => round_trip::<StrategyConfig>(wire),
        "StrategyEvent" => round_trip::<StrategyEvent>(wire),
        "StrategyScope" => round_trip::<StrategyScope>(wire),
        "TelemetryFields" => round_trip::<TelemetryFields>(wire),
        "TickerPrices" => round_trip::<TickerPrices>(wire),
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
    if error.contains("unknown variant") || error.contains("unknown strategy event type") {
        return "enum";
    }
    if error.contains("invalid characters") || error.contains("premature end") {
        return "format";
    }
    "type"
}

#[test]
fn python_authored_core_values_round_trip_structurally() {
    for family in ["events", "state", "broker", "runtime", "queries"] {
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
fn python_authored_invalid_core_values_are_rejected() {
    for family in ["events", "state", "broker", "runtime", "queries"] {
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
