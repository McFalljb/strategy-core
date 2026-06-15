"""Optional native strategy-kernel contract.

The normal strategy contract remains ``async def run(ctx)`` using
``StrategyContext``. This module defines the additive surface a runtime can
implement when it can execute a native strategy kernel without driving the
Python event iterator for every delivered event.
"""

from __future__ import annotations

from collections.abc import Awaitable, Callable, Mapping
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Literal, Protocol, runtime_checkable

from strategy_core.context import StrategyContext

if TYPE_CHECKING:
    from strategy_core.models import JSONValue, StrategyConfig


NativeKernelStatus = Literal["completed", "fallback_completed"]
FallbackHandler = Callable[[], Awaitable[None]]


class NativeKernelUnavailableError(RuntimeError):
    """Raised when a strategy requires native execution but the runtime cannot provide it."""


NativeKernelUnavailable = NativeKernelUnavailableError


@runtime_checkable
class NativeKernel(Protocol):
    """Opaque native strategy kernel handle supplied by a strategy wrapper."""

    @property
    def name(self) -> str: ...


@runtime_checkable
class NativeKernelFactory(Protocol):
    """Factory that builds a native kernel from strategy configuration."""

    def __call__(self, config: StrategyConfig, /) -> NativeKernel: ...


@dataclass(frozen=True, slots=True)
class NativeKernelResult:
    """Portable completion summary returned by a native-kernel runtime."""

    status: NativeKernelStatus = "completed"
    events_handled: int = 0
    actions_emitted: int = 0
    fallback_used: bool = False
    metadata: Mapping[str, JSONValue] = field(default_factory=dict)


@runtime_checkable
class NativeKernelRunner(Protocol):
    """Runtime-owned executor for native strategy kernels."""

    async def run_native_kernel(self, kernel: NativeKernel, /) -> NativeKernelResult: ...


@runtime_checkable
class NativeStrategyContext(StrategyContext, Protocol):
    """Strategy context extension implemented only by native-capable runtimes."""

    native_kernel_runner: NativeKernelRunner


def get_native_kernel_runner(ctx: StrategyContext, /) -> NativeKernelRunner | None:
    """Return the runtime native-kernel runner when the context advertises one."""

    if not ctx.capabilities.supports_native_kernels:
        return None
    runner = getattr(ctx, "native_kernel_runner", None)
    if isinstance(runner, NativeKernelRunner):
        return runner
    return None


async def run_native_or_fallback(
    ctx: StrategyContext,
    kernel: NativeKernel,
    /,
    *,
    fallback: FallbackHandler | None = None,
    require_native: bool = False,
) -> NativeKernelResult:
    """Run a native kernel when supported, otherwise run an explicit Python fallback."""

    runner = get_native_kernel_runner(ctx)
    if runner is not None:
        return await runner.run_native_kernel(kernel)
    if require_native:
        msg = f"native strategy kernel {kernel.name!r} was required, but this runtime does not support native kernels"
        raise NativeKernelUnavailable(msg)
    if fallback is None:
        msg = f"native strategy kernel {kernel.name!r} is unavailable and no Python fallback was provided"
        raise NativeKernelUnavailable(msg)
    await fallback()
    return NativeKernelResult(status="fallback_completed", fallback_used=True)
