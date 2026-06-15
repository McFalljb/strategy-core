"""Test fakes for the shared strategy contract."""

from __future__ import annotations

import logging
from dataclasses import dataclass, field
from datetime import UTC, date, datetime, timedelta
from types import MappingProxyType
from typing import TYPE_CHECKING

from strategy_core.broker import Action, ContractSide, OrderResult, OrderType, PendingOrder, Position
from strategy_core.capabilities import RuntimeCapabilities
from strategy_core.events import ShutdownEvent, StrategyEvent
from strategy_core.http import HttpHeaders, HttpParams, HttpRequest, HttpResponse
from strategy_core.minutetemp import (
    CityInfo,
    CursorPage,
    EffectiveLimits,
    ForecastRunData,
    ForecastRunsPage,
    LatestObservationData,
    LatestReportsData,
    ObservationRecord,
    OracleRankBy,
    OracleScoreData,
    OracleScoreMode,
    ReportType,
    StationForecastData,
    StationInfo,
    StationReportHistoryPage,
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
from strategy_core.runtime import RuntimeMode, StrategyScope, TimerHandle, WorkHandle
from strategy_core.state import (
    FreshnessDomain,
    FreshnessDomainSummary,
    FreshnessSnapshot,
    FreshnessStatus,
    FreshnessSummary,
)

if TYPE_CHECKING:
    from collections.abc import AsyncIterator, Awaitable, Callable, Mapping

    from strategy_core.models import JSONValue, TelemetryField, TelemetryFields
    from strategy_core.state import StationForecast, StationOracleScores, StationWeather, TickerPrices


@dataclass
class FakeTimerHandle:
    """Simple in-memory timer handle."""

    when: datetime
    name: str | None = None
    cancelled: bool = False

    def cancel(self) -> None:
        self.cancelled = True


@dataclass
class FakeWorkHandle:
    """Simple in-memory tracked-work handle."""

    work: Callable[[], Awaitable[None]]
    name: str | None = None
    event_id: str | None = None
    cancelled: bool = False
    done: bool = False
    exception: BaseException | None = None

    def cancel(self) -> None:
        self.cancelled = True

    async def drain(self, runtime: FakeRuntime) -> None:
        if self.done or self.cancelled:
            self.done = True
            return
        previous_event_id = runtime.current_event_id
        runtime.current_event_id = self.event_id
        try:
            await self.work()
        except Exception as exc:
            self.exception = exc
        finally:
            runtime.current_event_id = previous_event_id
            self.done = True


@dataclass
class FakeClock:
    """Clock that advances instantly for tests."""

    current_time: datetime = field(default_factory=lambda: datetime(2026, 4, 8, tzinfo=UTC))
    slept_for: list[float] = field(default_factory=list)
    slept_until: list[datetime] = field(default_factory=list)

    def now(self) -> datetime:
        return self.current_time

    async def sleep(self, seconds: float) -> None:
        self.slept_for.append(seconds)
        self.current_time = self.current_time + timedelta(seconds=seconds)

    async def sleep_until(self, when: datetime) -> None:
        self.slept_until.append(when)
        self.current_time = when


@dataclass
class FakeRuntime:
    """Runtime metadata fake with wake scheduling."""

    mode: RuntimeMode = RuntimeMode.PAPER
    run_id: str = "run-1"
    scope: StrategyScope = field(
        default_factory=lambda: StrategyScope(
            sleeve_id="demo:KMIA",
            strategy_name="demo",
            station_id="KMIA",
            tickers=("KXHIGHMIA-26APR08-B70.5",),
            market_type="high",
        ),
    )
    clock: FakeClock = field(default_factory=FakeClock)
    scheduled_wakes: list[FakeTimerHandle] = field(default_factory=list)
    scheduled_work: list[FakeWorkHandle] = field(default_factory=list)
    current_event_id: str | None = None
    suspended: bool = False
    runtime_identity: Mapping[str, object] = field(default_factory=lambda: MappingProxyType({"engine": "test"}))

    def wake_at(self, when: datetime, *, name: str | None = None) -> TimerHandle:
        handle = FakeTimerHandle(when=when, name=name)
        self.scheduled_wakes.append(handle)
        return handle

    def start_work(
        self,
        work: Callable[[], Awaitable[None]],
        *,
        name: str | None = None,
    ) -> WorkHandle:
        if self.suspended:
            msg = "tracked work is not enabled for this runtime"
            raise RuntimeError(msg)
        handle = FakeWorkHandle(work=work, name=name, event_id=self.current_event_id)
        self.scheduled_work.append(handle)
        return handle

    def start_event(self, event_id: str) -> None:
        self.current_event_id = event_id

    def finish_event(self) -> None:
        self.current_event_id = None

    def event_work_drained(self, event_id: str) -> bool:
        return all(handle.done for handle in self.scheduled_work if handle.event_id == event_id)


@dataclass
class FakeTelemetry:
    """In-memory telemetry sink for tests."""

    counters: list[tuple[str, float, dict[str, TelemetryField]]] = field(default_factory=list)
    gauges: list[tuple[str, float, dict[str, TelemetryField]]] = field(default_factory=list)
    annotations: list[tuple[str, TelemetryField, dict[str, TelemetryField]]] = field(default_factory=list)
    _logger: logging.Logger = field(default_factory=lambda: logging.getLogger("strategy_core.tests.telemetry"))

    @property
    def logger(self) -> logging.Logger:
        return self._logger

    def counter(self, name: str, value: float = 1.0, *, fields: TelemetryFields | None = None) -> None:
        self.counters.append((name, value, dict(fields or {})))

    def gauge(self, name: str, value: float, *, fields: TelemetryFields | None = None) -> None:
        self.gauges.append((name, value, dict(fields or {})))

    def annotate(
        self,
        name: str,
        *,
        value: TelemetryField = None,
        fields: TelemetryFields | None = None,
    ) -> None:
        self.annotations.append((name, value, dict(fields or {})))


@dataclass
class FakeStateView:
    """Minimal state view fake with explicit station/ticker stores."""

    weather: dict[str, StationWeather] = field(default_factory=dict)
    forecasts: dict[str, StationForecast] = field(default_factory=dict)
    oracle_scores: dict[str, StationOracleScores] = field(default_factory=dict)
    prices: dict[str, TickerPrices] = field(default_factory=dict)
    weather_freshness: dict[str, FreshnessSnapshot] = field(default_factory=dict)
    forecast_freshness: dict[str, FreshnessSnapshot] = field(default_factory=dict)
    oracle_freshness: dict[str, FreshnessSnapshot] = field(default_factory=dict)
    price_freshness: dict[str, FreshnessSnapshot] = field(default_factory=dict)
    summary: FreshnessSummary = field(
        default_factory=lambda: FreshnessSummary(
            as_of=datetime(2026, 4, 8, tzinfo=UTC),
            domains=tuple(
                FreshnessDomainSummary(
                    domain=domain,
                    tracked_count=0,
                    fresh_count=0,
                    stale_count=0,
                    stalest_age_seconds=None,
                )
                for domain in FreshnessDomain
            ),
        ),
    )

    def get_weather(self, station: str) -> StationWeather | None:
        return self.weather.get(station)

    def get_forecast(self, station: str) -> StationForecast | None:
        return self.forecasts.get(station)

    def get_oracle_scores(self, station: str) -> StationOracleScores | None:
        return self.oracle_scores.get(station)

    def get_prices(self, ticker: str) -> TickerPrices | None:
        return self.prices.get(ticker)

    def get_weather_freshness(self, station: str) -> FreshnessSnapshot:
        return self.weather_freshness.get(
            station,
            FreshnessSnapshot(
                domain=FreshnessDomain.WEATHER,
                key=station,
                status=FreshnessStatus.MISSING,
            ),
        )

    def get_forecast_freshness(self, station: str) -> FreshnessSnapshot:
        return self.forecast_freshness.get(
            station,
            FreshnessSnapshot(
                domain=FreshnessDomain.FORECAST,
                key=station,
                status=FreshnessStatus.MISSING,
            ),
        )

    def get_oracle_scores_freshness(self, station: str) -> FreshnessSnapshot:
        return self.oracle_freshness.get(
            station,
            FreshnessSnapshot(
                domain=FreshnessDomain.ORACLE,
                key=station,
                status=FreshnessStatus.MISSING,
            ),
        )

    def get_price_freshness(self, ticker: str) -> FreshnessSnapshot:
        return self.price_freshness.get(
            ticker,
            FreshnessSnapshot(
                domain=FreshnessDomain.PRICE,
                key=ticker,
                status=FreshnessStatus.MISSING,
            ),
        )

    def freshness_summary(self) -> FreshnessSummary:
        return self.summary


@dataclass
class FakeDataClient:
    """Simple data client that records normalized queries."""

    limits_payload: EffectiveLimits = field(default_factory=lambda: EffectiveLimits(max_history_days=0))
    forecast_payload: StationForecastData | None = None
    oracle_payload: OracleScoreData | None = None
    forecast_runs_payload: ForecastRunsPage = field(
        default_factory=lambda: ForecastRunsPage(page=CursorPage(next_cursor=None)),
    )
    forecast_run_payload: ForecastRunData | None = None
    latest_reports_payload: LatestReportsData = field(default_factory=LatestReportsData)
    reports_payload: StationReportsData = field(default_factory=StationReportsData)
    report_history_payload: StationReportHistoryPage = field(default_factory=StationReportHistoryPage)
    latest_observation_payload: LatestObservationData = field(
        default_factory=lambda: LatestObservationData(
            city=CityInfo(id="city-1", slug="mia", name="Miami", timezone="America/New_York"),
            station=StationInfo(station_id="KMIA", name="Miami International", temperature_unit="F"),
            observation=ObservationRecord(temperature_f=72.0),
        ),
    )
    limits_queries: list[LimitsQuery] = field(default_factory=list)
    forecast_queries: list[ForecastQuery] = field(default_factory=list)
    oracle_queries: list[OracleScoresQuery] = field(default_factory=list)
    forecast_runs_queries: list[ForecastRunsQuery] = field(default_factory=list)
    forecast_run_queries: list[ForecastRunQuery] = field(default_factory=list)
    latest_reports_queries: list[LatestReportsQuery] = field(default_factory=list)
    reports_queries: list[ReportsQuery] = field(default_factory=list)
    report_history_queries: list[ReportHistoryQuery] = field(default_factory=list)
    latest_observation_queries: list[LatestObservationQuery] = field(default_factory=list)

    async def fetch_limits(self, query: LimitsQuery | None = None, /, *, refresh: bool = False) -> EffectiveLimits:
        effective = query if query is not None else LimitsQuery(refresh=refresh)
        self.limits_queries.append(effective)
        return self.limits_payload

    async def fetch_forecast(
        self,
        query: ForecastQuery | None = None,
        /,
        *,
        model_id: str | None = None,
        refresh: bool = False,
    ) -> StationForecastData | None:
        effective = query if query is not None else ForecastQuery(model_id=model_id, refresh=refresh)
        self.forecast_queries.append(effective)
        return self.forecast_payload

    async def fetch_oracle_scores(
        self,
        query: OracleScoresQuery | None = None,
        /,
        *,
        days: str = "7",
        mode: OracleScoreMode | str = "day_ahead",
        rank_by: OracleRankBy | str = "high",
        refresh: bool = False,
    ) -> OracleScoreData | None:
        effective = (
            query
            if query is not None
            else OracleScoresQuery(
                days=days,
                mode=mode,
                rank_by=rank_by,
                refresh=refresh,
            )
        )
        self.oracle_queries.append(effective)
        return self.oracle_payload

    async def fetch_forecast_runs(
        self,
        query: ForecastRunsQuery | None = None,
        /,
        *,
        model_id: str | None = None,
        start: datetime | str | None = None,
        end: datetime | str | None = None,
        limit: int | None = None,
        cursor: str | None = None,
        refresh: bool = False,
    ) -> ForecastRunsPage:
        effective = (
            query
            if query is not None
            else ForecastRunsQuery(
                model_id=model_id,
                start=start,
                end=end,
                limit=limit,
                cursor=cursor,
                refresh=refresh,
            )
        )
        self.forecast_runs_queries.append(effective)
        return self.forecast_runs_payload

    async def fetch_forecast_run(
        self,
        run_id_or_query: str | ForecastRunQuery,
        /,
        *,
        refresh: bool = False,
    ) -> ForecastRunData | None:
        effective = (
            run_id_or_query
            if isinstance(run_id_or_query, ForecastRunQuery)
            else ForecastRunQuery(run_id=run_id_or_query, refresh=refresh)
        )
        self.forecast_run_queries.append(effective)
        return self.forecast_run_payload

    async def fetch_latest_reports(
        self,
        query: LatestReportsQuery | None = None,
        /,
        *,
        refresh: bool = False,
    ) -> LatestReportsData:
        effective = query if query is not None else LatestReportsQuery(refresh=refresh)
        self.latest_reports_queries.append(effective)
        return self.latest_reports_payload

    async def fetch_reports(
        self,
        query: ReportsQuery | None = None,
        /,
        *,
        report_type: ReportType | None = None,
        date: date | str | None = None,
        refresh: bool = False,
    ) -> StationReportsData:
        effective = (
            query
            if query is not None
            else ReportsQuery(
                report_type=report_type,
                date=date,
                refresh=refresh,
            )
        )
        self.reports_queries.append(effective)
        return self.reports_payload

    async def fetch_report_history(
        self,
        query: ReportHistoryQuery | None = None,
        /,
        *,
        report_type: ReportType | None = None,
        start: date | str | None = None,
        end: date | str | None = None,
        limit: int | None = None,
        cursor: str | None = None,
        refresh: bool = False,
    ) -> StationReportHistoryPage:
        effective = (
            query
            if query is not None
            else ReportHistoryQuery(
                report_type=report_type,
                start=start,
                end=end,
                limit=limit,
                cursor=cursor,
                refresh=refresh,
            )
        )
        self.report_history_queries.append(effective)
        return self.report_history_payload

    async def fetch_latest_observation(
        self,
        query: LatestObservationQuery | None = None,
        /,
        *,
        refresh: bool = False,
    ) -> LatestObservationData:
        effective = query if query is not None else LatestObservationQuery(refresh=refresh)
        self.latest_observation_queries.append(effective)
        return self.latest_observation_payload


@dataclass
class FakeBroker:
    """Broker fake implementing the shared strategy-facing broker protocol."""

    sleeve_id: str = "demo:KMIA"
    buying_power: float = 100.0
    positions: dict[str, Position] = field(default_factory=dict)
    pending_orders: list[PendingOrder] = field(default_factory=list)
    placed_order_calls: list[dict[str, object]] = field(default_factory=list)

    async def place_order(
        self,
        *,
        ticker: str,
        action: Action,
        contract_side: ContractSide,
        order_type: OrderType,
        quantity: int,
        limit_price: float | None = None,
        signal_type: str | None = None,
        signal_metadata: str | None = None,
        client_order_id: str | None = None,
    ) -> OrderResult:
        self.placed_order_calls.append(
            {
                "ticker": ticker,
                "action": action,
                "contract_side": contract_side,
                "order_type": order_type,
                "quantity": quantity,
                "limit_price": limit_price,
                "signal_type": signal_type,
                "signal_metadata": signal_metadata,
                "client_order_id": client_order_id,
            },
        )
        key = f"{ticker}:{contract_side}"
        if action == "buy":
            self.positions[key] = Position(ticker=ticker, side=contract_side, quantity=quantity, avg_price=0.42)
        else:
            self.positions.pop(key, None)
        return OrderResult(
            order_id=str(len(self.placed_order_calls)),
            sleeve_id=self.sleeve_id,
            status="filled",
            filled_quantity=quantity,
            fill_price=limit_price or 0.42,
        )

    async def cancel_order(self, order_id: str) -> bool:
        remaining = [order for order in self.pending_orders if order.order_id != order_id]
        cancelled = len(remaining) != len(self.pending_orders)
        self.pending_orders = remaining
        return cancelled

    async def cancel_all_orders(self) -> int:
        count = len(self.pending_orders)
        self.pending_orders = []
        return count

    def get_position(self, ticker: str, side: ContractSide = "yes") -> Position | None:
        return self.positions.get(f"{ticker}:{side}")

    def get_positions(self) -> dict[str, Position]:
        return dict(self.positions)

    def get_pending_orders(self) -> list[PendingOrder]:
        return list(self.pending_orders)

    def get_sleeve_buying_power(self) -> float:
        return self.buying_power


@dataclass
class FakeHttpClient:
    """HTTP client fake that records normalized requests."""

    requests: list[HttpRequest] = field(default_factory=list)

    async def request(self, request: HttpRequest) -> HttpResponse:
        self.requests.append(request)
        return HttpResponse(
            status_code=200,
            headers={"content-type": "application/json"},
            json_body={"method": request.method, "url": request.url},
        )

    async def get(
        self,
        url: str,
        *,
        headers: HttpHeaders | None = None,
        params: HttpParams | None = None,
        timeout_seconds: float | None = None,
    ) -> HttpResponse:
        return await self.request(
            HttpRequest(
                method="GET",
                url=url,
                headers=dict(headers or {}),
                params=dict(params or {}),
                timeout_seconds=timeout_seconds,
            ),
        )

    async def post(
        self,
        url: str,
        *,
        headers: HttpHeaders | None = None,
        params: HttpParams | None = None,
        json_body: JSONValue | None = None,
        text_body: str | None = None,
        timeout_seconds: float | None = None,
    ) -> HttpResponse:
        return await self.request(
            HttpRequest(
                method="POST",
                url=url,
                headers=dict(headers or {}),
                params=dict(params or {}),
                json_body=json_body,
                text_body=text_body,
                timeout_seconds=timeout_seconds,
            ),
        )


@dataclass
class FakeContext:
    """Strategy context fake backed by the shared protocol types."""

    state: FakeStateView = field(default_factory=FakeStateView)
    data: FakeDataClient = field(default_factory=FakeDataClient)
    broker: FakeBroker = field(default_factory=FakeBroker)
    http: FakeHttpClient = field(default_factory=FakeHttpClient)
    runtime: FakeRuntime = field(default_factory=FakeRuntime)
    capabilities: RuntimeCapabilities = field(
        default_factory=lambda: RuntimeCapabilities(
            supports_http=True,
            supports_one_shot_timers=True,
            supports_recurring_timers=False,
            queue_is_durable=False,
            replay_controls_event_progression=False,
        ),
    )
    config: dict[str, object] = field(default_factory=lambda: {"entry_max_yes": 0.40})
    telemetry: FakeTelemetry = field(default_factory=FakeTelemetry)
    _events: tuple[StrategyEvent, ...] = field(default_factory=lambda: (ShutdownEvent(reason="done"),))

    async def events(self) -> AsyncIterator[StrategyEvent]:
        for event in self._events:
            yield event


def assert_protocol_instances() -> tuple[
    FakeContext,
    FakeDataClient,
    FakeBroker,
    FakeHttpClient,
    FakeRuntime,
    FakeClock,
    FakeTelemetry,
    FakeStateView,
]:
    """Return fake instances typed to the shared protocols for smoke checks."""

    ctx = FakeContext()
    return (
        ctx,
        ctx.data,
        ctx.broker,
        ctx.http,
        ctx.runtime,
        ctx.runtime.clock,
        ctx.telemetry,
        ctx.state,
    )
