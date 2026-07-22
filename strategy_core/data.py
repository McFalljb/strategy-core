"""Grouped strategy data-client interface for engine-owned reads."""

from __future__ import annotations

from typing import TYPE_CHECKING, Protocol, runtime_checkable

if TYPE_CHECKING:
    from strategy_core.minutetemp import (
        EffectiveLimits,
        ForecastRunData,
        ForecastRunsPage,
        LatestObservationData,
        LatestReportsData,
        OracleRankBy,
        OracleScoreData,
        OracleScoreMode,
        ReportType,
        StationForecastData,
        StationReportHistoryPage,
        StationReportsData,
    )
    from strategy_core.queries import (
        DateLike,
        ForecastQuery,
        ForecastRunQuery,
        ForecastRunsQuery,
        LatestObservationQuery,
        LatestReportsQuery,
        LimitsQuery,
        LocalDateLike,
        OracleScoresQuery,
        ReportHistoryQuery,
        ReportsQuery,
    )


@runtime_checkable
class StrategyDataClient(Protocol):
    """Grouped read surface implemented by trader/backtester runtimes."""

    async def fetch_limits(self, query: LimitsQuery | None = None, /, *, refresh: bool = False) -> EffectiveLimits: ...

    async def fetch_forecast(
        self,
        query: ForecastQuery | None = None,
        /,
        *,
        model_id: str | None = None,
        refresh: bool = False,
    ) -> StationForecastData | None: ...

    async def fetch_oracle_scores(
        self,
        query: OracleScoresQuery | None = None,
        /,
        *,
        days: str = "7",
        mode: OracleScoreMode | str = "day_ahead",
        rank_by: OracleRankBy | str = "high",
        refresh: bool = False,
    ) -> OracleScoreData | None: ...

    async def fetch_forecast_runs(
        self,
        query: ForecastRunsQuery | None = None,
        /,
        *,
        model_id: str | None = None,
        start: DateLike | None = None,
        end: DateLike | None = None,
        limit: int | None = None,
        cursor: str | None = None,
        refresh: bool = False,
    ) -> ForecastRunsPage: ...

    async def fetch_forecast_run(
        self,
        run_id_or_query: str | ForecastRunQuery,
        /,
        *,
        refresh: bool = False,
    ) -> ForecastRunData | None: ...

    async def fetch_latest_reports(
        self,
        query: LatestReportsQuery | None = None,
        /,
        *,
        include_baseline: bool = False,
        refresh: bool = False,
    ) -> LatestReportsData: ...

    async def fetch_reports(
        self,
        query: ReportsQuery | None = None,
        /,
        *,
        report_type: ReportType | None = None,
        date: LocalDateLike | None = None,
        refresh: bool = False,
    ) -> StationReportsData: ...

    async def fetch_report_history(
        self,
        query: ReportHistoryQuery | None = None,
        /,
        *,
        report_type: ReportType | None = None,
        start: LocalDateLike | None = None,
        end: LocalDateLike | None = None,
        limit: int | None = None,
        cursor: str | None = None,
        refresh: bool = False,
    ) -> StationReportHistoryPage: ...

    async def fetch_latest_observation(
        self,
        query: LatestObservationQuery | None = None,
        /,
        *,
        refresh: bool = False,
    ) -> LatestObservationData: ...
