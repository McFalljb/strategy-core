"""Python-authored cases for the broad Rust conformance corpus.

The checked-in JSON files are build artifacts of these Python contract objects. Normal tests call
``build_core_fixtures`` and compare the result with disk; they never rewrite fixtures.
"""

from __future__ import annotations

import json
from collections.abc import Mapping
from dataclasses import fields, is_dataclass
from datetime import UTC, date, datetime
from enum import Enum
from pathlib import Path
from typing import Any, cast

from pydantic import BaseModel, TypeAdapter, ValidationError

from strategy_core.broker import (
    BrokerOrderUpdate,
    OrderIntent,
    OrderResult,
    PendingOrder,
    Position,
)
from strategy_core.capabilities import RuntimeCapabilities
from strategy_core.events import (
    ForecastUpdated,
    ForecastVersions,
    MarketBracket,
    NewHigh,
    NewLow,
    Observation,
    OracleScoreRow,
    OracleScoresUpdated,
    OracleScoreTable,
    PriceUpdate,
    ShutdownEvent,
    StationReport,
    StrategyEvent,
    TimerWake,
    WeatherEvent,
    WeatherEventSource,
)
from strategy_core.minutetemp import OracleRankBy, OracleScoreMode, ReportType, TemperatureDayMode
from strategy_core.models import JSONValue, OrderId
from strategy_core.native import NativeKernelResult
from strategy_core.queries import (
    ForecastQuery,
    ForecastRunQuery,
    ForecastRunsQuery,
    LatestObservationQuery,
    LatestReportsQuery,
    LimitsQuery,
    OracleScoresQuery,
    ReportHistoryQuery,
    ReportsQuery,
)
from strategy_core.runtime import RuntimeMode, StrategyScope
from strategy_core.state import (
    ForecastHourly,
    FreshnessDomain,
    FreshnessDomainSummary,
    FreshnessSnapshot,
    FreshnessStatus,
    FreshnessSummary,
    ModelForecast,
    OracleModelScore,
    StationForecast,
    StationOracleScores,
    StationWeather,
    TickerPrices,
)

FIXTURE_ROOT = Path(__file__).parent / "fixtures" / "conformance"
CORE_FIXTURE_NAMES = ("events", "state", "broker", "runtime", "queries")

_UTC = UTC
_T0 = datetime(2026, 7, 13, 12, 34, 56, 123456, tzinfo=_UTC)
_T1 = datetime(2026, 7, 13, 12, 35, 1, 987654, tzinfo=_UTC)


def _json_value(value: object) -> Any:
    if isinstance(value, BaseModel):
        return value.model_dump(mode="json")
    if isinstance(value, Enum):
        return value.value
    if isinstance(value, datetime):
        return value.isoformat().replace("+00:00", "Z")
    if isinstance(value, date):
        return value.isoformat()
    if is_dataclass(value) and not isinstance(value, type):
        return {item.name: _json_value(getattr(value, item.name)) for item in fields(value)}
    if isinstance(value, Mapping):
        return {str(key): _json_value(item) for key, item in value.items()}
    if isinstance(value, (tuple, list)):
        return [_json_value(item) for item in value]
    return value


def _valid(
    case_id: str,
    rust_type: str,
    covers: list[str],
    value: object,
    *,
    wire: object | None = None,
) -> dict[str, Any]:
    expected = _json_value(value)
    return {
        "id": case_id,
        "rust_type": rust_type,
        "covers": covers,
        "wire": expected if wire is None else _json_value(wire),
        "expected": expected,
    }


def _raw(case_id: str, rust_type: str, covers: list[str], value: object) -> dict[str, Any]:
    return _valid(case_id, rust_type, covers, value)


def _invalid(
    case_id: str,
    rust_type: str,
    category: str,
    value: object,
) -> dict[str, Any]:
    return {
        "id": case_id,
        "rust_type": rust_type,
        "category": category,
        "wire": _json_value(value),
    }


def _document(
    family: str,
    valid: list[dict[str, Any]],
    invalid: list[dict[str, Any]],
) -> dict[str, Any]:
    return {
        "schema_version": 1,
        "family": family,
        "authority": "python",
        "comparison": "structural-json",
        "wire_policy": {
            "unknown_object_fields": "ignored and absent from canonical output",
            "non_finite_numbers": "excluded because they are not JSON-compatible values",
            "portable_integer_domain": "signed 64-bit",
            "optional_fields": "omitted inputs resolve through defaults; explicit null is retained when nullable",
        },
        "valid": valid,
        "invalid": invalid,
    }


