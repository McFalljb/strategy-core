# Decision V6: one run per event

Decision V6 is the bounded shared contract between Trader V3 and a stateful Strategy. A
Strategy runs once per delivered event. Broker calls return a ticket at once, and every
command the run issues travels in one result. The host admits a decision's Broker commands
in the same durable write that saves its post-event checkpoint. What then happens to each
order reaches the Strategy as `OrderUpdate` events, derived by the runner from the Broker
state in each later context. Nothing re-runs, nothing is replayed, and no continuation is
stored.

Design: `traderv3/docs/plans/2026-09-24-decision-v6-single-run.md`. Points this contract had
to settle beyond the design note are listed in
[decision-v6-open-points.md](decision-v6-open-points.md).

## Decision

```text
V6 context (owner projection, scope, Broker state, command receipts, checkpoint, trigger)
  -> runner compares Broker state + receipts with the checkpoint's runner section
  -> on_event(OrderUpdate) for each change, in the order the commands were issued
  -> on_event(trigger event) / on_start (Bootstrap, Recovery)
  -> one result: post-event checkpoint + commands in issue order + acknowledged command ids
```

- `Completed` advances the checkpoint by exactly one (the first decision writes sequence 1).
- `Rejected` (the kernel returned an error for the trigger) carries no commands and no
  acknowledgements and keeps the checkpoint unchanged, so the next decision derives the same
  updates again.
- A kernel error on one order update does not reject the decision: the kernel and the
  decision's commands go back to how they were before that update, a `kernel_error`
  diagnostic names it, the update counts as seen, and the remaining updates and the trigger
  are delivered.
- The host validates with `validate_decision_result_v6(context, result)`, which binds the
  delivery, Sleeve, state fence (`decision_fence_v6_sha256`), Broker revision, Market scope,
  cancel targets, timer capability, acknowledgements and the plan row limit.

## Wire

| | |
|---|---|
| Context | `SDCTXV6A` + `DecisionContextV6`, at most 20 MiB |
| Result | `SDRESV6A` + `DecisionResultV6`, at most 1 MiB |
| Encoding | bincode 2, standard configuration, big-endian, variable integers (as V5) |

Decoding claims every length prefix (and every decoded integer at its full width) against
the bound of its magic before allocating, so a corrupt prefix is a `Decode` error, never an
allocation abort. Encoding checks the bytes decode within that limit. The runner keeps a
result within `RESULT_ENCODED_BUDGET_BYTES` (896 KiB), which leaves room for the decoder's
integer accounting.

`DecisionContextV6` is the V5 context without `continuation`, `broker_replay` or the
frozen-encoding evidence, plus:

- `deployment_mode`: `Paper` or `Live`.
- `capabilities`: `timers` and the sorted `external_requests` names the host allows (empty
  until Phase 4 grants any).
- `orders_complete`: true when `broker.orders` holds every order of the Sleeve the host
  keeps. The host sets it false when it had to truncate the view; a truncated view never
  reports an order as vanished and allows no cancel-all.
- `command_receipts`: the outcome of each recent command without an order record (a refused
  place, a cancel, a cancel-all), strictly sorted by command id, at most 256. A place receipt
  is always a refusal: an admitted place has an order record. A command never has both.

