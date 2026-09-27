# Phase 5: order intent parity

Scope: decision 5 and the "Order intent gap" in
`traderv3/docs/plans/2026-09-24-strategy-core-parity-and-legacy-removal.md`. The work ports
`time_policy` (GTC / IOC / FOK), `post_only`, expiry on sells and a per-order Market buy
price cap. `execution_style` becomes kernel constructors only, and `max_cost` is not
ported. Live Strategy sells get a dispatch path.

The repos land in order: strategy-core, then strategies, then traderv3 and the backtester.
Each repin is its own commit and updates its attestations. traderv3 references are at
`origin/main` 1eee060.

## The contract (strategy-core, branch `phase5/order-intent`)

Decision V6 is extended in place. There is no V7, because no V6 bytes are stored anywhere.
The corpus digest changes. See [decision-v6.md](../decision-v6.md) "Order terms" and
open points 45-50.

- **Kernel `PlaceOrderRequest`** gains three fields:
  - `market_price_cap: Option<f64>`, in dollars;
  - `time_policy: TimePolicy`, which defaults to `GoodTillCanceled`;
  - `post_only: bool`.

  It also gains three constructors:
  - `resting_limit` builds a GTC Limit order;
  - `direct` builds an IOC Limit order (for a sell, the limit is its price floor);
  - `sweep` builds an IOC Market buy at a cap.
- **Wire `PlaceOrderV6`** gains `time_policy: TimePolicyV6` and `post_only`.
  `market_price_cap_micros` now comes from the request.
- **Local errors**, which validation also rejects. `place_order_terms_error` gives the
  reason:
  - `post_only` is only allowed on a GTC Limit order;
  - an expiry is only allowed on a GTC order;
  - a Market sell must be IOC or FOK;
  - only a Market buy can have a cap, and the cap must be in (0, 1].
- **Reservation**, which must equal the Broker's commitment:
  - a Limit buy reserves at its limit;
  - a Market buy reserves at its own cap, or at $1 if it has none;
  - a sell reserves nothing;
  - the time policy and `post_only` change nothing.
- **Removed:** the `TransactionKernel::market_buy_price_cap_micros` hook, the cap closure
  argument of `run_native_decision`, and `MARKET_SELL_UNSUPPORTED_CODE`.
- **Live Market sells:** a live Market sell now counts 5 plan rows, not 6, and
  `capabilities().market_sell` is true in live.

## strategies

1. Repin `strategy-core-kernel` and `strategy-core-v3` in `Cargo.toml` and
   `trader-v3-strategy/Cargo.toml`, then update the attestations.
2. Add the three new fields to every `PlaceOrderRequest` literal: `market_price_cap: None`,
   `time_policy: TimePolicy::GoodTillCanceled`, `post_only: false`. The exceptions are
   below.
3. `dsm_reaction` (Base): the entry Market buy (`src/dsm_reaction.rs:906`) sets
   `market_price_cap: Some(self.config.max_pay)`, and `time_policy` stays GTC, so it still
   rests at the cap. Delete `market_buy_price_cap()` (`:117`), the
   `TransactionKernel::market_buy_price_cap_micros` impl
   (`trader-v3-strategy/src/decision_v6.rs:176`), and the cap closures in
   `trader-v3-strategy/src/decision_v6/resident.rs:42-66`. This is the hook's only user:
   `V10`, `V12` and `hourly` return `None`.
4. V10/V12 keep their behaviour. Their Market exit sells (`dsm_reaction_v10.rs:4346`,
   `dsm_reaction_v12.rs:6311`) must now say `ImmediateOrCancel`, which the paper Broker
   already applies to Market sells. A limit exit stays GTC.

   In live, `capabilities().market_sell` becomes true, so V10/V12 exit with a Market sell
   instead of a limit sell at the bid (`dsm_reaction_v10.rs:4273`, `dsm_reaction_v12.rs:6234`).
   Decide at the repin between keeping that and moving the live exit to `direct` at a
   floor. Remove `market_sell_unsupported` from `TEMPORARY_REFUSAL_CODES`
   (`src/helpers.rs:714`) once traderv3 lifts it.
5. The champions (Phase 6a):
   - entries use `sweep(ticker, side, qty, cap)` (capped Market) or
     `direct(ticker, Buy, side, qty, limit)` (IOC), sized by their own order-book preview;
   - the stale-position exit (`src/champion.rs:1034`, a Market sell with a
     `limit_price` floor, which V6 rejects) becomes
     `PlaceOrderRequest { reduce_only: true, ..direct(ticker, Sell, side, qty, floor) }`.
6. Also on repin: in traderv3, the fixtures that implement the hook
   (`crates/trader-fixtures/src/broker_sequence.rs:269`, `live_order.rs:100`,
   `entry_probe.rs:130`) move their cap onto their Market buys. The fixture Market sell at
   `broker_sequence.rs:219` sets IOC.

## traderv3

One change: the repin plus everything below. The plan-row parity test forces this. The
runner now counts 5 rows for a live Market sell, so a traderv3 that still refuses one
with a receipt row would fail `tests/decision_admission.rs:1744-1780`.

1. **Repin** in all of these places:
   - `Cargo.toml:34-35` and `Cargo.lock`;
   - `xtask/src/main.rs:16`;
   - `crates/trader-runtime/src/ipc/handshake.rs:15-21` (commit, crate digest, corpus
     digest);
   - `deploy/paper-daily/pins.env:4-8`;
   - `config/strategy-attestations.json`;
   - `strategy_core_revision` in `config/*.toml` and
     `crates/trader-app/tests/fixtures/mixed-fleet-daily-paper.toml`.
