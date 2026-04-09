"""Tests for MinuteTemp-aligned shared payload models."""

from __future__ import annotations

from dataclasses import FrozenInstanceError
from typing import Any, cast

import pytest

from strategy_core.minutetemp import LatestReportsData, ReportClockSchedule


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


def test_latest_reports_data_is_frozen() -> None:
    payload = LatestReportsData()

    with pytest.raises(FrozenInstanceError):
        cast("Any", payload).reports = ()
