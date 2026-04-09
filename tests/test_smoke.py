"""Smoke tests for the shared package's public surface."""

from __future__ import annotations

from strategy_core import RuntimeMode, StrategyContext, StrategyScope
from tests.fakes import FakeContext, assert_protocol_instances


def test_public_import_surface_exists() -> None:
    assert RuntimeMode.PAPER.value == "paper"
    assert StrategyScope(sleeve_id="demo:KMIA", strategy_name="demo").strategy_name == "demo"


def test_package_exposes_protocol_compatible_fakes() -> None:
    ctx, data, broker, http, runtime, clock, telemetry, state = assert_protocol_instances()
    assert isinstance(ctx, StrategyContext)
    assert isinstance(runtime.mode, RuntimeMode)
    assert runtime.scope.station_id == "KMIA"
    assert data is ctx.data
    assert broker is ctx.broker
    assert http is ctx.http
    assert clock is runtime.clock
    assert telemetry is ctx.telemetry
    assert state is ctx.state

    fake_ctx = FakeContext()
    assert fake_ctx._events[0].type == "shutdown"
