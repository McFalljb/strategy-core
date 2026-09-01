# Strategy Core Bot API and Usage Guide

This is the canonical, one-stop reference for writing bots against
`strategy-core`. It describes every portable bot entry point, context surface,
event, option, return type, helper, and native execution path currently exposed
by the Python and Rust packages.

`strategy-core` is a library, not an engine. Trader and Backtester construct the
context, deliver events, implement data access and broker behavior, and decide
which optional capabilities are available. A bot that stays inside the shared
surface can run in paper, replay, backtest, or live runtimes without changing
its strategy-facing API.

The package is still version `0.1.x`; treat the types and signatures documented
here as an early public contract that may change before `1.0.0`.

This document is complete for the shared bot contract. Runtime installation,
process configuration, data-source credentials, replay CLI flags, and live
safety gates remain engine-owned; use
[Trader's bot guide](https://github.com/McFalljb/trader/blob/main/docs/bots.md)
or the
[Backtester runner guide](https://github.com/McFalljb/backtester/blob/main/docs/running-backtester.md)
after choosing an execution path here.

## Contents

- [Python and broad Rust parity contract](#python-and-broad-rust-parity-contract)
- [Choose a bot execution path](#choose-a-bot-execution-path)
- [Install and import](#install-and-import)
- [Minimal Python bot](#minimal-python-bot)
- [The complete strategy context](#the-complete-strategy-context)
- [Events](#events)
- [Runtime, scope, clock, timers, and bounded work](#runtime-scope-clock-timers-and-bounded-work)
- [Latest state and freshness](#latest-state-and-freshness)
- [Explicit data reads](#explicit-data-reads)
- [Broker and orders](#broker-and-orders)
- [Configuration](#configuration)
- [HTTP](#http)
- [Telemetry and logging](#telemetry-and-logging)
- [Portable helpers](#portable-helpers)
- [Optional native execution and fallback](#optional-native-execution-and-fallback)
- [Broad Rust strategies](#broad-rust-strategies)
- [Native Rust kernel](#native-rust-kernel)
- [Adapter-only provider models](#adapter-only-provider-models)
- [Portability and ownership rules](#portability-and-ownership-rules)

## Python and broad Rust parity contract

Python is the contract authority. The broad Rust crate covers the same portable
wire values, helper behavior, and strategy-facing operations. “Parity” means a
bot can express the same strategy decision and observe the same contract data;
it does not mean Python and Rust use identical syntax or in-memory types.

The conformance inventory in
`tests/fixtures/conformance/manifest.json` currently records:

| Contract group | Python | Broad Rust | Difference |
|---|---:|---:|---|
| Serializable models, enums, queries, and values | 142 | 142 | None |
| Portable helpers and constants | 24 | 24 | None |
| Strategy-facing traits/protocols | 16 | 16 | None |
| Shared type aliases | 7 | 7 | None |
| Native-unavailability error names | 2 | 2 | None |
| Native fallback callable alias | 1 | 0 | Python names it `FallbackHandler`; Rust represents the same callback as a generic closure/future. |
| Rust representation support | 0 | 14 | Rust-only enums, aliases, and error/result carriers needed to express Python unions and exceptions safely. |
| Borrowed native-kernel contract | 0 | 0 | 45 additional names exist only in the kernel crate; they are not part of broad Python/Rust parity. |

The first five rows are the parity surface: names match one-for-one. Additional
Rust root exports are `DateLike`, `EventDelivery`, `JsonObject`, `JsonValue`,
`MarketType`, `PersistenceStatus`, `PriceLevel`, `ForecastRunLookup`,
`OracleScoreDays`, `ClimateDayError`, `FeeError`, `FeeResult`,
`NativeKernelRunError`, and `StationError`. They do not add Rust-only strategy
features: some give a Rust name to a module-local Python alias or union, and the
rest are typed Rust carriers for Python exception behavior.

### Type mapping

| Python | Broad Rust | Contract meaning |
|---|---|---|
| `str` / `Literal[...]` / `StrEnum` | `String`, `&str`, or a Rust enum | Closed Rust enums serialize to the same string values as Python literals. |
| `int` | `i64` | Contract integer within the signed 64-bit range. |
| `float` | `f64` | Contract number; non-finite helper inputs are rejected where documented. |
| `datetime` | `DateTime<Utc>` | Timezone-aware UTC instant serialized with the same JSON timestamp format. |
| `date` | `NaiveDate` | Calendar date serialized as `YYYY-MM-DD`. |
| `T | None` | `Option<T>` | Optional value / JSON `null`. |
| `list[T]` or `tuple[T, ...]` | `Vec<T>` | Ordered JSON array. |
| `dict[str, T]` or `Mapping[str, T]` | `BTreeMap<String, T>` | JSON object; key order is not semantically significant. |
| Frozen dataclass or Pydantic model | Rust struct | Same public JSON field names, defaults, omission, and null behavior. |
| Discriminated union | Rust enum | Same JSON variants and discriminator values. |
| Raised exception | `Result<T, E>` | Same success/error category where behavior is portable; Rust exposes typed errors. |
| Async protocol method | Method returning `impl Future` | Same asynchronous operation. |

### Context operation mapping

| Operation | Python | Broad Rust |
|---|---|---|
| Receive next event | `async for event in ctx.events()` | `while let Some(event) = ctx.next_event().await` |
| Latest state | `ctx.state` | `ctx.state()` |
| Explicit data reads | `ctx.data` | `ctx.data()` |
| Broker | `ctx.broker` | `ctx.broker()` |
| HTTP | `ctx.http` | `ctx.http()` |
| Runtime | `ctx.runtime` | `ctx.runtime()` |
| Capabilities | `ctx.capabilities` | `ctx.capabilities()` |
| Configuration | `ctx.config` | `ctx.config()` |
| Telemetry | `ctx.telemetry` | `ctx.telemetry()` |
| Oracle selector read | `get_oracle_scores(station, days=..., mode=..., rank_by=...)` | `get_oracle_scores_matching(station, days, mode, rank_by)`; unfiltered Rust lookup remains `get_oracle_scores(station)` for compatibility. |
| Full broker intent | `await broker.place_order(**intent_fields)` | `broker.place_order_with_intent(OrderIntent { ... })` |
| Native fallback | `run_native_or_fallback(ctx, kernel, fallback=..., require_native=...)` | `run_native_or_fallback(&mut ctx, &mut kernel, fallback, require_native)` |

All model, event, query, result, helper, and literal tables in this guide apply
to both Python and the broad Rust crate unless a section explicitly says
otherwise.

### Portable-subset caveats

These are language/API-shape differences, not separate strategy features:

- Rust event structs store the serialized JSON `type` field as `event_type`;
  dispatch through `StrategyEvent::<Variant>` rather than comparing that field.
  `WeatherEvent` also has a payload field named `event_type`; Rust calls that
  field `event_type_name` while still serializing it as `event_type`.
- Python logger methods accept `*args` and `**kwargs`; broad Rust logger methods
  accept one `&str`. Preformat a single message for code intended to translate
  directly between languages.
- Python HTTP query parameters are scalar JSON values. Rust's `HttpParams` uses
  `JsonValue` and can represent arrays or objects, but portable bots must stay
  within the Python scalar subset: `str`, `int`, `float`, `bool`, or `None`.
- Python exposes `runtime_identity` as a mapping. Rust stores it as `JsonValue`;
  portable runtimes and bots should use a JSON object.
- Python configuration values are typed as `object`; Rust configuration values
  are `JsonValue`. Use only JSON-compatible configuration values in portable
  bots.
- Python broker mutations are async. The current broad Rust broker trait returns
  synchronous `Result` values; the runtime adapter owns any asynchronous engine
  boundary behind that trait.

### Behavioral-parity enforcement

The conformance manifest proves public inventory, JSON structure, helper
behavior, and trait implementability. The Rust traits also normalize the two
call surfaces that previously depended on adapter discipline:

- Rust broker implementers must implement `place_order_with_intent(OrderIntent)`
  and therefore receive every execution field. The shorter legacy
  `place_order(...)` method is a default wrapper that constructs a complete
  `OrderIntent` with explicit defaults for advanced fields.
- Rust data implementers receive exactly one typed query object for each read.
  There are no duplicate positional values and therefore no adapter-specific
  precedence decision.

Interface tests cover full broker-intent preservation, legacy normalization,
canonical query calls, and query defaults.

## Choose a bot execution path

| Path | Entry point | Best for | Portability |
|---|---|---|---|
| Python strategy | `async def run(ctx: StrategyContext) -> None` | Normal bot development and unchanged execution through Python-capable Trader or Backtester adapters. | Canonical and recommended. |
| Broad Rust strategy | `StrategyHandler<C>` with a `StrategyContext` implementation | Rust strategies that want owned, serializable models and the same broad services as Python. | Semantically aligned with Python; the runtime supplies the concrete context. |
| Native Rust kernel | `strategy_core_kernel::NativeKernel` | Allocation-sensitive event loops using borrowed event and state views. | Portable across runtimes that advertise native-kernel support, but intentionally narrower than the broad contract. |
| Python wrapper with native fallback | `run_native_or_fallback(...)` | One Python entry point that prefers a native kernel and can fall back to Python. | Check `supports_native_kernels`; choose whether native execution is optional or required. |

The broad Rust crate is `native/strategy_core`. The narrow kernel crate is
`native/strategy_core_kernel` and is also re-exported as
`strategy_core::kernel`. The kernel is not an incomplete copy of the broad API;
it is a separate hot-loop contract.

## Install and import

Python requires Python 3.12 or newer. Consumer projects can use a sibling
checkout or Git source:

```toml
[project]
dependencies = ["strategy-core"]

[tool.uv.sources]
strategy-core = { path = "../strategy-core", editable = true }
# Or:
# strategy-core = { git = "https://github.com/McFalljb/strategy-core.git", rev = "main" }
```

Most bot-facing names are re-exported from the package root:

```python
from strategy_core import (
    ForecastUpdated,
    Observation,
    PriceUpdate,
    ShutdownEvent,
    StrategyContext,
    TimerWake,
)
```

Rust consumers add the broad or kernel crate through their workspace dependency
configuration. The broad crate re-exports its bot-facing models and traits from
`strategy_core::*`.

```toml
[dependencies]
# Local sibling checkout:
strategy-core = { path = "../strategy-core/native/strategy_core" }
strategy-core-kernel = { path = "../strategy-core/native/strategy_core_kernel" }

# Or use Git for either package:
# strategy-core = { git = "https://github.com/McFalljb/strategy-core.git", branch = "main" }
# strategy-core-kernel = { git = "https://github.com/McFalljb/strategy-core.git", branch = "main" }
```

## Minimal Python bot

Every portable Python bot exposes one async handler:

```python
from __future__ import annotations

from strategy_core import (
    ForecastUpdated,
    Observation,
    PriceUpdate,
    ShutdownEvent,
    StrategyContext,
    TimerWake,
)


async def run(ctx: StrategyContext) -> None:
    ctx.telemetry.logger.info(
        "strategy started",
        extra={"sleeve_id": ctx.runtime.scope.sleeve_id},
    )

    async for event in ctx.events():
        if isinstance(event, Observation):
            if event.temperature_f is not None:
                ctx.telemetry.gauge(
                    "observed_temperature_f",
                    event.temperature_f,
                    fields={"station": event.station_id},
                )
        elif isinstance(event, PriceUpdate):
            for market in event.markets:
                ctx.telemetry.gauge(
                    "yes_price",
                    market.yes_price,
                    fields={"ticker": market.ticker},
                )
        elif isinstance(event, ForecastUpdated):
            await ctx.data.fetch_forecast(model_id=event.model_id)
        elif isinstance(event, TimerWake):
            ctx.telemetry.counter("timer_wakes", fields={"name": event.name})
        elif isinstance(event, ShutdownEvent):
            break
```

Use `isinstance` for typed dispatch or branch on `event.type`. Events are frozen
Pydantic models and should be treated as immutable inputs.

## The complete strategy context

`StrategyContext` exposes exactly these surfaces:

| Surface | Type | Use |
|---|---|---|
| `ctx.events()` | `AsyncIterator[StrategyEvent]` | Receive the runtime's typed strategy event stream. |
| `ctx.state` | `MarketStateView` | Read latest-known weather, forecast, oracle, price, and freshness snapshots synchronously. |
| `ctx.data` | `StrategyDataClient` | Perform explicit engine-owned MinuteTemp reads asynchronously. |
| `ctx.broker` | `Broker` | Place and cancel orders and inspect positions, pending orders, and sleeve buying power. |
| `ctx.http` | `HttpClient` | Make runtime-mediated HTTP requests only when `supports_http` is true. |
| `ctx.runtime` | `StrategyRuntime` | Read mode, run identity, sleeve scope, clock, timers, and bounded-work facilities. |
| `ctx.capabilities` | `RuntimeCapabilities` | Discover optional behavior before using it. |
| `ctx.config` | `Mapping[str, object]` | Read strategy parameters supplied by the runtime. |
| `ctx.telemetry` | `Telemetry` | Emit structured logs, counters, gauges, and annotations. |

There are no portable top-level `ctx.fetch_*` methods and no portable raw
`ctx.queue`. Use the nested surfaces above.

### Public aliases used by the context

| Alias/protocol | Definition |
|---|---|
| `Action` | `"buy" | "sell"` |
| `ContractSide` | `"yes" | "no"` |
| `OrderExecutionStyle` | `"resting_limit" | "direct" | "sweep"` |
| `OrderTimePolicy` | `"good_till_canceled" | "immediate_or_cancel" | "fill_or_kill"` |
| `BrokerUpdateStatus` | The complete broker-transition status set documented in the broker section. |
| `OrderId` | `str` |
| `FeeType` | `"quadratic" | "quadratic_with_maker_fees" | "flat"` |
| `LiquidityRole` | `"maker" | "taker"` |
| `HttpMethod` | `"GET" | "POST" | "PUT" | "PATCH" | "DELETE"` |
| `LocalDateLike` | `date | str` |
| `JSONValue` | A recursive JSON primitive, list, or `dict[str, JSONValue]`. |
| `JSONObject` | `dict[str, JSONValue]` |
| `StrategyHandler` | `Callable[[StrategyContext], Awaitable[None]]`, normally implemented as `async def run(ctx)`. |
| `StrategyLogger` | Structured logger protocol used by `ctx.telemetry.logger`. |
| `TelemetryField` | `str | int | float | bool | None` |
| `TelemetryFields` | `Mapping[str, TelemetryField]` |
| `ReportScheduleEntry` | One report schedule or a tuple of report schedules. |
| `FallbackHandler` | `Callable[[], Awaitable[None]]` |
| `NativeKernelStatus` | `"completed" | "fallback_completed"` |
| `NativeStrategyContext` | `StrategyContext` extended with `native_kernel_runner`. |
| `NativeKernelUnavailableError` | Raised when required native execution is unavailable; `NativeKernelUnavailable` is its compatibility alias. |

## Events

`StrategyEvent` and its compatibility alias `EngineEvent` are a discriminated
union on the `type` field. The shared union has 11 variants:

| Python type | Broad Rust variant | JSON `type` | What it means | Typical bot action |
|---|---|---|---|---|
| `Observation` | `StrategyEvent::Observation` | `"observation"` | A station observation changed. | Inspect the payload or read the normalized latest weather state. |
| `PriceUpdate` | `StrategyEvent::PriceUpdate` | `"price_update"` | One or more scoped market brackets changed. | Read brackets directly or use the latest-state price lookup. |
| `ForecastUpdated` | `StrategyEvent::ForecastUpdated` | `"forecast_updated"` | A model version changed; this is an invalidation/update hint. | Read latest state or fetch the forecast. |
| `ForecastVersions` | `StrategyEvent::ForecastVersions` | `"forecast_versions"` | Bootstrap map of model ids to versions for a station. | Compare versions or decide which models to fetch. |
| `OracleScoresUpdated` | `StrategyEvent::OracleScoresUpdated` | `"oracle_scores_updated"` | Oracle rankings changed, optionally with mode-specific payloads. | Use the included table or read/fetch the desired mode and rank dimension. |
| `StationReport` | `StrategyEvent::StationReport` | `"station_report"` | An official station report was published or revised. | React to report values or fetch report history. |
| `WeatherEvent` | `StrategyEvent::WeatherEvent` | `"weather_event"` | A weather-event lifecycle update occurred. | React to the event state, tier, or source details. |
| `NewHigh` | `StrategyEvent::NewHigh` | `"new_high"` | The running station high increased. | Re-evaluate high-temperature markets. |
| `NewLow` | `StrategyEvent::NewLow` | `"new_low"` | The running station low decreased. | Re-evaluate low-temperature markets. |
| `TimerWake` | `StrategyEvent::TimerWake` | `"timer_wake"` | A one-shot wake scheduled through the runtime fired. | Perform the named deferred check. |
| `ShutdownEvent` | `StrategyEvent::ShutdownEvent` | `"shutdown"` | The runtime requested clean strategy shutdown. | Finish current synchronous cleanup and leave the event loop. |

### Common event metadata

Provider-facing events generally expose `event_id`, `sequence`, `emitted_at`,
`slug`, and `station_id`. Station/city events may also expose `city_sequence`.
Fields are optional when an upstream source or replay record did not provide
them. Required text fields reject empty strings.

### Complete event fields

| Event/value type | Fields |
|---|---|
| `Observation` | `type`, `event_id`, `sequence`, `city_sequence`, `emitted_at`, `slug`, `station_id`, `observed_at`, `lag_seconds`, `preliminary`, `temperature_f`, `temperature_c`, `temp_min_f`, `temp_max_f`, `temp_min_c`, `temp_max_c`, `is_from_report`, `report_type`, `source_report_id`, `wu_current_temp_f`, `wu_current_temp_c`, `wu_daily_high_f`, `wu_daily_low_f`, `wu_daily_high_c`, `wu_daily_low_c`, `wu_observation_time`, `wu_fetched_at`, `temperature_day_mode`, `temperature_day_date`, `wu_day_mode`, `wu_day_date`, `dewpoint`, `heat_index`, `wind_chill`, `relative_humidity`, `wind_speed`, `wind_direction`, `wind_gust`, `text_description`. |
| `PriceUpdate` | `type`, `event_id`, `sequence`, `city_sequence`, `emitted_at`, `source`, `slug`, `station_id`, `city_id`, `timestamp`, `markets`. |
| `MarketBracket` | `market_id`, `ticker`, `yes_price`, `no_price`, `event_ticker`, `event_date`, `strike_type`, `floor_strike`, `cap_strike`, `snapshot_time`, `yes_bid`, `yes_ask`, `no_bid`, `no_ask`, `yes_bid_depth`, `yes_ask_depth`, `no_bid_depth`, `no_ask_depth`, `yes_bid_levels`, `yes_ask_levels`, `no_bid_levels`, `no_ask_levels`, `orderbook_depth`, `volume`. Each price level is `(price, quantity)`. |
| `ForecastUpdated` | `type`, `event_id`, `sequence`, `emitted_at`, `slug`, `station_id`, `model_id`, `version`. |
| `ForecastVersions` | `type`, `event_id`, `sequence`, `emitted_at`, `slug`, `station_id`, `versions`. |
| `OracleScoresUpdated` | `type`, `event_id`, `sequence`, `emitted_at`, `slug`, `station_id`, `modes`, `updated_at`, `overall`, `day_ahead`, `day_of`. |
| `OracleScoreTable` | `station_id`, `range_start`, `range_end`, `days_requested`, `all_time`, `score_mode`, `rank_by`, `scores`. |
| `OracleScoreRow` | `model_id`, `model_name`, `is_public`, `combined_mae`, `high_mae`, `low_mae`, `high_bias`, `low_bias`, `day_count`. |
| `StationReport` | `type`, `event_id`, `sequence`, `city_sequence`, `emitted_at`, `slug`, `station_id`, `report_id`, `report_revision`, `report_updated_at`, `report_type`, `report_date`, `issuance_time`, `fetched_at`, `source_url`, `provider`, `max_temp_f`, `max_temp_c`, `max_temp_time_utc`, `min_temp_f`, `min_temp_c`, `min_temp_time_utc`, `temp_f`, `temp_c`. |
| `WeatherEvent` | `type`, `event_id`, `sequence`, `city_sequence`, `emitted_at`, `slug`, `station_id`, `id`, `event_type`, `tier`, `state`, `name`, `badge`, `detail`, `summary`, `started_at`, `last_confirmed_at`, `ended_at`, `source`. |
| `WeatherEventSource` | `metar_type`, `flight_category`, `wx_string`, `wx_token`, `wind_speed_kt`, `wind_gust_kt`, `peak_wind_kt`, `peak_wind_direction`, `visibility_mi`, `cb_location`. |
| `NewHigh` / `NewLow` | `type`, `event_id`, `sequence`, `city_sequence`, `emitted_at`, `event_key`, `source_timestamp`, `wmo_emit_time`, `producer_received_at`, `live_published_at`, `persistence_status`, `producer_sequence`, `slug`, `station_id`, `value_f`, `value_c`, `prev_value_f`, `observed_at`, `temperature_day_mode`, `temperature_day_date`, `is_from_report`, `report_type`, `source_report_id`. |
| `TimerWake` | `type`, `scheduled_for`, `fired_at`, `name`. |
| `ShutdownEvent` | `type`, `reason`. |

Event literal options are:

- `temperature_day_mode`: `"calendar_day"` or `"nws_climate_day"`.
- `wu_day_mode`: `"calendar_day"`.
- high/low `persistence_status`: `"uncommitted"`, `"committed"`, or
  `"failed"`.
- oracle modes: `"overall"`, `"day_ahead"`, or `"day_of"`.

Do not assume every runtime wakes a bot for every raw price change. Delivery
policy is runtime/configuration-owned; read `ctx.capabilities.event_delivery`
and the selected engine's bot configuration.

## Runtime, scope, clock, timers, and bounded work

### Runtime fields

| Field | Type/options | Meaning |
|---|---|---|
| `ctx.runtime.mode` | `RuntimeMode.PAPER`, `.REPLAY`, `.LIVE` (`"paper"`, `"replay"`, `"live"`) | Current execution mode. Never infer safety from repository or hostname. |
| `ctx.runtime.run_id` | `str` | Runtime-assigned run identity. Useful in telemetry and idempotency keys. |
| `ctx.runtime.scope` | `StrategyScope` | Facts for the current strategy sleeve. |
| `ctx.runtime.clock` | `EngineClock` | Runtime clock that stays deterministic in replay. |
| `ctx.runtime.runtime_identity` | `Mapping[str, object]` | Additional runtime identity facts. Keys are runtime-defined. |

`StrategyScope` has:

| Field | Type/options |
|---|---|
| `sleeve_id` | `str` |
| `strategy_name` | `str` |
| `station_id` | `str | None` |
| `tickers` | `tuple[str, ...]` |
| `market_type` | `"high" | "low" | "hourly" | None` |
| `event_ticker` | `str | None` |
| `event_date` | `date | None` |

Use scope fields instead of parsing sleeve ids or hard-coding configured
tickers. A portable bot should only submit orders for tickers in
`ctx.runtime.scope.tickers`.

### Engine clock

```python
now = ctx.runtime.clock.now()
await ctx.runtime.clock.sleep(0.25)
await ctx.runtime.clock.sleep_until(when)
```

Use the engine clock rather than `datetime.now()` or `asyncio.sleep()` for logic
whose timing must replay consistently.

Broad Rust uses the same clock operations:

```rust
use strategy_core::{EngineClock, StrategyContext, StrategyRuntime};

let now = ctx.runtime().clock().now();
ctx.runtime().clock().sleep(0.25).await;
ctx.runtime().clock().sleep_until(when).await;
```

### One-shot timers

```python
if ctx.capabilities.supports_one_shot_timers:
    handle = ctx.runtime.wake_at(when, name="recheck-signal")
    if signal_invalidated:
        handle.cancel()
```

`TimerHandle` exposes `cancelled` and `cancel()`. A fired timer arrives as a
`TimerWake`. The shared contract has no recurring-timer method even though the
capability object reserves `supports_recurring_timers`; recurring scheduling is
not portable until a shared method exists.

The Rust equivalent is
`ctx.runtime().wake_at(when, Some("recheck-signal"))`; its returned
`TimerHandle` has `cancelled()` and `cancel()`.

### Bounded immediate work

```python
async def refresh_model() -> None:
    await ctx.data.fetch_forecast(refresh=True)


handle = ctx.runtime.start_work(lambda: refresh_model(), name="refresh-model")
```

`WorkHandle` exposes `cancelled`, `done`, `exception`, and `cancel()`. Work must
be bounded, caused by the current event, and runtime-owned. Do not create
detached `asyncio` tasks. A runtime can reject work when it is suspended or when
tracked work is unavailable.

Rust passes the future directly rather than a Python callable factory:

```rust
let handle = ctx
    .runtime()
    .start_work(async move { do_bounded_work().await }, Some("refresh-model"));
```

The returned Rust `WorkHandle` exposes `cancelled()`, `done()`, `exception()`,
and `cancel()`.

### Capability flags

| Flag | Default | Meaning |
|---|---:|---|
| `supports_http` | `False` | `ctx.http` may make requests. |
| `supports_data_queries` | `True` | `ctx.data` reads are available. |
| `supports_one_shot_timers` | `False` | `ctx.runtime.wake_at` is available. |
| `supports_recurring_timers` | `False` | Reserved capability; no portable recurring API exists yet. |
| `supports_native_kernels` | `False` | The context may expose a `NativeKernelRunner`. |
| `queue_is_durable` | `False` | Runtime event delivery survives according to a durable queue contract. |
| `replay_controls_event_progression` | `False` | Replay owns progression and may wait for event-scoped work. |
| `event_delivery` | `"wake"` | `"wake"` delivers event wakeups; `"decision"` may suppress non-decision events while still updating state. |

Capability flags describe availability, not permission to bypass the shared
API. For example, `supports_http` does not make hidden provider reads portable.

## Latest state and freshness

`ctx.state` is synchronous, read-only, and intended for the latest normalized
snapshot. It exposes:

```python
weather = ctx.state.get_weather(station)
forecast = ctx.state.get_forecast(station)
oracle = ctx.state.get_oracle_scores(
    station,
    days=7,
    mode="day_of",
    rank_by="high",
)
prices = ctx.state.get_prices(ticker)

weather_freshness = ctx.state.get_weather_freshness(station)
forecast_freshness = ctx.state.get_forecast_freshness(station)
oracle_freshness = ctx.state.get_oracle_scores_freshness(station)
price_freshness = ctx.state.get_price_freshness(ticker)
summary = ctx.state.freshness_summary()
```

Broad Rust uses the same reads through accessors. Its selector-aware oracle
method has a distinct compatibility name:

```rust
use strategy_core::{MarketStateView, OracleScoreDays, StrategyContext};

let weather = ctx.state().get_weather("KNYC");
let forecast = ctx.state().get_forecast("KNYC");
let oracle = ctx.state().get_oracle_scores_matching(
    "KNYC",
    Some(OracleScoreDays::from(7_i64)),
    Some("day_of"),
    Some("high"),
);
let prices = ctx.state().get_prices(ticker);
let price_freshness = ctx.state().get_price_freshness(ticker);
let summary = ctx.state().freshness_summary();
```

The four value reads return `None` when no matching state exists. Freshness
reads always return a `FreshnessSnapshot` so the bot can distinguish fresh,
stale, and missing data.

### Freshness values

- `FreshnessStatus`: `"fresh"`, `"stale"`, `"missing"`.
- `FreshnessDomain`: `"weather"`, `"forecast"`, `"oracle"`, `"price"`.
- `FreshnessSnapshot` fields: `domain`, `key`, `status`, `source`, `updated_at`,
  `observed_at`, `stale_after_seconds`, `age_seconds`, `invalidation_reason`,
  `detail`; convenience properties: `is_stale`, `is_missing`.
- `FreshnessDomainSummary` fields: `domain`, `tracked_count`, `fresh_count`,
  `stale_count`, `stalest_age_seconds`.
- `FreshnessSummary` fields: `as_of`, `domains`; computed properties:
  `tracked_count`, `stale_count`.

Safe decision pattern:

```python
from strategy_core import StrategyContext, TickerPrices


def fresh_prices(ctx: StrategyContext, ticker: str) -> TickerPrices | None:
    prices = ctx.state.get_prices(ticker)
    freshness = ctx.state.get_price_freshness(ticker)
    if prices is None or freshness.is_missing or freshness.is_stale:
        return None
    return prices
```

### State value objects

| Type | Fields |
|---|---|
| `StationWeather` | `current_temp`, `running_high`, `running_low`, `last_metar_time`, `temp_min_f`, `temp_max_f`, `temp_min_c`, `temp_max_c`, `preliminary`, `lag_seconds`, `wu_current_temp_f`, `wu_current_temp_c`, `wu_daily_high_f`, `wu_daily_low_f`, `wu_daily_high_c`, `wu_daily_low_c`, `wu_observation_time`, `wu_fetched_at`, `asos_daily_high_f`, `asos_daily_low_f`, `dewpoint`, `heat_index`, `wind_chill`, `relative_humidity`, `wind_speed`, `wind_direction`, `wind_gust`, `text_description`, `dsm_high`, `dsm_low`, `dsm_high_time`, `dsm_low_time`, `six_hr_high`, `six_hr_low`, `last_dsm_time`, `last_six_hr_time`. |
| `StationForecast` | `model_forecasts`, `updated_at`. `model_forecasts` is an immutable mapping keyed by model id. |
| `ModelForecast` | `model_id`, `value`, `version`, `updated_at`, `run_issued_at`, `hourly`. |
| `ForecastHourly` | `time`, `temperature_2m_f`, `temperature_2m_c`, `apparent_temperature_f`, `relative_humidity_2m`, `dew_point_2m`, `pressure_msl`, `wind_speed_10m`, `wind_direction_10m`, `wind_gusts_10m`, `cloud_cover`, `precipitation_probability`. |
| `StationOracleScores` | `station_id`, `scores`, `rank_by`, `score_mode`, `days_requested`, `range_start`, `range_end`, `updated_at`. |
| `OracleModelScore` | `model_id`, `model_name`, `combined_mae`, `high_mae`, `low_mae`, `high_bias`, `low_bias`, `day_count`, `is_public`. |
| `TickerPrices` | `ticker`, `source`, `event_ticker`, `event_date`, `series_ticker`, `fee_type`, `fee_multiplier`, `strike_type`, `floor_strike`, `cap_strike`, `yes_price`, `no_price`, `yes_bid`, `yes_ask`, `no_bid`, `no_ask`, `yes_bid_depth`, `yes_ask_depth`, `no_bid_depth`, `no_ask_depth`, `yes_bid_levels`, `yes_ask_levels`, `no_bid_levels`, `no_ask_levels`, `orderbook_depth`, `volume`, `peak_yes_ask`, `last_update`. |

Price levels are immutable `(price, quantity)` tuples. `TickerPrices.fee_type`
uses `"quadratic"`, `"quadratic_with_maker_fees"`, or `"flat"` when known.

## Explicit data reads

`ctx.data` is asynchronous and engine-owned. Use it for explicit reads; use
`ctx.state` for hot latest-state decisions. Check
`ctx.capabilities.supports_data_queries` before relying on data access in a
runtime that may omit it.

Every method supports keyword arguments, and every read also has a frozen query
object. Choose one style per call rather than mixing a query object with
duplicate keyword options.

```python
from strategy_core import OracleScoresQuery

scores = await ctx.data.fetch_oracle_scores(
    days="30",
    mode="day_of",
    rank_by="high",
    refresh=False,
)

same_scores = await ctx.data.fetch_oracle_scores(
    OracleScoresQuery(
        days="30",
        mode="day_of",
        rank_by="high",
        refresh=False,
    )
)
```

Broad Rust keeps the same method names, returns a typed `Result`, and uses one
query object as the canonical call shape:

```rust
use strategy_core::{OracleScoresQuery, StrategyContext, StrategyDataClient};

let scores = ctx
    .data()
    .fetch_oracle_scores(OracleScoresQuery {
        days: "30".to_string(),
        mode: "day_of".to_string(),
        rank_by: "high".to_string(),
        refresh: false,
    })
    .await?;
```

Use `QueryType::default()` for the Python keyword defaults. A Rust runtime
cannot receive conflicting query-object and positional values because the trait
accepts only the query object.

### Complete data-client methods

| Method | Options | Returns |
|---|---|---|
| `fetch_limits(...)` | `LimitsQuery(refresh=False)` or `refresh=` | `EffectiveLimits` |
| `fetch_forecast(...)` | `ForecastQuery(model_id=None, refresh=False)` or `model_id=`, `refresh=` | `StationForecastData | None` |
| `fetch_oracle_scores(...)` | `OracleScoresQuery(days="7", mode="day_ahead", rank_by="high", refresh=False)` or the same keywords | `OracleScoreData | None` |
| `fetch_forecast_runs(...)` | `ForecastRunsQuery(model_id=None, start=None, end=None, limit=None, cursor=None, refresh=False)` or the same keywords | `ForecastRunsPage` |
| `fetch_forecast_run(run_id_or_query, ...)` | A run-id string or `ForecastRunQuery(run_id, refresh=False)`; string form also accepts `refresh=` | `ForecastRunData | None` |
| `fetch_latest_reports(...)` | `LatestReportsQuery(include_baseline=False, refresh=False)` or `include_baseline=`, `refresh=` | `LatestReportsData` |
| `fetch_reports(...)` | `ReportsQuery(report_type=None, date=None, refresh=False)` or `report_type=`, `date=`, `refresh=` | `StationReportsData` |
| `fetch_report_history(...)` | `ReportHistoryQuery(report_type=None, start=None, end=None, limit=None, cursor=None, refresh=False)` or the same keywords | `StationReportHistoryPage` |
| `fetch_latest_observation(...)` | `LatestObservationQuery(day_mode=None, refresh=False)`; the method's keyword-only surface exposes `refresh`, while `day_mode` is supplied through the query object | `LatestObservationData` |

Broad Rust uses the same nine method names with canonical query arguments. Each
method returns a future whose output is the listed `Result`:

| Method | Broad Rust query argument | Broad Rust result |
|---|---|---|
| `fetch_limits` | `LimitsQuery` | `Result<EffectiveLimits, Error>` |
| `fetch_forecast` | `ForecastQuery` | `Result<Option<StationForecastData>, Error>` |
| `fetch_oracle_scores` | `OracleScoresQuery` | `Result<Option<OracleScoreData>, Error>` |
| `fetch_forecast_runs` | `ForecastRunsQuery` | `Result<ForecastRunsPage, Error>` |
| `fetch_forecast_run` | `ForecastRunQuery` | `Result<Option<ForecastRunData>, Error>` |
| `fetch_latest_reports` | `LatestReportsQuery` | `Result<LatestReportsData, Error>` |
| `fetch_reports` | `ReportsQuery` | `Result<StationReportsData, Error>` |
| `fetch_report_history` | `ReportHistoryQuery` | `Result<StationReportHistoryPage, Error>` |
| `fetch_latest_observation` | `LatestObservationQuery` | `Result<LatestObservationData, Error>` |

`ForecastRunLookup` represents Python's run-id-or-query input union for Rust
callers. Convert `ForecastRunLookup::RunId(id)` or
`ForecastRunLookup::Query(query)` with `.into_query()` before calling
`fetch_forecast_run`; a plain run id normalizes to `refresh=false`.

Use `refresh=False` in decision paths unless a deliberate provider refresh is
required. The runtime defines caching, credentials, rate limits, and how the
implicit sleeve/station scope maps to an upstream request.

### Data literal and range options

| Option | Values |
|---|---|
| `ReportType` | `"cli"`, `"dsm"`, `"metar_tgroup"`, `"metar_6hr"` |
| `TemperatureUnit` | `"F"`, `"C"` |
| `PlanTier` | `"starter"`, `"pro"`, `"clanker"` |
| `DataResolution` | `"1m"`, `"5m"`, `"10m"` |
| `TemperatureDayMode` | `"calendar_day"`, `"nws_climate_day"` |
| `WuDayMode` | `"calendar_day"` |
| `OracleScoreMode` | `"overall"`, `"day_ahead"`, `"day_of"` |
| `OracleRankBy` | `"combined"`, `"high"`, `"low"` |
| `ReportScheduleBasis` | `"utc"`, `"local"` |
| Forecast run `start` / `end` | `datetime`, ISO-like string, or `None` |
| Report `date` / history `start` / `end` | `date`, local-date string, or `None` |
| Pagination | `limit: int | None`, `cursor: str | None`; response pages expose `next_cursor`. |

`days` is a provider-defined string with default `"7"`. Pick `rank_by="high"`
for high-temperature markets, `"low"` for low-temperature markets, or
`"combined"` for direction-neutral model comparison.

### Data response objects

Python response objects are frozen dataclasses whose collection fields are
immutable tuples or mappings. Broad Rust uses owned serde structs with the same
wire fields; collections are `Vec` or `BTreeMap` values.

| Type | Fields |
|---|---|
| `CityInfo` | `id`, `slug`, `name`, `timezone` |
| `StationInfo` | `station_id`, `name`, `temperature_unit`, `uses_nws_climate_day` |
| `ObservationRecord` | `observation_time`, `temperature_f`, `temperature_c`, `dewpoint`, `heat_index`, `wind_chill`, `relative_humidity`, `barometric_pressure`, `sea_level_pressure`, `wind_speed`, `wind_direction`, `wind_gust`, `text_description`, `precipitation_1h`, `precipitation_3h`, `precipitation_6h`, `is_locf`, `is_from_report`, `report_type`, `source_report_id`, `temp_min_f`, `temp_max_f`, `temp_min_c`, `temp_max_c` |
| `StationReportRecord` | `report_id`, `report_revision`, `report_updated_at`, `report_type`, `report_date`, `issuance_time`, `fetched_at`, `max_temp_f`, `max_temp_c`, `max_temp_time_utc`, `min_temp_f`, `min_temp_c`, `min_temp_time_utc`, `temp_f`, `temp_c` |
| `HourlyForecastRecord` | `time`, `temperature_2m_f`, `temperature_2m_c`, `apparent_temperature_f`, `relative_humidity_2m`, `dew_point_2m`, `pressure_msl`, `wind_speed_10m`, `wind_direction_10m`, `wind_gusts_10m`, `cloud_cover`, `precipitation_probability` |
| `ForecastBundleRun` | `id`, `fetched_at`, `timezone`, `utc_offset_seconds` |
| `ForecastBundle` | `model_id`, `forecast_run`, `hourly` |
| `OracleModelScoreRecord` | `model_id`, `model_name`, `is_public`, `high_mae`, `low_mae`, `high_bias`, `low_bias`, `combined_mae`, `day_count` |
| `OracleScoreData` | `station_id`, `range_start`, `range_end`, `days_requested`, `all_time`, `score_mode`, `rank_by`, `scores` |
| `ForecastRunSummary` | `id`, `station_id`, `model_id`, `forecast_time`, `fetched_at`, `timezone`, `utc_offset_seconds`, `data_hash` |
| `CursorPage` | `limit`, `next_cursor` |
| `IpGuardLimits` | `requests_per_second`, `burst` |
| `EffectiveLimits` | `tier`, `requests_per_minute`, `daily_max`, `max_history_days`, `ip_guard`, `rate_limit_remaining`, `rate_limit_reset_seconds` |
| `LatestObservationData` | `city`, `station`, `observation`, `daily_high_f`, `daily_low_f`, `daily_high_c`, `daily_low_c`, `asos_daily_high_f`, `asos_daily_low_f`, `asos_daily_high_c`, `asos_daily_low_c`, `wu_current_temp_f`, `wu_current_temp_c`, `wu_daily_high_f`, `wu_daily_low_f`, `wu_daily_high_c`, `wu_daily_low_c`, `wu_observation_time`, `wu_fetched_at`, `temperature_day_mode`, `temperature_day_date`, `wu_day_mode`, `wu_day_date` |
| `StationForecastData` | `city`, `station`, `forecasts`, `count` |
| `LatestReportsData` | `reports`, `report_schedules`; baseline records are opt-in and carry `baseline`, `provider_available_at`, and `baseline_cached_at` |
| `StationReportsData` | `reports` |
| `StationReportHistoryPage` | `city`, `station`, `reports`, `count`, `page`, `report_schedules` |
| `ForecastRunsPage` | `city`, `station`, `runs`, `count`, `page` |
| `ForecastRunData` | `city`, `station`, `forecast_run`, `hourly`, `count` |

Report schedule entries can be:

- `ReportClockSchedule(basis="utc", hour=None, minute=None, utc_hour=None,
  utc_minute=None, local_hour=None, local_minute=None, label="")`.
- `ReportIntervalSchedule(interval_minutes, utc_minute=None,
  local_minute=None, label="")`.
- `ReportMultiHourSchedule(utc_hours=(), local_hours=(), utc_minute=None,
  local_minute=None, label="")`.
- A tuple containing any of those schedule objects.

## Broker and orders

`ctx.broker` is the only portable order path. The shared protocol exposes:

```python
result = await ctx.broker.place_order(...)
cancelled = await ctx.broker.cancel_order(order_id)
cancelled_count = await ctx.broker.cancel_all_orders()

position = ctx.broker.get_position(ticker, side="yes")
positions = ctx.broker.get_positions()
pending_orders = ctx.broker.get_pending_orders()
buying_power = ctx.broker.get_sleeve_buying_power()
```

Runtime-specific broker extensions may exist, but they are not portable unless
they appear in this protocol. In particular, the shared `Broker` currently has
no `get_order_update(s)` method and `BrokerOrderUpdate` is not a
`StrategyEvent` variant.

### Place-order signature and options

```python
result = await ctx.broker.place_order(
    ticker=ticker,
    action="buy",
    contract_side="yes",
    order_type="limit",
    quantity=3,
    limit_price=0.42,
    max_price=None,
    max_cost=None,
    execution_style="resting_limit",
    time_policy="good_till_canceled",
    expires_after_ms=30_000,
    reduce_only=False,
    post_only=False,
    signal_type="forecast_edge",
    signal_metadata='{"model":"hrrr"}',
    client_order_id=f"{ctx.runtime.run_id}:{ticker}:forecast-edge",
)
```

| Field | Type/options | Meaning |
|---|---|---|
| `ticker` | `str` | Market ticker. Keep it inside `ctx.runtime.scope.tickers`. |
| `action` | `"buy"`, `"sell"` | Whether the order acquires or disposes of the selected side. |
| `contract_side` | `"yes"`, `"no"` | Binary contract side. |
| `order_type` | `"market"`, `"limit"` | Basic order type. |
| `quantity` | `int` | Maximum requested contracts. |
| `limit_price` | `float | None` | Limit/resting price or an engine-supported bound. |
| `max_price` | `float | None` | Maximum acceptable per-contract execution price. |
| `max_cost` | `float | None` | Maximum total cost. |
| `execution_style` | `"resting_limit"`, `"direct"`, `"sweep"`, or `None` | Whether to rest, make one bounded immediate attempt, or consume bounded depth. |
| `time_policy` | `"good_till_canceled"`, `"immediate_or_cancel"`, `"fill_or_kill"`, or `None` | Lifetime/fill policy. |
| `expires_after_ms` | `int | None` | Optional runtime-owned expiry duration, primarily for resting orders. |
| `reduce_only` | `bool` | Request that execution not add exposure. |
| `post_only` | `bool` | Request maker-only behavior when supported. |
| `signal_type` | `str | None` | Structured audit category. |
| `signal_metadata` | `str | None` | Serialized audit metadata. |
| `client_order_id` | `str | None` | Stable idempotency/audit key. Supply one for portable retry behavior. |

Shared price and cost values are decimal-dollar floats. Engines own validation,
risk limits, liquidity, provider translation, fills, reconciliation, and
accounting. Explicitly set execution and time-policy fields when their semantics
matter; `None` delegates the choice to the engine.

Common bounded patterns:

```python
# Rest and wait at a bounded price.
await ctx.broker.place_order(
    ticker=ticker,
    action="buy",
    contract_side="yes",
    order_type="limit",
    quantity=2,
    limit_price=0.40,
    execution_style="resting_limit",
    time_policy="good_till_canceled",
    expires_after_ms=30_000,
    client_order_id=f"{ctx.runtime.run_id}:{ticker}:rest",
)

# One immediate bounded attempt.
await ctx.broker.place_order(
    ticker=ticker,
    action="buy",
    contract_side="yes",
    order_type="market",
    quantity=1,
    max_price=0.42,
    max_cost=0.42,
    execution_style="direct",
    time_policy="immediate_or_cancel",
    client_order_id=f"{ctx.runtime.run_id}:{ticker}:direct",
)

# Consume available depth within quantity, unit-price, and total-cost caps.
await ctx.broker.place_order(
    ticker=ticker,
    action="buy",
    contract_side="yes",
    order_type="market",
    quantity=10,
    max_price=0.42,
    max_cost=3.00,
    execution_style="sweep",
    time_policy="immediate_or_cancel",
    client_order_id=f"{ctx.runtime.run_id}:{ticker}:sweep",
)
```

Broad Rust expresses the same full request with `OrderIntent`:

```rust
use strategy_core::{
    Action, Broker, ContractSide, OrderExecutionStyle, OrderIntent,
    OrderTimePolicy, OrderType, StrategyContext, StrategyRuntime,
};

let run_id = ctx.runtime().run_id().to_string();
let client_order_id = format!("{run_id}:{ticker}:sweep");
let result = ctx.broker().place_order_with_intent(OrderIntent {
    ticker: ticker.to_string(),
    action: Action::Buy,
    contract_side: ContractSide::Yes,
    order_type: OrderType::Market,
    quantity: 10,
    limit_price: None,
    max_price: Some(0.42),
    max_cost: Some(3.00),
    execution_style: Some(OrderExecutionStyle::Sweep),
    time_policy: Some(OrderTimePolicy::ImmediateOrCancel),
    expires_after_ms: None,
    reduce_only: false,
    post_only: false,
    signal_type: Some("forecast_edge".to_string()),
    signal_metadata: Some("{\"model\":\"hrrr\"}".to_string()),
    client_order_id: Some(client_order_id),
})?;
```

Every remaining broker operation maps directly:

| Python | Broad Rust |
|---|---|
| `await broker.cancel_order(order_id)` | `broker.cancel_order(order_id) -> Result<bool, Error>` |
| `await broker.cancel_all_orders()` | `broker.cancel_all_orders() -> Result<usize, Error>` |
| `broker.get_position(ticker, side="yes")` | `broker.get_position(ticker, ContractSide::Yes) -> Option<&Position>` |
| `broker.get_positions()` | `broker.get_positions() -> BTreeMap<String, &Position>` |
| `broker.get_pending_orders()` | `broker.get_pending_orders() -> Vec<&PendingOrder>` |
| `broker.get_sleeve_buying_power()` | `broker.get_sleeve_buying_power() -> f64` |

Rust broker implementers implement `place_order_with_intent`, so advanced fields
cannot be discarded by a shared default. The shorter `place_order(...)` method
remains available to callers and constructs a complete `OrderIntent` with
advanced options set to `None` or `false`.

### Broker return and view types

| Type | Fields/options |
|---|---|
| `OrderResult` | `order_id`, `sleeve_id`, `status`, `filled_quantity`, `fill_price`, `fee_cost`, `reason` |
| `Position` | `ticker`, `side`, `quantity`, `avg_price` |
| `PendingOrder` | `order_id`, `sleeve_id`, `ticker`, `action`, `contract_side`, `limit_price`, `requested_quantity`, `filled_quantity`, `reserved_global`, `reserved_sleeve`, `fee_type`, `fee_multiplier`, `fee_accumulator`, `signal_type`, `signal_metadata`, `created_at`, `client_order_id`, `expires_at` |
| `OrderIntent` | All `place_order` intent fields: `ticker`, `action`, `contract_side`, `order_type`, `quantity`, `limit_price`, `max_price`, `max_cost`, `execution_style`, `time_policy`, `reduce_only`, `post_only`, `signal_type`, `signal_metadata`, `client_order_id`, `expires_after_ms` |
| `BrokerOrderUpdate` | `order_id`, `sleeve_id`, `ticker`, `status`, `action`, `contract_side`, `requested_quantity`, `filled_quantity`, `remaining_quantity`, `fill_price`, `average_fill_price`, `fee_cost`, `reason`, `client_order_id`, `provider_order_id`, `provider_sequence`, `updated_at`, `expires_at` |

`OrderResult.status` values are `"filled"`, `"partial"`, `"pending"`,
`"rejected"`, and `"cancelled"`.

`BrokerOrderUpdate.status` values are `"accepted"`, `"rejected"`,
`"submitted"`, `"resting"`, `"partially_filled"`, `"filled"`,
`"cancel_requested"`, `"cancelled"`, `"expired"`, `"closed"`,
`"submission_unknown"`, and `"reconciled"`.

Never retry a rejection blindly. Inspect `reason`, freshness, buying power, and
the runtime's broker health/evidence first.

## Configuration

`ctx.config` is an immutable-by-convention `Mapping[str, object]` supplied by
the runtime. Validate and normalize values at the strategy boundary:

```python
def float_param(ctx: StrategyContext, name: str, default: float) -> float:
    raw = ctx.config.get(name, default)
    try:
        return float(raw)
    except (TypeError, ValueError):
        return default
```

Use configuration for thresholds, model weights, feature switches, and stable
artifact paths. Do not place secrets, mutable runtime state, or hidden provider
clients in strategy configuration.

Broad Rust configuration is a `BTreeMap<String, JsonValue>`:

```rust
use strategy_core::StrategyContext;

let threshold = ctx
    .config()
    .get("threshold")
    .and_then(|value| value.as_f64())
    .unwrap_or(0.05);
```

Keep portable configuration values JSON-compatible and apply equivalent
validation/defaulting in both languages.

## HTTP

HTTP is optional and runtime-mediated:

```python
if ctx.capabilities.supports_http:
    response = await ctx.http.get(
        "https://example.com/reference-data",
        headers={"accept": "application/json"},
        params={"station": "KMIA"},
        timeout_seconds=2.0,
    )
```

Broad Rust has the same request/get/post operations and returns `Result`:

```rust
use std::collections::BTreeMap;
use strategy_core::{HttpClient, JsonValue, StrategyContext};

if ctx.capabilities().supports_http {
    let response = ctx
        .http()
        .get(
            "https://example.com/reference-data",
            None,
            Some(BTreeMap::from([(
                "station".to_string(),
                JsonValue::String("KMIA".to_string()),
            )])),
            Some(2.0),
        )
        .await?;
}
```

`HttpClient` methods:

- `request(HttpRequest) -> HttpResponse` supports methods `"GET"`, `"POST"`,
  `"PUT"`, `"PATCH"`, and `"DELETE"`.
- `get(url, *, headers=None, params=None, timeout_seconds=None)`.
- `post(url, *, headers=None, params=None, json_body=None, text_body=None,
  timeout_seconds=None)`.

`HttpRequest` fields are `method`, `url`, `headers`, `params`, `json_body`,
`text_body`, and `timeout_seconds`. `HttpResponse` fields are `status_code`,
`headers`, `text`, and `json_body`. JSON bodies may be any JSON value, including
an array or primitive, not only an object.

Use HTTP for an explicitly permitted external input, not to bypass engine-owned
state, data, credentials, auditing, or replay semantics.

## Telemetry and logging

```python
ctx.telemetry.logger.debug("evaluating signal")
ctx.telemetry.logger.info("signal accepted")
ctx.telemetry.logger.warning("state stale")
ctx.telemetry.logger.error("order rejected")

ctx.telemetry.counter("events_seen", fields={"type": event.type})
ctx.telemetry.gauge("yes_ask", 0.42, fields={"ticker": ticker})
ctx.telemetry.annotate(
    "decision",
    value="skip",
    fields={"reason": "stale_price"},
)
```

Broad Rust exposes the same five logger levels and three telemetry operations.
It uses typed maps and a preformatted message:

```rust
use strategy_core::{StrategyContext, StrategyLogger, Telemetry};

ctx.telemetry().logger().info("signal accepted");
ctx.telemetry().counter("events_seen", 1.0, None);
ctx.telemetry().gauge("yes_ask", 0.42, None);
ctx.telemetry().annotate(
    "decision",
    strategy_core::TelemetryField::String("skip".to_string()),
    None,
);
```

The Python logger accepts standard message, positional-argument, and
keyword-argument shapes. The Rust logger exposes the same `debug`, `info`,
`warning`, `error`, and `exception` levels with one preformatted message.

Telemetry fields may be `str`, `int`, `float`, `bool`, or `None`. Telemetry is
observability, not a decision-state store; bot correctness must not depend on a
metric sink accepting a value.

Never place credentials, authorization headers, private keys, full provider
responses, or other secrets in logs, telemetry fields, signal metadata, native
result metadata, or client-order ids.

## Portable helpers

### Fee calculations

```python
from strategy_core import calculate_fill_fee, calculate_trade_fee

base_fee = calculate_trade_fee(
    price=0.42,
    quantity=5,
    liquidity_role="taker",
    fee_type="quadratic_with_maker_fees",
    fee_multiplier=1.0,
)

fill = calculate_fill_fee(
    action="buy",
    price=0.42,
    quantity=5,
    liquidity_role="taker",
    fee_accumulator=0.0,
    fee_type="quadratic_with_maker_fees",
    fee_multiplier=1.0,
)
```

Broad Rust calls the same helpers with enums and receives `FeeResult`:

```rust
use strategy_core::{
    Action, FeeType, LiquidityRole, calculate_fill_fee, calculate_trade_fee,
};

let base_fee = calculate_trade_fee(
    0.42,
    5,
    LiquidityRole::Taker,
    Some(FeeType::QuadraticWithMakerFees),
    Some(1.0),
)?;

let fill = calculate_fill_fee(
    Action::Buy,
    0.42,
    5,
    LiquidityRole::Taker,
    0.0,
    Some(FeeType::QuadraticWithMakerFees),
    Some(1.0),
)?;
```

Options:

- `liquidity_role`: `"maker"` or `"taker"`.
- `fee_type`: `"quadratic"`, `"quadratic_with_maker_fees"`, `"flat"`, or
  `None` (defaults to `"quadratic_with_maker_fees"`).
- `action`: `"buy"` or `"sell"` for `calculate_fill_fee`.
- `fee_multiplier`: optional scale applied to the schedule.
- `fee_accumulator`: carry the previous fractional-cent accumulator into the
  next fill.

`FeeCalculation` returns `trade_fee`, `rounding_fee`, `rebate`, `net_fee`,
`posted_balance_change`, and the next `fee_accumulator`.
`apply_fee_rounding(revenue=..., trade_fee=..., fee_accumulator=...)` is exposed
when a runtime already calculated raw revenue and fee. Non-finite numeric inputs
and unknown literal values raise `ValueError`.

These helpers calculate portable fee math. The engine still owns fee-policy
selection, maker/taker classification, posting, balances, settlement, and P/L.

### Station, city-code, and ticker helpers

| Function | Result |
|---|---|
| `primary_city_code_for_series(station)` | Primary Kalshi city suffix for a station. |
| `city_codes_for_market_type(station, market_type)` | All high/low city-code suffixes; `market_type` is `"high"` or `"low"`. |
| `primary_city_code_for_market_type(station, market_type)` | First market-type-specific suffix. |
| `ticker_prefixes_for_station(station, market_type)` | Possible daily `KXHIGH...` or `KXLOWT...` prefixes. Hourly callers must use the source-aware helper. |
| `hourly_series_for_station(station, settlement_source)` | Exact verified hourly series tickers for a canonical station/source profile. Unknown sources and unsupported pairs raise `ValueError`; no ticker is synthesized. |
| `station_from_event_ticker(event_ticker)` | ICAO station for an exact supported hourly series or known daily ticker; otherwise `None`. |

Canonical hourly settlement sources are `SettlementSource.WEATHER_COMPANY`
(`"weather_company"`) and `SettlementSource.SYNOPTIC` (`"synoptic"`). Settlement
source filters discovery eligibility and is intentionally not part of
`StrategyScope`, sleeve identity, routing keys, or the bot trading interface.

Verified hourly profiles are:

| ICAO | Canonical source | Exact series ticker(s) |
|---|---|---|
| `KDCA` | `weather_company` | `KXTEMPDCH` |
| `KNYC` | `weather_company` | `KXTEMPNYCH`, `KXHIGHNYD` |
| `KAUS` | `weather_company` | `KXTEMPAUSH` |
| `KBOS` | `weather_company` | `KXTEMPBOSH` |
| `KMDW` | `weather_company` | `KXTEMPCHIH` |
| `KLAX` | `weather_company` | `KXTEMPLAXH` |
| `KMIA` | `synoptic` | `KXTEMPMIAH` |

The exact identities and source families were verified on 2026-08-15 against
Kalshi's [hourly temperature series catalog](https://external-api.kalshi.com/trade-api/v2/series?category=Climate%20and%20Weather&tags=Hourly%20temperature).
The linked [KLAX event metadata](https://external-api.kalshi.com/trade-api/v2/events/KXTEMPLAXH-26AUG1515?with_nested_markets=true)
independently confirms the event series and Weather Company source. Profile
changes require new primary-source evidence and a contract update.

Exported mapping constants are `ICAO_TO_CITY_CODES`, `CITY_TO_ICAO`,
`STATION_TIMEZONES`, `MARKET_TYPE_PREFIX`, and `TICKER_PREFIXES`.

| ICAO | City codes | Timezone |
|---|---|---|
| `KATL` | `TATL`, `ATL` | `America/New_York` |
| `KAUS` | `AUS`, `AU` | `America/Chicago` |
| `KBOS` | `TBOS`, `BOS` | `America/New_York` |
| `KDCA` | `TDC`, `DC`, `DCA` | `America/New_York` |
| `KDEN` | `DEN` | `America/Denver` |
| `KDFW` | `TDAL`, `DAL`, `DFW` | `America/Chicago` |
| `KJFK` | `JFK` | `America/New_York` |
| `KHOU` | `THOU`, `HOU` | `America/Chicago` |
| `KLAS` | `TLV`, `LV`, `LAS` | `America/Los_Angeles` |
| `KLAX` | `LAX`, `LA` | `America/Los_Angeles` |
| `KMDW` | `CHI`, `MDW`, `MW` | `America/Chicago` |
| `KMIA` | `MIA`, `MI` | `America/New_York` |
| `KMSP` | `TMIN`, `MIN`, `MSP` | `America/Chicago` |
| `KMSY` | `TNOLA`, `NOLA`, `MSY` | `America/Chicago` |
| `KNYC` | `NY` | `America/New_York` |
| `KOKC` | `TOKC`, `OKC` | `America/Chicago` |
| `KORD` | `ORD` | `America/Chicago` |
| `KPHL` | `PHIL`, `PHL` | `America/New_York` |
| `KPHX` | `TPHX`, `PHX` | `America/Phoenix` |
| `KSAT` | `TSATX`, `SATX`, `SAT` | `America/Chicago` |
| `KSEA` | `TSEA`, `SEA` | `America/Los_Angeles` |
| `KSFO` | `TSFO`, `SFO` | `America/Los_Angeles` |

Unknown stations fall back to a stripped city code for ticker helpers but need
an explicit timezone mapping for climate-day helpers.

### Climate-day helpers

```python
event_date = parse_climate_date("20260714")
active_date = climate_day_date("KMIA", ctx.runtime.clock.now())
end_at = climate_day_end("KMIA", event_date)
ended = climate_day_has_ended("KMIA", event_date, ctx.runtime.clock.now())
tz = station_timezone("KMIA")
```

Broad Rust uses explicit UTC/date types and typed errors:

```rust
use strategy_core::{
    EngineClock, StrategyContext, StrategyRuntime, climate_day_date,
    climate_day_end, climate_day_has_ended, parse_climate_date,
    station_timezone,
};

let now = ctx.runtime().clock().now();
let event_date = parse_climate_date(Some("20260714")).expect("valid climate date");
let active_date = climate_day_date(Some("KMIA"), now, None)?;
let end_at = climate_day_end(Some("KMIA"), event_date, None)?;
let ended = climate_day_has_ended(Some("KMIA"), event_date, now, None)?;
let timezone = station_timezone(Some("KMIA"), None)?;
```

- `parse_climate_date` accepts a `date`, `YYYY-MM-DD`, `YYYYMMDD`, `YYMMDD`, or
  `None`; invalid input returns `None`.
- `station_timezone`, `climate_day_date`, `climate_day_end`, and
  `climate_day_has_ended` accept an optional `station_timezones` override map.
- Unknown or invalid timezones raise `ValueError`.
- NWS climate-day boundaries use local standard time, including during daylight
  saving time.

### Signal constants

The package exports these stable audit/signal labels:

- `SIGNAL_DSM_REACTION = "dsm_reaction"`
- `SIGNAL_METAR_6HR_LOW = "metar_6hr_low"`
- `SIGNAL_METAR_6HR_NEW_LOW = "metar_6hr_new_low"`

## Optional native execution and fallback

A Python strategy wrapper can prefer a native kernel:

```python
from strategy_core import StrategyContext, run_native_or_fallback


async def run(ctx: StrategyContext) -> None:
    kernel = build_kernel(ctx.config)

    async def python_fallback() -> None:
        async for event in ctx.events():
            await handle_in_python(ctx, event)

    await run_native_or_fallback(
        ctx,
        kernel,
        fallback=python_fallback,
        require_native=False,
    )
```

Broad Rust exposes the same preference/fallback decision with a closure whose
future returns `()`:

```rust
use strategy_core::run_native_or_fallback;

let result = run_native_or_fallback(
    ctx,
    &mut kernel,
    Some(|| async { run_owned_fallback().await }),
    false,
)
.await?;
```

`NativeKernel` exposes a `name` property. `NativeKernelFactory` builds a kernel
from `StrategyConfig`. `get_native_kernel_runner(ctx)` returns a runner only
when the capability and runtime protocol agree.

`run_native_or_fallback` behavior:

| Condition | Result |
|---|---|
| Native runner exists | Calls `run_native_kernel(kernel)`. |
| No runner, fallback supplied, `require_native=False` | Runs the fallback and returns status `"fallback_completed"`. |
| No runner, `require_native=True` | Raises `NativeKernelUnavailable`. |
| No runner and no fallback | Raises `NativeKernelUnavailable`. |

`NativeKernelResult` fields are `status` (`"completed"` or
`"fallback_completed"`), `events_handled`, `actions_emitted`, `fallback_used`,
and JSON-compatible `metadata`.

## Broad Rust strategies

The broad Rust crate mirrors the owned Python models and exposes these central
traits:

```rust
pub trait StrategyContext {
    type State: MarketStateView;
    type Data: StrategyDataClient;
    type Broker: Broker;
    type Http: HttpClient;
    type Runtime: StrategyRuntime;
    type Telemetry: Telemetry;

    fn state(&self) -> &Self::State;
    fn data(&self) -> &Self::Data;
    fn broker(&mut self) -> &mut Self::Broker;
    fn http(&self) -> &Self::Http;
    fn runtime(&mut self) -> &mut Self::Runtime;
    fn capabilities(&self) -> &RuntimeCapabilities;
    fn config(&self) -> &StrategyConfig;
    fn telemetry(&mut self) -> &mut Self::Telemetry;
    fn next_event(&mut self) -> impl Future<Output = Option<StrategyEvent>> + Send;
}

pub trait StrategyHandler<C: StrategyContext> {
    fn run(&mut self, ctx: &mut C) -> impl Future<Output = ()> + Send;
}
```

A broad Rust event loop dispatches the same 11 variants:

```rust
use strategy_core::{StrategyContext, StrategyEvent, Telemetry};

async fn run_events<C: StrategyContext>(ctx: &mut C) {
    while let Some(event) = ctx.next_event().await {
        match event {
            StrategyEvent::Observation(observation) => {
                if let Some(value) = observation.temperature_f {
                    ctx.telemetry().gauge("observed_temperature_f", value, None);
                }
            }
            StrategyEvent::ForecastUpdated(update) => {
                ctx.telemetry().counter(
                    &format!("forecast_updated.{}", update.model_id),
                    1.0,
                    None,
                );
            }
            StrategyEvent::ShutdownEvent(_) => break,
            _ => {}
        }
    }
}
```

The broad Rust modules match the Python module layout: `broker`,
`capabilities`, `climate_day`, `context`, `data`, `events`, `fees`, `http`,
`kalshi`, `minutetemp`, `models`, `native`, `queries`, `runtime`, `signals`,
`state`, `stations`, and `telemetry`. Serialization parity applies to the
declared JSON-compatible wire domain, not to language-identical constructors or
trait signatures.

## Native Rust kernel

Implement `strategy_core_kernel::NativeKernel` for the narrow hot loop:

```rust
use strategy_core_kernel::{
    KernelResult, NativeKernel, StrategyEventView, StrategyKernelContext,
};

struct MyKernel;

impl NativeKernel for MyKernel {
    fn name(&self) -> &str {
        "my-kernel"
    }

    fn on_event(
        &mut self,
        event: StrategyEventView<'_>,
        ctx: &mut dyn StrategyKernelContext,
    ) -> KernelResult<()> {
        match event {
            StrategyEventView::PriceUpdate(update) => {
                ctx.telemetry().counter(
                    "price_updates",
                    1.0,
                    &[("station", update.station_id)],
                )?;
            }
            StrategyEventView::Shutdown(_) => {}
            _ => {}
        }
        Ok(())
    }
}
```

Optional lifecycle hooks are `on_start`, `on_event`, and `on_finish`.

### Complete kernel export inventory

The kernel crate has 45 public contract names:

| Category | Exports |
|---|---|
| Lifecycle and context traits | `NativeKernel`, `StrategyKernelContext`, `StrategyKernelState`, `StrategyKernelData`, `StrategyKernelBroker`, `StrategyKernelRuntime`, `StrategyKernelTelemetry` |
| Errors | `KernelError`, `KernelResult` |
| Actions, orders, and action payloads | `KernelAction`, `OrderAction`, `ContractSide`, `OrderType`, `OrderStatus`, `PlaceOrderRequest`, `CancelOrderRequest`, `CancelAllOrdersRequest`, `PendingOrderView`, `OrderResult`, `WakeAtRequest`, `TelemetryAction`, `LogAction`, `StopAction` |
| Event and state views | `StrategyEventView`, `PriceLevelView`, `MarketBracketView`, `PriceUpdateView`, `ObservationView`, `StationReportView`, `StationWeatherView`, `ForecastHourlySnapshot`, `ForecastModelSnapshot`, `ForecastInputSnapshot`, `OracleModelScoreSnapshot`, `OracleInputSnapshot`, `ForecastUpdatedView`, `ForecastVersionsView`, `OracleScoresUpdatedView`, `WeatherEventSourceView`, `WeatherEventView`, `HighLowView`, `TimerWakeView`, `ShutdownView`, `TickerPriceView` |

`KernelResult<T>` is `Result<T, KernelError>`. `KernelError::new(message)`
constructs an error and `message()` returns its text.

### Kernel context operations

The native context exposes these exact trait surfaces:

- `StrategyKernelContext::state() -> &dyn StrategyKernelState`:
  `get_price(ticker)`, `get_weather(station_id)`,
  `latest_forecast(station_id)`,
  `latest_oracle_scores(station_id, mode, rank_by, days)`, and
  `state_read_diagnostics()`.
- `StrategyKernelContext::data() -> &dyn StrategyKernelData`: reserved narrow
  data trait; it has no methods today.
- `StrategyKernelContext::broker() -> &mut dyn StrategyKernelBroker`:
  `buying_power()`, `position_quantity(ticker, side)`,
  `position_avg_price(ticker, side)`, `pending_orders()`,
  `place_order(request)`, `cancel_order(request)`, and `cancel_all_orders()`.
- `StrategyKernelContext::runtime() -> &mut dyn StrategyKernelRuntime`:
  `wake_at(WakeAtRequest)`.
- `StrategyKernelContext::telemetry() -> &mut dyn StrategyKernelTelemetry`:
  `counter(name, value, fields)` where fields are `&[(&str, &str)]`.
- `StrategyKernelContext::emit(KernelAction)`: emit a
  place/cancel/cancel-all/wake/telemetry/log/stop action through the runtime.

`StateReadDiagnostic` fields are `kind`, `key`, `status`, `reason`,
`host_state_seq`, `source_feed_event_seq`, `source_state_seq`,
`source_observed_at`, and `source_updated_at`.

### Kernel event and state views

`StrategyEventView` variants are `PriceUpdate`, `Observation`,
`ForecastUpdated`, `ForecastVersions`, `OracleScoresUpdated`, `StationReport`,
`WeatherEvent`, `NewHigh`, `NewLow`, `TimerWake`, `Shutdown`, and `Unknown`.
`event_type()` returns the shared discriminator string. `Unknown` carries
`event_type` and optional `emitted_at`.

| View | Fields |
|---|---|
| `PriceLevelView` | `price`, `quantity` |
| `MarketBracketView` | The same fields as `MarketBracket`; bid/ask level collections contain `PriceLevelView`. |
| `PriceUpdateView` | The same fields as `PriceUpdate` except the serialized `type` discriminator. |
| `ObservationView` | The same fields as `Observation` except the serialized `type` discriminator. |
| `StationReportView` | The same fields as `StationReport` except the serialized `type` discriminator. |
| `WeatherEventSourceView` | The same fields as `WeatherEventSource`. |
| `WeatherEventView` | The same fields as `WeatherEvent` except `type`; its payload `event_type` is named `event_type_name`. |
| `HighLowView` | The same shared fields as `NewHigh` and `NewLow` except `type`; the enum variant determines high versus low. |
| `TimerWakeView` | `scheduled_for`, `fired_at`, `name` |
| `ShutdownView` | `reason` |
| `ForecastUpdatedView` | The same fields as `ForecastUpdated` except `type`. |
| `ForecastVersionsView` | The same fields as `ForecastVersions` except `type`. |
| `OracleScoresUpdatedView` | `event_id`, `sequence`, `emitted_at`, `slug`, `station_id`, `modes`, `updated_at`, `overall`, `day_ahead`, `day_of`; mode payloads are `OracleInputSnapshot`. |
| `TickerPriceView` | The same fields as `TickerPrices`. |
| `StationWeatherView` | `station_id`, `current_temp`, `running_high`, `running_low`, `last_metar_time`, `temp_min_f`, `temp_max_f`, `temp_min_c`, `temp_max_c`, `preliminary`, `dsm_high`, `dsm_low`, `dsm_high_time`, `dsm_low_time`, `six_hr_high`, `six_hr_low`, `last_dsm_time`, `last_six_hr_time`, `asos_daily_high_f`, `asos_daily_low_f`, `wu_daily_high_f`, `wu_daily_low_f`, `wu_current_temp_f`, `wu_current_temp_c`, `wu_daily_high_c`, `wu_daily_low_c`, `wu_observation_time`, `wu_fetched_at`, `dewpoint`, `heat_index`, `wind_chill`, `relative_humidity`, `wind_speed`, `wind_direction`, `wind_gust`, `text_description`, `lag_seconds` |
| `ForecastHourlySnapshot` | The same fields as `ForecastHourly`. |
| `ForecastModelSnapshot` | `model_id`, `value`, `version`, `updated_at`, `run_issued_at`, `hourly` |
| `ForecastInputSnapshot` | `station_id`, `received_at`, `source`, `models` |
| `OracleModelScoreSnapshot` | `model_id`, `model_name`, `is_public`, `high_mae`, `low_mae`, `combined_mae`, `high_bias`, `low_bias`, `day_count` |
| `OracleInputSnapshot` | `station_id`, `received_at`, `source`, `score_mode`, `rank_by`, `days_requested`, `range_start`, `range_end`, `scores` |

Kernel views borrow strings and slices where possible. They are valid only for
the event or state borrow that produced them; copy owned data before retaining
it beyond that call.

### Kernel actions and broker values

`ContractQuantity` is the kernel's authoritative quantity type and stores exact hundredths of one contract. Use `ContractQuantity::from_hundredths` for exact quantities such as `1` (0.01), `125` (1.25), and `250` (2.50). Whole-contract entry policies use the checked `ContractQuantity::checked_from_whole_contracts` conversion. There are no parallel whole/fractional fields or sentinel values. This is a compile-time interface change: downstream Strategies and Trader kernel adapters must construct `ContractQuantity`, return it from `position_quantity`, and read `.hundredths()` before adopting this Strategy Core revision.

Kernel action variants are:

| Action | Payload |
|---|---|
| `PlaceOrder` | `PlaceOrderRequest { ticker, action, contract_side, order_type, quantity, limit_price, expires_after_ms, reduce_only, signal_type, signal_metadata, client_order_id }` |
| `CancelOrder` | `CancelOrderRequest { order_id }` |
| `CancelAllOrders` | `CancelAllOrdersRequest {}` |
| `WakeAt` | `WakeAtRequest { when, name }` |
| `Telemetry` | `TelemetryAction { name, value, fields }` |
| `Log` | `LogAction { level, message }` |
| `Stop` | `StopAction { reason }` |

`PendingOrderView` fields are `order_id`, `ticker`, `status`, `action`,
`contract_side`, `limit_price`, `requested_quantity`, `filled_quantity`,
`remaining_quantity`, `reserved_cost`, `client_order_id`, `created_at`, and
`updated_at`.

The kernel `OrderResult` fields are `order_id`, `sleeve_id`, `status`,
`filled_quantity`, `fill_price`, `fee_cost`, and `reason`. `PlaceOrderRequest.quantity`, all pending/status quantities, `OrderResult.filled_quantity`, and `StrategyKernelBroker.position_quantity` use `ContractQuantity`; each value is authoritative hundredths.

Kernel order enums are `Buy`/`Sell`, `Yes`/`No`, `Market`/`Limit`, and result
statuses `Filled`, `Partial`, `Pending`, `Rejected`, `Cancelled`. The kernel
order request intentionally omits broad-only immediate-execution fields such as
`max_price`, `max_cost`, `execution_style`, `time_policy`, and `post_only`.

## Adapter-only provider models

These public models help engine adapters normalize provider payloads. Normal bot
logic should prefer `ctx.state`, `ctx.data`, and `ctx.broker` so credentials,
subscriptions, replay, and side effects remain engine-owned.

### MinuteTemp models

MinuteTemp response types are documented in the data-response section because
`ctx.data` returns them directly. The module aligns with OpenAPI `1.4.0`; shared
events align with AsyncAPI `1.13.0`. WebSocket subscription negotiation remains
an engine concern.

### Kalshi literal options

| Alias | Values |
|---|---|
| `KalshiMarketSide` | `"yes"`, `"no"` |
| `KalshiMarketResult` | `"yes"`, `"no"`, `"scalar"`, `""` |
| `KalshiOrderAction` | `"buy"`, `"sell"` |
| `KalshiOrderType` | `"limit"` |
| `KalshiOrderStatus` | `"resting"`, `"canceled"`, `"executed"` |
| `KalshiTimeInForce` | `"fill_or_kill"`, `"good_till_canceled"`, `"immediate_or_cancel"` |
| `KalshiImmediateTimeInForce` | `"fill_or_kill"`, `"immediate_or_cancel"` |
| `KalshiSelfTradePreventionType` | `"taker_at_cross"`, `"maker"` |
| `KalshiMarketStatus` | `"unopened"`, `"open"`, `"paused"`, `"closed"`, `"settled"` |
| `KalshiSubscriptionUpdateAction` | `"add_markets"`, `"delete_markets"` |
| `KalshiPriceLevelStructure` | `"linear_cent"`, `"deci_cent"`, `"tapered_deci_cent"` |
| `KalshiCollateralReturnType` | `"MECNET"`, `"DIRECNET"`, `""` |

`KalshiWsChannel` values are `"orderbook_delta"`, `"ticker"`, `"trade"`,
`"fill"`, `"market_positions"`, `"market_lifecycle_v2"`,
`"multivariate_market_lifecycle"`, `"multivariate"`, `"communications"`,
`"order_group_updates"`, and `"user_orders"`.

`KalshiMarketLifecycleEventType` values are `"created"`, `"deactivated"`,
`"activated"`, `"close_date_updated"`, `"determined"`, `"settled"`,
`"fractional_trading_updated"`, and `"price_level_structure_updated"`.

Fixed-point Kalshi prices and counts are represented as strings through
`KalshiFixedPrice` and `KalshiFixedCount`.

### Kalshi request, response, and message types

| Category | Public types |
|---|---|
| Order REST | `KalshiOrderCreateRequest`, `KalshiOrder`, `KalshiCreateOrderResponse`, `KalshiGetOrderResponse`, `KalshiGetOrdersResponse` |
| Orderbook REST | `KalshiOrderbookLevel`, `KalshiOrderbook`, `KalshiMarketOrderbook`, `KalshiGetOrderbookResponse`, `KalshiGetOrderbooksResponse` |
| Market REST | `KalshiPriceRange`, `KalshiMveSelectedLeg`, `KalshiMarket`, `KalshiGetMarketResponse`, `KalshiMarketsPage` |
| WebSocket commands | `KalshiSubscribeCommand`, `KalshiUnsubscribeCommand`, `KalshiListSubscriptionsCommand`, `KalshiUpdateSubscriptionCommand` |
| Public WebSocket messages | `KalshiOrderbookSnapshotMessage`, `KalshiOrderbookDeltaMessage`, `KalshiTickerMessage`, `KalshiTradeMessage` |
| Private WebSocket messages | `KalshiUserOrderMessage`, `KalshiUserFillMessage`, `KalshiMarketPositionMessage` |
| Lifecycle WebSocket messages | `KalshiMarketLifecycleMetadata`, `KalshiMarketLifecycleMessage`, `KalshiEventLifecycleMessage` |

`KalshiWsMessage` is the union of all public/private/lifecycle WebSocket message
types above. The module deliberately does not provide HTTP clients, WebSocket
connections, authentication, retries, subscriptions, or order execution.

## Portability and ownership rules

Portable bots should follow these rules:

1. Iterate `ctx.events()`; do not spin or access a raw queue.
2. Use `ctx.state` for latest-known values and check freshness before trading.
3. Use `ctx.data` for explicit runtime-owned reads and check its capability.
4. Place and cancel orders only through `ctx.broker` with explicit quantity and
   price/cost bounds.
5. Use `ctx.runtime.scope` instead of parsing runtime identifiers.
6. Use the engine clock, `wake_at`, and `start_work` instead of wall-clock calls
   or detached tasks when replay behavior matters.
7. Gate optional HTTP, timers, data, and native execution with capabilities.
8. Keep external data dependencies explicit and runtime-mediated.
9. Treat configuration and telemetry as inputs/observability, not mutable
   engine state.
10. Test in paper or replay before live execution; `RuntimeMode.LIVE` does not
    itself prove that a deployment's risk gates are safe.

Strategy Core owns portable types, helpers, and strategy-facing protocols.
Consumer engines own provider clients, credentials, subscriptions, mutable
caches, freshness policy, event ordering, replay progression, persistence,
process supervision, order execution, risk, reconciliation, accounting,
settlement, and operational deployment.

When a public contract changes, update this guide, the export inventory in
`tests/fixtures/conformance/manifest.json`, the applicable Python-authored
fixtures or behavior vectors, matching Rust evidence, and affected consumer
boundary tests in Trader and Backtester.
