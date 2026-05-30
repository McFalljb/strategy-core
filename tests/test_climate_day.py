"""Tests for shared station timezone and NWS climate-day helpers."""

from __future__ import annotations

from datetime import UTC, date, datetime

import pytest

from strategy_core.climate_day import (
    climate_day_date,
    climate_day_end,
    climate_day_has_ended,
    parse_climate_date,
    station_timezone,
)


def test_station_timezone_uses_market_local_date() -> None:
    assert station_timezone("KMIA").key == "America/New_York"
    assert station_timezone("KNYC").key == "America/New_York"
    assert station_timezone("KPHX").key == "America/Phoenix"
    assert station_timezone("KDEN").key == "America/Denver"
    assert station_timezone("KSEA").key == "America/Los_Angeles"

    before_midnight_utc = datetime(2026, 4, 2, 4, 55, tzinfo=UTC)
    after_midnight_utc = datetime(2026, 4, 2, 5, 5, tzinfo=UTC)

    assert climate_day_date("KMIA", before_midnight_utc) == date(2026, 4, 1)
    assert climate_day_date("KMIA", after_midnight_utc) == date(2026, 4, 2)


def test_unknown_station_requires_explicit_timezone_mapping() -> None:
    with pytest.raises(ValueError, match="unknown timezone for station EGLL"):
        station_timezone("EGLL")

    assert (
        station_timezone(
            "EGLL",
            station_timezones={"EGLL": "Europe/London"},
        ).key
        == "Europe/London"
    )
    assert climate_day_date(
        "EGLL",
        datetime(2026, 4, 3, 0, 10, tzinfo=UTC),
        station_timezones={"EGLL": "Europe/London"},
    ) == date(2026, 4, 3)


def test_parse_climate_date_accepts_known_event_formats() -> None:
    assert parse_climate_date("2026-04-03") == date(2026, 4, 3)
    assert parse_climate_date("20260403") == date(2026, 4, 3)
    assert parse_climate_date("260403") == date(2026, 4, 3)
    assert parse_climate_date("bad") is None
    assert parse_climate_date("2026-99-99") is None


def test_climate_day_close_uses_standard_time_boundary() -> None:
    event_date = date(2026, 7, 4)

    assert climate_day_end("KMIA", event_date) == datetime(2026, 7, 5, 5, 0, tzinfo=UTC)
    assert not climate_day_has_ended("KMIA", event_date, datetime(2026, 7, 5, 4, 59, tzinfo=UTC))
    assert climate_day_has_ended("KMIA", event_date, datetime(2026, 7, 5, 5, 0, tzinfo=UTC))
