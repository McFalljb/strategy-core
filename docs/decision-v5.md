# Decision transaction V5

Decision V5 is the bounded shared contract for stateful Trader V3 Strategies. A V5 context embeds the V4 owner projection and adds exact Strategy scope, side-aware Broker state, one typed trigger, an authoritative wall clock, supplied originals, and—only for a Broker outcome delivery—the durable continuation commitment created from the prior result. V4 wire layouts remain frozen even though active Rust weather fields no longer expose WU.

**2026-09-12 remediation checkpoint: Trader/Core R1 boundary implemented and locally reviewed; full acceptance blocked, not release-ready.** Required canonical access, all delivered contributors and WU/private-codec behavior have contract proof. Real application readiness attempts remain failing; Backtester and broader consumer migration are deferred. See `/tmp/trader-contract-fix.OEJmyi/evidence/r1-trader-core/current-status.md`. No publication or production change is implied.

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

### Broker quantity codec compatibility

Current contexts are identified by `SDCTXV5C` and current results by `SDRESV5H`; `SDCTXV5H` is the retained pre-supplied hundredths context. Broker positions, Broker orders, `PlaceOrderV5`, `BrokerOutcomeV5`, and `KernelOrderResultV5` encode every authoritative quantity explicitly in hundredths of one contract. Public wire fields use `quantity_hundredths`, `requested_quantity_hundredths`, `filled_quantity_hundredths`, and `remaining_quantity_hundredths`; scale is never inferred from value shape.

`decode_decision_context_v5` and `decode_decision_result_v5` also accept durable legacy `SDCTXV5\0` and `SDRESV5\0` payloads. Only those legacy magics interpret their old quantity fields as whole contracts. Decoding checked-multiplies each value by 100 into the explicit `u64` hundredths representation. A legacy value greater than `u64::MAX / 100` fails closed with `InvalidContract`; it is never wrapped, saturated, rounded, or reinterpreted. Current encoders write C contexts and H results; context and result codec versions are independent.

Durable continuation identities remain replayable. Originating-context validation accepts the exact legacy context digest when all reconstructed quantities are whole-contract multiples. `strategy_command_v5_digest_matches` similarly checks the current command encoding first and then the exact legacy whole-contract encoding, allowing a persisted pre-change command digest to be verified without changing it. New command and result digests intentionally bind the explicit-hundredths encoding.

The shared kernel uses the single-authority `ContractQuantity` type for order requests, Broker positions/views, and order returns. `ContractQuantity::from_hundredths` supports exact fractional exits; `checked_from_whole_contracts` preserves whole-contract entry behavior through an explicit checked conversion. Strategies and Trader adapters must update their kernel implementations and call sites to construct/read this type before repinning this Strategy Core revision.

## Supplied inputs and the current context encoding

Since the S encoding, a V5 context carries `supplied: SuppliedInputsV5`: the provider's retained fields at supplied precision plus the exact typed originating event. The active types are owned by `strategy_core_kernel::supplied` and re-exported by V3. C carries `supplied-inputs/2`, including independent forecast version/fetch/issuance and apparent Celsius absent from frozen S.

The embedded V4 owner projection remains bounded derived evidence (milli/micro fixed point, millisecond times) used by V4 consumers and V5 identity/fence checks. Supplied originals and the latest accepted host winner are distinct: an older original must not mask newer accepted state. The current numeric-equality freshness heuristic remains an R2 defect, not an approved source-precedence rule.

- Numbers are `DecimalV5 { coefficient, scale }`: Trader captures the original numeric token
  before floating-point conversion, then normalizes equal values to one encoding. The signed
  64-bit coefficient and scale at most 18 are checked bounds, not unlimited precision. No
  milli/micro rounding is performed. Historical producers that already converted to a double
  cannot recover discarded lexical digits.
- Supplied times are `*_unix_ns` nanosecond instants; derived times stay `*_unix_ms`.
- `Option` means absent-or-null on the wire; provider serializers omit nil pointers so the two
  are indistinguishable. Supplied strings may be present and empty.
- `originating_event` is present exactly when the originating owner trigger is an observation,
  station report, daily extreme (`NewHigh`/`NewLow`) or weather episode (`WeatherEvent`), and
  its identity must match the trigger. Ended episodes travel only in the event; current state
  never contains them.
- Each owner station has exactly one supplied station when the block is present. An absent
  block (empty version, no stations, no event) is the documented pre-supplied form and is what
  `SDCTXV5H` and `SDCTXV5\0` bytes decode to.

`decode_decision_context_v5` accepts C, S, H and whole (`SDCTXV5\0`) contexts. Originating-context verification tries C, frozen S when representable, H when no supplied block exists, and the whole-contract shape. The previously missing S-origin BrokerOutcome check now has replay/chaining/tamper proof. Current encoders write C. `OwnerTriggerV5::NewLow` and `OwnerTriggerV5::WeatherEvent` remain appended variants, preserving earlier indices.

### WU exclusion and private historical evidence

Trader ignores WU values and WU-specific timing/day metadata before typed domain retention. Active Core supplied/state/event views omit them. Ordinary observations, normal/ASOS extrema, DSM/CLI reports, weather events, forecasts and oracle scores remain in scope; malformed WU extras cannot invalidate otherwise valid normal observations, and WU cannot substitute for absent normal readings.

