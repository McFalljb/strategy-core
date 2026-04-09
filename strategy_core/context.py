"""Canonical strategy context contract shared across runtimes."""

from __future__ import annotations

from collections.abc import AsyncIterator, Awaitable, Callable
from typing import TYPE_CHECKING, Protocol, runtime_checkable

if TYPE_CHECKING:
    from strategy_core.broker import Broker
    from strategy_core.capabilities import RuntimeCapabilities
    from strategy_core.data import StrategyDataClient
    from strategy_core.events import StrategyEvent
    from strategy_core.http import HttpClient
    from strategy_core.models import StrategyConfig
    from strategy_core.runtime import StrategyRuntime
    from strategy_core.state import MarketStateView
    from strategy_core.telemetry import Telemetry


@runtime_checkable
class StrategyContext(Protocol):
    """Runtime-neutral strategy authoring surface."""

    state: MarketStateView
    data: StrategyDataClient
    broker: Broker
    http: HttpClient
    runtime: StrategyRuntime
    capabilities: RuntimeCapabilities
    config: StrategyConfig
    telemetry: Telemetry

    def events(self) -> AsyncIterator[StrategyEvent]: ...


StrategyHandler = Callable[[StrategyContext], Awaitable[None]]
