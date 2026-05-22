# Strategy Contract Map

This document maps the current `trader` strategy-facing contract to the shared `strategy_core` package. It is meant to make later `trader` and `backtester` adoption work explicit rather than implicit.

## Upstream spec alignment

Shared MinuteTemp models and events track these upstream specs:

| Spec | Version | Notable additive changes reflected in `strategy_core` |
|------|---------|--------------------------------------------------------|
| OpenAPI (`minutetemp/go/api/docs/specs/openapi.yaml`) | 1.4.0 | `day_mode` on latest observation reads; `temperature_day_*` / `wu_day_*` on latest observation payloads; `day_of` oracle score mode |
| AsyncAPI (`minutetemp/go/api/docs/specs/asyncapi.yaml`) | 1.13.0 | Same day-bucketing fields on `observation` / `new_high` / `new_low`; `oracle_scores_updated.day_of` and `modes` includes `day_of` |

WebSocket `subscribe.oracle_score_modes` is a connection protocol concern; engines pass through opt-in and surface resulting payloads on `OracleScoresUpdated`.

## Current source modules

| Current `trader` module | Shared package target | Notes |
|---|---|---|
| `trader/engine/context.py` | `strategy_core/context.py` | Shared contract now centers on nested services instead of a flat `fetch_*` surface |
| `trader/engine/events.py` | `strategy_core/events.py` | Event field names remain spec-aligned and immutable |
| `trader/engine/state.py` | `strategy_core/state.py` | Only strategy-visible value objects and the read-only state view moved; mutable cache/runtime logic stays in `trader` |
| `trader/engine/data_access.py` | `strategy_core/data.py` + `strategy_core/queries.py` + `strategy_core/minutetemp.py` | Shared package defines the grouped data-client contract plus spec-aligned MinuteTemp read models, not caching or invalidation behavior |
| `trader/engine/broker.py` | `strategy_core/broker.py` | Shared package defines the strategy-facing broker interface and value objects, not the paper broker implementation |
| `trader` runtime metadata | `strategy_core/runtime.py` + `strategy_core/capabilities.py` | Scope facts now live under runtime metadata rather than as loose top-level fields |
| `trader` logging/metrics internals | `strategy_core/telemetry.py` | Shared package exposes the strategy-facing telemetry interface only |

## Intentional contract shifts

- Top-level `ctx.fetch_*` helpers do not survive as the primary shared API.
  The shared contract prefers `ctx.data.*`.
- Raw `ctx.queue` access is intentionally not part of the canonical shared contract.
  Replay and paper runtimes may still implement queue internals however they want.
- Sleeve/station/ticker/market-type facts are expected to live under `ctx.runtime.scope`, not as unstructured top-level metadata forever.
- The shared package does not ship feed clients, cache implementations, replay engines, or paper broker implementations.

## Known adoption gaps for later runtime plans

- Current `trader` strategies still use direct metadata such as:
  - `ctx.station`
  - `ctx.sleeve_id`
  - `ctx.tickers`
  - `ctx.market_type`
- `trader` still exposes additional helpers not included in the first shared cut, such as:
  - `get_latest_order_metadata`
  - `get_sleeve_equity`
  - `get_daily_pool_remaining`
  - `get_realized_pnl`
  - `get_daily_loss`
  - `get_daily_trades_count`
- `trader` also has more read families than the first contract exposes. The first shared cut only includes the query families proven by current example strategies.

## What the shared package guarantees today

- One canonical `run(ctx)` strategy contract
- Shared immutable event models
- Shared strategy-visible weather/forecast/oracle/price value objects
- Shared strategy-visible freshness snapshots and summaries on the read-only `ctx.state` surface
- Shared MinuteTemp OpenAPI-aligned read models for `ctx.data`
- Shared Kalshi OpenAPI/AsyncAPI-aligned exchange models for engine adapters
- Shared broker/data/HTTP/runtime/telemetry protocols
- Small runtime and capability metadata surfaces

## What it intentionally does not guarantee yet

- Replay execution semantics
- Paper/live broker behavior
- Provider/client implementations
- Packaging or publishing strategy beyond normal Python library use
