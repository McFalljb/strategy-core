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
- `Rejected` (the kernel returned an error, in an update handler or for the trigger) carries
  no commands and no acknowledgements and keeps the checkpoint unchanged, so the next
  decision derives the same updates again.
- The host validates with `validate_decision_result_v6(context, result)`, which binds the
  delivery, Sleeve, state fence (`decision_fence_v6_sha256`), Broker revision, Market scope,
  cancel targets, timer capability, acknowledgements and the plan row limit.

## Wire

| | |
|---|---|
| Context | `SDCTXV6A` + `DecisionContextV6`, at most 20 MiB |
| Result | `SDRESV6A` + `DecisionResultV6`, at most 1 MiB |
| Encoding | bincode 2, standard configuration, big-endian, variable integers (as V5) |

`DecisionContextV6` is the V5 context without `continuation`, `broker_replay` or the
frozen-encoding evidence, plus:

- `deployment_mode`: `Paper` or `Live`.
- `capabilities`: `timers` and the sorted `external_requests` names the host allows (empty
  until Phase 4 grants any).
- `command_receipts`: the outcome of each recent command without an order record (a refused
  place, a cancel, a cancel-all), strictly sorted by command id, at most 256. A place receipt
  is always a refusal: an admitted place has an order record. A command never has both.

Contributor stations are the owner projection's `opportunity.contributor_stations`; V6
requires them to be exactly the owner projection's stations (at most `MAX_STATIONS = 5`). The
multi-station owner projection is unchanged. `BrokerOrderV6` adds `fees_micros`, the
execution fees charged for the order's fills so far.

`TriggerV6` is `Owner(OwnerTriggerV6)` or `BrokerState { broker_revision }`, valid on its own
(not only under Recovery). `BrokerOutcome` is gone.

`DecisionResultV6` is `{ delivery_id, sleeve_identity, state_fence, expected_broker_revision,
disposition, kernel_checkpoint, commands, acknowledged_command_ids, evidence, diagnostics,
telemetry }`.

- Commands are `PlaceOrder`, `CancelOrder { target }`, `CancelAllOrders`, `ScheduleTimer`,
  `CancelTimer` and `Stop`, at most 64, all of which may be Broker commands. The command at
  index `n` is `command.<delivery_id>.<n>`; timer and stop commands take ordinals too.
  `IntentId` is `sha256(DecisionId, n)` as before.
- Broker commands carry no fence of their own: `expected_broker_revision` fences the
  decision. A cancel names `Order { order_id, expected_order_revision }` (an order in the
  context) or `SameDecision { provider_client_id }` (a place earlier in the same result).
- A place's `provider_client_id` is the kernel's `client_order_id`, else
  `derive_provider_client_id_v6`: `tv3<mode>_` + the first 24 hex digits of the IntentId, as
  the Broker derives it. Client ids are unique within a decision.
- `ScheduleTimer` carries the decision's generation `timer.<delivery_id>`; `CancelTimer`
  carries the generation the timer was scheduled under. One timer operation per key.
- `acknowledged_command_ids` lists the receipts and terminal orders in the context whose
  final outcome the Strategy has now seen. When they arrive in a durable write the host may
  delete the receipts and stop keeping those terminal orders ahead of acknowledged ones.
- `telemetry` holds typed `Counter`, `Gauge` and `Annotation` entries in call order; floats
  keep their exact bits. Logs are `kernel_log` diagnostics; a kernel error is a `kernel_error`
  diagnostic; entries past the bounds are counted in a `kernel_telemetry_overflow`
  diagnostic.
- `evidence` carries the order updates the runner delivered, as `order_updates` entries
  (`decode_order_update_evidence`), each at most 64 KiB, in delivery order.

`KernelCheckpointV6` is the V5 checkpoint plus the runner section, sealed under
`strategy-core/decision-v6/checkpoint/v1`. The kernel's private state stays at most 128 KiB
and its codec stays the kernel's. The runner section is bounded separately: at most 256
entries of bounded identifiers.

## Plan rows

The host admits a decision's Broker commands in one account plan limited to
`MAX_DECISION_PLAN_ROWS = 512`. The runner counts the rows as the kernel issues commands:
4 per decision, 5 per place, 3 per cancel (1 when the target is already final, which the
Broker refuses), 3 plus one per order it cancels for a cancel-all (every open context order
and every place of the decision), and 1 when the result acknowledges anything. A decision
without Broker commands has no plan. `decision_plan_rows_v6` gives the same count for a
result; validation rejects a result over the limit.

## Order updates

The runner section records, for each order and command the Strategy has issued and not yet
seen finish: the command id and kind, the client order id, the order id once known, Market,
action, side, requested quantity, and the last status, filled quantity and order revision the
Strategy was shown. Before the trigger, for each entry in issue order:

| Context shows | Update | Entry |
|---|---|---|
| The order, with a new status or filled quantity | status, `newly_filled` since the last update | kept, or pruned when terminal |
| The order, unchanged | none | kept |
| A refusal receipt for the place | `Refused { code, reason }`, final | pruned |
| Neither (the order vanished) | the last status seen (`Accepted` if none), `remaining = 0`, final | pruned |
| A refusal receipt for a cancel or cancel-all | `Refused`, final, `command_kind` names the cancel | pruned |
| An accepted cancel receipt | none: the target's own update reports the cancel | pruned |

Broker statuses map to update statuses: accepted and dispatched are `Accepted`, resting
`Resting`, partially filled `PartiallyFilled`, filled `Filled`, cancelled `Cancelled`, expired
`Expired`, rejected `Refused { code: "provider_rejected" }`. A cancellation request or a
recovery hold is not news of its own: the order keeps its last status (`PartiallyFilled`
once anything filled).

An open order the section does not track is recorded as seen without an update. That is the
case after a V5 checkpoint is converted (`convert_v5_kernel_checkpoint` keeps the kernel's
bytes and starts an empty section) and on a Sleeve's first decision, so no Strategy receives
a burst of updates for old orders. A section that would exceed 256 entries fails the decision
before the kernel runs; a place or cancel that would need entry 257 is a local `Err`.

Because nothing is queued, a checkpoint that goes back (a crash between durable writes)
produces the same updates again, and a dropped or coalesced Broker-state trigger loses
nothing.

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
bytes or already used by an order in the context, the runner section or this decision; a
cancel naming no order the context or the decision knows; the 65th command; the plan row
limit; the runner section bound. Timers need the timer capability.

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
with their context and plan rows), invalid contexts and results with their error category,
and the provisional overlay arithmetic. The `decision_v6::corpus_tests` gate rebuilds the
corpus and checks every verdict; regenerate it with
`cargo test -p strategy-core-v3 -- --ignored write_v6_corpus`.
