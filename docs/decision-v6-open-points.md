# Decision V6: points settled in strategy-core

The V6 design note (`traderv3/docs/plans/2026-09-24-decision-v6-single-run.md`) and the
admission spike leave these details open or say something the implementation could not do
literally. Each entry records the choice made on branch `phase3/decision-v6` and why. The
most conservative reading consistent with the note was taken where there was a choice.
traderv3 and strategies must build to these choices or change them here first.

## Wire

1. **Contributor stations.** The note lists a contributor station list as a context
   addition. The V4 owner projection already carries it
   (`owner_state.opportunity.contributor_stations`), so V6 adds no second copy: the context
   exposes it (`DecisionContextV6::contributor_stations`) and validation requires it to be
   exactly the owner projection's stations, unique, at most `MAX_STATIONS = 5`, including
   the primary station. traderv3 must deliver a station for every contributor.
2. **Order fees and rejection text.** `OrderUpdate.fee_cost` needs the fees charged on
   each order, which V5's Broker order did not carry: `BrokerOrderV6` adds `fees_micros`.
   Kernels (V10, V12) classify transient rejections by the provider's text, so
   `BrokerOrderV6` also adds `rejection_reason`, which the host fills from the provider's
   rejection message and writes with the order's status, atomically. Validation is lenient:
   up to 4 KiB (as V5 accepted reasons), empty is the same as none, ignored unless the order
   is `Rejected`. The runner reports all of it as the `provider_rejected` refusal's reason
   (V5 parity: kernels classify by keywords anywhere in it), with a fixed text when absent;
   only the evidence copy is cut to 512 bytes on a character boundary. The harness's V5
   conversion fills it from the V5 outcome's reason.
3. **Capabilities.** `CapabilityGrantV6 { timers, external_requests }`; the request names
   are strictly sorted identifiers, at most 32, each `http:<name>` or `command:<name>`
   (point 37). Without `timers`,
   `wake_at` and `cancel_timer` are local errors and validation rejects timer commands.
   `KernelCapabilities` gained `external_requests`.
4. **Deployment mode** is `Paper` or `Live` (traderv3's `DeploymentMode`); there is no
   `Replay` on the wire. Audit re-runs reuse the recorded mode.
5. **Typed telemetry.** The note's `DecisionResultV6` field list has no telemetry field but
   also says gauges and annotations become typed entries in the result. The result has a
   `telemetry` field; counters are typed too (so the recorder has one telemetry shape).
   Logs stay `kernel_log` diagnostics.
6. **Derived updates as evidence.** Evidence code `order_updates`; the payload is the wire
   encoding of `Vec<OrderUpdateRecordV6>`, split into entries of at most 64 KiB, in delivery
   order. Rejected results carry them too (they were delivered, though not recorded as seen).
7. **Command ids and ordinals.** Every command, timer and stop commands included, takes
   the next ordinal `n` (its index in the result). Its id is `command.` plus the first 32
   hex digits of its IntentId `sha256(DecisionId, n)`, and a derived client order id uses the
   same IntentId. The note's `command.<delivery_id>.<n>` collided across Sleeves that share
   a delivery id; the IntentId binds the Sleeve and incarnation too. traderv3 must derive
   IntentIds with the runner's ordinal, counting timer and stop commands. (V5 numbered timers
   from 100.)
8. **Receipts.** `CommandReceiptV6 { command_id, kind, outcome: Accepted | Refused { code,
   reason } }`, strictly sorted by command id. A place receipt must be a refusal (an admitted
   place has an order record), and no command has both. A place cancelled in the same
   decision is expected as an order record with status `Cancelled` plus an `Accepted` cancel
   receipt, not as a place receipt.
9. **Acknowledgements.** `acknowledged_command_ids` names receipts and terminal orders (the
   note's order view keeps unacknowledged terminal orders, so orders need acknowledging
   too). Because only durable writes apply acknowledgements, every result repeats them while
   the context still shows them: every receipt and every terminal order the result's runner
   section does not track. There is no list of reported orders: an entry is pruned only
   once its outcome was delivered (or abandoned), and every decision seeds the section
   (point 23), which then tracks every open order of the Sleeve until its final update, so
   an untracked terminal order is no news: one whose final update this or an earlier
   decision delivered, one that was terminal before the section first saw it, or one whose
   tombstone expired or was evicted (acknowledged without an update). This holds over
   truncated views too. A result never acknowledges a command its section still tracks
   (a pending, failed update keeps its receipt or order unacknowledged), and a rejected
   result acknowledges nothing.