def _events_fixture() -> dict[str, Any]:
    score_table = OracleScoreTable(
        station_id="KMIA",
        range_start="2026-06-01",
        range_end="2026-07-12",
        days_requested=30,
        all_time=False,
        score_mode="day_of",
        rank_by="high",
        scores=[
            OracleScoreRow(
                model_id="ncep_hrrr_conus",
                model_name="HRRR",
                is_public=True,
                combined_mae=1.25,
                high_mae=1.0,
                low_mae=1.5,
                high_bias=-0.0,
                low_bias=-0.25,
                day_count=30,
            )
        ],
    )
    events: list[tuple[str, object, list[str]]] = [
        (
            "observation-full",
            Observation(
                event_id="obs-1",
                sequence=9_007_199_254_740_991,
                city_sequence=0,
                emitted_at=_T0,
                slug="miami",
                station_id="KMIA",
                observed_at=_T1,
                lag_seconds=0,
                preliminary=True,
                temperature_f=90.5,
                temperature_c=32.5,
                temp_min_f=88.0,
                temp_max_f=91.0,
                temp_min_c=31.1,
                temp_max_c=32.8,
                is_from_report=True,
                report_type="cli",
                source_report_id="cli-1",
                wu_current_temp_f=90.0,
                wu_current_temp_c=32.2,
                wu_daily_high_f=91.0,
                wu_daily_low_f=77.0,
                wu_daily_high_c=32.8,
                wu_daily_low_c=25.0,
                wu_observation_time=_T0,
                wu_fetched_at=_T1,
                temperature_day_mode="nws_climate_day",
                temperature_day_date="2026-07-13",
                wu_day_mode="calendar_day",
                wu_day_date="2026-07-13",
                dewpoint=73.0,
                heat_index=101.0,
                wind_chill=-0.0,
                relative_humidity=60.0,
                wind_speed=8.0,
                wind_direction=180.0,
                wind_gust=15.0,
                text_description="Partly Cloudy",
            ),
            ["Observation", "TemperatureDayMode", "WuDayMode"],
        ),
        (
            "price-update-full",
            PriceUpdate(
                event_id="prices-1",
                sequence=2,
                city_sequence=3,
                emitted_at=_T0,
                source="kalshi",
                slug="miami",
                station_id="KMIA",
                city_id="MIA",
                timestamp=_T1,
                markets=[
                    MarketBracket(
                        market_id="market-1",
                        ticker="KXHIGHMIA-26JUL13-B90.5",
                        yes_price=0.61,
                        no_price=0.39,
                        event_ticker="KXHIGHMIA-26JUL13",
                        event_date="2026-07-13",
                        strike_type="between",
                        floor_strike=89.5,
                        cap_strike=90.5,
                        snapshot_time=_T0,
                        yes_bid=0.60,
                        yes_ask=0.62,
                        no_bid=0.38,
                        no_ask=0.40,
                        yes_bid_depth=1_000_000,
                        yes_ask_depth=20,
                        no_bid_depth=30,
                        no_ask_depth=40,
                        yes_bid_levels=[(0.60, 10), (0.59, 20)],
                        yes_ask_levels=[(0.62, 12)],
                        no_bid_levels=[],
                        no_ask_levels=[(0.40, 8)],
                        orderbook_depth=50,
                        volume=0.0,
                    )
                ],
            ),
            ["PriceUpdate", "MarketBracket", "PriceLevel"],
        ),
        (
            "forecast-updated",
            ForecastUpdated(
                event_id="forecast-1",
                sequence=3,
                emitted_at=_T0,
                slug="miami",
                station_id="KMIA",
                model_id="ncep_hrrr_conus",
                version="2026-07-13T12:00:00Z",
            ),
            ["ForecastUpdated"],
        ),
        (
            "forecast-versions",
            ForecastVersions(
                event_id="versions-1",
                sequence=4,
                emitted_at=_T0,
                station_id="KMIA",
                versions={"ncep_hrrr_conus": "v2", "ncep_gfs": "v1"},
            ),
            ["ForecastVersions"],
        ),
        (
            "oracle-scores-updated",
            OracleScoresUpdated(
                event_id="oracle-1",
                sequence=5,
                emitted_at=_T0,
                station_id="KMIA",
                modes=["overall", "day_ahead", "day_of"],
                updated_at=_T1,
                overall=score_table,
                day_ahead=score_table.model_copy(update={"score_mode": "day_ahead"}),
                day_of=score_table,
            ),
            ["OracleScoresUpdated", "OracleScoreTable", "OracleScoreRow", "OracleScoreMode"],
        ),
        (
            "station-report-full",
            StationReport(
                event_id="report-event-1",
                sequence=6,
                city_sequence=7,
                emitted_at=_T0,
                slug="miami",
                station_id="KMIA",
                report_id="cli-20260713",
                report_revision=2,
                report_updated_at=_T1,
                report_type="cli",
                report_date="2026-07-13",
                issuance_time=_T0,
                fetched_at=_T1,
                source_url="https://example.test/report",
                provider="nws",
                max_temp_f=91.0,
                max_temp_c=32.8,
                max_temp_time_utc=_T0,
                min_temp_f=77.0,
                min_temp_c=25.0,
                min_temp_time_utc=_T1,
                temp_f=90.0,
                temp_c=32.2,
            ),
            ["StationReport"],
        ),
        (
            "weather-event-full",
            WeatherEvent(
                event_id="weather-1",
                sequence=8,
                city_sequence=9,
                emitted_at=_T0,
                slug="miami",
                station_id="KMIA",
                id="wx-1",
                event_type="thunderstorm",
                tier="severe",
                state="active",
                name="Thunderstorm",
                badge="TS",
                detail="Nearby cell",
                summary="Storm observed",
                started_at=_T0,
                last_confirmed_at=_T1,
                ended_at=None,
                source=WeatherEventSource(
                    metar_type="SPECI",
                    flight_category="IFR",
                    wx_string="+TSRA",
                    wx_token="TSRA",
                    wind_speed_kt=25.0,
                    wind_gust_kt=40.0,
                    peak_wind_kt=45.0,
                    peak_wind_direction=270,
                    visibility_mi=0.5,
                    cb_location="NW",
                ),
            ),
            ["WeatherEvent", "WeatherEventSource"],
        ),
        (
            "new-high-full",
            NewHigh(
                event_id="high-1",
                sequence=10,
                city_sequence=11,
                emitted_at=_T0,
                event_key="KMIA:2026-07-13:high",
                source_timestamp=_T0,
                wmo_emit_time=_T0,
                producer_received_at=_T1,
                live_published_at=_T1,
                persistence_status="committed",
                producer_sequence=12,
                slug="miami",
                station_id="KMIA",
                value_f=91.0,
                value_c=32.7777777778,
                prev_value_f=90.0,
                observed_at=_T0,
                temperature_day_mode="calendar_day",
                temperature_day_date="2026-07-13",
                is_from_report=True,
                report_type="cli",
                source_report_id="cli-1",
            ),
            ["NewHigh", "PersistenceStatus"],
        ),
        (
            "new-low-full",
            NewLow(
                event_id="low-1",
                sequence=13,
                city_sequence=14,
                emitted_at=_T0,
                event_key="KMIA:2026-07-13:low",
                source_timestamp=_T0,
                wmo_emit_time=_T0,
                producer_received_at=_T1,
                live_published_at=_T1,
                persistence_status="failed",
                producer_sequence=15,
                slug="miami",
                station_id="KMIA",
                value_f=-0.0,
                value_c=-17.7777777778,
                prev_value_f=1.0,
                observed_at=_T0,
                temperature_day_mode="nws_climate_day",
                temperature_day_date="2026-07-13",
                is_from_report=False,
            ),
            ["NewLow"],
        ),
        (
            "timer-wake",
            TimerWake(scheduled_for=_T0, fired_at=_T1, name="rebalance"),
            ["TimerWake"],
        ),
        ("shutdown-defaults", ShutdownEvent(), ["ShutdownEvent"]),
    ]
    valid = [
        _valid(case_id, "StrategyEvent", [*covers, "StrategyEvent", "EngineEvent"], event)
        for case_id, event, covers in events
    ]
    return _document(
        "events",
        valid,
        [
            _invalid("missing-discriminator", "StrategyEvent", "required_field", {"station_id": "KMIA"}),
            _invalid("unknown-discriminator", "StrategyEvent", "enum", {"type": "unknown"}),
            _invalid("blank-required-station", "StrategyEvent", "range", {"type": "observation", "station_id": ""}),
            _invalid(
                "malformed-nested-market",
                "StrategyEvent",
                "type",
                {"type": "price_update", "source": "kalshi", "station_id": "KMIA", "markets": ["bad"]},
            ),
        ],
    )


