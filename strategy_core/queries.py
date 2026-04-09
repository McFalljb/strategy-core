"""Typed query objects for the grouped strategy data client."""

from __future__ import annotations

from dataclasses import dataclass
from datetime import date, datetime
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from strategy_core.minutetemp import OracleRankBy, OracleScoreMode, ReportType

DateLike = datetime | str
LocalDateLike = date | str


@dataclass(frozen=True, slots=True)
class LimitsQuery:
    refresh: bool = False


@dataclass(frozen=True, slots=True)
class ForecastQuery:
    model_id: str | None = None
    refresh: bool = False


@dataclass(frozen=True, slots=True)
class OracleScoresQuery:
    days: str = "7"
    mode: OracleScoreMode | str = "day_ahead"
    rank_by: OracleRankBy | str = "high"
    refresh: bool = False


@dataclass(frozen=True, slots=True)
class ForecastRunsQuery:
    model_id: str | None = None
    start: DateLike | None = None
    end: DateLike | None = None
    limit: int | None = None
    cursor: str | None = None
    refresh: bool = False


@dataclass(frozen=True, slots=True)
class ForecastRunQuery:
    run_id: str
    refresh: bool = False


@dataclass(frozen=True, slots=True)
class LatestReportsQuery:
    refresh: bool = False


@dataclass(frozen=True, slots=True)
class ReportsQuery:
    report_type: ReportType | None = None
    date: LocalDateLike | None = None
    refresh: bool = False


@dataclass(frozen=True, slots=True)
class ReportHistoryQuery:
    report_type: ReportType | None = None
    start: LocalDateLike | None = None
    end: LocalDateLike | None = None
    limit: int | None = None
    cursor: str | None = None
    refresh: bool = False


@dataclass(frozen=True, slots=True)
class LatestObservationQuery:
    refresh: bool = False