10. **Result size.** V5's 256 KiB result bound cannot hold a 128 KiB state, the runner
    section, 64 commands and the update evidence, so the V6 result bound is 1 MiB. Decoding
    charges every length prefix and integer (at its full width) against that bound before
    allocating. Commands are admitted against 896 KiB (`RESULT_ENCODED_BUDGET_BYTES`) less
    the room the checkpoint (kernel state counted at 128 KiB), runner section, update
    evidence and acknowledgements need; a command past it is a local error. Logs and
    telemetry fill what is left (at most 256 telemetry entries and 256 KiB). The budget's
    margin is a heuristic: the runner then checks the result decodes under the bound and
    sheds telemetry, then logs, until it does, all counted as overflow, so `run_transaction`
    never fails for size on a valid kernel. A refusal reason in the evidence copy is cut to
    512 bytes (the kernel sees up to 4 KiB).
11. **Runner section size.** An entry holds up to four bounded identifiers (160 bytes each),
    so the worst case is about 730 bytes, not the note's ~350; the section's 288 entries
    (`MAX_RUNNER_SECTION_ENTRIES`: 256 plus 32 tombstones) are about 210 KB. `MAX_ENCODED_KERNEL_CHECKPOINT_BYTES` includes them;
    traderv3's checkpoint row bound must grow to match.
12. **State fence.** `decision_fence_v6_sha256` is domain-separated, length-prefixes its
    identifiers, and also binds the deployment mode, capabilities and receipts.
13. **Frozen encodings.** V6 has one encoding. The V5 `retained_supplied_encoding` evidence
    is gone. The V4 owner projection's positional observation/weather codec (with its
    retained WU slots) is unchanged; it moved from `wire_v4.rs` to
    `decision_v4/observation_codec.rs`.
14. **Handshake.** strategy-core defines no handshake; `HANDSHAKE_CAPABILITIES_V6`
    (`decision-v6`, `kernel-checkpoint`, `liveness`) is exported for the executable and host
    to share.
15. **V5 checkpoint conversion** was the last V5-named code:
    `KernelCheckpointV5Layout` (the V5 field order, for the host to decode its stored bytes
    with its own codec) and `convert_v5_kernel_checkpoint`, which checked the V5 digest,
    kept the kernel bytes and sequence, and started an unseeded runner section. traderv3
    deleted its only caller in Phase 8, and Phase 10a deleted both (point 48).
16. **No V5 decoding.** strategy-core has no V5 context or result decoder. The equivalence
    harness reads recorded V5 contexts and results by depending on strategy-core revision
    c2f654b under a renamed dependency. Its V5 → V6 context conversion fills
    `BrokerOrderV6.fees_micros` from the V5 fill data (0 where absent), leaves
    `command_receipts` empty, fills `rejection_reason` from the V5 outcome's reason, sets
    `orders_complete` true, and converts the checkpoint with `convert_v5_kernel_checkpoint`.
    Historical: that harness and the converter are gone; strategy-core `1025824` is the last
    revision with the converter (point 15).
17. **Truncated views.** The context carries `orders_complete`; the host sets it false when
    it had to truncate the Sleeve's order view. A truncated view reports no order as
    vanished and allows no cancel-all (a local error; validation rejects one). Host
    invariant: a truncated view drops terminal orders only, never an open one (traderv3
    computes `orders_complete` over active and unacknowledged orders), so the runner seeds
    over it.
18. **Early-stopped terminal orders.** `Expired` and `Rejected` orders, like `Cancelled`
    ones, may report no remaining quantity with less than their whole quantity filled.
    `validate_broker_order_v6` checks one order record on its own so the host can quarantine
    a record that would make every context of the Sleeve invalid.
19. **Decoding limits.** Contexts and results decode under bincode's limit set to their
    bound (20 MiB, 1 MiB); encoding checks the bytes decode within it.

## Order updates

