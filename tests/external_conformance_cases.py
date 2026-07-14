"""Python-authored external-model and portable-helper conformance cases."""

from __future__ import annotations

import collections.abc
import json
from datetime import UTC, date, datetime
from typing import TYPE_CHECKING, Any, cast

from pydantic import TypeAdapter, ValidationError

import strategy_core as strategy_contract
from strategy_core.climate_day import (
    climate_day_date,
    climate_day_end,
    climate_day_has_ended,
    parse_climate_date,
    station_timezone,
)
from strategy_core.fees import apply_fee_rounding, calculate_fill_fee, calculate_trade_fee
from strategy_core.http import HttpMethod, HttpRequest, HttpResponse
from strategy_core.kalshi import (
    KalshiCollateralReturnType,
    KalshiCreateOrderResponse,
    KalshiEventLifecycleMessage,
    KalshiGetMarketResponse,
    KalshiGetOrderbookResponse,
    KalshiGetOrderbooksResponse,
    KalshiGetOrderResponse,
    KalshiGetOrdersResponse,
    KalshiImmediateTimeInForce,
    KalshiListSubscriptionsCommand,
    KalshiMarket,
    KalshiMarketLifecycleEventType,
    KalshiMarketLifecycleMessage,
    KalshiMarketLifecycleMetadata,
    KalshiMarketOrderbook,
    KalshiMarketPositionMessage,
    KalshiMarketResult,
    KalshiMarketSide,
    KalshiMarketsPage,
    KalshiMarketStatus,
    KalshiMveSelectedLeg,
    KalshiOrder,
    KalshiOrderAction,
    KalshiOrderbook,
    KalshiOrderbookDeltaMessage,
    KalshiOrderbookLevel,
    KalshiOrderbookSnapshotMessage,
    KalshiOrderCreateRequest,
    KalshiOrderStatus,
    KalshiOrderType,
    KalshiPriceLevelStructure,
    KalshiPriceRange,
    KalshiSelfTradePreventionType,
    KalshiSubscribeCommand,
    KalshiSubscriptionUpdateAction,
    KalshiTickerMessage,
    KalshiTimeInForce,
    KalshiTradeMessage,
    KalshiUnsubscribeCommand,
    KalshiUpdateSubscriptionCommand,
    KalshiUserFillMessage,
    KalshiUserOrderMessage,
    KalshiWsChannel,
    KalshiWsMessage,
)
from strategy_core.minutetemp import (
    CityInfo,
    CursorPage,
    DataResolution,
    EffectiveLimits,
    ForecastBundle,
    ForecastBundleRun,
    ForecastRunData,
    ForecastRunsPage,
    ForecastRunSummary,
    HourlyForecastRecord,
    IpGuardLimits,
    LatestObservationData,
    LatestReportsData,
    ObservationRecord,
    OracleModelScoreRecord,
    OracleRankBy,
    OracleScoreData,
    PlanTier,
    ReportClockSchedule,
    ReportIntervalSchedule,
    ReportMultiHourSchedule,
    ReportScheduleBasis,
    ReportType,
    StationForecastData,
    StationInfo,
    StationReportHistoryPage,
    StationReportRecord,
    StationReportsData,
    TemperatureUnit,
)
from strategy_core.models import JSONValue
from strategy_core.signals import SIGNAL_DSM_REACTION, SIGNAL_METAR_6HR_LOW, SIGNAL_METAR_6HR_NEW_LOW
from strategy_core.stations import (
    CITY_TO_ICAO,
    ICAO_TO_CITY_CODES,
    MARKET_TYPE_PREFIX,
    STATION_TIMEZONES,
    TICKER_PREFIXES,
    city_codes_for_market_type,
    primary_city_code_for_market_type,
    primary_city_code_for_series,
    station_from_event_ticker,
    ticker_prefixes_for_station,
)
from tests.conformance_cases import (
    FIXTURE_ROOT,
    _document,
    _invalid,
    _json_value,
    _raw,
    _valid,
    _validate_wire,
    _validation_category,
    direct_model_cases,
)

if TYPE_CHECKING:
    from collections.abc import Mapping

EXTERNAL_FIXTURE_NAMES = ("helpers", "minutetemp", "kalshi", "http-data")
EXTERNAL_DIRECT_MODEL_NAMES = (
    "CityInfo",
    "CursorPage",
    "EffectiveLimits",
    "ForecastBundle",
    "ForecastBundleRun",
    "ForecastRunData",
    "ForecastRunSummary",
    "ForecastRunsPage",
    "HourlyForecastRecord",
    "HttpRequest",
    "HttpResponse",
    "IpGuardLimits",
    "KalshiCreateOrderResponse",
    "KalshiEventLifecycleMessage",
    "KalshiGetMarketResponse",
    "KalshiGetOrderResponse",
    "KalshiGetOrderbookResponse",
    "KalshiGetOrderbooksResponse",
    "KalshiGetOrdersResponse",
    "KalshiListSubscriptionsCommand",
    "KalshiMarket",
    "KalshiMarketLifecycleMessage",
    "KalshiMarketLifecycleMetadata",
    "KalshiMarketOrderbook",
    "KalshiMarketPositionMessage",
    "KalshiMarketsPage",
    "KalshiMveSelectedLeg",
    "KalshiOrder",
    "KalshiOrderCreateRequest",
    "KalshiOrderbook",
    "KalshiOrderbookDeltaMessage",
    "KalshiOrderbookLevel",
    "KalshiOrderbookSnapshotMessage",
    "KalshiPriceRange",
    "KalshiSubscribeCommand",
    "KalshiTickerMessage",
    "KalshiTradeMessage",
    "KalshiUnsubscribeCommand",
    "KalshiUpdateSubscriptionCommand",
    "KalshiUserFillMessage",
    "KalshiUserOrderMessage",
    "LatestObservationData",
    "LatestReportsData",
    "ObservationRecord",
    "OracleModelScoreRecord",
    "OracleScoreData",
    "ReportClockSchedule",
    "ReportIntervalSchedule",
    "ReportMultiHourSchedule",
    "StationForecastData",
    "StationInfo",
    "StationReportHistoryPage",
    "StationReportRecord",
    "StationReportsData",
)
EXTERNAL_DIRECT_MODELS = {
    name: cast("type[object]", getattr(strategy_contract, name)) for name in EXTERNAL_DIRECT_MODEL_NAMES
}

_T0 = datetime(2026, 7, 13, 12, 34, 56, 123456, tzinfo=UTC)
_T1 = datetime(2026, 7, 13, 13, 45, 1, 987654, tzinfo=UTC)


def _with_coverage(case: dict[str, Any], paths: dict[str, list[str | int]]) -> dict[str, Any]:
    case.pop("covers", None)
    case["coverage_paths"] = paths
    root_type = cast("str", case["rust_type"])
    root_dimensions = cast("list[str]", case["evidence_dimensions"])
    case["evidence"] = {
        surface: root_dimensions if surface == root_type else ["non_default_round_trip"] for surface in paths
    }
    return case


def _direct_enum_cases(rust_type: str, values: tuple[str, ...]) -> list[dict[str, Any]]:
    return [
        _with_coverage(
            _raw(f"{rust_type}-{value or 'empty'}", rust_type, [], value),
            {rust_type: []},
        )
        for value in values
    ]