def _state_fixture() -> dict[str, Any]:
    forecast = StationForecast(
        model_forecasts={
            "ncep_hrrr_conus": ModelForecast(
                model_id="ncep_hrrr_conus",
                value=91.25,
                version="v2",
                updated_at=_T1,
                run_issued_at=_T0,
                hourly=(
                    ForecastHourly(
                        time="2026-07-13T13:00:00-04:00",
                        temperature_2m_f=90.0,
                        temperature_2m_c=32.2,
                        apparent_temperature_f=101.0,
                        relative_humidity_2m=70.0,
                        dew_point_2m=75.0,
                        pressure_msl=1012.3,
                        wind_speed_10m=12.0,
                        wind_direction_10m=180.0,
                        wind_gusts_10m=20.0,
                        cloud_cover=0.0,
                        precipitation_probability=25.0,
                    ),
                ),
            )
        },
        updated_at=_T1,
    )
    scores = StationOracleScores(
        station_id="KMIA",
        scores=(
            OracleModelScore(
                model_id="ncep_hrrr_conus",
                model_name="HRRR",
                combined_mae=1.1,
                high_mae=1.0,
                low_mae=1.2,
                high_bias=-0.0,
                low_bias=-0.1,
                day_count=30,
                is_public=True,
            ),
        ),
        rank_by="high",
        score_mode="day_of",
        days_requested="30",
        range_start="2026-06-01",
        range_end="2026-07-12",
        updated_at=_T1,
    )
    weather = StationWeather(
        current_temp=90.0,
        running_high=91.0,
        running_low=77.0,
        last_metar_time=_T0,
        temp_min_f=89.0,
        temp_max_f=91.0,
        temp_min_c=31.7,
        temp_max_c=32.8,
        preliminary=True,
        lag_seconds=0,
        wu_current_temp_f=90.0,
        wu_current_temp_c=32.2,
        wu_daily_high_f=91.0,
        wu_daily_low_f=77.0,
        wu_daily_high_c=32.8,
        wu_daily_low_c=25.0,
        wu_observation_time=_T0,
        wu_fetched_at=_T1,
        asos_daily_high_f=91.0,
        asos_daily_low_f=77.0,
        dewpoint=73.0,
        heat_index=101.0,
        wind_chill=-0.0,
        relative_humidity=60.0,
        wind_speed=8.0,
        wind_direction=180.0,
        wind_gust=15.0,
        text_description="Partly Cloudy",
        dsm_high=91.0,
        dsm_low=77.0,
        dsm_high_time=_T0,
        dsm_low_time=_T0,
        six_hr_high=91.0,
        six_hr_low=77.0,
        last_dsm_time=_T0,
        last_six_hr_time=_T1,
    )
    prices = TickerPrices(
        ticker="KXHIGHMIA-26JUL13-B90.5",
        source="kalshi",
        event_ticker="KXHIGHMIA-26JUL13",
        event_date="2026-07-13",
        series_ticker="KXHIGHMIA",
        fee_type="quadratic",
        fee_multiplier=1.0,
        strike_type="between",
        floor_strike=89.5,
        cap_strike=90.5,
        yes_price=0.61,
        no_price=0.39,
        yes_bid=0.60,
        yes_ask=0.62,
        no_bid=0.38,
        no_ask=0.40,
        yes_bid_depth=10,
        yes_ask_depth=20,
        no_bid_depth=30,
        no_ask_depth=40,
        yes_bid_levels=((0.60, 10),),
        yes_ask_levels=((0.62, 20),),
        no_bid_levels=(),
        no_ask_levels=((0.40, 40),),
        orderbook_depth=50,
        volume=1_000_000.0,
        peak_yes_ask=0.99,
        last_update=_T1,
    )
    freshness = FreshnessSnapshot(
        domain=FreshnessDomain.PRICE,
        key=prices.ticker,
        status=FreshnessStatus.STALE,
        source="kalshi",
        updated_at=_T0,
        observed_at=_T0,
        stale_after_seconds=30.0,
        age_seconds=31.25,
        invalidation_reason="forecast_updated",
        detail="waiting for price refresh",
    )
    summary = FreshnessSummary(
        as_of=_T1,
        domains=(
            FreshnessDomainSummary(FreshnessDomain.WEATHER, 1, 1, 0, 0.0),
            FreshnessDomainSummary(FreshnessDomain.PRICE, 2, 1, 1, 31.25),
        ),
    )
    valid = [
        _valid(
            "freshness-full",
            "FreshnessSnapshot",
            ["FreshnessSnapshot", "FreshnessDomain", "FreshnessStatus"],
            freshness,
        ),
        _valid("freshness-summary-full", "FreshnessSummary", ["FreshnessSummary", "FreshnessDomainSummary"], summary),
        _valid("forecast-full", "StationForecast", ["StationForecast", "ModelForecast", "ForecastHourly"], forecast),
        _valid("oracle-scores-full", "StationOracleScores", ["StationOracleScores", "OracleModelScore"], scores),
        _valid("weather-full", "StationWeather", ["StationWeather"], weather),
        _valid("weather-defaults", "StationWeather", ["StationWeather"], StationWeather(), wire={}),
        _valid("ticker-prices-full", "TickerPrices", ["TickerPrices", "PriceLevel", "FeeType"], prices),
        _valid("ticker-prices-defaults", "TickerPrices", ["TickerPrices"], TickerPrices(), wire={}),
    ]
    valid.extend(
        _raw(f"freshness-status-{value.value}", "FreshnessStatus", ["FreshnessStatus"], value.value)
        for value in FreshnessStatus
    )
    valid.extend(
        _raw(f"freshness-domain-{value.value}", "FreshnessDomain", ["FreshnessDomain"], value.value)
        for value in FreshnessDomain
    )
    valid.extend(
        _raw(f"fee-type-{value}", "FeeType", ["FeeType"], value)
        for value in ("quadratic", "quadratic_with_maker_fees", "flat")
    )
    return _document(
        "state",
        valid,
        [
            _invalid(
                "unknown-freshness-status",
                "FreshnessSnapshot",
                "enum",
                {**_json_value(freshness), "status": "unknown"},
            ),
            _invalid(
                "malformed-freshness-count",
                "FreshnessSummary",
                "format",
                {"as_of": "not-a-time", "domains": []},
            ),
            _invalid("wrong-hourly-shape", "StationForecast", "type", {"model_forecasts": []}),
        ],
    )