Private `wire_v4` and `wire_supplied` codecs preserve excluded historical slots. Opaque codec-owned evidence travels with decoded contexts through cloning, outcome construction, persistence and reopening, separately from active kernel models. Encoding combines current ordinary facts with that evidence; it does not return a cached old BLOB or hash that could mask a changed fact. Fresh contexts use empty historical evidence. Standalone V4 retains fixed-integer encoding; V4 embedded in V5 retains the enclosing variable-integer encoding.

When reconstructing a persisted continuation, use `decode_continuation_v5(context_bytes, result_bytes)` to preserve the original context and command encoding identities independently. Chained commands retain the validated original event-context digest. Never rewrite old financial bytes or rebuild an old commitment from a WU-free projection alone.

## Kernel projection and transaction runner

With the `kernel` feature, `strategy_core_v3::kernel_v5` is the single definition of how a
context is presented to a `strategy_core_kernel::NativeKernel` and how its synchronous Broker
calls are carried through the transaction. `KernelEvent` projects the originating event
(supplied first, derived fallback) into the existing view types; `KernelSnapshot` serves the
state views from its owned `StationState`/`MarketState` projections for every delivered contributor and market; the old `canonical_context`/`Any` hook is gone. `StrategyKernelState::station/market` are required and return `Option<&StationState>` / `Option<&MarketState>`. Absence means missing or out of scope. Convenience accessors are inherent methods on `dyn StrategyKernelState`, derived from those same models rather than independently overridable host methods. Hosts must retain the models for the invocation and perform no provider read in these getters. Existing legacy/Backtester hosts still require migration; the mandatory API is not a compatibility claim for them.
`KernelHost` defers the first economic call into `AwaitingBrokerOutcome` and replays the exact
return; `run_transaction` assembles the fenced result and checkpoint. Strategy executables
supply only kernel construction, restore and checkpoint codecs through
`TransactionKernelFactory`. Event `emitted_at` is the provider's publication time, never the
decision clock. `PriceLevelView` still carries whole-contract floors beside `exact` hundredths; actual planner use of those floors remains an R3 defect.

The stable measurements for the V5 corpus are in `conformance/v5/decision-transactions.json`.
Corpus schema 5 retains the historical whole/H/S measurements through their frozen encoders and adds C measurements. Targeted codec/conformance passes do not attest a published immutable revision or replace final cross-consumer qualification.

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

Lifecycle status and return value are both validated. Place returns preserve order identity, filled quantity in hundredths, fill price, fee cost, reason, and the legacy order-status vocabulary. A zero-fill return has zero fill price and fee; a positive fill uses the exact canonical outcome price and fee cannot exceed `filled_quantity_hundredths * 1_000_000 / 100`.

## Identity and authority

The Strategy and Binding IDs are bound to the authoritative Sleeve ID by reproducing Trader V3's domain-separated Sleeve derivation over `(Strategy, Binding, Venue, Opportunity)`. Profile, attestation digest, station, event ticker/date, and complete Market membership are checked against the embedded V4 projection.

Typed owner triggers are checked against the exact V4 component revision and source cursor. Forecast, oracle, Market price, extrema, and timer timestamps are tied to owner metadata or durable timer recovery evidence. Broker-trigger V4 projections use `Recovery` as the neutral recomposition trigger.

## Canonical and bounded data

- Strategy parameters are canonical sorted typed values; the adapter projects them losslessly into the frozen JSON initializer.
- Market IDs, positions, orders, and cancel-all affected IDs use strict canonical ordering.
- Position cost basis excludes fees; average entry price is exactly `cost_basis * 100 / quantity_hundredths`.
- YES and NO positions have distinct `(Market, side)` identities.
- A Market buy may carry `market_price_cap_micros` as its authoritative maximum per-contract execution price. A present cap is positive and at most `1_000_000`; Market orders never carry a limit price. Limit orders carry a limit price and never a Market price cap. Market sells never carry a buy-side price cap.
- The Market buy cap participates in canonical command encoding and the durable command commitment. A host must not execute any fill above a present cap; absence of the optional cap preserves the pre-extension Market-order contract.
- Sell and terminal orders reserve no cash. Active limit-buy principal is exactly `remaining_quantity_hundredths * limit_price_micros / 100`; non-exact products are rejected rather than rounded. Fee reserve is separate and bounded by remaining notional.
- Sleeve order reservations sum exactly to the V5 reserved-cash total, do not exceed the V4 account reservation, and combine with position cost and paid fees to equal the V4 Sleeve commitment.
- Broker-state, command, outcome, and return hundredths values fit the frozen kernel's signed 64-bit `ContractQuantity` interface. Whole-contract entry policies convert once with `checked_from_whole_contracts`; fractional reduce-only exits use `from_hundredths`.
- All identifiers, text, metadata, diagnostics, evidence, collections, and private kernel checkpoints have explicit bounds.

Corpus schema 3 retained exact legacy whole-contract context and result measurements, added explicit-hundredths command/outcome measurements, and kept the unchanged V4 owner-projection measurement. Legacy bytes and digests are read and verified, never rewritten.
