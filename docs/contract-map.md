# Strategy Core kernel contract

This is the reference for writing a Strategy against `strategy-core`: the kernel API a
Strategy implements, the context it reads and calls, the events and state it sees, the order
and request values it produces, and the pure helpers it may use.

`strategy-core` is a library, not an engine. It holds two Rust crates:

| Crate | Path | Owns |
|---|---|---|
| `strategy-core-kernel` | `native/strategy_core_kernel` | The kernel traits (`NativeKernel`, `StrategyKernelContext`), the canonical owned state and event model, the borrowed views kernels read, order and request values, and pure helpers: exact fees, stations and series tickers, climate days, component freshness. |
| `strategy-core-v3` | `native/strategy_core_v3` | The canonical profile (`strategy-core-canonical-v1`), the Decision V6 wire (context, result, checkpoint, commands), and, with the `kernel` feature, the runner that presents a V6 context to a `NativeKernel` and assembles its result (`kernel_v6`). |

Trader V3 and the Backtester are the hosts. They build the context, deliver events, own the
Broker, providers, persistence and timers, and decide which capabilities a Strategy gets.
A kernel that stays inside this contract runs unchanged in paper, live and replay.

Detailed contracts:

- [Decision V6](decision-v6.md): one run per event, order updates, the provisional view,
  order terms, external requests and the wire. Points it settled:
  [decision-v6-open-points.md](decision-v6-open-points.md).
- [V3 canonical profile](v3-contract.md): canonical bytes and the V3 value bounds.

## Contents