2. **Intake** (`crates/trader-app/src/compose_decisions.rs:326-394`). Any `Err` here fails
   the whole decision (`:219-221`). So every command that V6 validation accepts must map to
   an intent, or be refused per command. Specifically:
   - a limit sell with an expiry (the `:358` guard);
   - a Market buy with an expiry (the `:417` guard; a GTC Market buy rests at its cap);
   - `time_policy` and `post_only` on every shape;
   - a non-reduce-only sell, which is still unrepresentable (`:394`). Refuse it per
     command.
3. **Intent types** (`crates/trader-broker/src/admission/identity.rs`):
   - `ExposureIntent` (`:30-50`) gains the time policy and `post_only`;
   - `ReduceOnlyIntent` (`:314-328`) gains an expiry, `post_only`, and FOK beside
     `immediate_or_cancel`;
   - Strategy limit sells (`new_inner`, `:470-502`) take IOC from the command, not
     `false`;
   - each new field goes into the canonical payloads (`:278-310`, `:545-584`; expiry via
     `expiry.rs`), the outbox `AcceptedOperation` (`store/outbox.rs:27-41`), and
     `inventory.rs:82-99` / `allowance.rs:8`.
4. **Admission and reservation:**
   - Buys still commit `quantity × price + fee reservation` (`identity.rs:270-276`), at
     the limit, at the Market cap, or at $1 without a cap (`compose_decisions.rs:402-431`).
     This must equal the runner's figure: the parity check is the `overlay` section of the
     corpus.
   - Admission refuses a buy whose limit is below the best ask (`admission/mod.rs:482-487`,
     `price_moved`). A post-only buy inverts this check: refuse it when it would cross
     (limit ≥ best ask).
   - Refuse an uncapped live Market buy per command. The signer cannot sign a $1 price
     (`orders.rs:343-352`).
5. **Lift the live sell refusals** in the same change as the signer:
   - `LiveSellUnsupported` and `MarketSellUnsupported`
     (`owner/decision_admission.rs:1067-1078`, `strategy_decision.rs:127-131, 164-165, 205-206`);
   - the `admit_reduce_only` backstop (`admission/mod.rs:642-644`);
   - `"reduce-only live dispatch is unavailable"` (`compose.rs:5719-5722`).

   Once live sells dispatch, the V10/V12 exits no longer retry a refused live sell every
   few seconds without end.
6. **Live signer** (`crates/trader-providers/src/account/orders.rs:190-240`, called from
   `live_transport.rs:772`):
   - send `time_in_force` from the intent (`good_till_canceled`, `immediate_or_cancel`,
     `fill_or_kill`);
   - send `post_only` when set;
   - send `expiration_time` for GTC orders only;
   - sign Strategy sells the way the liquidation signer does (`:242-285`): a YES sell is
     `ask` at the price, a NO sell is `bid` at the complement, with `reduce_only: true`;
   - a live Market sell is an IOC/FOK sell at the lowest price the grid allows. A Market
     buy is a limit at its cap, as today.

   Check with Kalshi's order docs before merging:
   - that `post_only` is refused with IOC/FOK;
   - that `reduce_only` is accepted on a resting (GTC) order. If it is not, send resting
     sells without it and rely on the Broker's inventory check;
   - that `buy_max_cost` forces FOK (decision 5, not ported).
7. **Cancel-all in live.** Today's code already expands a live cancel-all into per-order
   cancels (`decision_admission.rs:1358-1360`, `classify_live_cancel_all`
   `:1406-1484`). The refusal the parity plan cites is now `paper_cancel_all`
   (`admission/mod.rs:1761-1763`), which is reached only in paper. Confirm that each
   expanded cancel runs the same checks as `classify_cancel` (`:1195-1331`: transport
   current, priority slot, expected revision). Then drop the parity plan's "cancel-all is
   refused outside paper" item.
8. **Paper venue:**
   - `execution/paper.rs:481-535` (`record_verified_dispatch`) rests every buy remainder
     today. IOC should cancel the remainder, FOK should fill only if the whole quantity is
     there at dispatch (else fill nothing), and post-only should be refused if it would
     cross.
   - Reduce-only IOC is already handled at `compose.rs:5744-5760`; extend it with FOK.
   - Expiry covers buys only today (`owner.rs:5006-5035`, `admission/mod.rs:3273`,
     `compose.rs:5216-5234, 5496-5523`). Extend it to resting sells.
9. **Recorder:** `decision_recorder.rs:1177` records `time_policy` and `post_only` as well.
10. **Tests:** extend the existing suites in a table-driven way:
    - `tests/decision_admission.rs` (per-field admission, sell expiry, live sells, and
      `runner_rows` parity including live cancel-all);
    - the paper venue tests (IOC / FOK / post-only / sell expiry);
    - the signer body tests (one per `time_in_force`, `post_only`, sell side);
    - `compose.rs:16317-16351` (Market buy reservation at a per-order cap).

## backtester (B11)

`traderv3/docs/plans/2026-09-25-phase6b-backtester-v6-port.md` B11 already has the fill
rules: IOC never rests, FOK fills whole or nothing, post-only is refused if it would cross,
sells can expire, and a Market buy is capped per order. Admission comes through the shared
Broker crate with the Phase 5 repin. The field names are strategy-core's: `time_policy`,
`post_only`, `market_price_cap_micros`, `expires_after_ms`. Replay recorded paper
decisions to check that fills agree.
