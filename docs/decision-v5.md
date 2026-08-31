# Decision transaction V5

Decision V5 is the bounded shared contract for stateful Trader V3 Strategies. It does not change Decision Context V4. A V5 context embeds the complete V4 owner projection and adds exact Strategy scope, side-aware Broker state, one typed trigger, an authoritative wall clock, and—only for a Broker outcome delivery—the durable continuation commitment created from the prior result.

## Transaction

```text
V5 owner/Broker snapshot + one typed trigger
  -> one Strategy invocation
  -> non-economic commands, or exactly one fenced Broker command
  -> durable continuation commitment
  -> exact Broker command return
  -> resumed Strategy invocation
```

A result containing a Broker command must use `AwaitingBrokerOutcome`. It may contain exactly one Broker command, and its continuation identity and generation must match that command's fence. Completed results cannot contain Broker commands. This makes deferred replay serial and deterministic.

The host must call `validate_decision_result_v5(context, result)` rather than validating a result in isolation. Context-aware validation binds delivery, Sleeve, V5 state fence, Broker revision, Market scope, and cancellation target. The host then creates and durably stores `continuation_commitment_v5(context, result)` before admitting the command.

A Broker outcome is deliverable only when its commitment is still present and unconsumed in the host ledger for the exact Sleeve identity, incarnation, originating process attempt, route epoch, continuation identity/generation, command identity/digest, expected Broker revision, and canonical originating-context digest. A later runtime process may recover and route the ledger row, but the replay context itself retains the originating process attempt. Delivery consumes the commitment once.

Trader persists the canonical bytes from `encode_decision_context_v5` and `decision_context_v5_sha256` with the commitment before command admission. Outcome delivery decodes those stored bytes, preserves their exact owner, Strategy, Broker, checkpoint, and decision-clock state, replaces only the typed trigger with `BrokerOutcomeV5` plus the bounded original trigger, and attaches the commitment. Validation reconstructs and hashes the originating context before the frozen kernel may replay it. Current or advanced owner/Broker state must never substitute for the stored snapshot. The outcome may carry a later Broker revision because its exact return is durable evidence; it does not replace the original Broker snapshot used by the replayed event.

## Durable kernel checkpoints

`KernelCheckpointV5` is the bounded, versioned private-state boundary for frozen kernels. It binds:

- the checkpoint codec profile and nonzero codec version;
- exact Strategy ID and Strategy profile;
- the attested profile/calculator digest;
- a strictly advancing checkpoint sequence;
- at most 128 KiB of nonempty opaque state;
- a domain-separated SHA-256 over every binding field and the state bytes.

The host never interprets checkpoint state. The attested Strategy artifact owns the codec and rejects unsupported versions.

Every successful transaction carries a checkpoint. For `Completed`, it is post-event state and advances the input sequence exactly once. For `AwaitingBrokerOutcome`, it is the exact pre-event state: an existing input checkpoint must be preserved byte-for-byte, while the first invocation creates sequence 1 before handling the event. For `Rejected`, the checkpoint is unchanged and may be absent only when no state has ever committed.

A continuation commitment contains the complete pre-event checkpoint, not only its digest. Trader persists that commitment and command atomically. On Broker outcome delivery—even after a process restart—the context checkpoint must exactly equal the committed pre-event checkpoint. The Strategy restores it before deterministic replay, then returns the next completed checkpoint. The V5 decision fence includes the input checkpoint digest, preventing a result for one private-state version from being accepted against another.

## Exact frozen-kernel returns

`BrokerCommandReturnV5` represents the existing synchronous kernel capability without inference:

- place order: exact `KernelOrderResultV5` or bounded Broker error;
- cancel order: exact Boolean or bounded Broker error;
- cancel all: the canonical sorted set of affected order IDs or bounded Broker error. The frozen `usize` return is the set length.

Lifecycle status and return value are both validated. Place returns preserve order identity, filled quantity, fill price, fee cost, reason, and the legacy order-status vocabulary. A zero-fill return has zero fill price and fee; a positive fill uses the exact canonical outcome price and fee cannot exceed filled notional.

## Identity and authority

The Strategy and Binding IDs are bound to the authoritative Sleeve ID by reproducing Trader V3's domain-separated Sleeve derivation over `(Strategy, Binding, Venue, Opportunity)`. Profile, attestation digest, station, event ticker/date, and complete Market membership are checked against the embedded V4 projection.

Typed owner triggers are checked against the exact V4 component revision and source cursor. Forecast, oracle, Market price, extrema, and timer timestamps are tied to owner metadata or durable timer recovery evidence. Broker-trigger V4 projections use `Recovery` as the neutral recomposition trigger.

## Canonical and bounded data

- Strategy parameters are canonical sorted typed values; the adapter projects them losslessly into the frozen JSON initializer.
- Market IDs, positions, orders, and cancel-all affected IDs use strict canonical ordering.
- Position cost basis excludes fees; average entry price is exactly `cost_basis / quantity`.
- YES and NO positions have distinct `(Market, side)` identities.
- Sell and terminal orders reserve no cash. Active limit-buy principal is exactly remaining quantity times limit price; fee reserve is separate and bounded by remaining notional.
- Sleeve order reservations sum exactly to the V5 reserved-cash total, do not exceed the V4 account reservation, and combine with position cost and paid fees to equal the V4 Sleeve commitment.
- Quantities fit the frozen kernel's signed 64-bit interface.
- All identifiers, text, metadata, diagnostics, evidence, collections, and private kernel checkpoints have explicit bounds.

The stable measurements for the initial V5 corpus are in `conformance/v5/decision-transactions.json`. The corpus includes an unchanged V4 owner-projection measurement to guard backward compatibility.