def _minute_temp_fixture() -> dict[str, Any]:
    city = CityInfo(id="city-1", slug="miami", name="Miami", timezone="America/New_York")
    station = StationInfo(
        station_id="KMIA",
        name="Miami International",
        temperature_unit="F",
        uses_nws_climate_day=True,
    )
    observation = ObservationRecord(
        observation_time=_T0,
        temperature_f=90.5,
        temperature_c=32.5,
        dewpoint=72.0,
        heat_index=101.0,
        wind_chill=-0.0,
        relative_humidity=64.0,
        barometric_pressure=29.95,
        sea_level_pressure=1014.0,
        wind_speed=8.0,
        wind_direction=180.0,
        wind_gust=14.0,
        text_description="Partly Cloudy",
        precipitation_1h=0.0,
        precipitation_3h=0.1,
        precipitation_6h=0.2,
        is_locf=True,
        is_from_report=True,
        report_type="cli",
        source_report_id="cli-1",
        temp_min_f=88.0,
        temp_max_f=91.0,
        temp_min_c=31.1,
        temp_max_c=32.8,
    )
    report = StationReportRecord(
        report_id="report-1",
        report_revision=2,
        report_updated_at=_T1,
        report_type="dsm",
        report_date=date(2026, 7, 13),
        issuance_time=_T0,
        fetched_at=_T1,
        max_temp_f=91.0,
        max_temp_c=32.8,
        max_temp_time_utc=_T0,
        min_temp_f=78.0,
        min_temp_c=25.6,
        min_temp_time_utc=_T1,
        temp_f=90.0,
        temp_c=32.2,
    )
    hourly = HourlyForecastRecord(
        time=_T1,
        temperature_2m_f=91.0,
        temperature_2m_c=32.8,
        apparent_temperature_f=102.0,
        relative_humidity_2m=61.0,
        dew_point_2m=73.0,
        pressure_msl=1012.5,
        wind_speed_10m=9.0,
        wind_direction_10m=185.0,
        wind_gusts_10m=16.0,
        cloud_cover=35.0,
        precipitation_probability=20.0,
    )
    run = ForecastRunSummary(
        id="run-1",
        station_id="KMIA",
        model_id="ncep_hrrr_conus",
        forecast_time=_T0,
        fetched_at=_T1,
        timezone="America/New_York",
        utc_offset_seconds=-14_400,
        data_hash="sha256:abc",
    )

    valid = [
        _with_coverage(
            _valid(
                "latest-observation-full",
                "LatestObservationData",
                [],
                LatestObservationData(
                    city=city,
                    station=station,
                    observation=observation,
                    daily_high_f=91.0,
                    daily_low_f=78.0,
                    daily_high_c=32.8,
                    daily_low_c=25.6,
                    asos_daily_high_f=90.0,
                    asos_daily_low_f=79.0,
                    asos_daily_high_c=32.2,
                    asos_daily_low_c=26.1,
                    wu_current_temp_f=90.0,
                    wu_current_temp_c=32.2,
                    wu_daily_high_f=92.0,
                    wu_daily_low_f=77.0,
                    wu_daily_high_c=33.3,
                    wu_daily_low_c=25.0,
                    wu_observation_time=_T0,
                    wu_fetched_at=_T1,
                    temperature_day_mode="nws_climate_day",
                    temperature_day_date=date(2026, 7, 13),
                    wu_day_mode="calendar_day",
                    wu_day_date=date(2026, 7, 13),
                ),
            ),
            {
                "LatestObservationData": [],
                "CityInfo": ["city"],
                "StationInfo": ["station"],
                "ObservationRecord": ["observation"],
                "TemperatureUnit": ["station", "temperature_unit"],
                "ReportType": ["observation", "report_type"],
            },
        ),
        _with_coverage(
            _valid(
                "forecast-full",
                "StationForecastData",
                [],
                StationForecastData(
                    city=city,
                    station=station,
                    forecasts=(
                        ForecastBundle(
                            model_id="ncep_hrrr_conus",
                            forecast_run=ForecastBundleRun(
                                id="run-1",
                                fetched_at=_T1,
                                timezone="America/New_York",
                                utc_offset_seconds=-14_400,
                            ),
                            hourly=(hourly,),
                        ),
                    ),
                    count=1,
                ),
            ),
            {
                "StationForecastData": [],
                "ForecastBundle": ["forecasts", 0],
                "ForecastBundleRun": ["forecasts", 0, "forecast_run"],
                "HourlyForecastRecord": ["forecasts", 0, "hourly", 0],
            },
        ),
        _with_coverage(
            _valid(
                "oracle-scores-full",
                "OracleScoreData",
                [],
                OracleScoreData(
                    station_id="KMIA",
                    range_start=date(2026, 6, 1),
                    range_end=date(2026, 7, 12),
                    days_requested=30,
                    all_time=False,
                    score_mode="day_of",
                    rank_by="high",
                    scores=(
                        OracleModelScoreRecord(
                            model_id="ncep_hrrr_conus",
                            model_name="HRRR",
                            is_public=True,
                            high_mae=1.0,
                            low_mae=1.5,
                            high_bias=-0.0,
                            low_bias=-0.25,
                            combined_mae=1.25,
                            day_count=30,
                        ),
                    ),
                ),
            ),
            {
                "OracleScoreData": [],
                "OracleModelScoreRecord": ["scores", 0],
                "OracleRankBy": ["rank_by"],
            },
        ),
        _with_coverage(
            _valid(
                "latest-reports-all-schedules",
                "LatestReportsData",
                [],
                LatestReportsData(
                    reports=(report,),
                    report_schedules={
                        "cli": ReportClockSchedule(
                            basis="local", hour=8, minute=0, local_hour=8, local_minute=0, label="CLI"
                        ),
                        "metar_tgroup": ReportIntervalSchedule(interval_minutes=60, utc_minute=53, label="Hourly"),
                        "dsm": (
                            ReportMultiHourSchedule(
                                utc_hours=(0, 6, 12, 18),
                                local_hours=(1, 7, 13, 19),
                                utc_minute=0,
                                local_minute=0,
                                label="DSM",
                            ),
                        ),
                    },
                ),
            ),
            {
                "LatestReportsData": [],
                "StationReportRecord": ["reports", 0],
                "ReportClockSchedule": ["report_schedules", "cli"],
                "ReportIntervalSchedule": ["report_schedules", "metar_tgroup"],
                "ReportMultiHourSchedule": ["report_schedules", "dsm", 0],
                "ReportSchedule": ["report_schedules", "cli"],
                "ReportScheduleEntry": ["report_schedules", "dsm"],
                "ReportScheduleBasis": ["report_schedules", "cli", "basis"],
            },
        ),
        _with_coverage(
            _valid("reports-page", "StationReportsData", [], StationReportsData(reports=(report,))),
            {"StationReportsData": [], "ReportType": ["reports", 0, "report_type"]},
        ),
        _with_coverage(
            _valid(
                "report-history-page",
                "StationReportHistoryPage",
                [],
                StationReportHistoryPage(
                    city=city,
                    station=station,
                    reports=(report,),
                    count=1,
                    page=CursorPage(limit=100, next_cursor="next-1"),
                    report_schedules={"cli": ReportClockSchedule(utc_hour=12)},
                ),
            ),
            {"StationReportHistoryPage": [], "CursorPage": ["page"]},
        ),
        _with_coverage(
            _valid(
                "forecast-runs-page",
                "ForecastRunsPage",
                [],
                ForecastRunsPage(city=city, station=station, runs=(run,), count=1, page=CursorPage(limit=1)),
            ),
            {"ForecastRunsPage": [], "ForecastRunSummary": ["runs", 0]},
        ),
        _with_coverage(
            _valid(
                "forecast-run-detail",
                "ForecastRunData",
                [],
                ForecastRunData(city=city, station=station, forecast_run=run, hourly=(hourly,), count=1),
            ),
            {"ForecastRunData": []},
        ),
        _with_coverage(
            _valid(
                "limits-full",
                "EffectiveLimits",
                [],
                EffectiveLimits(
                    tier="clanker",
                    requests_per_minute=600,
                    daily_max=100_000,
                    max_history_days=365,
                    ip_guard=IpGuardLimits(requests_per_second=10, burst=20),
                    rate_limit_remaining=599,
                    rate_limit_reset_seconds=1,
                ),
            ),
            {"EffectiveLimits": [], "IpGuardLimits": ["ip_guard"], "PlanTier": ["tier"]},
        ),
        _with_coverage(
            _valid(
                "latest-observation-defaults",
                "LatestObservationData",
                [],
                LatestObservationData(),
                wire={},
            ),
            {"LatestObservationData": []},
        ),
        _with_coverage(
            _valid("forecast-runs-empty-page", "ForecastRunsPage", [], ForecastRunsPage(), wire={}),
            {"ForecastRunsPage": []},
        ),
        _with_coverage(
            _valid("limits-defaults", "EffectiveLimits", [], EffectiveLimits(), wire={}),
            {"EffectiveLimits": []},
        ),
    ]
    enum_values = {
        "DataResolution": ("1m", "5m", "10m"),
        "OracleRankBy": ("combined", "high", "low"),
        "PlanTier": ("starter", "pro", "clanker"),
        "ReportScheduleBasis": ("utc", "local"),
        "ReportType": ("cli", "dsm", "metar_tgroup", "metar_6hr"),
        "TemperatureUnit": ("F", "C"),
    }
    for rust_type, values in enum_values.items():
        valid.extend(_direct_enum_cases(rust_type, values))
    invalid = [
        _invalid("interval-missing-minutes", "ReportIntervalSchedule", "required_field", {"label": "bad"}),
        _invalid("observation-invalid-time", "ObservationRecord", "format", {"observation_time": "bad"}),
        _invalid("limits-invalid-ip-guard", "EffectiveLimits", "type", {"ip_guard": []}),
    ]
    invalid.extend(_invalid(f"unknown-{rust_type}", rust_type, "enum", "unknown") for rust_type in enum_values)
    document = _document("minutetemp", valid, invalid)
    document["wire_policy"]["enum_strings"] = (
        "Literal aliases are exercised through fields that intentionally also accept strings for upstream extension."
    )
    return document