def _broker_fixture() -> dict[str, Any]:
    position = Position("KXHIGHMIA-26JUL13-B90.5", "yes", 1_000_000, 0.61)
    pending = PendingOrder(
        order_id="order-1",
        sleeve_id="demo:KMIA",
        ticker=position.ticker,
        action="buy",
        contract_side="yes",
        limit_price=0.61,
        requested_quantity=1_000_000,
        filled_quantity=0,
        reserved_global=610_000.0,
        reserved_sleeve=610_000.0,
        fee_type="quadratic",
        fee_multiplier=1.5,
        fee_accumulator=-0.0,
        signal_type="forecast_edge",
        signal_metadata='{"model":"hrrr"}',
        created_at="2026-07-13T08:34:56.123456-04:00",
        client_order_id="client-1",
        expires_at="2026-07-13T09:34:56.123456-04:00",
    )
    result = OrderResult(
        order_id="order-1",
        sleeve_id="demo:KMIA",
        status="partial",
        filled_quantity=500_000,
        fill_price=0.60,
        fee_cost=123.45,
        reason="partial liquidity",
    )
    intent = OrderIntent(
        ticker=position.ticker,
        action="sell",
        contract_side="no",
        order_type="limit",
        quantity=1_000_000,
        limit_price=0.39,
        max_price=0.40,
        max_cost=400_000.0,
        execution_style="sweep",
        time_policy="fill_or_kill",
        reduce_only=True,
        post_only=False,
        signal_type="risk_exit",
        signal_metadata='{"reason":"limit"}',
        client_order_id="client-2",
        expires_after_ms=86_400_000,
    )
    update = BrokerOrderUpdate(
        order_id="order-1",
        sleeve_id="demo:KMIA",
        ticker=position.ticker,
        status="reconciled",
        action="sell",
        contract_side="no",
        requested_quantity=1_000_000,
        filled_quantity=500_000,
        remaining_quantity=500_000,
        fill_price=0.60,
        average_fill_price=0.605,
        fee_cost=123.45,
        reason="provider replay",
        client_order_id="client-2",
        provider_order_id="provider-1",
        provider_sequence="999999999999999999",
        updated_at="2026-07-13T08:35:01.987-04:00",
        expires_at=None,
    )
    valid = [
        _valid("position", "Position", ["Position"], position),
        _valid("pending-order-full", "PendingOrder", ["PendingOrder", "OrderId"], pending),
        _valid(
            "pending-order-defaults-and-null",
            "PendingOrder",
            ["PendingOrder"],
            PendingOrder("order-2", "demo:KMIA", position.ticker, "buy", "yes", 0.50, 1),
            wire={
                "order_id": "order-2",
                "sleeve_id": "demo:KMIA",
                "ticker": position.ticker,
                "action": "buy",
                "contract_side": "yes",
                "limit_price": 0.50,
                "requested_quantity": 1,
                "client_order_id": None,
            },
        ),
        _valid("order-result", "OrderResult", ["OrderResult", "OrderStatus"], result),
        _valid(
            "order-intent-full",
            "OrderIntent",
            ["OrderIntent", "OrderType", "OrderExecutionStyle", "OrderTimePolicy"],
            intent,
        ),
        _valid("broker-order-update", "BrokerOrderUpdate", ["BrokerOrderUpdate", "BrokerUpdateStatus"], update),
    ]
    enum_values: dict[str, list[str]] = {
        "Action": ["buy", "sell"],
        "ContractSide": ["yes", "no"],
        "OrderType": ["market", "limit"],
        "OrderStatus": ["filled", "partial", "pending", "rejected", "cancelled"],
        "OrderExecutionStyle": ["resting_limit", "direct", "sweep"],
        "OrderTimePolicy": ["good_till_canceled", "immediate_or_cancel", "fill_or_kill"],
        "BrokerUpdateStatus": [
            "accepted",
            "rejected",
            "submitted",
            "resting",
            "partially_filled",
            "filled",
            "cancel_requested",
            "cancelled",
            "expired",
            "closed",
            "submission_unknown",
            "reconciled",
        ],
    }
    for rust_type, values in enum_values.items():
        valid.extend(_raw(f"{rust_type}-{value}", rust_type, [rust_type], value) for value in values)
    document = _document(
        "broker",
        valid,
        [
            _invalid(
                "unknown-action",
                "OrderIntent",
                "enum",
                {**_json_value(intent), "action": "hold"},
            ),
            _invalid("missing-ticker", "OrderIntent", "required_field", {"action": "buy"}),
            _invalid(
                "wrong-quantity-type",
                "BrokerOrderUpdate",
                "type",
                {**_json_value(update), "requested_quantity": []},
            ),
        ],
    )
    document["drift_decisions"] = [
        {
            "id": "optional-expiry-null-serialization",
            "classification": "python-canonical",
            "python_before": "Dataclass serialization includes optional expiry fields as explicit null.",
            "rust_before": "Serde omitted optional expiry fields when their value was None.",
            "consumer_impact": (
                "Omitted and null inputs both decode as None; emitting null is an additive "
                "structural-output correction."
            ),
            "resolution": "Rust serializes expires_at and expires_after_ms as null when unset.",
        }
    ]
    return document


