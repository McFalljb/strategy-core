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
2. **Order fees.** `OrderUpdate.fee_cost` needs the fees charged on each order, which V5's
   Broker order did not carry. `BrokerOrderV6` adds `fees_micros`.
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
7. **Command ordinals.** Every command, timer and stop commands included, takes the next
   ordinal: the command at index `n` is `command.<delivery_id>.<n>`, and a derived client
   order id uses the same `n` for its IntentId. (V5 numbered timers from 100.)
8. **Receipts.** `CommandReceiptV6 { command_id, kind, outcome: Accepted | Refused { code,
   reason } }`, strictly sorted by command id. A place receipt must be a refusal (an admitted
   place has an order record), and no command has both. A place cancelled in the same
   decision is expected as an order record with status `Cancelled` plus an `Accepted` cancel
   receipt, not as a place receipt.
9. **Acknowledgements.** `acknowledged_command_ids` names receipts and terminal orders (the
   note's order view keeps unacknowledged terminal orders, so orders need acknowledging
   too). Every result lists every final outcome in its context that the runner no longer
   tracks, because only durable writes apply acknowledgements and an in-memory result's
   list would otherwise be lost. A rejected result acknowledges nothing.
10. **Result size.** V5's 256 KiB result bound cannot hold a 128 KiB state, the runner
    section, 64 commands and the update evidence, so the V6 result bound is 1 MiB. Telemetry
    keeps at most 256 entries and 256 KiB; the rest is counted as overflow.
11. **Runner section size.** An entry holds up to four bounded identifiers (160 bytes each),
    so the worst case is about 700 bytes, not the note's ~350; 256 entries are about 180 KB.
    `MAX_ENCODED_KERNEL_CHECKPOINT_BYTES` includes it; traderv3's checkpoint row bound must
    grow to match.
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
    keeps the kernel bytes and sequence, and starts an empty runner section. It goes when
    the cutover is done.

## Order updates

16. **Shape.** `OrderUpdate` adds `command_kind` and `is_final` to the note's fields.
    `action` and `contract_side` are `Option` because a cancel-all refusal concerns no
    order (its `client_order_id` and `ticker` are empty). A cancel's refusal is an update
    with `command_kind: CancelOrder` whose order fields describe the target; kernels must
    check `command_kind` before treating a `Refused` update as their order's fate. An
    admitted cancel has no update of its own: the target's `Cancelled` update reports it.
17. **Status mapping.** Durably accepted and dispatched are `Accepted`. A cancellation
    request or a recovery hold keeps the last status (`PartiallyFilled` once anything
    filled). A provider-rejected order is `Refused { code: "provider_rejected" }`. An order
    that vanishes before its first update is reported as `Accepted`, final, nothing
    remaining.
18. **Seeding.** Instead of a special "empty section" state, one rule covers conversion and
    a Sleeve's first decision: an open order the section does not track is recorded as seen
    without an update, and receipts or terminal orders it does not track are acknowledged
    silently. A section that would pass 256 entries this way fails the decision before the
    kernel runs.
19. **Order of delivery.** Updates precede `on_start` for Bootstrap and Recovery too. A
    Broker-state trigger delivers `Unknown { event_type: "broker_state" }` after its
    updates, as V5 did. A kernel error in an update handler rejects the decision; nothing
    after it is delivered.
20. **Order view size.** The note bounds the Sleeve's order view at 128 orders and the
    runner section at 256 entries. An open order missing from a truncated view would be
    reported as vanished, so traderv3 must never drop an open order from the view.

## Provisional view and local errors

21. **Cancels keep the reservation.** A cancel marks its target `cancellation_requested`
    but the overlay keeps its reservation until the Broker releases it (the conservative
    estimate).
22. **Market buys** reserve at the kernel's price cap, else one dollar. The cap is asked
    once, when the kernel places the order, of the kernel as restored for the decision (the
    kernel cannot be called while it runs).
23. **Local errors** (not Broker refusals): a Market outside scope; a bad quantity or
    price; a Market without valid fee terms (no reservation can be computed); a client order
    id that is invalid, over 128 bytes (the Broker's bound) or already used by an order in
    the context, the runner section or the decision; a cancel naming no order the context or
    the decision knows (V5 failed the whole transaction); the 65th command; the row limit;
    entry 257.
24. **Cancel targets.** `CancelTarget::ClientOrderId` naming an order the context already
    reports is sent as an `Order` target with that order's revision; only an order placed
    in the same decision goes on the wire by client id.
25. **Row counts.** A cancel of an order the context shows as final counts 1 row (the
    Broker refuses it). A cancel-all counts every open context order and every place of the
    decision, including ones already marked (an upper bound). Acknowledgements add 1 row
    whenever there are any.

## Kernel API

26. `BrokerOrderStatus` (for `order_status`) replaces the V5 `OrderStatus`; `as_str` keeps
    the V5 pending-order texts (`pending` for resting, `partial`) and adds `submitted`.
27. `cancel_order` and `cancel_all_orders` default to an error (they returned `Ok(false)` /
    `Ok(0)`); every host implements them.
28. Emitting `PlaceOrder`, `CancelOrder`, `CancelAllOrders` or `WakeAt` is the matching call
    with its ticket or handle discarded (so emitted timers follow the per-key rule and the
    grant).
29. The factory's `gate_telemetry_code` is gone: logs and telemetry never take a command
    ordinal, so nothing needs exempting.
30. Extreme events from a non-primary station use that station's climate date.
