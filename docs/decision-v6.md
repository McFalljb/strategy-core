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
     (updates the previous decision deferred first; a cancel's never before its target's)
  -> on_event(trigger event) / on_start (Bootstrap, Recovery)
  -> one result: post-event checkpoint + commands in issue order + acknowledged command ids
```

- `Completed` advances the checkpoint by exactly one (the first decision writes sequence 1).
- `Rejected` (the kernel returned an error for the trigger) carries no commands and no
  acknowledgements and keeps the checkpoint unchanged, so the next decision derives the same
  updates again.
- A kernel error on one order update does not reject the decision. The runner snapshots the
  kernel with its own checkpoint codec before each update; on an error it restores the
  kernel through the factory, undoes the commands and telemetry of that update (its logs
  stay), records a `kernel_error` diagnostic naming the update and its attempt, and goes on
  with the remaining updates and the trigger. The entry stays as it was and the update is
  delivered again in the next decisions, up to `MAX_DELIVERY_ATTEMPTS = 3` counted
  failures; then it counts as seen (`order_update_abandoned`, an `error` diagnostic with
  the fill the kernel was never told of). Until then its receipt or terminal order is not
  acknowledged.
- An update is deferred instead of failed when the kernel returns the runner's refusal
  itself (not another error after catching one) for room in the decision (64 commands, 512
  plan rows, the result's byte budget) that earlier updates of the same decision took:
  the update's own commands would fit a decision without them. It waits, as it was
  (`order_update_deferred`, a `warn` diagnostic), for up to `MAX_DELIVERY_DEFERRALS = 8`
  decisions in a row (`delivery_deferrals`), then is abandoned like a failed one. The next
  decision delivers deferred updates first, the most deferred first (then in issue order),
  before the others (in issue order), so the update closest to the bound runs without
  earlier commands: it then succeeds, or its refusal counts. A cancel's update never
  precedes an update of its target: the target's is delivered just before it (for a
  cancel-all, the updates of every order placed before it), and the pulled updates and the
  cancel's form one delivery unit whose commands count as the cancel's own, so a refusal
  for room inside the unit counts rather than defers. A cancel's update is held when an
  update of any of its targets failed anywhere in the decision (in its unit or an earlier
  one, for any reason): it waits as it was, to follow its target's (`order_update_held`, a
  warning). A hold counts as a deferral; rather than reach `MAX_DELIVERY_DEFERRALS`, the
  runner delivers the cancel's update anyway, out of order (`order_update_out_of_order`, a
  warning, once that delivery succeeds), so a cancel is never abandoned for its targets'
  failures: a cancel's or cancel-all's refusal for room whose deferral would reach the
  bound counts as a failure (resetting its deferrals), in order or out of order, instead of
  deferring into abandonment. A unit that can never
  fit (targets and cancel together over a limit every decision) ends with the cancel
  abandoned after three counted failures of its own. The order is deterministic, and the evidence follows it. A refusal
  at a Sleeve-wide bound (the open-order cap, 256 live runner entries), or at a decision
  limit the update exceeds on its own, is an ordinary counted failure.
- A snapshot the kernel's codec cannot take before an update is a counted failure of that
  update (it is not delivered). A snapshot the factory cannot restore after a failed update
  is a counted failure too (a kernel or factory defect); the kernel and the decision go back
  to how they were before the first update, and every other update is delivered again
  later without counting. If the kernel cannot be restored even to the decision's start,
  the decision fails.
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
allocation abort. Encoding checks the bytes decode within that limit. The runner admits
commands and telemetry against `RESULT_ENCODED_BUDGET_BYTES` (896 KiB), leaving room for the
decoder's integer accounting, then checks the result decodes under its bound and sheds
telemetry, then logs, until it does (`kernel_telemetry_overflow` counts them). Commands, the
checkpoint and order updates are never shed; `run_transaction` does not fail for size.

`DecisionContextV6` is the V5 context without `continuation`, `broker_replay` or the
frozen-encoding evidence, plus:

- `deployment_mode`: `Paper` or `Live`.
- `capabilities`: `timers` and the sorted `external_requests` the host allows, each
  `http:<endpoint>` or `command:<name>` (see "External requests").
- `orders_complete`: true when `broker.orders` holds every order of the Sleeve the host
  keeps. The host sets it false when it had to truncate the view; a truncated view never
  reports an order as vanished and allows no cancel-all.

Host invariants on the order view:
- A truncated view drops terminal orders only, never an open one: the runner seeds over any
  view.
- A view never shows an order older than a view at a lower Broker revision did (each view
  is the Sleeve's orders as of its revision, and a terminal order stays terminal). The
  runner forgets an order once its final update is delivered and adopts an untracked open
  order only from a view newer than any it has seen; an older record in such a view would
  be adopted and its fills reported again.
- `command_receipts`: the outcome of each recent command without an order record (a refused
  place, a cancel, a cancel-all), strictly sorted by command id, at most 256. A place receipt
  is always a refusal: an admitted place has an order record. A command never has both.

The Sleeve's order view holds at most `MAX_BROKER_ORDERS = 256` orders. A Sleeve holds at
most `max_open_orders(mode)` open orders (open context orders plus the decision's places):
192 in paper, 168 in live (`(512 - 5 - 3) / 3`), so a cancel-all over every open order
always fits the decision plan in either mode. That leaves at least 64 slots for terminal
orders whose outcome the Strategy has not yet acknowledged; the host keeps those ahead of
acknowledged ones. `validate_broker_order_v6`
checks one order record on its own, so a host can quarantine a bad record before building
the context. `Cancelled`, `Expired` and `Rejected` orders may report no remaining quantity
with less than their whole quantity filled.

Contributor stations are the owner projection's `opportunity.contributor_stations`; V6
requires them to be exactly the owner projection's stations (at most `MAX_STATIONS = 5`). The
multi-station owner projection is unchanged. `BrokerOrderV6` adds `fees_micros`, the
execution fees charged for the order's fills so far, and `rejection_reason`, the provider's
rejection text of a `Rejected` order when it gave one (at most 4 KiB; empty is the same as
none; ignored on other statuses). The host writes the reason with the status, atomically.

`TriggerV6` is `Owner(OwnerTriggerV6)`, `BrokerState { broker_revision }`, valid on its own
(not only under Recovery), or `ExternalResponse { request_id, outcome }` (see "External
requests"). `BrokerOutcome` is gone.

`DecisionResultV6` is `{ delivery_id, sleeve_identity, state_fence, expected_broker_revision,
disposition, kernel_checkpoint, commands, acknowledged_command_ids, evidence, diagnostics,
telemetry }`.

- Commands are `PlaceOrder`, `CancelOrder { target }`, `CancelAllOrders`, `ScheduleTimer`,
  `CancelTimer`, `Stop` and `ExternalRequest`, at most 64, all of which may be Broker
  commands (at most 8 external requests). The command at
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
  tracks, and, when the trigger is an external response, the request it answers. When they arrive in a durable write the host deletes the receipts and stops keeping
  those terminal orders ahead of acknowledged ones.
- `telemetry` holds typed `Counter`, `Gauge` and `Annotation` entries in call order; floats
  keep their exact bits. Logs are `kernel_log` diagnostics; a kernel error is a `kernel_error`
  diagnostic; entries past the bounds are counted in a `kernel_telemetry_overflow`
  diagnostic.
- `evidence` carries the order updates the runner delivered, as `order_updates` entries
  (`decode_order_update_evidence`), each at most 64 KiB, in delivery order; a refusal reason
  there is cut to 512 bytes.
- `diagnostics` also carry what the order-update comparison did: `runner_order_adopted`,
  `runner_order_not_adopted`, `runner_tombstone_evicted`, `runner_tombstone_expired` and
  `order_update_abandoned`, one per kind with a count and up to 8 command ids (the
  abandoned one is an `error` and also lists the fill each update carried and their
  total); a failed update is a `kernel_error`, a deferred one an `order_update_deferred`
  warning.

`KernelCheckpointV6` is the V5 checkpoint plus the runner section, sealed under
`strategy-core/decision-v6/checkpoint/v1`. The kernel's private state stays at most 128 KiB
and its codec stays the kernel's. The runner section is bounded separately: whether it is
seeded, the newest Broker revision it has compared (`newest_view_revision`), and at most `MAX_RUNNER_SECTION_ENTRIES = 288` entries of bounded identifiers
(`MAX_RUNNER_ENTRIES = 256` plus `MAX_TOMBSTONES = 32`), of which at most 32 are
tombstones.

## External requests

A Strategy may ask the host for an HTTP call or a command run; the host performs it after the
decision is saved, off the decision path, and the answer arrives as a later decision's
trigger. The call belongs to the Strategy (it builds the request, chooses when, handles the
answer); the host only does the I/O.

- **Kernel API.** `ctx.runtime().request_http(HttpRequest { endpoint, method, path, body,
  timeout_ms })` and `request_command(CommandRequest { command, args, stdin, timeout_ms })`
  return a `RequestTicket { request_id }` at once. The answer is the event
  `ExternalResponse { request_id, outcome: Ok { status, body } | Err { kind, message } }`.
- **Names, not addresses.** `endpoint` and `command` are allowlist names. The context grants
  them in `capabilities.external_requests` as `http:<endpoint>` and `command:<name>` (a name
  is an identifier without `:`), so the runner and validation know each one's kind. The
  host owns the base URL, credentials, program, leading arguments and environment.
- **Wire.** `StrategyCommandV6::ExternalRequest { command_id, kind, target, payload,
  timeout_ms }`: `kind` is `Http { method: Get | Post, path }` or `Command { args }`,
  `target` the allowlist name, `payload` the HTTP body or the command's standard input.
  `TriggerV6::ExternalResponse { request_id, outcome }` carries the answer's bytes in the
  context, so a recorded decision shows exactly what the Strategy saw and re-runs to the
  same result.
- **Ids.** A request's id is its command id (`command.` + 32 hex digits of its IntentId),
  like every other command of the decision.
- **Bounds.** A path starts with one `/`, has no `.` or `..` segment, no `\` or `#`, and is at
  most 2 KiB of visible ASCII (a query is allowed); at most 64 arguments without NUL; the path
  or arguments plus the payload at most 64 KiB (`MAX_EXTERNAL_REQUEST_BYTES`); a timeout of
  1 ms to 120 s; at most 8 requests per decision (`MAX_OUTSTANDING_EXTERNAL_REQUESTS`). A
  response body is at most 256 KiB; an `Ok` status is 0 (a command) or 2xx, an error's
  message 1 byte to 4 KiB, and a `Status` error names a status outside 2xx.
