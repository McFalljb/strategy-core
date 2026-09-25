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
   `BrokerOrderV6` also adds `rejection_reason` (non-empty, at most 512 bytes, only on a
   `Rejected` order), which the host fills from the provider's rejection message; the runner
   reports it as the `provider_rejected` refusal's reason, with a fixed text when absent. The
   harness's V5 conversion leaves it absent.
3. **Capabilities.** `CapabilityGrantV6 { timers, external_requests }`; the request names
   are strictly sorted identifiers, at most 32, empty until Phase 4. Without `timers`,
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
   the context still shows them: every receipt the runner section no longer tracks (an entry
   is pruned only after its outcome was reported), and every terminal order the section
   recorded as reported (`RunnerSectionV6::reported`, kept until a complete view no longer
   shows the order). Other terminal orders are acknowledged silently only by the seeding
   decision. A result never acknowledges a command its section still tracks, and a rejected
   result acknowledges nothing.
10. **Result size.** V5's 256 KiB result bound cannot hold a 128 KiB state, the runner
    section, 64 commands and the update evidence, so the V6 result bound is 1 MiB. Decoding
    charges every length prefix and integer against that bound before allocating, so the
    runner keeps results within 896 KiB (`RESULT_ENCODED_BUDGET_BYTES`). Commands are
    admitted against the room the checkpoint (kernel state counted at 128 KiB), runner
    section, update evidence and acknowledgements leave; a command past it is a local error.
    Logs and telemetry fill what is left (at most 256 telemetry entries and 256 KiB) and the
    rest is counted as overflow. A refusal reason in the evidence copy is cut to 512 bytes.
11. **Runner section size.** An entry holds up to four bounded identifiers (160 bytes each),
    so the worst case is about 730 bytes, not the note's ~350; 256 entries are about 190 KB,
    plus up to 256 reported command ids (about 42 KB). `MAX_ENCODED_KERNEL_CHECKPOINT_BYTES`
    includes both; traderv3's checkpoint row bound must grow to match.
12. **State fence.** `decision_fence_v6_sha256` is domain-separated, length-prefixes its
    identifiers, and also binds the deployment mode, capabilities and receipts.
13. **Frozen encodings.** V6 has one encoding. The V5 `retained_supplied_encoding` evidence
    is gone. The V4 owner projection's positional observation/weather codec (with its
    retained WU slots) is unchanged; it moved from `wire_v4.rs` to
    `decision_v4/observation_codec.rs`.
14. **Handshake.** strategy-core defines no handshake; `HANDSHAKE_CAPABILITIES_V6`
    (`decision-v6`, `kernel-checkpoint`, `liveness`) is exported for the executable and host
    to share.
15. **V5 checkpoint conversion** is the only V5-named code left:
    `KernelCheckpointV5Layout` (the V5 field order, for the host to decode its stored bytes
    with its own codec) and `convert_v5_kernel_checkpoint`, which checks the V5 digest,
    keeps the kernel bytes and sequence, and starts an unseeded runner section. It goes when
    the cutover is done.
16. **No V5 decoding.** strategy-core has no V5 context or result decoder. The equivalence
    harness reads recorded V5 contexts and results by depending on strategy-core revision
    c2f654b under a renamed dependency. Its V5 → V6 context conversion fills
    `BrokerOrderV6.fees_micros` from the V5 fill data (0 where absent), leaves
    `command_receipts` empty, sets `orders_complete` true, and converts the checkpoint with
    `convert_v5_kernel_checkpoint`.
17. **Truncated views.** The context carries `orders_complete`; the host sets it false when
    it had to truncate the Sleeve's order view. A truncated view reports no order as
    vanished and allows no cancel-all (a local error; validation rejects one).
18. **Early-stopped terminal orders.** `Expired` and `Rejected` orders, like `Cancelled`
    ones, may report no remaining quantity with less than their whole quantity filled.
    `validate_broker_order_v6` checks one order record on its own so the host can quarantine
    a record that would make every context of the Sleeve invalid.
