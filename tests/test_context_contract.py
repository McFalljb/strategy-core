"""Contract tests for the shared StrategyContext surface."""

from __future__ import annotations

from typing import TYPE_CHECKING

import pytest

from strategy_core import RuntimeMode, StrategyContext, WorkHandle
from strategy_core.events import Observation, ShutdownEvent
from tests.fakes import FakeContext, FakeWorkHandle

if TYPE_CHECKING:
    from collections.abc import Awaitable


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


@pytest.mark.asyncio
async def test_runtime_tracked_work_handle_is_exposed() -> None:
    ctx = FakeContext()
    ran = False

    async def work() -> None:
        nonlocal ran
        ran = True

    handle = ctx.runtime.start_work(lambda: work(), name="bootstrap")

    assert isinstance(handle, WorkHandle)
    assert handle.cancelled is False
    assert handle.done is False
    assert handle.exception is None

    await next(h for h in ctx.runtime.scheduled_work if h.name == "bootstrap").drain(ctx.runtime)

    assert ran is True
    assert handle.done is True
    assert handle.exception is None


@pytest.mark.asyncio
async def test_runtime_tracked_work_rejects_before_factory_runs() -> None:
    ctx = FakeContext()
    factory_called = False
    ctx.runtime.suspended = True

    async def work() -> None:
        msg = "factory should not have been called"
        raise AssertionError(msg)

    def factory() -> Awaitable[None]:
        nonlocal factory_called
        factory_called = True
        return work()

    with pytest.raises(RuntimeError, match="tracked work is not enabled"):
        ctx.runtime.start_work(factory)

    assert factory_called is False
    assert ctx.runtime.scheduled_work == []


@pytest.mark.asyncio
async def test_runtime_tracked_work_is_event_scoped_for_replay() -> None:
    ctx = FakeContext()
    events: list[str] = []

    async def child_work() -> None:
        events.append("child")

    async def parent_work() -> None:
        events.append("parent")
        ctx.runtime.start_work(lambda: child_work(), name="child")

    ctx.runtime.start_event("event-1")
    parent = ctx.runtime.start_work(lambda: parent_work(), name="parent")
    ctx.runtime.finish_event()

    assert isinstance(parent, FakeWorkHandle)
    assert parent.event_id == "event-1"
    assert not ctx.runtime.event_work_drained("event-1")

    await parent.drain(ctx.runtime)

    child = next(handle for handle in ctx.runtime.scheduled_work if handle.name == "child")
    assert child.event_id == "event-1"
    assert events == ["parent"]
    assert not ctx.runtime.event_work_drained("event-1")

    await child.drain(ctx.runtime)

    assert events == ["parent", "child"]
    assert ctx.runtime.event_work_drained("event-1")