def _runtime_fixture() -> dict[str, Any]:
    capabilities = RuntimeCapabilities(
        supports_http=True,
        supports_data_queries=False,
        supports_one_shot_timers=True,
        supports_recurring_timers=True,
        supports_native_kernels=True,
        queue_is_durable=True,
        replay_controls_event_progression=True,
        event_delivery="decision",
    )
    scope = StrategyScope(
        sleeve_id="demo:KMIA",
        strategy_name="demo",
        station_id="KMIA",
        tickers=("KXHIGHMIA-26JUL13-B90.5", "KXLOWTMIA-26JUL13-B75.5"),
        market_type="high",
        event_ticker="KXHIGHMIA-26JUL13",
        event_date=date(2026, 7, 13),
    )
    native_result = NativeKernelResult(
        status="fallback_completed",
        events_handled=1_000_000,
        actions_emitted=0,
        fallback_used=True,
        metadata={"reason": "unsupported", "attempt": 1, "ratio": -0.0, "active": False, "detail": None},
    )
    valid = [
        _valid("capabilities-full", "RuntimeCapabilities", ["RuntimeCapabilities", "EventDelivery"], capabilities),
        _valid("capabilities-defaults", "RuntimeCapabilities", ["RuntimeCapabilities"], RuntimeCapabilities(), wire={}),
        _valid(
            "capabilities-unknown-field-normalized",
            "RuntimeCapabilities",
            ["RuntimeCapabilities"],
            RuntimeCapabilities(),
            wire={"future_flag": True},
        ),
        _valid("strategy-scope", "StrategyScope", ["StrategyScope", "MarketType"], scope),
        _valid(
            "strategy-scope-defaults",
            "StrategyScope",
            ["StrategyScope"],
            StrategyScope("demo:KMIA", "demo"),
            wire={"sleeve_id": "demo:KMIA", "strategy_name": "demo"},
        ),
        _valid(
            "native-result-full",
            "NativeKernelResult",
            ["NativeKernelResult", "NativeKernelStatus", "JSONValue", "JSONObject"],
            native_result,
        ),
        _valid("native-result-defaults", "NativeKernelResult", ["NativeKernelResult"], NativeKernelResult(), wire={}),
        _raw(
            "strategy-config",
            "StrategyConfig",
            ["StrategyConfig"],
            {"station": "KMIA", "retries": 3, "enabled": True, "nested": {"value": None}},
        ),
        _raw(
            "telemetry-fields",
            "TelemetryFields",
            ["TelemetryField", "TelemetryFields"],
            {"text": "ok", "count": 3, "ratio": -0.0, "active": True, "empty": None},
        ),
    ]
    valid.extend(_raw(f"runtime-mode-{mode.value}", "RuntimeMode", ["RuntimeMode"], mode.value) for mode in RuntimeMode)
    valid.extend(_raw(f"market-type-{value}", "MarketType", ["MarketType"], value) for value in ("high", "low"))
    valid.extend(
        _raw(f"event-delivery-{value}", "EventDelivery", ["EventDelivery"], value) for value in ("wake", "decision")
    )
    valid.extend(
        _raw(f"native-status-{value}", "NativeKernelStatus", ["NativeKernelStatus"], value)
        for value in ("completed", "fallback_completed")
    )
    return _document(
        "runtime",
        valid,
        [
            _invalid(
                "unknown-event-delivery",
                "RuntimeCapabilities",
                "enum",
                {**_json_value(capabilities), "event_delivery": "batch"},
            ),
            _invalid(
                "unknown-market-type",
                "StrategyScope",
                "enum",
                {**_json_value(scope), "market_type": "mid"},
            ),
            _invalid("invalid-telemetry-field", "TelemetryFields", "type", {"nested": []}),
        ],
    )