def _kalshi_fixture() -> dict[str, Any]:
    level = KalshiOrderbookLevel(price_dollars="0.6100", count_fp="100.00")
    orderbook = KalshiOrderbook(yes_dollars=(level,), no_dollars=(level,))
    order = KalshiOrder(
        order_id="order-1",
        user_id="user-1",
        client_order_id="client-1",
        ticker="KXHIGHMIA-26JUL13-B90.5",
        side="yes",
        action="buy",
        type="limit",
        status="resting",
        yes_price_dollars="0.6100",
        no_price_dollars="0.3900",
        fill_count_fp="2.00",
        remaining_count_fp="8.00",
        initial_count_fp="10.00",
        taker_fill_cost_dollars="1.2200",
        maker_fill_cost_dollars="0.0000",
        taker_fees_dollars="0.0300",
        maker_fees_dollars="0.0000",
        expiration_time=_T1,
        created_time=_T0,
        last_update_time=_T1,
        self_trade_prevention_type="taker_at_cross",
        order_group_id="group-1",
        cancel_order_on_pause=True,
        subaccount_number=2,
    )
    market = KalshiMarket(
        ticker="KXHIGHMIA-26JUL13-B90.5",
        event_ticker="KXHIGHMIA-26JUL13",
        market_type="binary",
        status="open",
        title="Miami high",
        subtitle="90 to 91",
        yes_sub_title="Yes",
        no_sub_title="No",
        created_time=_T0,
        updated_time=_T1,
        open_time=_T0,
        close_time=_T1,
        latest_expiration_time=_T1,
        expected_expiration_time=_T1,
        expiration_time=_T1,
        settlement_timer_seconds=3_600,
        result="",
        can_close_early=True,
        fractional_trading_enabled=True,
        yes_bid_dollars="0.6000",
        yes_bid_size_fp="100.00",
        yes_ask_dollars="0.6200",
        yes_ask_size_fp="20.00",
        no_bid_dollars="0.3800",
        no_ask_dollars="0.4000",
        last_price_dollars="0.6100",
        volume_fp="1000.00",
        volume_24h_fp="500.00",
        open_interest_fp="750.00",
        dollar_volume=1_000,
        dollar_open_interest=750,
        notional_value_dollars="1.0000",
        liquidity_dollars="500.0000",
        previous_yes_bid_dollars="0.5900",
        previous_yes_ask_dollars="0.6300",
        previous_price_dollars="0.6000",
        expiration_value="91",
        rules_primary="Primary rules",
        rules_secondary="Secondary rules",
        response_price_units="usd_cent",
        settlement_value_dollars="1.0000",
        settlement_ts=_T1,
        fee_waiver_expiration_time=_T1,
        early_close_condition="Official report",
        price_level_structure="deci_cent",
        price_ranges=(KalshiPriceRange(start="0.0000", end="1.0000", step="0.0010"),),
        tick_size=1,
        strike_type="between",
        floor_strike=90,
        cap_strike=91,
        functional_strike="90.5",
        custom_strike={"threshold": 90.5},
        mve_collection_ticker="MVE-1",
        mve_selected_legs=(
            KalshiMveSelectedLeg(
                event_ticker="EVENT-1",
                market_ticker="MARKET-1",
                side="yes",
                yes_settlement_value_dollars="1.0000",
            ),
        ),
        primary_participant_key="participant-1",
        is_provisional=False,
    )

    valid = [
        _with_coverage(
            _valid(
                "create-order-full",
                "KalshiOrderCreateRequest",
                [],
                KalshiOrderCreateRequest(
                    ticker=market.ticker,
                    side="yes",
                    action="buy",
                    client_order_id="client-1",
                    count=10,
                    count_fp="10.00",
                    yes_price=61,
                    no_price=39,
                    yes_price_dollars="0.6100",
                    no_price_dollars="0.3900",
                    expiration_ts=1_786_800_000,
                    time_in_force="fill_or_kill",
                    buy_max_cost=610,
                    post_only=False,
                    reduce_only=False,
                    sell_position_floor=0,
                    self_trade_prevention_type="taker_at_cross",
                    order_group_id="group-1",
                    cancel_order_on_pause=True,
                    subaccount=2,
                ),
            ),
            {
                "KalshiOrderCreateRequest": [],
                "KalshiMarketSide": ["side"],
                "KalshiOrderAction": ["action"],
                "KalshiFixedCount": ["count_fp"],
                "KalshiFixedPrice": ["yes_price_dollars"],
                "KalshiTimeInForce": ["time_in_force"],
                "KalshiImmediateTimeInForce": ["time_in_force"],
                "KalshiSelfTradePreventionType": ["self_trade_prevention_type"],
            },
        ),
        _with_coverage(
            _valid("order-full", "KalshiOrder", [], order),
            {
                "KalshiOrder": [],
                "KalshiOrderType": ["type"],
                "KalshiOrderStatus": ["status"],
            },
        ),
        _with_coverage(
            _valid("create-response", "KalshiCreateOrderResponse", [], KalshiCreateOrderResponse(order=order)),
            {"KalshiCreateOrderResponse": [], "KalshiOrder": ["order"]},
        ),
        _with_coverage(
            _valid("get-order-response", "KalshiGetOrderResponse", [], KalshiGetOrderResponse(order=order)),
            {"KalshiGetOrderResponse": []},
        ),
        _with_coverage(
            _valid(
                "orders-page",
                "KalshiGetOrdersResponse",
                [],
                KalshiGetOrdersResponse(orders=(order,), cursor="next-order"),
            ),
            {"KalshiGetOrdersResponse": []},
        ),
        _with_coverage(
            _valid(
                "orderbook-response",
                "KalshiGetOrderbookResponse",
                [],
                KalshiGetOrderbookResponse(orderbook_fp=orderbook),
            ),
            {
                "KalshiGetOrderbookResponse": [],
                "KalshiOrderbook": ["orderbook_fp"],
                "KalshiOrderbookLevel": ["orderbook_fp", "yes_dollars", 0],
            },
        ),
        _with_coverage(
            _valid(
                "multi-orderbook-response",
                "KalshiGetOrderbooksResponse",
                [],
                KalshiGetOrderbooksResponse(
                    orderbooks=(KalshiMarketOrderbook(ticker=market.ticker, orderbook_fp=orderbook),)
                ),
            ),
            {
                "KalshiGetOrderbooksResponse": [],
                "KalshiMarketOrderbook": ["orderbooks", 0],
            },
        ),
        _with_coverage(
            _valid("market-full", "KalshiMarket", [], market),
            {
                "KalshiMarket": [],
                "KalshiMarketStatus": ["status"],
                "KalshiMarketResult": ["result"],
                "KalshiPriceLevelStructure": ["price_level_structure"],
                "KalshiPriceRange": ["price_ranges", 0],
                "KalshiMveSelectedLeg": ["mve_selected_legs", 0],
            },
        ),
        _with_coverage(
            _valid("get-market-response", "KalshiGetMarketResponse", [], KalshiGetMarketResponse(market=market)),
            {"KalshiGetMarketResponse": []},
        ),
        _with_coverage(
            _valid("markets-page", "KalshiMarketsPage", [], KalshiMarketsPage(markets=(market,), cursor="next")),
            {"KalshiMarketsPage": []},
        ),
        _with_coverage(
            _valid(
                "subscribe",
                "KalshiSubscribeCommand",
                [],
                KalshiSubscribeCommand(
                    id=1,
                    channels=("orderbook_delta", "ticker"),
                    market_ticker=market.ticker,
                    market_tickers=(market.ticker,),
                    market_id="market-1",
                    market_ids=("market-1",),
                ),
            ),
            {"KalshiSubscribeCommand": [], "KalshiWsChannel": ["channels", 0]},
        ),
        _with_coverage(
            _valid("unsubscribe", "KalshiUnsubscribeCommand", [], KalshiUnsubscribeCommand(id=2, sids=(1, 2))),
            {"KalshiUnsubscribeCommand": []},
        ),
        _with_coverage(
            _valid("list-subscriptions", "KalshiListSubscriptionsCommand", [], KalshiListSubscriptionsCommand(id=3)),
            {"KalshiListSubscriptionsCommand": []},
        ),
        _with_coverage(
            _valid(
                "update-subscription",
                "KalshiUpdateSubscriptionCommand",
                [],
                KalshiUpdateSubscriptionCommand(
                    id=4, action="add_markets", market_tickers=(market.ticker,), sid=1, sids=(1,)
                ),
            ),
            {
                "KalshiUpdateSubscriptionCommand": [],
                "KalshiSubscriptionUpdateAction": ["action"],
            },
        ),
        _with_coverage(
            _valid(
                "orderbook-snapshot",
                "KalshiOrderbookSnapshotMessage",
                [],
                KalshiOrderbookSnapshotMessage(
                    sid=1,
                    seq=2,
                    market_ticker=market.ticker,
                    market_id="market-1",
                    yes_dollars_fp=(level,),
                    no_dollars_fp=(level,),
                ),
            ),
            {"KalshiOrderbookSnapshotMessage": []},
        ),
        _with_coverage(
            _valid(
                "orderbook-delta",
                "KalshiOrderbookDeltaMessage",
                [],
                KalshiOrderbookDeltaMessage(
                    sid=1,
                    seq=3,
                    market_ticker=market.ticker,
                    market_id="market-1",
                    price_dollars="0.6200",
                    delta_fp="-2.00",
                    side="yes",
                    client_order_id="client-1",
                    subaccount=2,
                    ts=_T1,
                ),
            ),
            {"KalshiOrderbookDeltaMessage": []},
        ),
        _with_coverage(
            _valid(
                "ticker",
                "KalshiTickerMessage",
                [],
                KalshiTickerMessage(
                    sid=1,
                    market_ticker=market.ticker,
                    market_id="market-1",
                    price_dollars="0.6100",
                    yes_bid_dollars="0.6000",
                    yes_ask_dollars="0.6200",
                    volume_fp="100.00",
                    open_interest_fp="50.00",
                    dollar_volume=100,
                    dollar_open_interest=50,
                    yes_bid_size_fp="10.00",
                    yes_ask_size_fp="12.00",
                    last_trade_size_fp="1.00",
                    ts=1_786_800_000,
                    time=_T1,
                ),
            ),
            {"KalshiTickerMessage": []},
        ),
        _with_coverage(
            _valid(
                "trade",
                "KalshiTradeMessage",
                [],
                KalshiTradeMessage(
                    sid=1,
                    trade_id="trade-1",
                    market_ticker=market.ticker,
                    yes_price_dollars="0.6100",
                    no_price_dollars="0.3900",
                    count_fp="2.00",
                    taker_side="yes",
                    ts=1_786_800_000,
                ),
            ),
            {"KalshiTradeMessage": []},
        ),
        _with_coverage(
            _valid(
                "user-order",
                "KalshiUserOrderMessage",
                [],
                KalshiUserOrderMessage(
                    sid=1,
                    order_id="order-1",
                    user_id="user-1",
                    ticker=market.ticker,
                    status="executed",
                    side="yes",
                    is_yes=True,
                    yes_price_dollars="0.6100",
                    fill_count_fp="10.00",
                    remaining_count_fp="0.00",
                    initial_count_fp="10.00",
                    taker_fill_cost_dollars="6.1000",
                    maker_fill_cost_dollars="0.0000",
                    client_order_id="client-1",
                    order_group_id="group-1",
                    self_trade_prevention_type="maker",
                    created_time=_T0,
                    expiration_time=_T1,
                    subaccount_number=2,
                ),
            ),
            {"KalshiUserOrderMessage": []},
        ),
        _with_coverage(
            _valid(
                "user-fill",
                "KalshiUserFillMessage",
                [],
                KalshiUserFillMessage(
                    sid=1,
                    trade_id="trade-1",
                    order_id="order-1",
                    market_ticker=market.ticker,
                    is_taker=True,
                    side="yes",
                    yes_price_dollars="0.6100",
                    count_fp="2.00",
                    fee_cost="0.0300",
                    action="buy",
                    ts=1_786_800_000,
                    client_order_id="client-1",
                    post_position_fp="2.00",
                    purchased_side="yes",
                    subaccount=2,
                ),
            ),
            {"KalshiUserFillMessage": []},
        ),
        _with_coverage(
            _valid(
                "market-position",
                "KalshiMarketPositionMessage",
                [],
                KalshiMarketPositionMessage(
                    sid=1,
                    user_id="user-1",
                    market_ticker=market.ticker,
                    position_fp="2.00",
                    position_cost_dollars="1.2200",
                    realized_pnl_dollars="0.1000",
                    fees_paid_dollars="0.0300",
                    position_fee_cost_dollars="0.0300",
                    volume_fp="2.00",
                ),
            ),
            {"KalshiMarketPositionMessage": []},
        ),
        _with_coverage(
            _valid(
                "market-lifecycle",
                "KalshiMarketLifecycleMessage",
                [],
                KalshiMarketLifecycleMessage(
                    sid=1,
                    event_type="created",
                    market_ticker=market.ticker,
                    open_ts=1_786_700_000,
                    close_ts=1_786_800_000,
                    result="yes",
                    determination_ts=1_786_900_000,
                    settlement_value="1.0000",
                    settled_ts=1_786_900_001,
                    is_deactivated=False,
                    fractional_trading_enabled=True,
                    price_level_structure="linear_cent",
                    additional_metadata=KalshiMarketLifecycleMetadata(
                        name="Miami high",
                        title="Miami high",
                        yes_sub_title="Yes",
                        no_sub_title="No",
                        rules_primary="Primary",
                        rules_secondary="Secondary",
                        can_close_early=True,
                        event_ticker=market.event_ticker,
                        expected_expiration_ts=1_786_800_000,
                        strike_type="between",
                        floor_strike=90.0,
                        cap_strike=91.0,
                        custom_strike={"threshold": 90.5},
                    ),
                ),
            ),
            {
                "KalshiMarketLifecycleMessage": [],
                "KalshiMarketLifecycleEventType": ["event_type"],
                "KalshiMarketLifecycleMetadata": ["additional_metadata"],
            },
        ),
        _with_coverage(
            _valid(
                "event-lifecycle",
                "KalshiEventLifecycleMessage",
                [],
                KalshiEventLifecycleMessage(
                    sid=1,
                    event_ticker=market.event_ticker,
                    title="Miami high",
                    subtitle="July 13",
                    collateral_return_type="MECNET",
                    series_ticker="KXHIGHMIA",
                    strike_date=20_260_713,
                    strike_period="day",
                ),
            ),
            {
                "KalshiEventLifecycleMessage": [],
                "KalshiCollateralReturnType": ["collateral_return_type"],
            },
        ),
        _with_coverage(
            _valid(
                "websocket-union",
                "KalshiWsMessage",
                [],
                KalshiOrderbookDeltaMessage(
                    sid=1,
                    seq=3,
                    market_ticker=market.ticker,
                    market_id="market-1",
                    price_dollars="0.6200",
                    delta_fp="1.00",
                    side="yes",
                ),
            ),
            {"KalshiWsMessage": []},
        ),
        _with_coverage(
            _valid("order-defaults", "KalshiOrder", [], KalshiOrder()),
            {"KalshiOrder": []},
        ),
        _with_coverage(
            _valid("orders-empty-page", "KalshiGetOrdersResponse", [], KalshiGetOrdersResponse()),
            {"KalshiGetOrdersResponse": []},
        ),
        _with_coverage(
            _valid("markets-empty-page", "KalshiMarketsPage", [], KalshiMarketsPage()),
            {"KalshiMarketsPage": []},
        ),
        _with_coverage(
            _valid(
                "forward-compatible-enum-string",
                "KalshiOrderCreateRequest",
                [],
                KalshiOrderCreateRequest(ticker="future-market", side="future-side", action="future-action"),
            ),
            {"KalshiOrderCreateRequest": []},
        ),
    ]
    enum_values = {
        "KalshiCollateralReturnType": ("MECNET", "DIRECNET", ""),
        "KalshiImmediateTimeInForce": ("fill_or_kill", "immediate_or_cancel"),
        "KalshiMarketLifecycleEventType": (
            "created",
            "deactivated",
            "activated",
            "close_date_updated",
            "determined",
            "settled",
            "fractional_trading_updated",
            "price_level_structure_updated",
        ),
        "KalshiMarketResult": ("yes", "no", "scalar", ""),
        "KalshiMarketSide": ("yes", "no"),
        "KalshiMarketStatus": ("unopened", "open", "paused", "closed", "settled"),
        "KalshiOrderAction": ("buy", "sell"),
        "KalshiOrderStatus": ("resting", "canceled", "executed"),
        "KalshiOrderType": ("limit",),
        "KalshiPriceLevelStructure": ("linear_cent", "deci_cent", "tapered_deci_cent"),
        "KalshiSelfTradePreventionType": ("taker_at_cross", "maker"),
        "KalshiSubscriptionUpdateAction": ("add_markets", "delete_markets"),
        "KalshiTimeInForce": ("fill_or_kill", "good_till_canceled", "immediate_or_cancel"),
        "KalshiWsChannel": (
            "orderbook_delta",
            "ticker",
            "trade",
            "fill",
            "market_positions",
            "market_lifecycle_v2",
            "multivariate_market_lifecycle",
            "multivariate",
            "communications",
            "order_group_updates",
            "user_orders",
        ),
    }
    for rust_type, values in enum_values.items():
        valid.extend(_direct_enum_cases(rust_type, values))
    invalid = [
        _invalid(
            "create-order-missing-ticker",
            "KalshiOrderCreateRequest",
            "required_field",
            {"side": "yes", "action": "buy"},
        ),
        _invalid("orderbook-missing-required", "KalshiGetOrderbookResponse", "required_field", {}),
        _invalid(
            "snapshot-bad-level",
            "KalshiOrderbookSnapshotMessage",
            "type",
            {"sid": 1, "seq": 2, "market_ticker": "M", "market_id": "1", "yes_dollars_fp": [1]},
        ),
        _invalid("unrecognized-websocket-shape", "KalshiWsMessage", "type", {"sid": 1}),
    ]
    invalid.extend(_invalid(f"unknown-{rust_type}", rust_type, "enum", "unknown") for rust_type in enum_values)
    document = _document("kalshi", valid, invalid)
    document["wire_policy"]["enum_strings"] = (
        "Fields annotated as Literal | str intentionally retain unknown upstream strings; required shapes still reject."
    )
    return document