20. **Shape.** `OrderUpdate` adds `command_kind`, `is_final` and `vanished` to the note's
    fields. A vanish is reported distinctly: `vanished` set, the last status seen,
    `is_final` true, nothing remaining; updates follow if the order reappears, so a kernel
    should keep a vanished order in its books.
    `action` and `contract_side` are `Option` because a cancel-all refusal concerns no
    order (its `client_order_id` and `ticker` are empty). A cancel's refusal is an update
    with `command_kind: CancelOrder` whose order fields describe the target; kernels must
    check `command_kind` before treating a `Refused` update as their order's fate. An
    admitted cancel has no update of its own: the target's `Cancelled` update reports it.
21. **Status mapping.** Durably accepted and dispatched are `Accepted`. A cancellation
    request or a recovery hold keeps the last status (`PartiallyFilled` once anything
    filled). A provider-rejected order is `Refused { code: "provider_rejected" }` with the
    order's `rejection_reason` (or a fixed text). An order
    that vanishes before its first update is reported as `Accepted`, final, nothing
    remaining.
22. **Stale views and tombstones.** Each entry records the Broker revision it was issued
    (or adopted) at. An order missing from a context at or below that revision, or from a
    truncated view, is no news; an order record older than the one last reported is
    ignored. An order missing from a complete, newer view is reported final once and kept
    as a tombstone; if it reappears, its real update follows with `newly_filled` counted
    from the tombstone. A cancel or cancel-all whose receipt a complete newer view does not
    show waits as a tombstone too (unless its target is shown final), so a late refusal
    still reaches the kernel. Tombstones do not count toward the 256 live entries a Broker
    command is checked against; at most 32 are kept (the oldest is evicted, with a
    diagnostic; a tombstone whose order is back with a failed update goes last) and one
    expires after 16 complete views above its vanish revision that do not show it. A
    revived tombstone (and an adopted order) is live again, so a derived section may hold
    more than 256 live entries: derivation and validation bound the section at 288 entries
    (`MAX_RUNNER_SECTION_ENTRIES`), and no Broker command is issued while 256 or more are
    live. The revision check cannot see every stale view, because the Broker revision
    is account-wide and moves with other Sleeves; the tombstone makes a false vanish
    recoverable. Orders are matched by command id only.
23. **Seeding and adoption.** The runner section records whether it is seeded. The seeding
    decision (a Sleeve's first; until strategy-core `1025824`, also the first after
    converting a V5 checkpoint), over a
    complete or a truncated view, records the open orders as seen without updates. Since
    the Sleeve's view holds only its own orders, an open order the section does not track
    later (its tombstone expired or was evicted) is adopted the same way, reported as
    `runner_order_adopted`, and its updates are delivered from then on; kernels must accept
    updates for orders they do not know (the fill before adoption is not reported). The
    section records the newest Broker revision it has compared
    (`RunnerSectionV6::newest_view_revision`) and adopts only from a newer view: a stale view
    could show an order older than the Strategy was told of, even one whose final update
    was delivered, and its fills would be reported again. Host invariant: a view never shows
    an order older than a view at a lower revision did. An order is not adopted while the
    section holds 288 entries (`runner_order_not_adopted`). Acknowledgements follow (point
    9). A checkpoint whose section holds more than 288 entries fails the decision before the
    kernel runs.
