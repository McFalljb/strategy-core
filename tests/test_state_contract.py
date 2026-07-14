"""Tests for read-only state views and shared value objects."""

from __future__ import annotations

from dataclasses import FrozenInstanceError
from datetime import UTC, datetime
from typing import Any, cast

import pytest

from strategy_core import MarketStateView
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
from tests.fakes import FakeStateView


def test_market_state_view_exposes_read_only_helpers() -> None:
    forecast = StationForecast(
        model_forecasts={
            "ncep_hrrr_conus": ModelForecast(
                model_id="ncep_hrrr_conus",
                value=83.0,
                hourly=(ForecastHourly(time="2026-04-08T13:00:00Z", temperature_2m_f=80.0),),
            ),
        },
    )
    oracle = StationOracleScores(
        station_id="KMIA",
        scores=(OracleModelScore(model_id="ncep_hrrr_conus", high_mae=1.2),),
        rank_by="high",
        score_mode="day_of",
        days_requested="7",
    )
    weather = StationWeather(current_temp=79.5, running_high=81.0)
    prices = TickerPrices(
        ticker="KXHIGHMIA-26APR08-B70.5",
        yes_bid=0.41,
        yes_ask=0.43,
        floor_strike=70.0,
        cap_strike=71.0,
        last_update=datetime(2026, 4, 8, 13, 0, tzinfo=UTC),
    )
    forecast_freshness = FreshnessSnapshot(
        domain=FreshnessDomain.FORECAST,
        key="KMIA",
        status=FreshnessStatus.FRESH,
        source="minutetemp_rest",
        updated_at=datetime(2026, 4, 8, 13, 0, tzinfo=UTC),
        age_seconds=0.0,
    )
    price_freshness = FreshnessSnapshot(
        domain=FreshnessDomain.PRICE,
        key=prices.ticker,
        status=FreshnessStatus.STALE,
        source="kalshi_ws",
        updated_at=datetime(2026, 4, 8, 12, 58, tzinfo=UTC),
        age_seconds=120.0,
        invalidation_reason="aged_out",
    )
    summary = FreshnessSummary(
        as_of=datetime(2026, 4, 8, 13, 0, tzinfo=UTC),
        domains=(
            FreshnessDomainSummary(
                domain=FreshnessDomain.WEATHER,
                tracked_count=1,
                fresh_count=1,
                stale_count=0,
                stalest_age_seconds=10.0,
            ),
            FreshnessDomainSummary(
                domain=FreshnessDomain.FORECAST,
                tracked_count=1,
                fresh_count=1,
                stale_count=0,
                stalest_age_seconds=0.0,
            ),
            FreshnessDomainSummary(
                domain=FreshnessDomain.ORACLE,
                tracked_count=1,
                fresh_count=1,
                stale_count=0,
                stalest_age_seconds=60.0,
            ),
            FreshnessDomainSummary(
                domain=FreshnessDomain.PRICE,
                tracked_count=1,
                fresh_count=0,
                stale_count=1,
                stalest_age_seconds=120.0,
            ),
        ),
    )

    state = FakeStateView(
        forecasts={"KMIA": forecast},
        oracle_scores={"KMIA": oracle},
        weather={"KMIA": weather},
        prices={prices.ticker: prices},
        forecast_freshness={"KMIA": forecast_freshness},
        price_freshness={prices.ticker: price_freshness},
        summary=summary,
    )

    assert isinstance(state, MarketStateView)
    assert state.get_forecast("KMIA") is forecast
    assert state.get_oracle_scores("KMIA") is oracle
    assert state.get_oracle_scores("KMIA", days="7", mode="day_of", rank_by="high") is oracle
    assert state.get_oracle_scores("KMIA", days=7, mode="day_of", rank_by="high") is oracle
    assert state.get_oracle_scores("KMIA", days="30", mode="day_of", rank_by="high") is None
    assert state.get_oracle_scores("KMIA", days="7", mode="day_ahead", rank_by="high") is None
    assert state.get_oracle_scores("KMIA", days="7", mode="day_of", rank_by="combined") is None
    assert state.get_oracle_scores("KORD", days=7, mode="day_of", rank_by="high") is None
    assert state.get_weather("KMIA") is weather
    assert state.get_prices(prices.ticker) is prices
    assert state.get_forecast_freshness("KMIA") is forecast_freshness
    assert state.get_price_freshness(prices.ticker) is price_freshness
    assert state.get_weather_freshness("KORD").is_missing
    assert state.get_oracle_scores_freshness("KORD").status is FreshnessStatus.MISSING
    assert state.freshness_summary() is summary
    assert summary.tracked_count == 4
    assert summary.stale_count == 1


def test_state_value_objects_are_immutable() -> None:
    forecast = StationForecast(
        model_forecasts={
            "ncep_hrrr_conus": ModelForecast(
                model_id="ncep_hrrr_conus",
                value=83.0,
                hourly=(ForecastHourly(time="2026-04-08T13:00:00Z", temperature_2m_f=80.0),),
            ),
        },
    )

    with pytest.raises(FrozenInstanceError):
        cast("Any", forecast).updated_at = datetime(2026, 4, 8, 13, 0, tzinfo=UTC)

    with pytest.raises(TypeError):
        cast("Any", forecast.model_forecasts)["other"] = ModelForecast(model_id="other", value=70.0)

    hourly_points = forecast.model_forecasts["ncep_hrrr_conus"].hourly
    assert isinstance(hourly_points, tuple)


def test_freshness_value_objects_are_immutable() -> None:
    snapshot = FreshnessSnapshot(
        domain=FreshnessDomain.PRICE,
        key="KXHIGHMIA-26APR08-B70.5",
        status=FreshnessStatus.FRESH,
        source="kalshi_ws",
        updated_at=datetime(2026, 4, 8, 13, 0, tzinfo=UTC),
    )
    summary = FreshnessSummary(
        as_of=datetime(2026, 4, 8, 13, 5, tzinfo=UTC),
        domains=(
            FreshnessDomainSummary(
                domain=FreshnessDomain.PRICE,
                tracked_count=1,
                fresh_count=1,
                stale_count=0,
                stalest_age_seconds=0.0,
            ),
        ),
    )

    with pytest.raises(FrozenInstanceError):
        cast("Any", snapshot).status = FreshnessStatus.STALE

    with pytest.raises(FrozenInstanceError):
        cast("Any", summary).as_of = datetime(2026, 4, 8, 14, 0, tzinfo=UTC)
