"""Runtime metadata and engine-clock interfaces shared by strategy contexts."""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import StrEnum
from typing import TYPE_CHECKING, Literal, Protocol, runtime_checkable

if TYPE_CHECKING:
    from collections.abc import Awaitable, Callable
    from datetime import date, datetime

MarketType = Literal["high", "low"]


class RuntimeMode(StrEnum):
    """Known runtime modes for the shared contract."""

    PAPER = "paper"
    REPLAY = "replay"
    LIVE = "live"


@dataclass(frozen=True, slots=True)
class StrategyScope:
    """Scope facts for one running strategy sleeve."""

    sleeve_id: str
    strategy_name: str
    station_id: str | None = None
    tickers: tuple[str, ...] = field(default_factory=tuple)
    market_type: MarketType | None = None
    event_ticker: str | None = None
    event_date: date | None = None


@runtime_checkable
class TimerHandle(Protocol):
    """Handle for a scheduled wake that may be cancelled by the strategy/runtime."""

    @property
    def cancelled(self) -> bool: ...

    def cancel(self) -> None: ...


@runtime_checkable
class WorkHandle(Protocol):
    """Handle for bounded immediate work owned by the runtime."""

    @property
    def cancelled(self) -> bool: ...

    @property
    def done(self) -> bool: ...

    @property
    def exception(self) -> BaseException | None: ...

    def cancel(self) -> None: ...


@runtime_checkable
class EngineClock(Protocol):
    """Engine-owned clock so strategy logic can stay portable across runtimes."""

    def now(self) -> datetime: ...

    async def sleep(self, seconds: float) -> None: ...

    async def sleep_until(self, when: datetime) -> None: ...


@runtime_checkable
class StrategyRuntime(Protocol):
    """Runtime metadata, timer, and bounded work surface exposed to strategies."""

    mode: RuntimeMode
    run_id: str
    scope: StrategyScope
    clock: EngineClock

    def wake_at(self, when: datetime, *, name: str | None = None) -> TimerHandle: ...

    def start_work(
        self,
        work: Callable[[], Awaitable[None]],
        *,
        name: str | None = None,
    ) -> WorkHandle: ...
