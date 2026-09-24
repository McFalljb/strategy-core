# Decision transaction V5

Decision V5 is the bounded shared contract for stateful Trader V3 Strategies. A V5 context embeds the V4 owner projection and adds exact Strategy scope, side-aware Broker state, one typed trigger, an authoritative wall clock, supplied originals, and—only for a Broker outcome delivery—the durable continuation commitment created from the prior result. V4 wire layouts remain frozen even though active Rust weather fields no longer expose WU.

**Local acceptance: scoped Trader/Core R1 and R2 pass their owning/application gates and bounded review disposition.** The owner approved a clean current packet with historical readers. D now carries explicit weather winners, accepted forecast issuance/current advertisements, host-query oracle records and captured queued weather inputs. Historical corpus hashes remain unchanged. This is not immutable release or production qualification.

**R3 candidate:** E adds bounded verified multi-call Broker replay. Codec and mixed place/cancel/refusal transaction contracts pass locally, but configured-consumer migration, full application/restart qualification and final review remain open. R2 evidence does not qualify this newer candidate.

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

Trader persists the canonical input/result bytes and continuation commitment before command admission. `broker_outcome_context_v5` preserves the original owner, Strategy, Broker, pre-event checkpoint and decision clock, then attaches the typed outcome and `BrokerReplayV5`. Each completed call carries its request digest, pre-call revision, exact return and coherent returned Broker detail/financial state. The current return remains in the trigger; only predecessors are repeated. The retained origin encoding explicitly binds F, E, D, C, S, H or whole bytes. Source inputs and the original Broker snapshot never change; only the kernel's Broker capability advances after consuming each verified return.

Replay admits at most 64 synchronous calls (63 predecessors), within the 20-MiB context bound and outside the 128-KiB private checkpoint budget. Market-buy cap verification recreates the corresponding suspension point from the pre-event kernel and exact earlier returns: a later private planning state cannot supply an earlier cap. This adds at most one bounded prefix pass per replayed Market buy; Limit and cancellation calls need none. Missing earlier returned states fail closed; balances are never reconstructed from fill averages. The first suspended call wins, and speculative actions after suspension are discarded. Every replayed request must match its command identity/digest and fence before a result is exposed. New commands use the last returned Broker revision via `admission_broker()`, not the original revision.

Trader verifies every extension against its exact retained invocation and earlier returns. Committing a next call atomically consumes its exact preceding outcome and persists the unchanged pre-event checkpoint; final completion consumes the last outcome with the post-event checkpoint. Recovery retains historical financial bytes and cannot discard unresolved obligations.

### Broker quantity codec compatibility

New candidate contexts are identified by `SDCTXV5F` and current results by `SDRESV5H`; `SDCTXV5E`, `SDCTXV5D` and `SDCTXV5C` remain frozen historical context formats; `SDCTXV5H` is the retained pre-supplied hundredths context. Broker positions, Broker orders, `PlaceOrderV5`, `BrokerOutcomeV5`, and `KernelOrderResultV5` encode every authoritative quantity explicitly in hundredths of one contract. Public wire fields use `quantity_hundredths`, `requested_quantity_hundredths`, `filled_quantity_hundredths`, and `remaining_quantity_hundredths`; scale is never inferred from value shape.

`decode_decision_context_v5` and `decode_decision_result_v5` also accept durable legacy `SDCTXV5\0` and `SDRESV5\0` payloads. Only those legacy magics interpret their old quantity fields as whole contracts. Decoding checked-multiplies each value by 100 into the explicit `u64` hundredths representation. A legacy value greater than `u64::MAX / 100` fails closed with `InvalidContract`; it is never wrapped, saturated, rounded, or reinterpreted. New contexts encode as F; decoded E, D and C contexts retain their private encoding selectors and re-encode through their frozen layouts. Results remain H. Context and result codec versions are independent.

Durable continuation identities remain replayable. Originating-context validation accepts the exact legacy context digest when all reconstructed quantities are whole-contract multiples. `strategy_command_v5_digest_matches` similarly checks the current command encoding first and then the exact legacy whole-contract encoding, allowing a persisted pre-change command digest to be verified without changing it. New command and result digests intentionally bind the explicit-hundredths encoding.