def _http_data_fixture() -> dict[str, Any]:
    valid = [
        _with_coverage(
            _valid(
                "http-request-full",
                "HttpRequest",
                [],
                HttpRequest(
                    method="POST",
                    url="https://example.test/v1/data",
                    headers={"authorization": "Bearer redacted"},
                    params={"limit": 10, "refresh": True, "cursor": None, "ratio": 0.5},
                    json_body={"station": "KMIA", "models": ["hrrr"]},
                    text_body="body",
                    timeout_seconds=2.5,
                ),
            ),
            {"HttpRequest": [], "HttpMethod": ["method"]},
        ),
        _with_coverage(
            _valid(
                "http-response-full",
                "HttpResponse",
                [],
                HttpResponse(
                    status_code=207,
                    headers={"content-type": "application/json"},
                    text="ok",
                    json_body={"items": [1, True, None]},
                ),
            ),
            {"HttpResponse": []},
        ),
        _with_coverage(
            _valid(
                "http-request-defaults",
                "HttpRequest",
                [],
                HttpRequest(method="GET", url="https://example.test/health"),
            ),
            {"HttpRequest": []},
        ),
        _with_coverage(
            _valid("http-response-defaults", "HttpResponse", [], HttpResponse(status_code=204)),
            {"HttpResponse": []},
        ),
    ]
    valid.extend(_direct_enum_cases("HttpMethod", ("GET", "POST", "PUT", "PATCH", "DELETE")))
    invalid = [
        _invalid("invalid-http-method", "HttpMethod", "enum", "TRACE"),
        _invalid("request-missing-url", "HttpRequest", "required_field", {"method": "GET"}),
        _invalid("response-invalid-status", "HttpResponse", "type", {"status_code": []}),
    ]
    document = _document("http-data", valid, invalid)
    document["wire_policy"]["enum_strings"] = "HttpMethod is a closed literal/enum and rejects unknown methods."
    return document


