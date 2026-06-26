# Rust Strategy-Core Parity

The Python package remains the complete source contract today. The Rust crate in
`native/strategy_core` is the parity target for moving consumer runtimes such as
Trader and backtesters to a shared Rust-owned contract while keeping Python bots
supported through engine adapters.

## Crates

- `native/strategy_core_kernel`: existing narrow native-strategy hot-loop
  contract used by the Rust backtester.
- `native/strategy_core`: broader Rust parity surface for Python
  `strategy_core` modules. This crate should own shared helper logic and value
  objects that consumer runtimes need.

## Current Rust Parity Status

| Python module | Rust module | Status |
| --- | --- | --- |
| `strategy_core.models` | `strategy_core::models` | Initial aliases and telemetry field enum |
| `strategy_core.broker` | `strategy_core::broker` | Initial value objects, enums, and broker trait |
| `strategy_core.capabilities` | `strategy_core::capabilities` | Initial parity |
| `strategy_core.runtime` | `strategy_core::runtime` | Initial value-object and trait parity |
| `strategy_core.fees` | `strategy_core::fees` | Initial helper parity with matching tests |
| `strategy_core.stations` | `strategy_core::stations` | Initial helper parity with matching tests |
| `strategy_core.events` | `strategy_core::events` plus `strategy_core_kernel::events` | Initial owned event-model parity and hot-loop view parity |
| `strategy_core.state` | `strategy_core::state` | Initial owned value-object and trait parity |
| `strategy_core.context` | `strategy_core::context` | Initial trait parity |
| `strategy_core.data` | `strategy_core::data` | Initial trait parity with concrete MinuteTemp response models |
| `strategy_core.http` | `strategy_core::http` | Initial request/response and trait parity |
| `strategy_core.telemetry` | `strategy_core::telemetry` | Initial trait parity |
| `strategy_core.queries` | `strategy_core::queries` | Initial query-object parity |
| `strategy_core.climate_day` | `strategy_core::climate_day` | Initial helper parity with timezone support |
| `strategy_core.kalshi` | `strategy_core::kalshi` | Initial REST/WebSocket model parity |
| `strategy_core.minutetemp` | `strategy_core::minutetemp` | Initial OpenAPI-aligned model parity |
| `strategy_core.signals` | `strategy_core::signals` | Initial constant parity |
| `strategy_core.native` | `strategy_core::native` plus `strategy_core_kernel` | Initial native-kernel helper and hot-loop parity |

## Parity Rules

- Rust enum serde names must match the Python literal values.
- Shared helper functions need direct Rust tests mirroring the Python tests.
- Engine-specific behavior stays out of this repo. Consumer runtimes own runtime
  adapters, broker execution, replay ordering, persistence, and risk.
- New Rust bot work should depend on `native/strategy_core` for shared types and
  `native/strategy_core_kernel` for the hot-loop execution trait.
- Python bot compatibility remains an adapter responsibility: Python scripts
  keep using `async def run(ctx)`, while Rust engines translate the shared Rust
  state/event/action contract at the Python boundary.

## Remaining Integration Work

- Wire consumer runtime adapters to use the shared Rust state/event types for
  price, forecast, oracle, weather, freshness, and fee status instead of local
  copies.
- Add fixture-based cross-language conformance tests that serialize Python
  objects and deserialize them into Rust, then serialize Rust objects back to the
  same JSON shape.
- Add CI commands for the workspace-level Rust tests once the crate is accepted
  as a required contract surface.