def _queries_fixture() -> dict[str, Any]:
    valid = [
        _valid("limits-defaults", "LimitsQuery", ["LimitsQuery"], LimitsQuery(), wire={}),
        _valid("forecast-full", "ForecastQuery", ["ForecastQuery"], ForecastQuery("ncep_hrrr_conus", True)),
        _valid("forecast-defaults", "ForecastQuery", ["ForecastQuery"], ForecastQuery(), wire={}),
        _valid(
            "oracle-full",
            "OracleScoresQuery",
            ["OracleScoresQuery"],
            OracleScoresQuery("30", "day_of", "combined", True),
        ),
        _valid("oracle-defaults", "OracleScoresQuery", ["OracleScoresQuery"], OracleScoresQuery(), wire={}),
        _valid(
            "forecast-runs-full",
            "ForecastRunsQuery",
            ["ForecastRunsQuery", "DateLike"],
            ForecastRunsQuery("ncep_hrrr_conus", _T0, "2026-07-14T12:00:00Z", 1_000_000, "cursor-1", True),
        ),
        _valid("forecast-runs-defaults", "ForecastRunsQuery", ["ForecastRunsQuery"], ForecastRunsQuery(), wire={}),
        _valid("forecast-run", "ForecastRunQuery", ["ForecastRunQuery"], ForecastRunQuery("run-1", True)),
        _valid("latest-reports-defaults", "LatestReportsQuery", ["LatestReportsQuery"], LatestReportsQuery(), wire={}),
        _valid(
            "reports-full",
            "ReportsQuery",
            ["ReportsQuery", "LocalDateLike"],
            ReportsQuery("cli", date(2026, 7, 13), True),
        ),
        _valid("reports-defaults", "ReportsQuery", ["ReportsQuery"], ReportsQuery(), wire={}),
        _valid(
            "report-history-full",
            "ReportHistoryQuery",
            ["ReportHistoryQuery", "LocalDateLike"],
            ReportHistoryQuery("cli", date(2026, 7, 1), "2026-07-13", 1_000_000, "cursor-2", True),
        ),
        _valid("report-history-defaults", "ReportHistoryQuery", ["ReportHistoryQuery"], ReportHistoryQuery(), wire={}),
        _valid(
            "latest-observation-full",
            "LatestObservationQuery",
            ["LatestObservationQuery", "TemperatureDayMode"],
            LatestObservationQuery("nws_climate_day", True),
        ),
        _valid(
            "latest-observation-defaults",
            "LatestObservationQuery",
            ["LatestObservationQuery"],
            LatestObservationQuery(),
            wire={},
        ),
    ]
    return _document(
        "queries",
        valid,
        [
            _invalid("wrong-refresh-type", "LimitsQuery", "type", {"refresh": []}),
            _invalid("missing-run-id", "ForecastRunQuery", "required_field", {"refresh": False}),
            _invalid("wrong-limit-type", "ForecastRunsQuery", "type", {"limit": []}),
        ],
    )


