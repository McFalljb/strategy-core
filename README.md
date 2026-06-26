# Strategy Core

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Shared strategy contract package for Python and Rust strategy code that runs across paper, replay, backtest, and live engines. Consumer repos (such as [`trader`](https://github.com/McFalljb/trader)) implement the runtime; this library defines the strategy-facing surface.

This repo is a **library, not an engine**. It holds the canonical Python `run(ctx)` contract, typed events, shared value objects, runtime-neutral protocols, and Rust parity crates for native strategy adapters.

## Status

Version **0.1.x** is an early public contract. Breaking changes may land without a major bump until **1.0.0**. See [docs/contract-map.md](docs/contract-map.md) for how the shared package maps to engine code today.

## Prerequisites

- [uv](https://docs.astral.sh/uv/)
- Python 3.12+ (see `.python-version`)
- Rust 1.85+ when working on the optional native crates under `native/`

## Quick start

```bash
uv sync --group dev
uv run ruff check . && uv run ruff format --check . && uv run mypy . && uv run pytest
cargo fmt --manifest-path native/Cargo.toml --all -- --check
cargo test --manifest-path native/Cargo.toml
```

When you change dependencies in `pyproject.toml`, run `uv lock` (or `uv sync`) and commit `uv.lock` in the same change. CI uses `uv sync --frozen --group dev`.

## Install

**From a sibling checkout (development):**

```toml
[tool.uv.sources]
strategy-core = { path = "../strategy-core", editable = true }
```

**From Git:**

```toml
[tool.uv.sources]
strategy-core = { git = "https://github.com/McFalljb/strategy-core.git", rev = "main" }
```

## Public surface

The contract centers on one strategy context with nested services:

| Surface | Role |
|---------|------|
| `ctx.events()` | Async stream of typed engine events |
| `ctx.state` | Read-only normalized snapshots and freshness |
| `ctx.data` | Engine-owned MinuteTemp reads (typed models) |
| `ctx.broker` | Orders, positions, buying power |
| `ctx.http` | Optional HTTP client (when enabled) |
| `ctx.runtime` | Scope, clock, `wake_at` one-shot timers |
| `ctx.capabilities` | Feature flags for portable strategy code |
| `ctx.config` | Strategy configuration mapping |
| `ctx.telemetry` | Counters, gauges, structured logging hooks |

`ctx.state` and `ctx.data` are intentionally separate: state is the fast latest-known view; data is explicit engine-owned reads against upstream APIs. `ctx.http` is optional and must be enabled by the runtime; strategies should check `ctx.capabilities.supports_http` before using it.

Provider-aligned model modules for engine adapters:

- `strategy_core.minutetemp` — MinuteTemp REST/WebSocket payload types
- `strategy_core.kalshi` — Kalshi REST/WebSocket payload types

### Example

```python
from strategy_core import ForecastUpdated, PriceUpdate, StrategyContext


async def run(ctx: StrategyContext) -> None:
    async for event in ctx.events():
        if isinstance(event, ForecastUpdated):
            await ctx.data.fetch_forecast(refresh=True)

        if isinstance(event, PriceUpdate):
            await ctx.broker.place_order(
                ticker="DEMO-TICKER",
                action="buy",
                contract_side="yes",
                order_type="market",
                quantity=1,
            )
```

Engines implement these protocols differently while keeping the same strategy-facing API.

## Repository layout

```text
strategy_core/   # Importable Python package
tests/           # Python pytest suite
native/          # Rust strategy_core and strategy_core_kernel crates
docs/            # Contract and Rust parity documentation
AGENTS.md        # Contributor commands and conventions
```

## Documentation

- [docs/contract-map.md](docs/contract-map.md) — shared contract boundaries and engine responsibilities
- [docs/rust-parity-strategy-core.md](docs/rust-parity-strategy-core.md) — Rust parity status and native crate rules

## License

MIT — see [LICENSE](LICENSE).