def _special_float(value: object) -> object:
    if isinstance(value, dict):
        non_finite = value.get("non_finite")
        if non_finite in {"nan", "inf", "-inf"}:
            return float(cast("str", non_finite))
    return value


def _helper_case(
    case_id: str,
    helper: str,
    covers: list[str],
    input_value: dict[str, Any],
) -> dict[str, Any]:
    case = {"id": case_id, "helper": helper, "covers": covers, "input": input_value}
    case["expected"] = evaluate_python_helper_case(case)
    dimensions = {"helper_or_trait_behavior"}
    if case_id in {
        "fee-rounding-boundary",
        "fee-rounding-signed",
        "fee-negative-multiplier",
        "fee-negative-quantity",
        "fill-fee-sell",
        "fee-invalid-decimal",
        "fee-rounding-invalid-revenue",
        "fee-rounding-invalid-trade-fee",
        "fee-rounding-invalid-accumulator",
        "fee-positive-infinity",
        "fee-negative-infinity",
        "fee-invalid-multiplier",
        "fee-invalid-multiplier-and-type",
        "fill-fee-invalid-price",
        "fill-fee-invalid-accumulator",
        "fill-fee-invalid-multiplier",
        "fill-fee-invalid-price-and-role",
    }:
        dimensions.add("numeric_boundaries")
    if "error" in case["expected"]:
        dimensions.add("invalid_input")
    if helper == "climate_day_end":
        dimensions.add("timestamp_formatting")
    case["evidence_dimensions"] = sorted(dimensions)
    case["evidence"] = {surface: sorted(dimensions) for surface in covers}
    return case


