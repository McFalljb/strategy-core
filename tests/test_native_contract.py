"""Contract tests for optional native strategy-kernel support."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import cast

import pytest

from strategy_core import RuntimeCapabilities, StrategyContext
from strategy_core.native import (
    NativeKernel,
    NativeKernelResult,
    NativeKernelUnavailable,
    NativeStrategyContext,
    get_native_kernel_runner,
    run_native_or_fallback,
)
from tests.fakes import FakeContext


@dataclass(frozen=True, slots=True)
class FakeKernel:
    """Opaque native kernel handle used by contract tests."""

    name: str = "fake-kernel"


@dataclass(slots=True)
class FakeNativeRunner:
    """Native runner fake that records the kernel passed by the helper."""

    calls: list[NativeKernel] = field(default_factory=list)

    async def run_native_kernel(self, kernel: NativeKernel, /) -> NativeKernelResult:
        self.calls.append(kernel)
        return NativeKernelResult(
            status="completed",
            events_handled=3,
            actions_emitted=1,
            metadata={"kernel": kernel.name},
        )


@dataclass
class FakeNativeContext(FakeContext):
    """Fake context that opts into the native-kernel extension protocol."""

    native_kernel_runner: FakeNativeRunner = field(default_factory=FakeNativeRunner)
    capabilities: RuntimeCapabilities = field(
        default_factory=lambda: RuntimeCapabilities(
            supports_http=True,
            supports_one_shot_timers=True,
            supports_native_kernels=True,
        ),
    )


def test_existing_context_contract_remains_unchanged() -> None:
    ctx = FakeContext()

    assert isinstance(ctx, StrategyContext)
    assert not isinstance(ctx, NativeStrategyContext)
    assert ctx.capabilities.supports_native_kernels is False
    assert get_native_kernel_runner(ctx) is None


def test_native_context_satisfies_native_extension_protocol() -> None:
    ctx = FakeNativeContext()

    assert isinstance(ctx, StrategyContext)
    assert isinstance(ctx, NativeStrategyContext)
    assert get_native_kernel_runner(ctx) is ctx.native_kernel_runner


@pytest.mark.asyncio
async def test_run_native_or_fallback_calls_native_runner_when_available() -> None:
    ctx = FakeNativeContext()
    kernel = FakeKernel()

    result = await run_native_or_fallback(cast("StrategyContext", ctx), kernel)

    assert result.status == "completed"
    assert result.events_handled == 3
    assert result.actions_emitted == 1
    assert result.metadata == {"kernel": "fake-kernel"}
    assert ctx.native_kernel_runner.calls == [kernel]


@pytest.mark.asyncio
async def test_run_native_or_fallback_uses_python_fallback_when_allowed() -> None:
    ctx = FakeContext()
    kernel = FakeKernel()
    fallback_calls = 0

    async def fallback() -> None:
        nonlocal fallback_calls
        fallback_calls += 1

    result = await run_native_or_fallback(cast("StrategyContext", ctx), kernel, fallback=fallback)

    assert result.status == "fallback_completed"
    assert result.fallback_used is True
    assert fallback_calls == 1


@pytest.mark.asyncio
async def test_run_native_or_fallback_rejects_required_native_without_runner() -> None:
    ctx = FakeContext()

    with pytest.raises(NativeKernelUnavailable, match="was required"):
        await run_native_or_fallback(cast("StrategyContext", ctx), FakeKernel(), require_native=True)


@pytest.mark.asyncio
async def test_run_native_or_fallback_rejects_missing_fallback() -> None:
    ctx = FakeContext()

    with pytest.raises(NativeKernelUnavailable, match="no Python fallback"):
        await run_native_or_fallback(cast("StrategyContext", ctx), FakeKernel())
