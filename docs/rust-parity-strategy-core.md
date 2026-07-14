# Rust Strategy-Core Parity Completion Record

Rust/Python strategy-contract parity was completed on 2026-07-14. Python remains
the contract authority, and `native/strategy_core` is the certified broad Rust
surface for portable models, enums, helpers, and strategy-facing traits.

Parity covers the declared JSON-compatible wire domain and portable behavior,
not language-identical APIs. Equivalent values have the same structural JSON;
accepted inputs normalize alike; rejected inputs share an error category; and
portable helpers return the same result or error.

## Evidence

- `tests/fixtures/conformance/manifest.json` classifies every public Python,
  broad Rust, and kernel export by ownership and required evidence.
- Python-authored fixtures under `tests/fixtures/conformance/` cover the core and
  external models, helper vectors, and all 11 shared event variants.
- `tests/test_rust_conformance.py` and
  `native/strategy_core/tests/conformance.rs` consume the same corpus. Python
  contract tests and `native/strategy_core/tests/interface.rs` cover trait
  behavior.
- Normal pytest and Cargo workspace gates discover these suites. The kernel
  contract remains independently exercised by its own Cargo tests.

The corpus compares structural JSON: field presence, `null`, enum and timestamp
strings, and numeric values are significant; object-key order is not. Intentional
cross-language differences are recorded in the manifest before behavior changes.

## Adoption outcome

`native/strategy_core` owns the broad, serializable contract.
`native/strategy_core_kernel` remains a separate borrowed, allocation-sensitive
hot-loop contract, not an incomplete copy of the broad crate.

Trader tests every supported shared event through Rust IPC and the Python
adapter. Nine variants use `event.deliver`; `timer_wake` is adapter-local; and
shutdown uses `runtime.shutdown`. Unsupported or malformed transported events
enter resync without publishing partial state.

Backtester retains replay records, timelines, read fences, provenance, lazy
hydration, and borrowed kernel views. `StrategyCoreAdapter` supplies its owned
price, weather, forecast, oracle, and freshness projections to the PyO3 bridge.
Backtester delegates portable fee math to `strategy_core::fees` while retaining
fee-policy selection, zero-fee replay behavior, liquidity mutation, accounting,
and artifacts.

Normative ownership remains in `docs/contract-map.md`. Strategy Core does not
own provider clients, mutable caches, replay ordering, persistence, execution,
risk, reconciliation, accounting, or lifecycle supervision.

No parity implementation backlog remains. Future public-contract changes must
update the inventory, applicable Python fixtures or vectors, matching Rust
evidence, and affected consumer-boundary tests.