The shared kernel uses the single-authority `ContractQuantity` type for order requests, Broker positions/views, and order returns. `ContractQuantity::from_hundredths` supports exact fractional exits; `checked_from_whole_contracts` preserves whole-contract entry behavior through an explicit checked conversion. Strategies and Trader adapters must update their kernel implementations and call sites to construct/read this type before repinning this Strategy Core revision.

### Exact native Market strikes

F adds `market_strikes`, an optional complete collection in owner-Market order. Each entry
binds its Market ID and optional `cap_strike_milli_f`. The existing V4 identity already carries
an exact Fahrenheit floor; its frozen layout is unchanged. Kalshi temperature strikes remain
native milli-Fahrenheit, never raw Fahrenheit labelled as Celsius or rounded through milli-C.
A present native cap cannot coexist with a Celsius cap, and a native floor cannot exceed it.
Missing, extra or out-of-order entries fail validation. The block participates in the decision
fence and originating-context digest. Kernel snapshot and price-event conveniences prefer
native Fahrenheit; historical contexts retain the Celsius fallback.

E's layout is frozen in `wire_e`. Historical encodings cannot attest or silently discard native
caps. New facts require F; existing continuation bytes and commitments must not be rewritten.
The Strategy-owned bracket cache must compare current geometry, not only ticker membership,
before reusing private checkpoint rows. This does not reset bought/pending state or relax gates.

## Supplied inputs and the current context encoding

Since the S encoding, a V5 context carries `supplied: SuppliedInputsV5`: the provider's retained fields at supplied precision plus the exact typed originating event. The active types are owned by `strategy_core_kernel::supplied` and re-exported by V3. Frozen C carries `supplied-inputs/2`, including independent forecast version/fetch/issuance and apparent Celsius absent from frozen S. D introduced `supplied-inputs/3` with private historical evidence separately; E preserves those facts and adds bounded Broker replay. Frozen `wire_d` preserves D's positional layout rather than appending a field under its old magic. The active oracle original retains the table's own nanosecond update time and optional explicit row rank. Table update, notification update and host receipt remain distinct; positional rank is not fabricated as a supplied original. Historical decoding lifts the older supplied shapes into the current model with unavailable fields absent, while historical encoding restores their original version/layout.

The embedded V4 owner projection remains bounded derived evidence (milli/micro fixed point, millisecond times) used by V4 consumers and V5 identity/fence checks. Supplied originals and the latest accepted host winner are distinct: an older original must not mask newer accepted state. Production D weather conveniences derive from the host's bounded per-field winners, with independent native C/F and accepted station revision. Numeric-equality arbitration remains only in the explicitly named legacy projection for contexts without captured winners. Current D owner projections carrying winners admit 256 in-window forecast points; standalone V4 and contexts without those winners retain the 100-point limit. No window or query expansion is implied.

Production D also carries one `forecast_issuance` set per delivered station. Each accepted model/version retains its exact issuance instant, original time text when available, supplied/version-derived/normalized-derived basis, source and receipt, accepted station generation/revision and independent forecast replacement generation. Source selects this winner at admission under the existing millisecond age policy; missing or later same-version issuance cannot renew it, and ties retain the first accepted evidence. The 72-hour rule and coverage checks are unchanged. Core's forecast model convenience uses this winner while `supplied` preserves the latest refresh's original issuance, including absence. Current advertised versions come from the owner snapshot, not the older fetched bundle. Historical/controlled contexts without this evidence retain their compatibility projection. Station revisions are scoped by their cursor/baseline generation; forecast generations are never interpreted as transport sequences.

- Numbers are `DecimalV5 { coefficient, scale }`: Trader captures the original numeric token
  before floating-point conversion, then normalizes equal values to one encoding. The signed
  64-bit coefficient and scale at most 18 are checked bounds, not unlimited precision. No
  milli/micro rounding is performed. MinuteTemp oracle score analytics are the sole adapter
  exception: an already binary-floating provider token that exceeds the scale profile is rounded
  once to 18 fractional decimal places before entering `SuppliedOracleScoreV5`; in-profile tokens
  remain exact. Historical producers that already converted to a double cannot recover discarded
  lexical digits.
- Supplied times are `*_unix_ns` nanosecond instants; derived times stay `*_unix_ms`.
- `Option` means absent-or-null on the wire; provider serializers omit nil pointers so the two
  are indistinguishable. Supplied strings may be present and empty.
