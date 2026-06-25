# Strategy Contract Map

This document maps the shared `strategy_core` package to the strategy-facing responsibilities that consumer runtimes such as `trader2` and backtesters implement. The package defines portable contracts and value objects; engines own feeds, adapters, broker execution, persistence, and replay/live semantics.

## Upstream spec alignment

Shared MinuteTemp models and events track these upstream specs:

| Spec | Version | Notable additive changes reflected in `strategy_core` |
|------|---------|--------------------------------------------------------|
| OpenAPI (`minutetemp/go/api/docs/specs/openapi.yaml`) | 1.4.0 | `day_mode` on latest observation reads; `temperature_day_*` / `wu_day_*` on latest observation payloads; `day_of` oracle score mode |
| AsyncAPI (`minutetemp/go/api/docs/specs/asyncapi.yaml`) | 1.13.0 | Same day-bucketing fields on `observation` / `new_high` / `new_low`; `oracle_scores_updated.day_of` and `modes` includes `day_of` |

WebSocket `subscribe.oracle_score_modes` is a connection protocol concern; engines pass through opt-in and surface resulting payloads on `OracleScoresUpdated`.

## Package modules

| Shared package module | Contract role | Engine-owned behavior intentionally excluded |
|---|---|---|
| `strategy_core.context` | Canonical `StrategyContext` protocol and `run(ctx)` handler type. | Context construction, process supervision, and adapter IPC. |
| `strategy_core.events` | Immutable strategy-visible event models. | Provider subscriptions, ordering, replay progression, and fanout. |
| `strategy_core.state` | Strategy-visible weather/forecast/oracle/price value objects plus read-only state view protocol. | Mutable latest-state caches, freshness policy enforcement, and persistence. |
| `strategy_core.data`, `strategy_core.queries`, `strategy_core.minutetemp` | Grouped data-client protocol and MinuteTemp read models. | REST/WebSocket clients, caching, refresh throttling, credentials, and invalidation. |
| `strategy_core.broker` | Strategy-facing order, position, buying-power, and broker protocol types. | Paper simulation, live order placement, reconciliation, risk gates, and ledgers. |
| `strategy_core.runtime`, `strategy_core.capabilities` | Scope facts, runtime mode, clock, one-shot timers, bounded work handles, and feature flags. | Engine scheduling loops and lifecycle supervision. |
| `strategy_core.http` | Optional runtime-mediated HTTP protocol. | Whether HTTP is enabled and how requests are authorized/audited. |
| `strategy_core.telemetry` | Strategy-facing counters, gauges, and structured logging hooks. | Metrics backends, log sinks, and operator status aggregation. |
| `strategy_core.kalshi` | Kalshi REST/WebSocket payload types for engine adapters. | Kalshi clients, authentication, rate limits, and replay storage. |
| `strategy_core.native` | Python-side native-kernel discovery/fallback helper contract. | Native strategy loading policy and engine-specific hot-loop execution. |
| `native/strategy_core` | Rust parity surface for the Python strategy contract. | Engine adapters and provider/broker implementations. |
| `native/strategy_core_kernel` | Narrow native strategy hot-loop context/action/event contract. | Runtime loop ownership and broker/feed side effects. |

## Intentional contract shifts

- Top-level `ctx.fetch_*` helpers do not survive as the primary shared API.
  The shared contract prefers `ctx.data.*`.
- Raw `ctx.queue` access is intentionally not part of the canonical shared contract.
  Replay and paper runtimes may still implement queue internals however they want.
- Sleeve/station/ticker/market-type facts are expected to live under `ctx.runtime.scope`, not as unstructured top-level metadata forever.
- Strategies should not create detached async work with raw `asyncio` task primitives.
  Use `ctx.runtime.wake_at(...)` for future wake events, and use `ctx.runtime.start_work(...)` only for bounded immediate child work caused by current event handling.
- The shared package does not ship feed clients, cache implementations, replay engines, or paper broker implementations.

## Engine adoption notes

- Runtime-specific shortcuts such as direct `ctx.station`, `ctx.sleeve_id`,
  `ctx.tickers`, or `ctx.market_type` should stay out of portable strategy code;
  use `ctx.runtime.scope` instead.
- Engines may expose additional local helpers while migrating, but portable bots
  should use the nested shared surfaces: `ctx.state`, `ctx.data`, `ctx.broker`,
  `ctx.runtime`, `ctx.capabilities`, `ctx.config`, `ctx.telemetry`, and optional
  `ctx.http` only when the runtime advertises HTTP support.
- Engine adapters may support more read families than this package currently
  models. New portable reads should be added here first, then implemented by
  each engine adapter.

## What the shared package guarantees today

- One canonical `run(ctx)` strategy contract
- Shared immutable event models
- Shared strategy-visible weather/forecast/oracle/price value objects
- Shared strategy-visible freshness snapshots and summaries on the read-only `ctx.state` surface
- Shared MinuteTemp OpenAPI-aligned read models for `ctx.data`
- Shared Kalshi OpenAPI/AsyncAPI-aligned exchange models for engine adapters
- Shared broker/data/HTTP/runtime/telemetry protocols
- Small runtime and capability metadata surfaces, including engine clock, one-shot timers, and bounded tracked-work handles
- Additive native-kernel discovery and fallback helpers in `strategy_core.native`
  for runtimes that can execute a native strategy hot loop without changing the
  existing Python `run(ctx)` contract

## What it intentionally does not guarantee yet

- Replay execution semantics
- Full replay progression/drain-loop implementation for tracked work
- Paper/live broker behavior
- Provider/client implementations
- Packaging or publishing strategy beyond normal Python library use
- A bundled native-kernel runtime implementation. `strategy_core.native` only
  defines the Python-side helper/protocol contract; consumer runtimes own their
  engine adapters.