19. **Decoding limits.** Contexts and results decode under bincode's limit set to their
    bound (20 MiB, 1 MiB); encoding checks the bytes decode within it.

## Order updates

20. **Shape.** `OrderUpdate` adds `command_kind` and `is_final` to the note's fields.
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
    from the tombstone. A tombstone stays for the Sleeve's life (it counts toward the 256
    entries). The revision check cannot see every stale view, because the Broker revision
    is account-wide and moves with other Sleeves; the tombstone makes a false vanish
    recoverable. Orders are matched by command id only.
23. **Seeding.** The runner section records whether it is seeded. The seeding decision (a
    Sleeve's first, or the first after converting a V5 checkpoint) records open orders as
    seen without updates and acknowledges terminal orders silently. After it, an untracked
    open or terminal order in the view is ignored (a host anomaly: it is neither adopted nor
    acknowledged). A section that would pass 256 entries fails the decision before the
    kernel runs.
24. **Order of delivery.** Updates precede `on_start` for Bootstrap and Recovery too. A
    Broker-state trigger delivers `Unknown { event_type: "broker_state" }` after its
    updates, as V5 did. A kernel error on one update restores the kernel (a clone taken
    before the update) and the decision's commands to how they were before it, records a
    `kernel_error` diagnostic naming the update, counts the update as seen and goes on; the
    decision completes. A kernel error on the trigger rejects the decision.
25. **Failed commits.** When a decision's durable write fails as a whole (stale fence,
    storage error), the host must discard that result, its checkpoint included, and resume
    from the last saved checkpoint with `Recovery` (as the note's crash table says). Running
    the next decision on the failed result's checkpoint would report its never-admitted
    places as vanished (`Accepted`, final); their tombstones would then stay.
26. **Order view size.** The Sleeve's order view holds 256 orders (`MAX_BROKER_ORDERS`); a
    Sleeve holds at most 192 open orders (`MAX_OPEN_ORDERS`: open context orders plus the
    decision's places), leaving 64 slots for terminal orders not yet acknowledged. A place
    past 192 is a local error and validation rejects a result past it. If the host still
    has to truncate, it sets `orders_complete` false.

## Provisional view and local errors

27. **Cancels keep the reservation.** A cancel marks its target `cancellation_requested`
    but the overlay keeps its reservation until the Broker releases it (the conservative
    estimate).
28. **Market buys** reserve at the kernel's price cap, else one dollar. The cap is asked
    once, when the kernel places the order, of the kernel as restored for the decision (the
    kernel cannot be called while it runs).
29. **Local errors** (not Broker refusals): a Market outside scope; a bad quantity or
    price; a Market without valid fee terms (no reservation can be computed); a client order
    id that is invalid, over 128 bytes (the Broker's bound), starts with `tv3` (reserved for
    derived ids) or is already used by an order in the context, the runner section or the
    decision; the 193rd open order; a cancel naming no order the context or the decision
    knows (V5 failed the whole transaction); a cancel-all over a truncated view; the 65th
    command; the row limit; entry 257; the result size budget. Kernel client ids must be
    unique for the account's lifetime: the runner cannot see acknowledged orders that left
    the view, and the Broker refuses a reused id.
30. **Cancel targets.** `CancelTarget::ClientOrderId` naming an order the context already
    reports is sent as an `Order` target with that order's revision; only an order placed
    in the same decision goes on the wire by client id.
31. **Row counts.** A cancel of an order the context shows as final counts 1 row (the
    Broker refuses it). A cancel-all counts every order open when it is issued: every open
    context order and every place issued before it, including ones already marked by a
    cancel (an upper bound); a later cancel-all counts only places issued after the earlier
    one. Acknowledgements add 1 row whenever there are any: traderv3 removes acknowledged
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