def evaluate_python_helper_case(case: Mapping[str, Any]) -> dict[str, Any]:
    """Evaluate one portable vector and normalize its result or error."""

    helper = cast("str", case["helper"])
    inputs = cast("dict[str, Any]", case["input"])
    try:
        if helper == "calculate_trade_fee":
            result: object = calculate_trade_fee(
                price=cast("float", _special_float(inputs["price"])),
                quantity=inputs["quantity"],
                liquidity_role=inputs["liquidity_role"],
                fee_type=inputs.get("fee_type"),
                fee_multiplier=cast("float | None", _special_float(inputs.get("fee_multiplier"))),
            )
        elif helper == "apply_fee_rounding":
            result = apply_fee_rounding(
                revenue=cast("float", _special_float(inputs["revenue"])),
                trade_fee=cast("float", _special_float(inputs["trade_fee"])),
                fee_accumulator=cast("float", _special_float(inputs["fee_accumulator"])),
            )
        elif helper == "calculate_fill_fee":
            result = calculate_fill_fee(
                action=inputs["action"],
                price=cast("float", _special_float(inputs["price"])),
                quantity=inputs["quantity"],
                liquidity_role=inputs["liquidity_role"],
                fee_accumulator=cast("float", _special_float(inputs["fee_accumulator"])),
                fee_type=inputs.get("fee_type"),
                fee_multiplier=cast("float | None", _special_float(inputs.get("fee_multiplier"))),
            )
        elif helper == "station_constants":
            result = {
                "icao_to_city_codes": ICAO_TO_CITY_CODES,
                "city_to_icao": CITY_TO_ICAO,
                "station_timezones": STATION_TIMEZONES,
                "market_type_prefix": MARKET_TYPE_PREFIX,
                "ticker_prefixes": TICKER_PREFIXES,
            }
        elif helper == "signal_constants":
            result = {
                "dsm_reaction": SIGNAL_DSM_REACTION,
                "metar_6hr_low": SIGNAL_METAR_6HR_LOW,
                "metar_6hr_new_low": SIGNAL_METAR_6HR_NEW_LOW,
            }
        elif helper == "primary_city_code_for_series":
            result = primary_city_code_for_series(inputs["station"])
        elif helper == "city_codes_for_market_type":
            result = city_codes_for_market_type(inputs["station"], inputs["market_type"])
        elif helper == "primary_city_code_for_market_type":
            result = primary_city_code_for_market_type(inputs["station"], inputs["market_type"])
        elif helper == "ticker_prefixes_for_station":
            result = ticker_prefixes_for_station(inputs["station"], inputs["market_type"])
        elif helper == "station_from_event_ticker":
            result = station_from_event_ticker(inputs["event_ticker"])
        elif helper == "station_timezone":
            result = station_timezone(inputs.get("station"), station_timezones=inputs.get("station_timezones")).key
        elif helper == "parse_climate_date":
            parsed = parse_climate_date(inputs.get("raw"))
            result = parsed.isoformat() if parsed is not None else None
        elif helper == "climate_day_date":
            result = climate_day_date(
                inputs.get("station"),
                datetime.fromisoformat(inputs["now"].replace("Z", "+00:00")),
                station_timezones=inputs.get("station_timezones"),
            ).isoformat()
        elif helper == "climate_day_end":
            result = (
                climate_day_end(
                    inputs.get("station"),
                    date.fromisoformat(inputs["event_date"]),
                    station_timezones=inputs.get("station_timezones"),
                )
                .isoformat()
                .replace("+00:00", "Z")
            )
        elif helper == "climate_day_has_ended":
            result = climate_day_has_ended(
                inputs.get("station"),
                date.fromisoformat(inputs["event_date"]),
                datetime.fromisoformat(inputs["now"].replace("Z", "+00:00")),
                station_timezones=inputs.get("station_timezones"),
            )
        else:
            raise AssertionError(f"unknown helper vector: {helper}")
    except ValueError as error:
        message = str(error)
        if "invalid decimal value" in message:
            return {"error": "invalid_decimal"}
        if "unknown Kalshi fee type" in message:
            return {"error": "unknown_fee_type"}
        if "unknown order action" in message:
            return {"error": "unknown_action"}
        if "unknown liquidity role" in message:
            return {"error": "unknown_liquidity_role"}
        if "unknown market_type" in message:
            return {"error": "unknown_market_type"}
        if "timezone" in message:
            return {"error": "timezone"}
        return {"error": "value"}
    return {"ok": _json_value(result)}