- [Depend on the crates](#depend-on-the-crates)
- [Write a kernel](#write-a-kernel)
- [Kernel context](#kernel-context)
- [Events and state](#events-and-state)
- [Orders and Broker values](#orders-and-broker-values)
- [Pure helpers](#pure-helpers)
- [Hosts: the Decision V6 runner](#hosts-the-decision-v6-runner)
- [Conformance](#conformance)
- [Ownership rules](#ownership-rules)

## Depend on the crates

Consumers pin both crates to one GitHub release tag (`strategy-core-v3-v<version>`):

```toml
[dependencies]
strategy-core-kernel = { git = "https://github.com/McFalljb/strategy-core.git", tag = "strategy-core-v3-v0.3.0", version = "=0.3.0" }
strategy-core-v3 = { git = "https://github.com/McFalljb/strategy-core.git", tag = "strategy-core-v3-v0.3.0", version = "=0.3.0", features = ["kernel"] }
```

Kernels need only `strategy-core-kernel`. A Strategy executable or host that runs Decision V6
also needs `strategy-core-v3` with the `kernel` feature. Use one tag for both crates
everywhere in a build: the V3 crate depends on the kernel crate by path, so two revisions
put two incompatible kernel crates in the graph.

A consumer records the tag's commit and two digests next to the tag.
`scripts/pin-digests.sh <tag>` prints all three:

- the `strategy-core-v3` source tree: sha256 of
  `git archive --format=tar <commit> native/strategy_core_v3` (git's pax header embeds the
  commit id, so this changes with every commit);
- the Decision V6 corpus: sha256 of `conformance/v6/decision-transactions.json`.

## Write a kernel

Implement `strategy_core_kernel::NativeKernel`:

```rust
use strategy_core_kernel::{
    ContractQuantity, ContractSide, KernelResult, NativeKernel, OrderAction, PlaceOrderRequest,
    StrategyEventView, StrategyKernelContext,
};

struct MyKernel {
    ticker: String,
}

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
            StrategyEventView::PriceUpdate(_) => {
                let ask = ctx.state().get_price(&self.ticker).and_then(|price| price.yes_ask);
                let held = ctx.broker().position_quantity(&self.ticker, ContractSide::Yes);
                if held == ContractQuantity::ZERO && ask.is_some_and(|ask| ask <= 0.30) {
                    ctx.broker().place_order(PlaceOrderRequest::resting_limit(
                        self.ticker.clone(),
                        OrderAction::Buy,
                        ContractSide::Yes,
                        ContractQuantity::from_hundredths(100),
                        0.30,
                    ))?;
                }
            }
            StrategyEventView::OrderUpdate(update) => {
                ctx.telemetry().counter(
                    "order_updates",
                    1.0,
                    &[("client_order_id", &update.client_order_id)],
                )?;
            }
            _ => {}
        }
        Ok(())
    }
}
```

Lifecycle hooks are `on_start` (Bootstrap and Recovery) and `on_event`. Under Decision V6 a
kernel runs once per event: Broker calls and external requests return a ticket at once, and
what happens to each order or request arrives in a later decision as an `OrderUpdate` or
`ExternalResponse` event. `KernelResult<T>` is `Result<T, KernelError>`;
`KernelError::new(message)` constructs an error and `message()` returns its text. An `Err`
from `on_event` for the trigger rejects the decision (no commands, the checkpoint unchanged).

The crate root re-exports every type named below; `native/strategy_core_kernel/src/lib.rs`
is the full export list.

## Kernel context

- `StrategyKernelContext::state() -> &dyn StrategyKernelState`:
  hosts must implement `station(station_id) -> Option<&StationState>` and
  `market(ticker) -> Option<&MarketState>`, and may implement
  `hourly_index(city) -> Option<&HourlyIndexState>` (default `None`; see
  [Events and state](#events-and-state)). Models remain valid for the invocation;
  absent or out-of-scope state returns `None`. Core's non-overridable trait-object
  conveniences `get_price(ticker)`, `get_weather(station_id)`,
  `latest_forecast(station_id)` and
  `latest_oracle_scores(station_id, mode, rank_by, days)` derive from those models.
  Getters must not fetch live provider data. A replay host may load recorded history
  on demand from its historical storage, restricted to the invocation's fixed read fence
  and scope. Repeated reads must retain consistent views for the invocation. A loading
  failure must fail the invocation before admitting any commands or other effects; it
  must not silently become `None` in a successful decision. This is a replay-host policy,
  not a change to Trader's delivered-context policy or the bot-facing method signatures.
- `StrategyKernelContext::parameters() -> &StrategyParameters`: the Strategy's
  configured parameters, read-only, by key (`get`, `iter`); each is a
  `ParameterValue` (`Null`, `Bool`, `I64`, `U64`, exact `Decimal { coefficient,
  scale }`, `String`) with `as_bool`/`as_i64`/`as_u64`/`as_f64`/`as_str`. The
  Decision V6 host supplies `StrategyScopeV6.parameters`; hosts without them
  return an empty set.
- `StrategyKernelContext::capabilities() -> KernelCapabilities`: what the host
  grants: `mode` (`Paper`, `Live`, `Replay`, or `None` when the host does not
  state it), `timers`, `timer_handles`, `gauges`, `annotations`,
  `external_requests` (the allowed requests, sorted: `http:<endpoint>` and
  `command:<name>`) and `market_sell` (the Broker admits Market
  sells; the Decision V6 runner sets it in paper and live). The default grants nothing.
  The Decision V6 host states the context's deployment mode, grants timers (and
  handles) as the context grants them, always records gauges and annotations, and
  reports the granted requests.
- `StrategyKernelContext::contributor_stations() -> &[String]`: every station whose
  data settles the Sleeve's event (the primary station among them), so a kernel
  need not hard-code them. Hosts that do not state them return none.
- `StrategyKernelContext::data() -> &dyn StrategyKernelData`: reserved narrow
  data trait; it has no methods today.
- `StrategyKernelContext::broker() -> &mut dyn StrategyKernelBroker`:
  `financial_state()`, `buying_power()`, `position_quantity(ticker, side)`,
  `position_avg_price(ticker, side)`, `pending_orders()`, `order_status(client_order_id)`,
  `place_order(request) -> KernelResult<OrderTicket>`,
  `cancel_order(CancelOrderRequest { target }) -> KernelResult<CommandTicket>` and
  `cancel_all_orders() -> KernelResult<CommandTicket>`. Tickets return at once; `Err`
  means only a local problem (an invalid request or a bound exceeded). A Broker refusal
  is never an `Err`: it arrives as an `OrderUpdate` event, as does every later change
  of the order. A kernel's own `client_order_id` may not start with `tv3` (reserved for ids
  the host derives) and must be unique for the account's lifetime. A kernel error while
  handling an `OrderUpdate` is undone and recorded, the decision goes on, and the update is
  delivered again in the next decisions (up to three failures). Returning the runner's own
  refusal for room in the decision that earlier updates took (64 commands, plan rows,
  result bytes) defers the update instead, for up to eight decisions in a row; refusals at
  the open-order cap or the runner's 256 live entries count as failures.
  Updates may arrive for orders the kernel does not know: the runner adopts the Sleeve's
  open orders it does not track (on its first decision, or when a lost order returns).
  Inside a decision the reads are provisional: the decision's own orders
  are pending with status `submitted` and reserve budget with the Broker's formula,
  cancels mark their targets `cancellation_requested`, and positions are unchanged.
- `StrategyKernelContext::runtime() -> &mut dyn StrategyKernelRuntime`:
  `now()`, `wake_at(WakeAtRequest) -> KernelResult<TimerHandle>`,
  `cancel_timer(&TimerHandle)` and `pending_timers() -> Vec<PendingTimer>`
  (when `capabilities().timer_handles`). A `TimerHandle` is the timer's key (the
  request name, or `kernel.wake`) and the generation the host scheduled it under;
  it serializes, so a kernel can keep it in its checkpoint and cancel in a later
  decision. A cancel applies only while the pending timer still has that
  generation, so a stale handle never cancels a newer schedule of the same key.
  `pending_timers()` lists the Sleeve's pending timers as delivered with the
  decision. Under Decision V6 the generation is `timer.<delivery_id>` of the
  scheduling decision, a decision may carry one timer operation per key (a second
  `wake_at`/`cancel_timer` for a key is refused; rescheduling a key in a later
  decision replaces the timer), and a cancel is a `CancelTimer` command. Hosts
  without handles refuse `cancel_timer`.
  `request_http(HttpRequest { endpoint, method, path, body, timeout_ms })` and
  `request_command(CommandRequest { command, args, stdin, timeout_ms })` ask the host
  to make an HTTP call or run a command after the decision is saved, and return a
  `RequestTicket { request_id }` at once. `endpoint` and `command` are allowlist
  names the host grants (`http:<endpoint>`, `command:<name>` in
  `capabilities().external_requests`), never URLs or paths; the host owns the base URL,
  credentials, program, leading arguments and environment. The answer arrives in a
  later decision as the `ExternalResponse { request_id, outcome }` event, where
  `outcome` is `Ok { status, body }` (a 2xx status, or 0 and a command's standard
  output) or `Err { kind, message }` with an `ExternalErrorKind` (`Refused`,
  `Timeout`, `Transport`, `Status(code)` for a non-2xx status, `TooLarge`,
  `Malformed`, `Exit(code)`, `Abandoned` after a host restart). `Err` from the call
  itself means only a local problem: not granted, outside the bounds (a path from one
  `/` without `.`/`..` segments or `%2e`, a timeout of 1 ms to 120 s, at most 64 KiB of path or
  arguments plus payload), or more than 8 requests in one decision. Hosts without
  requests refuse both calls.
- `StrategyKernelContext::telemetry() -> &mut dyn StrategyKernelTelemetry`:
  `counter(name, value, fields)` where fields are `&[(&str, &str)]`;
  `gauge(name, value, fields)`; and `annotate(name, value, fields)` with an
  `AnnotationValue` (`Text`, `Integer`, `Float`, `Bool`, `Null`). Hosts that do
  not record gauges or annotations (`capabilities().gauges` / `.annotations`
  false) drop them. The Decision V6 host records counters, gauges and annotations
  as typed telemetry entries in the result, in call order (floats keep their exact
  bits); logs are result diagnostics.
- `StrategyKernelContext::emit(KernelAction)`: emit a
  place/cancel/cancel-all/wake/telemetry/log/stop action through the runtime.

## Events and state

`StrategyEventView` variants are `PriceUpdate`, `Observation`, `ForecastUpdated`,
`OracleScoresUpdated`, `StationReport`, `WeatherEvent`, `NewHigh`, `NewLow`, `TimerWake`,
`OrderUpdate`, `ExternalResponse`, and `Unknown`. `event_type()` returns the shared
discriminator string. `Unknown` carries `event_type` and optional `emitted_at`; a Broker-state
trigger arrives as `Unknown { event_type: "broker_state" }` after its updates.

| View | Carries |
|---|---|
| `PriceUpdateView` | Event identity and sequence, `source`, `slug`, `station_id`, `city_id`, `timestamp`, and the Market brackets (`MarketBracketView`: ticker, strikes, close time, prices, bid/ask levels as `PriceLevelView` with a whole-floor `quantity` and authoritative hundredths `exact`). |
| `ObservationView` | Temperatures in independent C/F, day/report metadata, pressure, precipitation, `is_locf`, provenance, `ValueOrigin` and the supplied original. |
| `StationReportView` | A station report: report id, type, date and revision, issuance and fetch times, maximum/minimum and current temperatures. |
| `WeatherEventView` | A weather event: id, `event_type_name`, tier, state, texts, start/confirm/end times, source, provenance and origin. |
| `HighLowView` | A new running high (`NewHigh`) or low (`NewLow`): value and previous value in F/C, observation time, temperature-day mode and date, report linkage, provenance. |
| `ForecastUpdatedView` | A forecast update for one station and model (`model_id`, `version`). |
| `OracleScoresUpdatedView` | Oracle scores for one station: `modes`, `updated_at`, and `overall`, `day_ahead`, `day_of` as `OracleInputSnapshot`. |
| `TimerWakeView` | `scheduled_for`, `fired_at`, `name`. |
| `OrderUpdate` | See [Orders and Broker values](#orders-and-broker-values). |
| `ExternalResponse` | `request_id` and `outcome` (see `request_http` above). |

State is the canonical owned model. `StationState` holds the station's identity and climate
day, observation, weather view and facts, daily extremes, reports, weather events, forecast
and oracle tables; `MarketState` holds the Market's identity, strikes, fee terms, lifecycle,
quote, book levels, last trade and final fact. Each has per-component `ComponentMeta` (update
time, authority, refresh error) in `components`. The Decision V6 runner builds every component
from its supplied original when the context carries one and records its `ValueOrigin`.
Convenience views over them are `TickerPriceView` (`get_price`), `StationWeatherView`
(`get_weather`), `ForecastInputSnapshot` (`latest_forecast`) and `OracleInputSnapshot`
(`latest_oracle_scores`). `native/strategy_core_kernel/src/events.rs`
and `state.rs` list every field.

Each oracle score (`OracleScore`, `OracleModelScoreSnapshot`) may carry `error_distribution`:
a run-weighted histogram of the model's signed errors (forecast − observed, °F), with 22
`bin_edges_f` from −10.5 to +10.5 and 23 `high_counts` / `low_counts` (underflow, 21
one-degree bands that include their lower edge, overflow), each summing to `sample_count`,
over `day_count` covered days. `None` means unavailable, never zero error. Merge histograms by
summing their counters. See [Decision V6](decision-v6.md#oracle-error-distributions).

`StrategyKernelState::hourly_index(city)` returns MinuteTemp's hourly Kalshi Weather Index of
an index city (`miami`, `nyc`, `chicago`, `la-coastal`) as `HourlyIndexState`, when the host
delivers one (a Decision V6 context carries at most one, its scope's index city). It is state
only: no event announces a change, so a kernel reads it when it wakes, typically on its own
timer. The default method returns `None`, so hosts without hourly indexes compile unchanged.
`HourlyIndexState` (`native/strategy_core_kernel/src/hourly_index.rs`) holds the city's
identity and last folded provider `seq`; `latest`, `latest_valued` and `recent_minutes`
(newest first, at most 75, each `IndexMinute` with its phase, `is_final`, revision, values,
quorum and up to 16 `IndexStationReading`s with `pull_f`, `change_5m_f` and the hour's
member high/low); `current_hour` (`IndexHour`: settle-now value, forecast settle, bias and
adjusted settle, `feed_conditions`); `forecast` (`IndexForecast`: 15-minute `steps`, upcoming
`settles` and `bias`); `recent_settlements` (at most 3) and `calibration`. Each value has its
`f64` convenience, the supplied original (`Supplied*`, `Decimal`) and `ValueOrigin::Supplied`.
`components` holds one `ComponentMeta` per stream (`minutes`, `hour`, `forecast`, `bias`,
`settlements`, `calibration`); check its authority before trusting a stream. The adjusted
forecast is `value_f + bias.bias_f`; it is not stored. See
[Decision V6](decision-v6.md#hourly-index).

Kernel views borrow strings and slices where possible. They are valid only for the event or
state borrow that produced them; copy owned data before retaining it beyond that call.

## Orders and Broker values

`ContractQuantity` is the kernel's authoritative quantity type and stores exact hundredths of one contract. Use `ContractQuantity::from_hundredths` for exact quantities such as `1` (0.01), `125` (1.25), and `250` (2.50). Whole-contract entry policies use the checked `ContractQuantity::checked_from_whole_contracts` conversion. There are no parallel whole/fractional fields or sentinel values.

Kernel action variants are:

| Action | Payload |
|---|---|
| `PlaceOrder` | `PlaceOrderRequest { ticker, action, contract_side, order_type, quantity, limit_price, market_price_cap, time_policy, post_only, expires_after_ms, reduce_only, signal_type, signal_metadata, client_order_id }`; constructors `resting_limit`, `direct`, `sweep` |
| `CancelOrder` | `CancelOrderRequest { target: CancelTarget::OrderId(..) \| CancelTarget::ClientOrderId(..) }` |
| `CancelAllOrders` | `CancelAllOrdersRequest {}` |
| `WakeAt` | `WakeAtRequest { when, name }` |
| `Telemetry` | `TelemetryAction { name, value, fields }` |
| `Log` | `LogAction { level, message }` |
| `Stop` | `StopAction { reason }` |

`PendingOrderView` fields are `order_id`, `ticker`, `status`, `action`,
`contract_side`, `limit_price`, `requested_quantity`, `filled_quantity`,
`remaining_quantity`, `reserved_cost`, `client_order_id`, `created_at`, and
`updated_at`.

Emitting `PlaceOrder`, `CancelOrder`, `CancelAllOrders` or `WakeAt` is the same as the
matching broker or runtime call with its ticket or handle discarded.

`OrderTicket` is `{ command_id, client_order_id }` (the kernel's client order id, or the
one the host derives); `CommandTicket` is `{ command_id }`. `OrderStatusView` fields are
`order_id` (empty for an order placed earlier in the same decision), `client_order_id`,
`status` (`BrokerOrderStatus`: `Submitted`, `Accepted`, `Dispatched`, `Resting`,
`PartiallyFilled`, `Filled`, `CancellationRequested`, `Cancelled`, `Expired`, `Rejected`,
`RecoveryRequired`; `as_str()` is the `PendingOrderView.status` text), `requested_quantity`,
`filled_quantity`, `remaining_quantity`, `reason`, and `updated_at`.
`PlaceOrderRequest.quantity`, all pending/status/update quantities, and
`StrategyKernelBroker.position_quantity` use `ContractQuantity`; each value is authoritative
hundredths.

Kernel order enums are `Buy`/`Sell`, `Yes`/`No`, and `Market`/`Limit`. An `OrderUpdate`
reports a place's order, or the refusal of a cancel (`command_kind` `CancelOrder`, the order
fields describe the target, whose own status is unchanged) or of a cancel-all (no order:
empty `client_order_id` and `ticker`, no `action` or `contract_side`). An admitted cancel
shows as the target order's own `Cancelled` update. See [Decision V6](decision-v6.md).

`time_policy` (`GoodTillCanceled`, the default; `ImmediateOrCancel`; `FillOrKill`),
`post_only`, `market_price_cap` (a Market buy's per-contract cap) and `expires_after_ms` say
how an order executes. The execution styles are constructors: `resting_limit` (a
`GoodTillCanceled` Limit), `direct` (an `ImmediateOrCancel` Limit; a sell's limit is its
price floor) and `sweep` (an `ImmediateOrCancel` Market buy at a cap). A place whose terms
do not make one order is a local error of `place_order`; the rules are in
[Decision V6: Order terms](decision-v6.md#order-terms). There is no total-cost cap.

## Pure helpers

Pure functions with no host access. They are safe to call anywhere in a kernel.

### Fees (what the Broker charges and reserves)

`strategy_core_kernel::fees` helpers take a `ContractQuantity` and the Market's `FeeTerms`
(`MarketState::fee_terms()`, or `FeeTerms::from_market(fee_type, fee_multiplier_millionths)`,
which refuses an unknown fee type or an absent/negative multiplier as the Broker does):

| Helper | Result |
|---|---|
| `calculate_trade_fee_micros(price, quantity, role, terms)` | Trade fee rounded up to $0.000001. |
| `calculate_fill_fee_micros(action, price, quantity, role, accumulator, terms)` | The Broker's charge for one fill (`calculate_direct_member_fill_fee_micros`). |
| `apply_fee_rounding_micros(revenue, trade_fee, accumulator)` | Posting to the $0.0001 grid with the capped rebate. |
| `buy_fee_reservation_micros(cap, quantity, terms)` | The fee reservation the Broker requires for a new buy. |
| `buy_commitment_micros(cap, quantity, terms)` | Exact principal plus that reservation: the cash a buy commits at admission. |
| `price_micros(price)` | The host's `f64` price to microdollars conversion. |

### Direct-member fill fees

`strategy_core_kernel::fees::calculate_direct_member_fill_fee_micros` is the
fixed-unit interface for the selected direct-member `$0.0001` posting grid:

```rust
use strategy_core_kernel::{OrderAction, fees::{
    FeeType, LiquidityRole, calculate_direct_member_fill_fee_micros,
}};

let charge = calculate_direct_member_fill_fee_micros(
    OrderAction::Buy, 600_000, 500, LiquidityRole::Taker,
    0, FeeType::Quadratic, 1_000_000,
)?;
assert_eq!(charge.trade_fee_micros, 84_000);
assert_eq!(charge.net_fee_micros, 84_000);
assert_eq!(charge.posted_balance_change_micros, -3_084_000);
```

Inputs are price microdollars, quantity hundredths, the order's prior accumulator
in microdollars, and explicit fee type/multiplier millionths. `FeeCalculationMicros`
returns unsigned trade fee, rounding fee, rebate, net fee, and next accumulator;
its signed cash change is `i128`. No amount passes through floating point.
Non-exact microdollar principal and monetary overflow reject.

Trade fees ceil to six decimals. Signed cash postings floor to `$0.0001`;
rebates are grid-aligned and capped so a fill's net fee cannot be negative.
Keep the accumulator across the same order's partial fills and maker/taker
transitions. Cancellation does not create an additional terminal rebate.
Conservative reservations are separate from these actual execution charges.

This is pure arithmetic, not proof of venue authority, liquidity role, admission,
durable posting, or live execution parity. The host owns those obligations.

### Stations and series tickers

`strategy_core_kernel::stations`:

| Function | Result |
|---|---|
| `primary_city_code_for_series(station)` | Primary Kalshi city suffix for a station. |
| `city_codes_for_market_type(station, market_type)` | All high/low city-code suffixes; `market_type` is `"high"` or `"low"`. |
| `primary_city_code_for_market_type(station, market_type)` | First market-type-specific suffix. |
| `ticker_prefixes_for_station(station, market_type)` | Possible daily `KXHIGH...` or `KXLOWT...` prefixes. Hourly callers must use the source-aware helper. |
| `hourly_series_for_station(station, settlement_source)` | Exact verified hourly series tickers for a canonical station/source profile. Unknown sources and unsupported pairs return a `StationError`; no ticker is synthesized. |
| `station_from_event_ticker(event_ticker)` | ICAO station for an exact supported hourly series or known daily ticker; otherwise `None`. |

Canonical hourly settlement sources are `"weather_company"` and `"synoptic"`. Settlement
source filters discovery eligibility and is intentionally not part of the Strategy scope,
Sleeve identity, routing keys, or the kernel trading interface.

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
| `KLGA` | `synoptic` | `KXTEMPNYCHS` |
| `KMDW` | `synoptic` | `KXTEMPCHIHS` |
| `KLAX` | `synoptic` | `KXTEMPLAXHS` |

The `*HS` rows are the live Kalshi Weather Index series (checked 2026-09-28 against the
series catalog: open events, settled on the index via Synoptic). Their index has several
member stations; the row's station is the index city's forecast station in MinuteTemp
(`nyc` → KLGA, `chicago` → KMDW, `la-coastal` → KLAX), so `station_from_event_ticker`
returns it. The older `weather_company` rows are kept; Kalshi lists them without open events.
The index itself reaches a kernel as `StrategyKernelState::hourly_index` (see
[Events and state](#events-and-state)), never through these lookups.

The exact identities and source families were verified on 2026-08-15 against
Kalshi's [hourly temperature series catalog](https://external-api.kalshi.com/trade-api/v2/series?category=Climate%20and%20Weather&tags=Hourly%20temperature).
The linked [KLAX event metadata](https://external-api.kalshi.com/trade-api/v2/events/KXTEMPLAXH-26AUG1515?with_nested_markets=true)
independently confirms the event series and Weather Company source. Profile
changes require new primary-source evidence and a contract update.

Exported mapping constants are `ICAO_TO_CITY_CODES`, `CITY_TO_ICAO`,
`STATION_TIMEZONES`, `MARKET_TYPE_PREFIX`, `HOURLY_SERIES_BY_PROFILE` and `TICKER_PREFIXES`.

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
| `KLGA` | (none) | `America/New_York` |
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

### Climate days and freshness

`strategy_core_kernel::climate_day`:

```rust
use strategy_core_kernel::climate_day::{
    climate_day_date, climate_day_end, climate_day_has_ended, parse_climate_date,
    station_timezone,
};

let now = ctx.runtime().now().expect("the host states the decision time");
let event_date = parse_climate_date(Some("20260714")).expect("valid climate date");
let active_date = climate_day_date(Some("KMIA"), now, None)?;
let end_at = climate_day_end(Some("KMIA"), event_date, None)?;
let ended = climate_day_has_ended(Some("KMIA"), event_date, now, None)?;
let timezone = station_timezone(Some("KMIA"), None)?;
```

- `parse_climate_date` accepts `YYYY-MM-DD`, `YYYYMMDD`, `YYMMDD`, or `None`; invalid input
  returns `None`.
- `station_timezone`, `climate_day_date`, `climate_day_end`, and `climate_day_has_ended`
  accept an optional station-to-timezone override map.
- An unknown station or invalid timezone returns a `ClimateDayError`.
- NWS climate-day boundaries use local standard time, including during daylight saving
  time.

Component age is computed from state, not queried: `ComponentMeta::age_at(now)` and
`ComponentMeta::freshness_at(now, stale_after)` (`strategy_core_kernel::freshness`) return
`Fresh`, `Stale` (older than `stale_after`) or `Missing` (no update time), with the host's
authority and refresh error beside it; pass `ctx.runtime().now()` as `now`.

## Hosts: the Decision V6 runner

With the `kernel` feature, `strategy_core_v3::kernel_v6` is the single definition of how a
Decision V6 context is presented to a `NativeKernel` and how one run becomes a result. Hosts
and Strategy executables use it instead of re-interpreting fields.

- `run_transaction(factory, context)`: the out-of-process path. The Strategy executable
  supplies a `TransactionKernelFactory` (create, restore, checkpoint codec) whose kernel is a
  `TransactionKernel` (a `NativeKernel` that encodes its private checkpoint state). The runner
  restores the kernel, delivers the derived order updates and the trigger, and returns the
  result with the post-event checkpoint and the commands in issue order.
- `run_native_decision(kernel, invocation)`: the resident path for in-process replay hosts.
  The host keeps one kernel per Sleeve and passes borrowed canonical views; no checkpoint or
  rollback, and an error fails that kernel instance. Tickets, the provisional view,
  order-update derivation, command bounds and external requests are the same code as the
  transaction's.

[Decision V6](decision-v6.md) specifies both, the wire, and the supplied inputs.

## Conformance

`cargo test --manifest-path native/Cargo.toml --workspace --all-features` runs every check:

- `conformance/v6/decision-transactions.json`: the Decision V6 corpus (exact bytes, lengths,
  digests and verdicts), rebuilt and checked by `decision_v6::corpus_tests`; regenerate with
  `cargo test -p strategy-core-v3 -- --ignored write_v6_corpus`.
- `conformance/v3/vectors.json`: canonical-profile vectors, checked by
  `native/strategy_core_v3/tests/conformance.rs`.
- `native/strategy_core_v3/tests/kernel_projection.rs`: the kernel projection and runner
  (decisions, delivery, host services, simulation, resident execution).
- `native/strategy_core_kernel/tests/`: the kernel contract, exact fees (with the retired
  floating schedule's results for the same inputs, for comparison) and the station,
  climate-day and freshness lookups.

## Ownership rules

1. React to events; do not poll or spin. A kernel runs once per event.
2. Read state through `ctx.state()` and check component freshness before trading on it.
3. Place and cancel only through `ctx.broker()`, with explicit quantity and price bounds.
   A ticket is not a fill: act on the `OrderUpdate` events that follow.
4. Use `ctx.runtime().now()` and `wake_at`, never the wall clock, so replay matches live.
5. Gate timers, timer handles, external requests and Market sells on `ctx.capabilities()`.
6. Keep external calls host-mediated: `request_http` and `request_command` name endpoints and
   commands the host grants; the host owns URLs, credentials and programs.
7. Treat parameters and telemetry as inputs and observability, not engine state.
8. Test in paper or replay before live; `RuntimeMode::Live` does not prove a deployment's
   risk gates are safe.

Strategy Core owns the kernel traits, the canonical model, the V6 wire and runner, and the
pure helpers. Hosts own provider clients, credentials, subscriptions, freshness policy,
event ordering, replay progression, persistence, process supervision, order execution, risk,
reconciliation, accounting, settlement, and deployment.

When a public contract changes, update this guide, [Decision V6](decision-v6.md) when the
wire or runner changes, the affected conformance corpus and tests, and the consumers' pins.