def build_core_fixtures() -> dict[str, dict[str, Any]]:
    """Build the canonical corpus from Python contract objects."""

    return {
        "events": _events_fixture(),
        "state": _state_fixture(),
        "broker": _broker_fixture(),
        "runtime": _runtime_fixture(),
        "queries": _queries_fixture(),
    }


def load_core_fixture(name: str) -> dict[str, Any]:
    """Load one checked-in fixture without mutating it."""

    if name not in CORE_FIXTURE_NAMES:
        msg = f"unknown core conformance fixture: {name}"
        raise ValueError(msg)
    return cast("dict[str, Any]", json.loads((FIXTURE_ROOT / f"{name}.json").read_text()))


def write_core_fixtures() -> None:
    """Explicitly regenerate checked-in fixtures for an intentional contract update."""

    for name, document in build_core_fixtures().items():
        path = FIXTURE_ROOT / f"{name}.json"
        path.write_text(f"{json.dumps(document, indent=2, sort_keys=True)}\n")


_PYTHON_WIRE_TYPES: dict[str, object] = {
    "Action": str,
    "BrokerOrderUpdate": BrokerOrderUpdate,
    "BrokerUpdateStatus": str,
    "ContractSide": str,
    "EventDelivery": str,
    "FeeType": str,
    "ForecastQuery": ForecastQuery,
    "ForecastRunQuery": ForecastRunQuery,
    "ForecastRunsQuery": ForecastRunsQuery,
    "FreshnessDomain": FreshnessDomain,
    "StrategyEvent": StrategyEvent,
    "FreshnessSnapshot": FreshnessSnapshot,
    "FreshnessStatus": FreshnessStatus,
    "FreshnessSummary": FreshnessSummary,
    "LatestObservationQuery": LatestObservationQuery,
    "LatestReportsQuery": LatestReportsQuery,
    "LimitsQuery": LimitsQuery,
    "MarketType": str,
    "NativeKernelResult": NativeKernelResult,
    "NativeKernelStatus": str,
    "OracleScoresQuery": OracleScoresQuery,
    "OrderExecutionStyle": str,
    "OrderIntent": OrderIntent,
    "OrderResult": OrderResult,
    "OrderStatus": str,
    "OrderTimePolicy": str,
    "OrderType": str,
    "PendingOrder": PendingOrder,
    "Position": Position,
    "ReportHistoryQuery": ReportHistoryQuery,
    "ReportsQuery": ReportsQuery,
    "RuntimeCapabilities": RuntimeCapabilities,
    "RuntimeMode": RuntimeMode,
    "StationForecast": StationForecast,
    "StationOracleScores": StationOracleScores,
    "StationWeather": StationWeather,
    "StrategyConfig": dict[str, object],
    "StrategyScope": StrategyScope,
    "TelemetryFields": dict[str, str | int | float | bool | None],
    "TickerPrices": TickerPrices,
}
_TYPE_NAMESPACE: dict[str, object] = {
    "JSONValue": JSONValue,
    "OracleRankBy": OracleRankBy,
    "OracleScoreMode": OracleScoreMode,
    "OrderId": OrderId,
    "ReportType": ReportType,
    "TemperatureDayMode": TemperatureDayMode,
}