def _helpers_fixture() -> dict[str, Any]:
    cases = [
        _helper_case(
            "fee-taker-default",
            "calculate_trade_fee",
            ["calculate_trade_fee", "LiquidityRole"],
            {"price": 0.5, "quantity": 100, "liquidity_role": "taker"},
        ),
        _helper_case(
            "fee-maker-quadratic",
            "calculate_trade_fee",
            ["calculate_trade_fee"],
            {"price": 0.25, "quantity": 10, "liquidity_role": "maker", "fee_type": "quadratic"},
        ),
        _helper_case(
            "fee-flat-multiplier",
            "calculate_trade_fee",
            ["calculate_trade_fee"],
            {"price": 0.3, "quantity": 100, "liquidity_role": "taker", "fee_type": "flat", "fee_multiplier": 2.0},
        ),
        _helper_case(
            "fee-rounding-boundary",
            "apply_fee_rounding",
            ["apply_fee_rounding", "FeeCalculation"],
            {"revenue": -0.055, "trade_fee": 0.0085, "fee_accumulator": 0.0065},
        ),
        _helper_case(
            "fee-negative-quantity",
            "calculate_trade_fee",
            ["calculate_trade_fee"],
            {"price": 0.25, "quantity": -1, "liquidity_role": "taker"},
        ),
        _helper_case(
            "fee-negative-multiplier",
            "calculate_trade_fee",
            ["calculate_trade_fee"],
            {
                "price": 0.25,
                "quantity": 1,
                "liquidity_role": "taker",
                "fee_multiplier": -1.0,
            },
        ),
        _helper_case(
            "fee-rounding-signed",
            "apply_fee_rounding",
            ["apply_fee_rounding", "FeeCalculation"],
            {"revenue": -0.055, "trade_fee": -0.00851, "fee_accumulator": -0.0065},
        ),
        _helper_case(
            "fill-fee-sell",
            "calculate_fill_fee",
            ["calculate_fill_fee"],
            {
                "action": "sell",
                "price": 0.9,
                "quantity": 3,
                "liquidity_role": "taker",
                "fee_accumulator": 0.0099,
                "fee_type": "quadratic_with_maker_fees",
                "fee_multiplier": 1.0,
            },
        ),
        _helper_case(
            "fee-unknown-type",
            "calculate_trade_fee",
            ["calculate_trade_fee"],
            {"price": 0.5, "quantity": 1, "liquidity_role": "taker", "fee_type": "unknown"},
        ),
        _helper_case(
            "fee-unknown-liquidity-role",
            "calculate_trade_fee",
            ["calculate_trade_fee", "LiquidityRole"],
            {"price": 0.5, "quantity": 1, "liquidity_role": "passive"},
        ),
        _helper_case(
            "fee-invalid-role-and-type",
            "calculate_trade_fee",
            ["calculate_trade_fee", "LiquidityRole"],
            {
                "price": 0.5,
                "quantity": 1,
                "liquidity_role": "passive",
                "fee_type": "unknown",
            },
        ),
        _helper_case(
            "fill-fee-unknown-action",
            "calculate_fill_fee",
            ["calculate_fill_fee"],
            {
                "action": "hold",
                "price": 0.5,
                "quantity": 1,
                "liquidity_role": "taker",
                "fee_accumulator": 0.0,
            },
        ),
        _helper_case(
            "fill-fee-multiple-invalid-literals",
            "calculate_fill_fee",
            ["calculate_fill_fee"],
            {
                "action": "hold",
                "price": 0.5,
                "quantity": 1,
                "liquidity_role": "passive",
                "fee_accumulator": 0.0,
                "fee_type": "unknown",
            },
        ),
        _helper_case(
            "fee-invalid-decimal",
            "calculate_trade_fee",
            ["calculate_trade_fee"],
            {"price": {"non_finite": "nan"}, "quantity": 1, "liquidity_role": "taker"},
        ),
        _helper_case(
            "fee-positive-infinity",
            "calculate_trade_fee",
            ["calculate_trade_fee"],
            {"price": {"non_finite": "inf"}, "quantity": 1, "liquidity_role": "taker"},
        ),
        _helper_case(
            "fee-negative-infinity",
            "calculate_trade_fee",
            ["calculate_trade_fee"],
            {"price": {"non_finite": "-inf"}, "quantity": 1, "liquidity_role": "taker"},
        ),
        _helper_case(
            "fee-invalid-multiplier",
            "calculate_trade_fee",
            ["calculate_trade_fee"],
            {
                "price": 0.5,
                "quantity": 1,
                "liquidity_role": "taker",
                "fee_multiplier": {"non_finite": "nan"},
            },
        ),
        _helper_case(
            "fee-invalid-multiplier-and-type",
            "calculate_trade_fee",
            ["calculate_trade_fee"],
            {
                "price": 0.5,
                "quantity": 1,
                "liquidity_role": "taker",
                "fee_type": "unknown",
                "fee_multiplier": {"non_finite": "nan"},
            },
        ),
        _helper_case(
            "fill-fee-invalid-price",
            "calculate_fill_fee",
            ["calculate_fill_fee"],
            {
                "action": "buy",
                "price": {"non_finite": "nan"},
                "quantity": 1,
                "liquidity_role": "taker",
                "fee_accumulator": 0.0,
            },
        ),
        _helper_case(
            "fill-fee-invalid-accumulator",
            "calculate_fill_fee",
            ["calculate_fill_fee"],
            {
                "action": "buy",
                "price": 0.5,
                "quantity": 1,
                "liquidity_role": "taker",
                "fee_accumulator": {"non_finite": "inf"},
            },
        ),
        _helper_case(
            "fill-fee-invalid-multiplier",
            "calculate_fill_fee",
            ["calculate_fill_fee"],
            {
                "action": "buy",
                "price": 0.5,
                "quantity": 1,
                "liquidity_role": "taker",
                "fee_accumulator": 0.0,
                "fee_multiplier": {"non_finite": "-inf"},
            },
        ),
        _helper_case(
            "fill-fee-invalid-price-and-role",
            "calculate_fill_fee",
            ["calculate_fill_fee", "LiquidityRole"],
            {
                "action": "buy",
                "price": {"non_finite": "nan"},
                "quantity": 1,
                "liquidity_role": "passive",
                "fee_accumulator": 0.0,
            },
        ),
        _helper_case(
            "fee-rounding-invalid-revenue",
            "apply_fee_rounding",
            ["apply_fee_rounding", "FeeCalculation"],
            {"revenue": {"non_finite": "nan"}, "trade_fee": 0.0, "fee_accumulator": 0.0},
        ),
        _helper_case(
            "fee-rounding-invalid-trade-fee",
            "apply_fee_rounding",
            ["apply_fee_rounding", "FeeCalculation"],
            {"revenue": 0.0, "trade_fee": {"non_finite": "inf"}, "fee_accumulator": 0.0},
        ),
        _helper_case(
            "fee-rounding-invalid-accumulator",
            "apply_fee_rounding",
            ["apply_fee_rounding", "FeeCalculation"],
            {"revenue": 0.0, "trade_fee": 0.0, "fee_accumulator": {"non_finite": "-inf"}},
        ),
        _helper_case(
            "station-constants",
            "station_constants",
            ["CITY_TO_ICAO", "ICAO_TO_CITY_CODES", "MARKET_TYPE_PREFIX", "STATION_TIMEZONES", "TICKER_PREFIXES"],
            {},
        ),
        _helper_case(
            "signal-constants",
            "signal_constants",
            ["SIGNAL_DSM_REACTION", "SIGNAL_METAR_6HR_LOW", "SIGNAL_METAR_6HR_NEW_LOW"],
            {},
        ),
        _helper_case(
            "primary-city-known", "primary_city_code_for_series", ["primary_city_code_for_series"], {"station": "kdfw"}
        ),
        _helper_case(
            "primary-city-fallback",
            "primary_city_code_for_series",
            ["primary_city_code_for_series"],
            {"station": "KXYZ"},
        ),
        _helper_case(
            "market-city-high",
            "city_codes_for_market_type",
            ["city_codes_for_market_type"],
            {"station": "KMDW", "market_type": "high"},
        ),
        _helper_case(
            "market-city-low",
            "city_codes_for_market_type",
            ["city_codes_for_market_type"],
            {"station": "KDFW", "market_type": "low"},
        ),
        _helper_case(
            "primary-market-city",
            "primary_city_code_for_market_type",
            ["primary_city_code_for_market_type"],
            {"station": "KDFW", "market_type": "low"},
        ),
        _helper_case(
            "ticker-prefixes",
            "ticker_prefixes_for_station",
            ["ticker_prefixes_for_station"],
            {"station": "KMIA", "market_type": "high"},
        ),
        _helper_case(
            "ticker-prefixes-invalid",
            "ticker_prefixes_for_station",
            ["ticker_prefixes_for_station"],
            {"station": "KMIA", "market_type": "invalid"},
        ),
        _helper_case(
            "ticker-station-known",
            "station_from_event_ticker",
            ["station_from_event_ticker"],
            {"event_ticker": "kxlowtdal-260713"},
        ),
        _helper_case(
            "ticker-station-unknown",
            "station_from_event_ticker",
            ["station_from_event_ticker"],
            {"event_ticker": "OTHER-260713"},
        ),
        _helper_case("station-timezone-known", "station_timezone", ["station_timezone"], {"station": "KMIA"}),
        _helper_case("station-timezone-default", "station_timezone", ["station_timezone"], {"station": None}),
        _helper_case(
            "station-timezone-custom",
            "station_timezone",
            ["station_timezone"],
            {"station": "EGLL", "station_timezones": {"EGLL": "Europe/London"}},
        ),
        _helper_case("station-timezone-unknown", "station_timezone", ["station_timezone"], {"station": "EGLL"}),
        _helper_case("parse-date-iso", "parse_climate_date", ["parse_climate_date"], {"raw": "2026-07-13"}),
        _helper_case("parse-date-compact", "parse_climate_date", ["parse_climate_date"], {"raw": "20260713"}),
        _helper_case("parse-date-short", "parse_climate_date", ["parse_climate_date"], {"raw": "260713"}),
        _helper_case("parse-date-invalid", "parse_climate_date", ["parse_climate_date"], {"raw": "2026-99-99"}),
        _helper_case(
            "climate-date-before-standard-midnight",
            "climate_day_date",
            ["climate_day_date"],
            {"station": "KMIA", "now": "2026-07-13T04:59:59Z"},
        ),
        _helper_case(
            "climate-date-after-standard-midnight",
            "climate_day_date",
            ["climate_day_date"],
            {"station": "KMIA", "now": "2026-07-13T05:00:00Z"},
        ),
        _helper_case(
            "climate-date-unknown-station",
            "climate_day_date",
            ["climate_day_date"],
            {"station": "EGLL", "now": "2026-07-13T05:00:00Z"},
        ),
        _helper_case(
            "climate-day-end-dst",
            "climate_day_end",
            ["climate_day_end"],
            {"station": "KMIA", "event_date": "2026-07-13"},
        ),
        _helper_case(
            "climate-day-end-unknown-station",
            "climate_day_end",
            ["climate_day_end"],
            {"station": "EGLL", "event_date": "2026-07-13"},
        ),
        _helper_case(
            "climate-day-open",
            "climate_day_has_ended",
            ["climate_day_has_ended"],
            {"station": "KMIA", "event_date": "2026-07-13", "now": "2026-07-14T04:59:59Z"},
        ),
        _helper_case(
            "climate-day-ended",
            "climate_day_has_ended",
            ["climate_day_has_ended"],
            {"station": "KMIA", "event_date": "2026-07-13", "now": "2026-07-14T05:00:00Z"},
        ),
        _helper_case(
            "climate-day-ended-unknown-station",
            "climate_day_has_ended",
            ["climate_day_has_ended"],
            {"station": "EGLL", "event_date": "2026-07-13", "now": "2026-07-14T05:00:00Z"},
        ),
    ]
    return {
        "schema_version": 1,
        "family": "helpers",
        "authority": "python",
        "comparison": "result-or-normalized-error",
        "cases": cases,
    }


