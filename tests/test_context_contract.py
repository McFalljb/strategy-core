"""Contract tests for the shared StrategyContext surface."""

from __future__ import annotations

import pytest

from strategy_core import RuntimeMode, StrategyContext
from strategy_core.events import Observation, ShutdownEvent
from tests.fakes import FakeContext


async def _consume_one(ctx: StrategyContext) -> str | None:
    async for event in ctx.events():
        ctx.telemetry.counter("events_seen", fields={"type": event.type})
        return event.type
    return None


@pytest.mark.asyncio
async def test_fake_context_satisfies_shared_contract() -> None:
    ctx = FakeContext(_events=(ShutdownEvent(reason="done"),))
    assert isinstance(ctx, StrategyContext)

    event_type = await _consume_one(ctx)
    assert event_type == "shutdown"
    assert ctx.telemetry.counters == [("events_seen", 1.0, {"type": "shutdown"})]


@pytest.mark.asyncio
async def test_runtime_scope_clock_and_capabilities_are_exposed() -> None:
    ctx = FakeContext(
        _events=(
            Observation(type="observation", station_id="KMIA", temperature_f=81.0),
            ShutdownEvent(reason="done"),
        ),
    )

    assert ctx.runtime.mode is RuntimeMode.PAPER
    assert ctx.runtime.scope.sleeve_id == "demo:KMIA"
    assert ctx.runtime.scope.tickers == ("KXHIGHMIA-26APR08-B70.5",)
    assert ctx.capabilities.supports_http is True
    assert ctx.capabilities.supports_one_shot_timers is True
    assert ctx.capabilities.queue_is_durable is False

    now = ctx.runtime.clock.now()
    handle = ctx.runtime.wake_at(now, name="bootstrap")
    assert handle.cancelled is False
    handle.cancel()
    assert handle.cancelled is True

    await ctx.runtime.clock.sleep(2.0)
    assert ctx.runtime.clock.slept_for == [2.0]