def _wire_adapter(rust_type: str) -> TypeAdapter[Any]:
    python_type = _PYTHON_WIRE_TYPES[rust_type]
    adapter: TypeAdapter[Any] = TypeAdapter(python_type)
    adapter.rebuild(_types_namespace={**globals(), **_TYPE_NAMESPACE})
    return adapter


def python_round_trip_valid_case(case: Mapping[str, Any]) -> Any:
    """Decode and canonicalize a valid case through its Python contract type."""

    rust_type = cast("str", case["rust_type"])
    return _json_value(_wire_adapter(rust_type).validate_python(case["wire"]))


def python_invalid_category(case: Mapping[str, Any]) -> str | None:
    """Return Python's normalized rejection category for an invalid wire case."""

    adapter = _wire_adapter(cast("str", case["rust_type"]))
    try:
        adapter.validate_python(case["wire"])
    except ValidationError as error:
        error_type = error.errors()[0]["type"]
        if error_type in {"missing", "union_tag_not_found"}:
            return "required_field"
        if error_type in {"enum", "literal_error", "union_tag_invalid"}:
            return "enum"
        if error_type in {"string_too_short", "greater_than", "less_than"}:
            return "range"
        if "parsing" in error_type:
            return "format"
        return "type"
    return None


if __name__ == "__main__":
    write_core_fixtures()
