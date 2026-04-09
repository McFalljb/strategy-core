"""Tests for read-only state views and shared value objects."""

from __future__ import annotations

from dataclasses import FrozenInstanceError
from datetime import UTC, datetime
from typing import Any, cast

import pytest

from strategy_core import MarketStateView
from strategy_core.state import (
    ForecastHourly,
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

    state = FakeStateView(
        forecasts={"KMIA": forecast},
        oracle_scores={"KMIA": oracle},
        weather={"KMIA": weather},
        prices={prices.ticker: prices},
    )

    assert isinstance(state, MarketStateView)
    assert state.get_forecast("KMIA") is forecast
    assert state.get_oracle_scores("KMIA") is oracle
    assert state.get_weather("KMIA") is weather
    assert state.get_prices(prices.ticker) is prices


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