- Historical/controlled weather triggers use `supplied.originating_event`. Current captured
  triggers instead use `current_inputs.originating.supplied`; no duplicate event authority is
  allowed. The optional original must match the captured family, identity and provider cursor.
  Ended episodes travel only in the event; current state never contains them.
- Each owner station has exactly one supplied station when the block is present. An absent
  block (empty version, no stations, no event) is the documented pre-supplied form and is what
  `SDCTXV5H` and `SDCTXV5\0` bytes decode to.

`decode_decision_context_v5` accepts F, E, D, C, S, H and whole (`SDCTXV5\0`) contexts. Originating-context verification tries the context's current encoding first, then compatible historical shapes only when no newly attached fields would be omitted. Historical digests cannot attest new weather winners, forecast issuance sets, current-input records or new oracle fields; incompatible S/H/whole conversions and C encoding fail rather than discard them. The previously missing S-origin BrokerOutcome check has replay/chaining/tamper proof; the new layout also passes the existing durable ledger fixtures. New contexts encode as F; decoded E/D/C contexts re-encode as E/D/C. Replay-bearing contexts cannot downgrade to D or C. `OwnerTriggerV5::NewLow`, `WeatherEvent` and `CapturedWeather` are appended variants, preserving earlier indices.

### Current query and event ownership

`current_inputs.stations` follows the delivered station order. Each station carries at most two
occupied oracle components, ordered high then low. Each record couples the actual host query,
its normalized table, its own authority/revision/generation/provenance, and an optional original.
Noncurrent retained values keep their noncurrent metadata. Optional response labels do not
supply the host query. The selected V4 table must agree with its current record; unlinked oracle
originals are not duplicated in the older supplied-station list. Core table conveniences use the
host query, while original absent labels stay absent. MinuteTemp's pinned WebSocket contract
always supplies high-ranked inline tables; profile-required low tables come from the existing
REST query/refresh path. A profile does not relabel the inline table's query. Current oracle
events select their host query and never substitute notification time for unknown table freshness. Historical event
projection remains a separately identified compatibility path.

Source captures one immutable `AcceptedStationEventV1` per accepted weather delivery. D's
`OriginatingWeatherV5` holds that partial observation/report/extreme/episode data, original
provider cursor, accepted station/component identity, weather contributions and optional
supplied event. It does not reread a later observation, report membership or extreme side.
The 256-KiB captured payload, per-field text limits and fixed weather field inventory are
validated; absent extreme occurrence times remain absent. Existing Source history/FIFO bounds
apply—there is no new history store or provider lookup.

`KernelEvent` uses the captured input even after current components change. Its provenance
exposes `EventAcceptance` with station generation/revision, complete component metadata and
the event's weather contributions. Current state remains in `KernelSnapshot`. Component
metadata also exposes the retained provider/source/cursor/time evidence as typed
`ComponentProvenance`, independently of a supplied object's metadata. Source validates merged
current weather and aggregate station text bounds before publishing a mutation.

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
`KernelHost` defers the first economic call into `AwaitingBrokerOutcome` and replays ordered,
verified returns. `BrokerFinancialState` exposes exact allowance, commitment, provider balance
and local reservation micros; remaining event allowance and cash-limited buying power remain
distinct. `run_transaction` assembles the fenced result and checkpoint. Strategy executables
supply only kernel construction, restore and checkpoint codecs through
`TransactionKernelFactory`. Event `emitted_at` is the provider's publication time, never the
decision clock. `PriceLevelView` carries whole-contract floors beside `exact` hundredths. Configured planners must preserve exact execution depth independently of their deliberate requested-order sizing policy; their migration remains under R3 qualification.

The stable measurements for the V5 corpus are in `conformance/v5/decision-transactions.json`.
Corpus schema 10 preserves all 19 earlier measurements, including the E vectors (4743 and 5788
bytes), native-strike F vectors and retained partial-fill spending-budget F vector. It adds exact
unfilled and partially filled cancelled-place returns, plus six invalid cancellation-return cases:
21 valid and 42 invalid entries in total. Frozen E/D/C
production is explicit; old rows are not relabelled or rewritten. The corpus gate checks exact
historical and current roundtrips; a separate boundary test accepts 64 calls and rejects 65.
Its measured local digest is `sha256:41918b28b1593593a764c5299be6110966839b1f3e9dd6ba670c377a81af06db`.
Consumers must update their immutable Core, corpus and executable attestations together.
Local codec checks do not attest a published release or replace the deferred broader consumer
qualification.

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