24. **Order of delivery.** Updates precede `on_start` for Bootstrap and Recovery too. A
    Broker-state trigger delivers `Unknown { event_type: "broker_state" }` after its
    updates, as V5 did. Before each update the runner snapshots the kernel with its own
    checkpoint codec; a kernel error on the update restores the kernel through the factory,
    undoes the decision's commands and the update's telemetry (its logs stay), records a
    `kernel_error` diagnostic and goes on; the decision completes. The entry keeps a
    delivery-failure count and stays as it was, so the update is delivered again, up to 3
    counted failures (per entry, consecutively), then counts as seen
    (`order_update_abandoned`, severity `error`, with the `newly_filled` each abandoned update
    carried and their total); its receipt or terminal order is not acknowledged until then.
    An update is deferred, not failed, only when the kernel returns the runner's refusal
    itself (compared by its text: an error returned after catching a refusal counts) and
    the refusal was for room in the decision (64 commands, 512 plan rows, the result byte
    budget) that earlier updates of the same decision took, so the update's own commands
    would fit a decision without them (`order_update_deferred`, a warning). A kernel that
    hedges a fill with `?` does not lose it to a burst of an earlier update. Deferrals are
    bounded separately: after `MAX_DELIVERY_DEFERRALS = 8` decisions in a row
    (`RunnerEntryV6::delivery_deferrals`) the update is abandoned. The next decision
    delivers deferred updates first, the most deferred first (then in issue order), then the
    others in issue order, so contenders for the same room take turns and none starves. A
    cancel's update is never delivered before an update of its target (the target's is
    pulled forward to just before it; for a cancel-all, those of every order placed before
    it). The pulled updates and the cancel's are one delivery unit: when deciding whether a
    refusal defers, the unit's commands are the update's own, so a moving target cannot
    keep its deferred cancel deferred until it is abandoned. A cancel's update is held when
    an update of any of its targets failed anywhere in the decision (`order_update_held`),
    and follows the target's later; each hold counts as a deferral, and at the bound the
    cancel's update is delivered anyway, out of order (`order_update_out_of_order`, emitted
    once the delivery succeeds), never abandoned for its targets' failures: a cancel's or
    cancel-all's room refusal whose deferral would reach the bound counts as a failure and
    resets the deferrals instead (in order or out of order). A unit that never fits is abandoned by design after
    three counted failures of the cancel. A refusal at a
    Sleeve-wide bound (open-order cap, 256 live entries), or one an update hits on its own,
    counts: waiting would never make room. A snapshot the kernel's codec cannot take before
    an update is a counted failure of that update, which is not delivered. A snapshot the
    factory cannot restore after a failed update is a counted failure (a kernel or factory
    defect, so the decisions cannot loop on it) and takes the kernel and the decision back
    to their start; every other update is delivered again later without counting, the
    trigger still runs, and only a failure to restore the decision's start fails the
    transaction. The snapshot carries an
    empty runner section, so it costs the kernel's state only.
    `TransactionKernel` no longer needs `Clone`. A kernel error on the trigger rejects the
    decision.
