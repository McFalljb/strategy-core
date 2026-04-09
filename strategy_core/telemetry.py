"""Telemetry and logging interfaces for strategy code."""

from __future__ import annotations

from typing import TYPE_CHECKING, Protocol, runtime_checkable

if TYPE_CHECKING:
    from strategy_core.models import TelemetryField, TelemetryFields


@runtime_checkable
class StrategyLogger(Protocol):
    """Minimal structured logger surface expected by strategies."""

    def debug(self, message: str, *args: object, **kwargs: object) -> None: ...

    def info(self, message: str, *args: object, **kwargs: object) -> None: ...

    def warning(self, message: str, *args: object, **kwargs: object) -> None: ...

    def error(self, message: str, *args: object, **kwargs: object) -> None: ...

    def exception(self, message: str, *args: object, **kwargs: object) -> None: ...


@runtime_checkable
class Telemetry(Protocol):
    """Unified logging and lightweight metrics surface."""

    @property
    def logger(self) -> StrategyLogger: ...

    def counter(self, name: str, value: float = 1.0, *, fields: TelemetryFields | None = None) -> None: ...

    def gauge(self, name: str, value: float, *, fields: TelemetryFields | None = None) -> None: ...

    def annotate(
        self,
        name: str,
        *,
        value: TelemetryField = None,
        fields: TelemetryFields | None = None,
    ) -> None: ...