- place order: exact `KernelOrderResultV5` or bounded Broker error. If cancellation completes before the awaited place return, the return is `Cancelled`, with the original requested quantity, actual fills/price/fees and actual remaining open quantity. Cancellation does not become a fill, rejection, or renewed reservation. A cancelled E/F replay return must match the exact cancelled order in the returned Broker state; missing orders, changed quantities/prices and merely requested cancellations reject. Coherent historical exact-sum representations remain readable;
- cancel order: exact Boolean or bounded Broker error. A confirmed cancelled order retains its original requested quantity and actual filled quantity even when remaining open quantity is zero. That cancelled residual is not a fill or an open reservation. E replay binds a successful cancel receipt's quantities and average fill price to the returned cancelled order; fabricated zero/reduced requests reject. Historical exact-sum representations remain readable;
- cancel all: the canonical sorted set of affected order IDs or bounded Broker error. The frozen `usize` return is the set length.

Lifecycle status and return value are both validated. Place returns preserve order identity, filled quantity in hundredths, fill price, fee cost, reason, and the legacy order-status vocabulary. A zero-fill return has zero fill price and fee; a positive fill uses the exact canonical outcome price and fee cannot exceed `filled_quantity_hundredths * 1_000_000 / 100`.

## Identity and authority

The Strategy and Binding IDs are bound to the authoritative Sleeve ID by reproducing Trader V3's domain-separated Sleeve derivation over `(Strategy, Binding, Venue, Opportunity)`. Profile, attestation digest, station, event ticker/date, and complete Market membership are checked against the embedded V4 projection.

Legacy typed owner triggers bind the V4 component revision and source cursor. Current captured weather triggers bind their immutable accepted payload and cannot claim a future station/component revision within the same station generation; they do not require the old value to remain current. Forecast, oracle, Market price and timer timestamps remain tied to owner metadata or durable timer recovery evidence. Broker-trigger V4 projections use `Recovery` as the neutral recomposition trigger.

## Canonical and bounded data

- Strategy parameters are canonical sorted typed values; the adapter projects them losslessly into the frozen JSON initializer.
- Market IDs, positions, orders, and cancel-all affected IDs use strict canonical ordering.
- Position cost basis excludes fees; average entry price is exactly `cost_basis * 100 / quantity_hundredths`.
- YES and NO positions have distinct `(Market, side)` identities.
- A Market buy may carry `market_price_cap_micros` as its authoritative maximum per-contract execution price. A present cap is positive and at most `1_000_000`; Market orders never carry a limit price. Limit orders carry a limit price and never a Market price cap. Market sells never carry a buy-side price cap.
- The Market buy cap participates in canonical command encoding and the durable command commitment. A host must not execute any fill above a present cap; absence of the optional cap preserves the pre-extension Market-order contract.
- Sell, terminal and zero-remaining orders reserve no cash. Active limit-buy principal is exactly `remaining_quantity_hundredths * limit_price_micros / 100`; non-exact products are rejected rather than rounded. `reserved_fee_micros` is the remaining admitted spending buffer above that principal, not charged fees. Broker may retain price-improvement savings in this buffer until completion to fund later fees; it can exceed the remaining contracts' payout value. Its bounds are the exact owner reservation and Sleeve commitment below, not remaining notional. This changes validation only: financial state, reservation policy, wire layouts and historical bytes remain unchanged.
- Sleeve order reservations sum exactly to the V5 reserved-cash total, do not exceed the V4 account reservation, and combine with position cost and paid fees to equal the V4 Sleeve commitment.
- Broker-state, command, outcome, and return hundredths values fit the frozen kernel's signed 64-bit `ContractQuantity` interface. Whole-contract entry policies convert once with `checked_from_whole_contracts`; fractional reduce-only exits use `from_hundredths`.
- All identifiers, text, metadata, diagnostics, evidence, collections, and private kernel checkpoints have explicit bounds.

Corpus schema 3 retained exact legacy whole-contract context and result measurements, added explicit-hundredths command/outcome measurements, and kept the unchanged V4 owner-projection measurement. Legacy bytes and digests are read and verified, never rewritten.
