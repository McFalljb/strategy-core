"""Tests for MinuteTemp-aligned shared payload models."""

from __future__ import annotations

from dataclasses import FrozenInstanceError
from datetime import date
from typing import Any, cast

import pytest

from strategy_core.minutetemp import LatestObservationData, LatestReportsData, ReportClockSchedule, StationInfo


def test_latest_reports_data_freezes_schedule_mappings_and_lists() -> None:
    payload = LatestReportsData(
        report_schedules=cast(
            "Any",
            {
                "cli": [ReportClockSchedule(hour=1), ReportClockSchedule(hour=13)],
            },
        ),
    )

    schedules = payload.report_schedules
    assert schedules is not None
    assert isinstance(schedules["cli"], tuple)

    with pytest.raises(TypeError):
        cast("Any", schedules)["other"] = (ReportClockSchedule(hour=5),)

    with pytest.raises(AttributeError):
        cast("Any", schedules["cli"]).append(ReportClockSchedule(hour=17))


def test_latest_observation_data_includes_day_bucketing_fields() -> None:
    payload = LatestObservationData(
        temperature_day_mode="nws_climate_day",
        temperature_day_date=date(2026, 5, 22),
        wu_day_mode="calendar_day",
        wu_day_date=date(2026, 5, 22),
        station=StationInfo(station_id="KMDW", uses_nws_climate_day=True),
    )

    assert payload.temperature_day_mode == "nws_climate_day"
    assert payload.wu_day_mode == "calendar_day"
    assert payload.station is not None
    assert payload.station.uses_nws_climate_day is True


def test_latest_reports_data_is_frozen() -> None:
    payload = LatestReportsData()

    with pytest.raises(FrozenInstanceError):
        cast("Any", payload).reports = ()
