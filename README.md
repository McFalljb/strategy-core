# Strategy Core

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

The shared Strategy contract for the Trader V3 runtime and the Backtester. A Strategy is a
Rust kernel that runs once per event; this library defines what it implements, what it
sees, and what it may ask for. It is a **library, not an engine**: hosts build the context,
deliver events, and own the Broker, providers, persistence and timers.

## Crates

| Crate | Path | Role |
|---|---|---|
| `strategy-core-kernel` | `native/strategy_core_kernel` | The kernel contract: `NativeKernel` and `StrategyKernelContext`, the canonical state and event model, order and request values, and pure helpers (exact fees, stations and series tickers, climate days, freshness). |
| `strategy-core-v3` | `native/strategy_core_v3` | The canonical profile, the Decision V6 wire, and (feature `kernel`) the runner that presents a V6 context to a kernel and assembles its result. |

Both crates are version `0.1.x`; the API may change before `1.0.0`.

## Use

Pin both crates to one git revision:

```toml
[dependencies]
strategy-core-kernel = { git = "https://github.com/McFalljb/strategy-core.git", rev = "<commit>", version = "=0.1.0" }
strategy-core-v3 = { git = "https://github.com/McFalljb/strategy-core.git", rev = "<commit>", version = "=0.1.0", features = ["kernel"] }
```

`scripts/pin-digests.sh <commit>` prints the two digests consumers record with the
revision: the `strategy-core-v3` source-tree archive and the Decision V6 conformance corpus.

A minimal kernel:

```rust
use strategy_core_kernel::{KernelResult, NativeKernel, StrategyEventView, StrategyKernelContext};

struct MyKernel;

impl NativeKernel for MyKernel {
    fn name(&self) -> &str {
        "my-kernel"
    }

    fn on_event(
        &mut self,
        event: StrategyEventView<'_>,
        ctx: &mut dyn StrategyKernelContext,
    ) -> KernelResult<()> {
        if let StrategyEventView::OrderUpdate(update) = event {
            ctx.telemetry()
                .counter("order_updates", 1.0, &[("client_order_id", &update.client_order_id)])?;
        }
        Ok(())
    }
}
```

## Develop

Requires Rust 1.85 (`rust-toolchain.toml`).

```bash
cargo fmt --manifest-path native/Cargo.toml --all -- --check
cargo clippy --manifest-path native/Cargo.toml --workspace --all-targets --all-features -- -D warnings
cargo test --manifest-path native/Cargo.toml --workspace --all-features
```

`scripts/legacy-free-check.sh` (run in CI) is the legacy-free gate. It fails on the legacy
`strategy-core`, `trader-core` or `trader-bot-ipc` crates in `cargo metadata` or a `Cargo.lock`;
`strategy_core::` in Rust (the kernel is `strategy_core_kernel::`); any Python file; the
`legacy-kernels` or `v2-bot` features; a path dependency on `../trader`; and the deleted Decision V4 IPC
and V5 continuation and checkpoint symbols. The V4 types Decision V6 embeds
(`strategy_core_v3::decision_v4::*`) are allowed. `docs/` and Markdown are not checked. The symbol
list and its allowlist are in the script; strategies and traderv3 enforce the same rules.

## Repository layout

```text
native/        # Rust workspace: strategy_core_kernel and strategy_core_v3
conformance/   # Shared corpora: v3/vectors.json (canonical profile), v6/decision-transactions.json
scripts/       # pin-digests.sh, legacy-free-check.sh
docs/          # Contract documentation
```

## Documentation

- [docs/contract-map.md](docs/contract-map.md): the kernel contract, from writing a kernel to the pure helpers
- [docs/decision-v6.md](docs/decision-v6.md): Decision V6, one run per event: order updates, provisional view, order terms, external requests and wire
- [docs/v3-contract.md](docs/v3-contract.md): the V3 canonical profile and value bounds

## License

MIT; see [LICENSE](LICENSE).
