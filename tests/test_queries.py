"""Tests for query-object and kwargs ergonomics on the grouped data client."""

from __future__ import annotations

from datetime import UTC, date, datetime

import pytest

from strategy_core.minutetemp import (
    EffectiveLimits,
    ForecastBundle,
    ForecastRunData,
    ForecastRunsPage,
    ForecastRunSummary,
    LatestReportsData,
    OracleModelScoreRecord,
    OracleScoreData,
    ReportClockSchedule,
    StationForecastData,
    StationReportHistoryPage,
    StationReportRecord,
    StationReportsData,
)
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
from tests.fakes import FakeDataClient


@pytest.mark.asyncio
async def test_fetch_forecast_accepts_kwargs_and_query_object() -> None:
    payload = StationForecastData(
        forecasts=(ForecastBundle(model_id="ncep_hrrr_conus"),),
        count=1,
    )
    data = FakeDataClient(forecast_payload=payload)

    assert await data.fetch_forecast(model_id="ncep_hrrr_conus", refresh=True) is payload
    assert await data.fetch_forecast(ForecastQuery(model_id="ncep_hrrr_conus", refresh=True)) is payload
    assert data.forecast_queries == [
        ForecastQuery(model_id="ncep_hrrr_conus", refresh=True),
        ForecastQuery(model_id="ncep_hrrr_conus", refresh=True),
    ]


@pytest.mark.asyncio
async def test_fetch_oracle_scores_accepts_kwargs_and_query_object() -> None:
    payload = OracleScoreData(
        station_id="KMIA",
        range_start=date(2026, 4, 1),
        range_end=date(2026, 4, 8),
        scores=(OracleModelScoreRecord(model_id="ncep_hrrr_conus", high_mae=1.0),),
    )
    data = FakeDataClient(oracle_payload=payload)

    assert await data.fetch_oracle_scores(days="30", mode="overall", rank_by="combined", refresh=True) is payload
    assert (
        await data.fetch_oracle_scores(
            OracleScoresQuery(days="30", mode="overall", rank_by="combined", refresh=True),
        )
        is payload
    )
    assert data.oracle_queries == [
        OracleScoresQuery(days="30", mode="overall", rank_by="combined", refresh=True),
        OracleScoresQuery(days="30", mode="overall", rank_by="combined", refresh=True),
    ]


@pytest.mark.asyncio
async def test_fetch_forecast_runs_and_run_accept_query_objects() -> None:
    run_payload = ForecastRunData(
        forecast_run=ForecastRunSummary(id="run-1", model_id="ncep_hrrr_conus"),
        count=1,
    )
    data = FakeDataClient(
        forecast_run_payload=run_payload,
        forecast_runs_payload=ForecastRunsPage(
            runs=(ForecastRunSummary(id="run-1", model_id="ncep_hrrr_conus"),),
            count=1,
        ),
    )
    start = datetime(2026, 4, 8, tzinfo=UTC)

    await data.fetch_forecast_runs(model_id="ncep_hrrr_conus", start=start, limit=5, refresh=True)
    await data.fetch_forecast_runs(ForecastRunsQuery(model_id="ncep_hrrr_conus", start=start, limit=5, refresh=True))
    assert await data.fetch_forecast_run("run-1", refresh=True) == run_payload
    assert await data.fetch_forecast_run(ForecastRunQuery(run_id="run-1", refresh=True)) == run_payload

    assert data.forecast_runs_queries == [
        ForecastRunsQuery(model_id="ncep_hrrr_conus", start=start, limit=5, refresh=True),
        ForecastRunsQuery(model_id="ncep_hrrr_conus", start=start, limit=5, refresh=True),
    ]
    assert data.forecast_run_queries == [
        ForecastRunQuery(run_id="run-1", refresh=True),
        ForecastRunQuery(run_id="run-1", refresh=True),
    ]


@pytest.mark.asyncio
async def test_limits_reports_and_latest_observation_accept_query_objects() -> None:
    data = FakeDataClient(
        limits_payload=EffectiveLimits(tier="pro", max_history_days=30),
        latest_reports_payload=LatestReportsData(
            reports=(StationReportRecord(report_id="report-0", report_date=date(2026, 4, 8)),),
            report_schedules={"cli": (ReportClockSchedule(hour=1),)},
        ),
        reports_payload=StationReportsData(
            reports=(StationReportRecord(report_id="report-1", report_date=date(2026, 4, 8)),),
        ),
        report_history_payload=StationReportHistoryPage(
            reports=(StationReportRecord(report_id="report-2", report_date=date(2026, 4, 7)),),
            count=1,
        ),
    )
    report_day = date(2026, 4, 8)
    history_start = date(2026, 4, 1)

    await data.fetch_limits(refresh=True)
    await data.fetch_limits(LimitsQuery(refresh=True))
    await data.fetch_latest_reports(refresh=True)
    await data.fetch_latest_reports(LatestReportsQuery(refresh=True))
    await data.fetch_reports(report_type="cli", date=report_day, refresh=True)
    await data.fetch_reports(ReportsQuery(report_type="cli", date=report_day, refresh=True))
    await data.fetch_report_history(report_type="cli", start=history_start, limit=50, refresh=True)
    await data.fetch_report_history(
        ReportHistoryQuery(report_type="cli", start=history_start, limit=50, refresh=True),
    )
    await data.fetch_latest_observation(refresh=True)
    await data.fetch_latest_observation(LatestObservationQuery(refresh=True))
    await data.fetch_latest_observation(LatestObservationQuery(day_mode="nws_climate_day", refresh=True))

    assert data.limits_queries == [LimitsQuery(refresh=True), LimitsQuery(refresh=True)]
    assert data.latest_reports_queries == [
        LatestReportsQuery(refresh=True),
        LatestReportsQuery(refresh=True),
    ]
    assert data.reports_queries == [
        ReportsQuery(report_type="cli", date=report_day, refresh=True),
        ReportsQuery(report_type="cli", date=report_day, refresh=True),
    ]
    assert data.report_history_queries == [
        ReportHistoryQuery(report_type="cli", start=history_start, limit=50, refresh=True),
        ReportHistoryQuery(report_type="cli", start=history_start, limit=50, refresh=True),
    ]
    assert data.latest_observation_queries == [
        LatestObservationQuery(refresh=True),
        LatestObservationQuery(refresh=True),
        LatestObservationQuery(day_mode="nws_climate_day", refresh=True),
    ]