def build_external_fixtures() -> dict[str, dict[str, Any]]:
    """Build the canonical external-model and helper corpus."""

    documents = {
        "helpers": _helpers_fixture(),
        "minutetemp": _minute_temp_fixture(),
        "kalshi": _kalshi_fixture(),
        "http-data": _http_data_fixture(),
    }
    direct_valid, direct_invalid = direct_model_cases(EXTERNAL_DIRECT_MODELS)
    documents["minutetemp"]["valid"].extend(
        _with_coverage(case, {cast("str", case["rust_type"]): []}) for case in direct_valid
    )
    documents["minutetemp"]["invalid"].extend(direct_invalid)
    return documents


def load_external_fixture(name: str) -> dict[str, Any]:
    """Load one checked-in fixture without mutating it."""

    if name not in EXTERNAL_FIXTURE_NAMES:
        raise ValueError(f"unknown external conformance fixture: {name}")
    return cast("dict[str, Any]", json.loads((FIXTURE_ROOT / f"{name}.json").read_text()))


_PYTHON_EXTERNAL_TYPES: dict[str, object] = {
    "CityInfo": CityInfo,
    "DataResolution": DataResolution,
    "EffectiveLimits": EffectiveLimits,
    "ForecastRunData": ForecastRunData,
    "ForecastRunsPage": ForecastRunsPage,
    "HttpMethod": HttpMethod,
    "HttpRequest": HttpRequest,
    "HttpResponse": HttpResponse,
    "KalshiCollateralReturnType": KalshiCollateralReturnType,
    "KalshiCreateOrderResponse": KalshiCreateOrderResponse,
    "KalshiEventLifecycleMessage": KalshiEventLifecycleMessage,
    "KalshiGetMarketResponse": KalshiGetMarketResponse,
    "KalshiGetOrderResponse": KalshiGetOrderResponse,
    "KalshiGetOrderbookResponse": KalshiGetOrderbookResponse,
    "KalshiGetOrderbooksResponse": KalshiGetOrderbooksResponse,
    "KalshiGetOrdersResponse": KalshiGetOrdersResponse,
    "KalshiListSubscriptionsCommand": KalshiListSubscriptionsCommand,
    "KalshiMarket": KalshiMarket,
    "KalshiMarketLifecycleEventType": KalshiMarketLifecycleEventType,
    "KalshiMarketLifecycleMessage": KalshiMarketLifecycleMessage,
    "KalshiMarketPositionMessage": KalshiMarketPositionMessage,
    "KalshiMarketResult": KalshiMarketResult,
    "KalshiMarketSide": KalshiMarketSide,
    "KalshiMarketStatus": KalshiMarketStatus,
    "KalshiMarketsPage": KalshiMarketsPage,
    "KalshiOrder": KalshiOrder,
    "KalshiOrderAction": KalshiOrderAction,
    "KalshiOrderCreateRequest": KalshiOrderCreateRequest,
    "KalshiOrderStatus": KalshiOrderStatus,
    "KalshiOrderType": KalshiOrderType,
    "KalshiOrderbookDeltaMessage": KalshiOrderbookDeltaMessage,
    "KalshiOrderbookSnapshotMessage": KalshiOrderbookSnapshotMessage,
    "KalshiPriceLevelStructure": KalshiPriceLevelStructure,
    "KalshiSelfTradePreventionType": KalshiSelfTradePreventionType,
    "KalshiSubscribeCommand": KalshiSubscribeCommand,
    "KalshiSubscriptionUpdateAction": KalshiSubscriptionUpdateAction,
    "KalshiTickerMessage": KalshiTickerMessage,
    "KalshiTimeInForce": KalshiTimeInForce,
    "KalshiTradeMessage": KalshiTradeMessage,
    "KalshiImmediateTimeInForce": KalshiImmediateTimeInForce,
    "KalshiUnsubscribeCommand": KalshiUnsubscribeCommand,
    "KalshiUpdateSubscriptionCommand": KalshiUpdateSubscriptionCommand,
    "KalshiUserFillMessage": KalshiUserFillMessage,
    "KalshiUserOrderMessage": KalshiUserOrderMessage,
    "KalshiWsChannel": KalshiWsChannel,
    "KalshiWsMessage": KalshiWsMessage,
    "LatestObservationData": LatestObservationData,
    "LatestReportsData": LatestReportsData,
    "ObservationRecord": ObservationRecord,
    "OracleRankBy": OracleRankBy,
    "OracleScoreData": OracleScoreData,
    "PlanTier": PlanTier,
    "ReportIntervalSchedule": ReportIntervalSchedule,
    "ReportScheduleBasis": ReportScheduleBasis,
    "ReportType": ReportType,
    "StationForecastData": StationForecastData,
    "StationReportHistoryPage": StationReportHistoryPage,
    "StationReportsData": StationReportsData,
    "TemperatureUnit": TemperatureUnit,
}
_PYTHON_EXTERNAL_TYPES.update(EXTERNAL_DIRECT_MODELS)
_TYPE_NAMESPACE = {"JSONValue": JSONValue, "Mapping": collections.abc.Mapping}


def _external_wire_adapter(rust_type: str) -> TypeAdapter[Any]:
    adapter: TypeAdapter[Any] = TypeAdapter(_PYTHON_EXTERNAL_TYPES[rust_type])
    adapter.rebuild(_types_namespace={**globals(), **_TYPE_NAMESPACE})
    return adapter


def python_external_round_trip_valid_case(case: Mapping[str, Any]) -> Any:
    """Decode and canonicalize an external valid case through Python."""

    return _json_value(_validate_wire(_external_wire_adapter(cast("str", case["rust_type"])), case["wire"]))


def python_external_invalid_category(case: Mapping[str, Any]) -> str | None:
    """Return Python's normalized rejection category for an invalid external case."""

    try:
        _validate_wire(_external_wire_adapter(cast("str", case["rust_type"])), case["wire"])
    except ValidationError as error:
        if case["rust_type"] == "KalshiWsMessage":
            return "type"
        return _validation_category(error)
    return None


def write_external_fixtures() -> None:
    """Explicitly regenerate checked-in fixtures after an intentional update."""

    for name, document in build_external_fixtures().items():
        path = FIXTURE_ROOT / f"{name}.json"
        path.write_text(f"{json.dumps(document, indent=2, sort_keys=True)}\n")


if __name__ == "__main__":
    write_external_fixtures()