- **Errors.** `Refused` (not sent: not allowed, the Sleeve's 8 outstanding, shutdown),
  `Timeout`, `Transport`, `Status(code)`, `TooLarge`, `Malformed`, `Exit(code)` and
  `Abandoned` (the host restarted while the request was in flight).
- **Local errors.** A request not granted for its kind, outside the bounds, the ninth of a
  decision, or past the decision's command or byte budget is a local `Err`. Room refusals
  inside an order update defer the update like any other.
- **Ordering and durability.** A request is not a Broker command: it has no fence, no runner
  entry and no plan rows, and may share a decision with orders. It leaves the host
  (`StrategyCommandV6::leaves_host`), so a decision with a request needs the durable write
  a decision with Broker commands has; the host submits each once, also after a restart,
  and answers every request that was in flight at a restart `Abandoned`.
- **Acknowledgement.** A completed result whose trigger is an external response
  acknowledges that request id; the host forgets the request once the acknowledgement is
  durable. A rejected result acknowledges nothing.
- **Handshake.** A Strategy executable that issues requests states
  `EXTERNAL_REQUESTS_CAPABILITY` (`external-requests`) besides `HANDSHAKE_CAPABILITIES_V6`;
  the host requires it exactly when it grants the Strategy any request.

## Plan rows

The host admits a decision's Broker commands in one account plan limited to
`MAX_DECISION_PLAN_ROWS = 512`. The runner counts the rows as the kernel issues commands:
4 per decision, 5 per place (6 for a live Market sell, which the Broker refuses with a
receipt), 3 per cancel (1 when the target is already final, which the
Broker refuses), for a cancel-all in paper 3 plus one per order open when it is issued (every
open context order and every place issued before it), in live (where the Broker expands a
cancel-all into per-order cancels on the priority lane) 3 per open context order plus 1 per
own earlier place it collapses, at least 1; a later cancel-all counts only places issued
after the earlier one; and 1 when the result acknowledges anything (the host removes
acknowledged receipts and orders with one statement). A decision without Broker commands has
no plan. `decision_plan_rows_v6` gives the same count for a result (traderv3's parity test
checks it is at least the owner's); validation rejects a result over the limit.

## Order updates

The runner section records, for each order and command the Strategy has issued and not yet
seen finish: the command id and kind, the client order id, the order id once known, Market,
action, side, requested quantity, the last status, filled quantity and order revision the
Strategy was shown, the Broker revision it was issued at, whether it is a tombstone (and
since which revision, for how many views), and how often the kernel failed on its pending
update (and for how many decisions in a row it was deferred). Orders are matched to
entries by command id only. Before the trigger, for each entry in issue order (the updates
are then delivered in issue order, those the previous decision deferred first, a cancel's
never before its target's):

| Context shows | Update | Entry |
|---|---|---|
| The order, with a new status or filled quantity | status, `newly_filled` since the last update | kept, or pruned when terminal |
| The order, unchanged, or an older revision of it than the one last reported | none | kept |
| A refusal receipt for the place | `Refused { code, reason }`, final | pruned |
| Neither, in a view at or below the entry's issue revision, or a truncated view | none | kept |
| Neither, in a complete newer view | `vanished`: the last status seen (`Accepted` if none), `remaining = 0`, final | kept as a tombstone |
| A vanished order again | its real update, `newly_filled` counted from the tombstone | kept, or pruned when terminal |
| A refusal receipt for a cancel or cancel-all | `Refused`, final, `command_kind` names the cancel | pruned |
| An accepted cancel receipt | none: the target's own update reports the cancel | pruned |
| No receipt for a cancel whose target is shown final | none | pruned |
| No receipt for a cancel otherwise, in a complete newer view | none | kept as a tombstone until its receipt comes |

An update with `vanished` set is final only as far as the runner knows: if the order
reappears, its updates continue, the next one reporting what was filled meanwhile. A kernel
that keeps a vanished order in its books counts every fill.

Tombstones do not count toward the 256 live entries a Broker command is checked against, so
they never keep a Strategy from cancelling. At most 32 are kept (the oldest is evicted; a
tombstone whose order is back but whose update failed goes last), and one expires after
`TOMBSTONE_EXPIRY_VIEWS = 16` complete views above its vanish revision that do not show it.
An evicted or expired order that later returns final is acknowledged without an update; one
that returns open is adopted (below). A tombstone whose order is back, and an adopted order,
are live again, so the live entries may pass 256 (up to 288, the section's bound); no Broker
command is issued until they are below 256 again.

Broker statuses map to update statuses: accepted and dispatched are `Accepted`, resting
`Resting`, partially filled `PartiallyFilled`, filled `Filled`, cancelled `Cancelled`, expired
`Expired`, rejected `Refused { code: "provider_rejected", reason }` where `reason` is the
order's `rejection_reason` (up to 4 KiB, all of it), or "the provider rejected the order"
when the provider gave none. Kernels classify transient rejections by that text; the
result's evidence records at most 512 bytes of it, cut on a character boundary. A cancellation request or a recovery hold is not news of its own: the order keeps
its last status (`PartiallyFilled` once anything filled).

Seeding: a Sleeve's first decision (or the first after `convert_v5_kernel_checkpoint`,
which keeps the kernel's bytes and starts an unseeded section), over a complete or a
truncated view, records the open orders as seen, from their current status, without
updates, so no Strategy receives a burst of updates for old orders. Afterwards an open
order the section does not track (its tombstone expired or was evicted) is adopted the same
way from a view newer than `newest_view_revision`, the highest Broker revision the section
has compared (`runner_order_adopted`); its updates are delivered from then on, and a kernel
should accept updates for orders it does not know. An order is not adopted when the section
holds 288 entries (`runner_order_not_adopted`; a later decision adopts it). A checkpoint
whose section holds more than 288 entries fails the decision before the kernel runs; a
place or cancel that would need live entry 257 is a local `Err`.

Acknowledgements: every receipt and every terminal order in the context the section does
not track. An entry is pruned only once its outcome was delivered (or abandoned), and every
decision seeds the section, which tracks each open order of the Sleeve until its final
update, so such an order is no news: an order whose final update was delivered, a terminal
order older than the section, or one whose tombstone expired or was evicted.

Because nothing is queued, a checkpoint that goes back (a crash between durable writes)
produces the same updates again, and a dropped, coalesced, stale, truncated or reordered view
loses nothing: every fill is reported exactly once in `newly_filled` (short of an evicted or
expired tombstone, whose order is adopted from its current fill if it returns open, or an
update abandoned after three counted failures, whose fill the `order_update_abandoned`
diagnostic reports).

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
or this decision; an open order past `max_open_orders(mode)`; a cancel naming no order the context or
the decision knows; a cancel-all over a truncated view; the 65th command; the plan row
limit; the runner section bound; a command that would take the result past its size budget
(the budget assumes the kernel's state at its 128 KiB bound). Timers need the timer
capability. A live Market sell is not a local error: the Broker refuses it per command
(`market_sell_unsupported`, until Phase 5) and the kernel sees a `Refused` update;
`capabilities().market_sell` is false in live so a kernel can exit with a limit sell. Logs and telemetry fill only the room the checkpoint, commands and update
evidence leave; the rest is counted in a `kernel_telemetry_overflow` diagnostic.

## Kernel runner

With the `kernel` feature, `strategy_core_v3::kernel_v6` presents a context to a
`strategy_core_kernel::NativeKernel` and assembles the result. Strategy executables supply
kernel construction, restore and checkpoint codecs through `TransactionKernelFactory`, and
`TransactionKernel::market_buy_price_cap_micros`, which the runner asks once, when the kernel
places a Market buy, of the kernel as restored for the decision. `TransactionKernel` needs no
`Clone`: the runner restores kernels only through the factory and the kernel's codec.

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