25. **Failed commits.** When a decision's durable write fails as a whole (stale fence,
    storage error), the host must discard that result, its checkpoint included, and resume
    from the last saved checkpoint with `Recovery` (as the note's crash table says). Running
    the next decision on the failed result's checkpoint would report its never-admitted
    places as vanished (`Accepted`, final); their tombstones would then stay.
26. **Order view size.** The Sleeve's order view holds 256 orders (`MAX_BROKER_ORDERS`). A
    Sleeve holds at most `max_open_orders(mode)` open orders (open context orders plus the
    decision's places), derived from the row budget so a cancel-all over all of them fits:
    192 in paper, 168 in live (`(512 - 5 - 3) / 3`, since a live cancel-all costs 3 rows
    per order). That leaves at least 64 slots for terminal orders not yet acknowledged. A
    place past the cap is a local error and validation rejects a result past it. If the
    host still has to truncate, it sets `orders_complete` false.

## Provisional view and local errors

27. **Cancels keep the reservation.** A cancel marks its target `cancellation_requested`
    but the overlay keeps its reservation until the Broker releases it (the conservative
    estimate).
28. **Market buys** reserve at their own price cap, else one dollar (point 47; the
    per-Strategy `market_buy_price_cap_micros` hook is gone).
29. **Local errors** (not Broker refusals): a Market outside scope; a bad quantity or
    price; a Market without valid fee terms (no reservation can be computed); a client order
    id that is invalid, over 128 bytes (the Broker's bound), starts with `tv3` (reserved for
    derived ids) or is already used by an order in the context, the runner section or the
    decision; an open order past the mode's cap; a cancel naming no order the context or the
    decision knows (V5 failed the whole transaction); a cancel-all over a truncated view; the
    65th command; the row limit; live entry 257; the result size budget. Kernel client ids
    must be unique for the account's lifetime: the runner cannot see acknowledged orders that
    left the view, and the Broker refuses a reused id. Order terms that do not make one
    order are local errors too (point 46); a live Market sell is not: it goes to the Broker
    (point 49).
30. **Cancel targets.** `CancelTarget::ClientOrderId` naming an order the context already
    reports is sent as an `Order` target with that order's revision; only an order placed
    in the same decision goes on the wire by client id.
31. **Row counts.** A cancel of an order the context shows as final counts 1 row (the
    Broker refuses it). A cancel-all counts every order open when it is issued: every open
    context order and every place issued before it, including ones already marked by a
    cancel (an upper bound); a later cancel-all counts only places issued after the earlier
    one. In live the Broker expands a cancel-all into per-order cancels on the priority
    lane, so a live cancel-all counts 3 per open context order it cancels plus 1 per own
    place it collapses (at least 1, for its receipt); paper keeps 3 plus the open orders.
    Acknowledgements add 1 row whenever there are any: traderv3 removes acknowledged
    receipts and orders with one statement in dedicated tables, never per-order plan rows.
    `decision_plan_rows_v6` is public for traderv3's parity test (runner count >= owner
    count).

## Kernel API

32. `BrokerOrderStatus` (for `order_status`) replaces the V5 `OrderStatus`; `as_str` keeps
    the V5 pending-order texts (`pending` for resting, `partial`) and adds `submitted`.
33. `cancel_order` and `cancel_all_orders` default to an error (they returned `Ok(false)` /
    `Ok(0)`); every host implements them.
34. Emitting `PlaceOrder`, `CancelOrder`, `CancelAllOrders` or `WakeAt` is the matching call
    with its ticket or handle discarded (so emitted timers follow the per-key rule and the
    grant).
35. The factory's `gate_telemetry_code` is gone: logs and telemetry never take a command
    ordinal, so nothing needs exempting.
36. Extreme events from a non-primary station use that station's climate date.

## External requests (Phase 4)

Branch `phase4/external-requests`, from the design in
`traderv3/docs/plans/2026-09-24-strategy-core-parity-and-legacy-removal.md` ("External
requests (HTTP and CLI) design").

37. **Kind-qualified grants.** The design lists allowlist names only. `CapabilityGrantV6`
    cannot gain a field without breaking stored contexts, so each grant carries its kind:
    `http:<endpoint>` or `command:<name>`, a name being an identifier without `:`. The runner
    and validation then know that `request_http("pi-forecast")` names a command, not an
    endpoint, and refuse it locally.
38. **Request ids are command ids.** The design names ids `request.<delivery_id>.<ordinal>`.
    That form has the collision point 7 fixed for command ids (Sleeves share delivery ids)
    and can pass the 160-byte identifier bound. A request's id is its command id
    (`command.` + 32 hex digits of the IntentId), one namespace with orders.
39. **Wire shape.** `ExternalRequest { command_id, kind, target, payload, timeout_ms }` as
    designed, with `kind` carrying what differs: `Http { method, path }` or `Command { args }`;
    `payload` is the HTTP body or standard input. Methods are `Get` and `Post` only. The
    new command and trigger variants are appended, so every V6 context and result encoded
    before them decodes unchanged and the magics stay `SDCTXV6A`/`SDRESV6A`. An older
    executable cannot decode an `ExternalResponse` trigger; the host sends one only to an
    executable that stated `external-requests`.
40. **The 64 KiB request bound** covers the HTTP path or the command arguments plus the
    payload (the design says "request payload"). Paths are relative to the endpoint's base
    URL and cannot climb out of it: one leading `/`, no `.`/`..` segment nor `%2e` in the route, no `\`, no `#`.
41. **Outstanding bound.** The context does not list outstanding requests (a new context
    field would break stored contexts), so the runner bounds requests per decision at 8
    and the host enforces 8 outstanding per Sleeve: a decision that would leave the Sleeve
    more than 8 stored, unacknowledged requests fails whole (nothing is stored or answered)
    and the Sleeve resumes through Recovery. traderv3 changed this from answering the excess
    `Refused`, which bounded nothing: each answer could issue 8 more requests.
42. **Errors are a closed kind plus a message.** `Status(code)` names a non-2xx status and
    `Exit(code)` a failed command, so a kernel can retry on 429/503 without parsing text.
    An `Ok` answer is always 2xx (or exit status 0).
43. **Acknowledging the answer.** A completed result whose trigger is an external response
    lists the request id in `acknowledged_command_ids` (the bound grew by one). The host
    keeps a request until that acknowledgement is durable, so a request whose answer a crash
    lost from the Strategy's saved state is answered `Abandoned` after the restart.
44. **No plan rows.** Requests are not account plan rows (the host keeps them in their own
    table, written in the decision's transaction), so `decision_plan_rows_v6` does not count
    them. `StrategyCommandV6::leaves_host` tells the host that a decision with a request
    needs the durable write even without a Broker command.

## Order intent (Phase 5)

Branch `phase5/order-intent`, from decision 5 and the "Order intent gap" of
`traderv3/docs/plans/2026-09-24-strategy-core-parity-and-legacy-removal.md`; the per-repo
steps are in [plans/2026-09-27-phase5-order-intent.md](plans/2026-09-27-phase5-order-intent.md).

45. **Extended in place.** V6 is deployed nowhere and the paper cutover starts from an empty
    store, so `PlaceOrderV6` gains `time_policy` and `post_only` without a new version: the
    magics stay `SDCTXV6A`/`SDRESV6A`, and results encoded before this change do not decode.
    The corpus and its digest changed with it.
46. **Terms that make no sense are local errors.** `place_order_terms_error` gives the reason
    for the runner and validation alike: `post_only` needs a `GoodTillCanceled` Limit order
    (a Market or immediate order takes liquidity), an expiry needs `GoodTillCanceled` (an
    `ImmediateOrCancel` or `FillOrKill` order leaves nothing resting to expire; Kalshi's
    `expiration_time` goes with `good_till_canceled`), a Market sell is immediate (it has no
    price to rest at), and a cap belongs to a Market buy. The tick grid (whole cents for
    Strategy orders in traderv3's signer) depends on the Market and stays the Broker's check.
47. **The Market buy cap is per order.** `PlaceOrderRequest::market_price_cap` fills the
    existing `market_price_cap_micros` and replaces the per-Strategy hook
    `TransactionKernel::market_buy_price_cap_micros` and the cap closure of
    `run_native_decision`. Its only Strategy user, `dsm_reaction`, set it to its `max_pay`
    for every Market buy; it now sets it on the request. The runner no longer restores a
    second kernel to ask it. A Market buy without a cap is still reserved at one dollar.
48. **Time policy does not change the reservation.** An immediate order reserves what a
    resting one does (the Broker commits it at admission and releases the unfilled rest when
    it cancels it), so the provisional view equals the Broker's commitment for every term.
    A `GoodTillCanceled` Market buy rests at its cap, as traderv3 dispatches Market buys
    today.
49. **Live Market sells.** Phase 5 gives live Strategy sells a dispatch path, so the runner
    no longer counts a refusal receipt row for a live Market sell (5 rows, like any place)
    and `capabilities().market_sell` is true in live; `MARKET_SELL_UNSUPPORTED_CODE` is gone.
    Until traderv3 lifts `market_sell_unsupported`, the Broker still refuses it per command
    and the kernel sees `Refused`, as before.
50. **Execution styles are constructors.** `resting_limit`, `direct` and `sweep` build a GTC
    Limit, an IOC Limit and an IOC capped Market buy; nothing about them reaches the wire.
    `max_cost` is not ported.

## Oracle error distributions (Phase 10a)

From the Phase 10a section of
`traderv3/docs/plans/2026-09-24-strategy-core-parity-and-legacy-removal.md` and MinuteTemp's
`OracleErrorDistribution` (`go/api/docs/specs/openapi.yaml`).

48. **Extended in place.** As in point 45, `OracleRowV4` and `SuppliedOracleScoreV6` gain
    `error_distribution` without a new version: the magics stay `SDCTXV6A`/`SDRESV6A`, and
    contexts encoded before this change do not decode. The corpus and its digest changed
    with it. The same change deleted the V5 checkpoint converter (point 15) and its vectors
    `converted-v5-checkpoint` and `converted-checkpoint-adopts-open-order`; seeding from a
    first decision is covered by the kernel projection suite.
49. **Fixed arrays.** Edges and counters are `[_; 22]` and `[_; 23]`, so their lengths are
    part of the type: the host rejects a provider array of another length while parsing. Canonical edges are millionths of a
    degree like the MAE fields; supplied edges are `DecimalV6`; the kernel view has `f64`.
    Counters and `sample_count` are `u64` (the provider's are non-negative int64);
    `day_count` is `u16` on the canonical row and `i64` beside the supplied row's `i64`.
50. **Invalid is the host's to drop.** A malformed distribution makes the context invalid.
    The host checks each with `OracleErrorDistributionV6::validate` or
    `supplied_v6::validate_error_distribution` and drops one that fails to `None` with a
    recorded reason, keeping the row's scores. Supplied and canonical rows are not compared
    value by value, as for the other oracle fields.