The Sleeve's order view holds at most `MAX_BROKER_ORDERS = 256` orders. A Sleeve holds at
most `MAX_OPEN_ORDERS = 192` open orders (open context orders plus the decision's places),
which leaves 64 slots for terminal orders whose outcome the Strategy has not yet
acknowledged; the host keeps those ahead of acknowledged ones. `validate_broker_order_v6`
checks one order record on its own, so a host can quarantine a bad record before building
the context. `Cancelled`, `Expired` and `Rejected` orders may report no remaining quantity
with less than their whole quantity filled.

Contributor stations are the owner projection's `opportunity.contributor_stations`; V6
requires them to be exactly the owner projection's stations (at most `MAX_STATIONS = 5`). The
multi-station owner projection is unchanged. `BrokerOrderV6` adds `fees_micros`, the
execution fees charged for the order's fills so far, and `rejection_reason`, the provider's
rejection text of a `Rejected` order when it gave one (non-empty, at most 512 bytes, only on
a `Rejected` order).

`TriggerV6` is `Owner(OwnerTriggerV6)` or `BrokerState { broker_revision }`, valid on its own
(not only under Recovery). `BrokerOutcome` is gone.

`DecisionResultV6` is `{ delivery_id, sleeve_identity, state_fence, expected_broker_revision,
disposition, kernel_checkpoint, commands, acknowledged_command_ids, evidence, diagnostics,
telemetry }`.

- Commands are `PlaceOrder`, `CancelOrder { target }`, `CancelAllOrders`, `ScheduleTimer`,
  `CancelTimer` and `Stop`, at most 64, all of which may be Broker commands. The command at
  index `n` of the result has ordinal `n`, timer and stop commands included. Its IntentId is
  `sha256(INTENT_DOMAIN, DecisionId, n)` with `DecisionId = sha256(DECISION_DOMAIN, Sleeve id,
  incarnation, delivery id)` (`intent_id_v6`), and its id is `command.` plus the first 32 hex
  digits of that IntentId (`command_id_v6`): deterministic, and unique across Sleeves.
  traderv3 must derive each command's IntentId with this ordinal, counting timer and stop
  commands, so its intents, provider client ids and command ids agree with the runner's.
- Broker commands carry no fence of their own: `expected_broker_revision` fences the
  decision. A cancel names `Order { order_id, expected_order_revision }` (an order in the
  context) or `SameDecision { provider_client_id }` (a place earlier in the same result).
- A place's `provider_client_id` is the kernel's `client_order_id`, else
  `derive_provider_client_id_v6`: `tv3<mode>_` + the first 24 hex digits of the IntentId, as
  the Broker derives it. A kernel's own client ids may not start with `tv3` and must be unique
  for the account's lifetime: the runner matches orders by command id, but the Broker refuses
  a reused client id.
- `ScheduleTimer` carries the decision's generation `timer.<delivery_id>`; `CancelTimer`
  carries the generation the timer was scheduled under. One timer operation per key.
- `acknowledged_command_ids` lists the receipts and terminal orders in the context whose
  final outcome the Strategy has seen, never a command the result's runner section still
  tracks. When they arrive in a durable write the host deletes the receipts and stops keeping
  those terminal orders ahead of acknowledged ones.
- `telemetry` holds typed `Counter`, `Gauge` and `Annotation` entries in call order; floats
  keep their exact bits. Logs are `kernel_log` diagnostics; a kernel error is a `kernel_error`
  diagnostic; entries past the bounds are counted in a `kernel_telemetry_overflow`
  diagnostic.
- `evidence` carries the order updates the runner delivered, as `order_updates` entries
  (`decode_order_update_evidence`), each at most 64 KiB, in delivery order; a refusal reason
  there is cut to 512 bytes (the kernel sees all of it).

`KernelCheckpointV6` is the V5 checkpoint plus the runner section, sealed under
`strategy-core/decision-v6/checkpoint/v1`. The kernel's private state stays at most 128 KiB
and its codec stays the kernel's. The runner section is bounded separately: whether it is
seeded, at most 256 entries of bounded identifiers, and at most 256 reported terminal
orders.

## Plan rows

The host admits a decision's Broker commands in one account plan limited to
`MAX_DECISION_PLAN_ROWS = 512`. The runner counts the rows as the kernel issues commands:
4 per decision, 5 per place, 3 per cancel (1 when the target is already final, which the
Broker refuses), 3 plus one per order open when it is issued for a cancel-all (every open
context order and every place issued before it; a later cancel-all counts only places issued
after the earlier one), and 1 when the result acknowledges anything (the host removes
acknowledged receipts and orders with one statement). A decision without Broker commands has
no plan. `decision_plan_rows_v6` gives the same count for a result (traderv3's parity test
checks it is at least the owner's); validation rejects a result over the limit.

## Order updates

The runner section records, for each order and command the Strategy has issued and not yet
seen finish: the command id and kind, the client order id, the order id once known, Market,
action, side, requested quantity, the last status, filled quantity and order revision the
Strategy was shown, the Broker revision it was issued at, and whether it vanished. Orders are
matched to entries by command id only. Before the trigger, for each entry in issue order:

| Context shows | Update | Entry |
|---|---|---|
| The order, with a new status or filled quantity | status, `newly_filled` since the last update | kept, or pruned (and recorded as reported) when terminal |
| The order, unchanged, or an older revision of it than the one last reported | none | kept |
| A refusal receipt for the place | `Refused { code, reason }`, final | pruned |
| Neither, in a view at or below the entry's issue revision, or a truncated view | none | kept |
| Neither, in a complete newer view | the last status seen (`Accepted` if none), `remaining = 0`, final, once | kept as a tombstone |
| A vanished order again | its real update, `newly_filled` counted from the tombstone | kept, or pruned when terminal |
| A refusal receipt for a cancel or cancel-all | `Refused`, final, `command_kind` names the cancel | pruned |
| An accepted cancel receipt | none: the target's own update reports the cancel | pruned |
| No receipt for a cancel (in a newer view) | none: it was acknowledged earlier | pruned |

Broker statuses map to update statuses: accepted and dispatched are `Accepted`, resting
`Resting`, partially filled `PartiallyFilled`, filled `Filled`, cancelled `Cancelled`, expired
`Expired`, rejected `Refused { code: "provider_rejected", reason }` where `reason` is the
order's `rejection_reason`, or "the provider rejected the order" when the provider gave
none. Kernels classify transient rejections by that text. A cancellation request or a
recovery hold is not news of its own: the order keeps its last status (`PartiallyFilled`
once anything filled).

Acknowledgements: every receipt the section no longer tracks (an entry is pruned only once
its outcome was reported), and every terminal order the section recorded as reported, until
a complete view no longer shows it. Only the seeding decision acknowledges other terminal
orders: a Sleeve's first decision, or the first after `convert_v5_kernel_checkpoint` (which
keeps the kernel's bytes and starts an unseeded section). It also records open orders as
seen, from their current status, without updates, so no Strategy receives a burst of updates
for old orders. A section that would exceed 256 entries fails the decision before the kernel
runs; a place or cancel that would need entry 257 is a local `Err`.

Because nothing is queued, a checkpoint that goes back (a crash between durable writes)
produces the same updates again, and a dropped, coalesced, stale, truncated or reordered view
loses nothing: every fill is reported exactly once in `newly_filled`.

## Provisional view

Within one decision the runner overlays the kernel's own commands on the Broker view:

| Read | After an earlier `place_order` in the decision |
|---|---|
| `pending_orders()` | lists it with status `submitted`, its client id, empty order id and `reserved_cost` |
| `buying_power()` / `financial_state()` | commitment and local reservation grow by its reservation |
| `order_status(client_id)` | `Submitted` |
| positions | unchanged |

A buy reserves `fees::buy_commitment_micros(price, quantity, market.fee_terms())`, the
Broker's own formula, at its limit price (a Market buy at the kernel's price cap, else one
dollar). A sell reserves nothing. A cancel or cancel-all marks its targets, the decision's
own orders included, `cancellation_requested`; their reservation stays until the Broker
releases it. The Broker remains the authority: a wrong estimate is refused and the Strategy
sees `Refused`.

Local errors from a Broker call: a Market outside the Sleeve's scope, an invalid quantity or
price, a Market without valid fee terms, a client order id that is invalid, longer than 128
bytes, starts with `tv3`, or is already used by an order in the context, the runner section
or this decision; the 193rd open order; a cancel naming no order the context or the decision
knows; a cancel-all over a truncated view; the 65th command; the plan row limit; the runner
section bound; a command that would take the result past its size budget (the budget
assumes the kernel's state at its 128 KiB bound). Timers need the timer capability. Logs and
telemetry fill only the room the checkpoint, commands and update evidence leave; the rest is
counted in a `kernel_telemetry_overflow` diagnostic.

## Kernel runner

With the `kernel` feature, `strategy_core_v3::kernel_v6` presents a context to a
`strategy_core_kernel::NativeKernel` and assembles the result. Strategy executables supply
kernel construction, restore and checkpoint codecs through `TransactionKernelFactory`, and
`TransactionKernel::market_buy_price_cap_micros`, which the runner asks once, when the kernel
places a Market buy, of the kernel as restored for the decision.

The context is projected once into the kernel crate's owned model (`StationState`,
`MarketState`, `StrategyEvent`): supplied originals first, the V4 owner projection otherwise,
each component with its `ValueOrigin`. Weather, forecast and oracle events come from the
station that triggered them, which may be any contributor station; price and timer events
from the primary station. A Broker-state trigger delivers `Unknown { event_type:
"broker_state" }` after its updates. Event `emitted_at` is the provider's publication time,
never the decision clock.

## Supplied inputs

`supplied: SuppliedInputsV6` carries the provider's retained fields at supplied precision
plus the exact typed originating event (types owned by `strategy_core_kernel::supplied`).
Numbers are `DecimalV6 { coefficient, scale }`, supplied times are `*_unix_ns`, derived times
`*_unix_ms`, and `Option` means absent-or-null. Current captured weather triggers use
`current_inputs.originating`; other weather triggers use `supplied.originating_event`. The
embedded V4 owner projection keeps its frozen positional observation and weather layout
(`decision_v4::observation_codec`), encoded with the enclosing variable-integer
configuration.

## Conformance

`conformance/v6/decision-transactions.json` (schema `strategy-core-decision-v6-corpus/1`)
records every vector's exact bytes, length and SHA-256: valid contexts and results (results
with their context and plan rows), invalid contexts and results with their error category
(including 2^62 length prefixes at each level of both), and the provisional overlay
arithmetic. The `decision_v6::corpus_tests` gate rebuilds the
corpus and checks every verdict; regenerate it with
`cargo test -p strategy-core-v3 -- --ignored write_v6_corpus`.
