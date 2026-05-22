"""Validation tests for strategy-visible event models."""

from __future__ import annotations

from datetime import UTC, datetime

import pytest
from pydantic import TypeAdapter, ValidationError

from strategy_core.events import (
    ForecastUpdated,
    ForecastVersions,
    NewHigh,
    NewLow,
    Observation,
    OracleScoresUpdated,
    PriceUpdate,
    ShutdownEvent,
    StationReport,
    StrategyEvent,
    WeatherEvent,
)


@pytest.mark.parametrize(
    ("payload", "expected_type"),
    [
        (
            {
                "type": "observation",
                "station_id": "KMIA",
                "temperature_f": 81.2,
                "temperature_day_mode": "nws_climate_day",
                "temperature_day_date": "2026-05-22",
                "wu_day_mode": "calendar_day",
                "wu_day_date": "2026-05-22",
            },
            Observation,
        ),
        (
            {
                "type": "price_update",
                "station_id": "KMIA",
                "source": "kalshi",
                "markets": [{"ticker": "KXHIGHMIA-26APR08-B70.5", "yes_price": 0.42}],
            },
            PriceUpdate,
        ),
        (
            {
                "type": "forecast_updated",
                "station_id": "KMIA",
                "model_id": "ncep_hrrr_conus",
                "version": "2026-04-08T12:00:00Z",
            },
            ForecastUpdated,
        ),
        (
            {
                "type": "forecast_versions",
                "station_id": "KMIA",
                "versions": {"ncep_hrrr_conus": "2026-04-08T12:00:00Z"},
            },
            ForecastVersions,
        ),
        (
            {
                "type": "oracle_scores_updated",
                "station_id": "KMIA",
                "modes": ["overall", "day_of"],
                "day_of": {
                    "station_id": "KMIA",
                    "score_mode": "day_of",
                    "scores": [{"model_id": "ncep_hrrr_conus"}],
                },
            },
            OracleScoresUpdated,
        ),
        ({"type": "station_report", "station_id": "KMIA", "report_id": "report-1"}, StationReport),
        ({"type": "weather_event", "station_id": "KMIA", "id": "storm-1", "event_type": "thunderstorm"}, WeatherEvent),
        (
            {
                "type": "new_high",
                "station_id": "KMIA",
                "value_f": 92.1,
                "value_c": 33.4,
                "temperature_day_mode": "calendar_day",
                "temperature_day_date": "2026-05-22",
                "persistence_status": "uncommitted",
                "event_key": "obs-1",
                "producer_sequence": 42,
            },
            NewHigh,
        ),
        ({"type": "new_low", "station_id": "KMIA", "value_f": 61.3, "value_c": 16.3}, NewLow),
        ({"type": "shutdown", "reason": "done"}, ShutdownEvent),
    ],
)
def test_strategy_event_union_validates_representative_payloads(
    payload: dict[str, object],
    expected_type: type[object],
) -> None:
    adapter: TypeAdapter[StrategyEvent] = TypeAdapter(StrategyEvent)
    event = adapter.validate_python(payload)
    assert isinstance(event, expected_type)


def test_event_models_are_immutable() -> None:
    event = Observation(type="observation", station_id="KMIA", temperature_f=80.0, emitted_at=datetime.now(UTC))
    with pytest.raises((ValidationError, TypeError, ValueError, AttributeError)):
        event.station_id = "KJFK"


def test_observation_rejects_invalid_temperature_day_mode() -> None:
    with pytest.raises(ValidationError):
        Observation.model_validate(
            {
                "type": "observation",
                "station_id": "KMIA",
                "temperature_day_mode": "invalid_mode",
            },
        )


@pytest.mark.parametrize(
    "payload",
    [
        {"type": "price_update"},
        {"type": "price_update", "station_id": "KMIA", "source": ""},
        {"type": "forecast_updated", "station_id": "KMIA"},
        {"type": "station_report", "station_id": "KMIA"},
        {"type": "new_high", "station_id": "KMIA"},
    ],
)
def test_event_models_reject_missing_or_blank_required_fields(payload: dict[str, object]) -> None:
    adapter: TypeAdapter[StrategyEvent] = TypeAdapter(StrategyEvent)

    with pytest.raises(ValidationError):
        adapter.validate_python(payload)
