# Strategy Core

Shared Python contract package for strategy code that should run across the sibling [`trader`](https://github.com/McFalljb/trader) and `backtester` runtimes.

This repo is intentionally a library, not an engine. It is where the shared strategy-facing types live: the canonical `run(ctx)` contract, typed events, shared value objects, and runtime-neutral protocols that consumer repos implement.

## Prerequisites

- [uv](https://docs.astral.sh/uv/)
- Python 3.12+ (the repo default is in `.python-version`)

## Quick start

```bash
uv sync
uv run ruff check . && uv run ruff format --check . && uv run mypy . && uv run pytest
```

Dependency changes must update the lockfile in the same commit: edit `pyproject.toml`, then run `uv lock` (or `uv sync`) and commit `uv.lock`. CI uses `uv sync --frozen --group dev`.

## Public surface

The first-cut contract is organized around one strategy context with nested services:

- `ctx.events()`
- `ctx.state`
- `ctx.data`
- `ctx.broker`
- `ctx.http`
- `ctx.runtime`
- `ctx.capabilities`
- `ctx.config`
- `ctx.telemetry`

`ctx.state` and `ctx.data` intentionally serve different jobs:

- `ctx.state` exposes normalized latest-known runtime snapshots for fast strategy reads
- `ctx.data` exposes engine-owned read methods that return typed MinuteTemp contract models aligned to the upstream OpenAPI surface

Example:

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

`trader` and `backtester` are expected to implement these protocols differently while preserving the same strategy-facing surface.

## Local sibling usage

During development, sibling repos can depend on this package via a local editable path:

```toml
[tool.uv.sources]
strategy-core = { path = "../strategy-core", editable = true }
```

See [docs/contract-map.md](docs/contract-map.md) for the current mapping from `trader` runtime modules to the shared package.

## Layout

- `strategy_core/` — importable typed package (`import strategy_core`)
- `tests/` — pytest suite
- `docs/plans/` — implementation plans
- `docs/brainstorms/` — requirements and idea notes

See [AGENTS.md](AGENTS.md) for the full command reference and repo conventions.
