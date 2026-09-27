# Strategy Core

Shared Rust Strategy contract for the Trader V3 runtime and the Backtester. The repository is
Rust-only: do not add Python packages, tooling or lockfiles.

## Project structure

```text
strategy-core/
  native/         # Rust workspace: strategy_core_kernel (kernel contract), strategy_core_v3 (canonical profile, Decision V6 wire and runner)
  conformance/    # v3/vectors.json and v6/decision-transactions.json, checked by the Rust tests
  scripts/        # pin-digests.sh: the digests consumers pin for a revision
  docs/           # Contract documentation
```

## Commands

```bash
# Format check (CI)
cargo fmt --manifest-path native/Cargo.toml --all -- --check

# Lint (CI denies warnings)
cargo clippy --manifest-path native/Cargo.toml --workspace --all-targets --all-features -- -D warnings

# Tests, including the V3 and V6 conformance corpora
cargo test --manifest-path native/Cargo.toml --workspace --all-features

# Regenerate the Decision V6 corpus after an intended wire change
cargo test --manifest-path native/Cargo.toml -p strategy-core-v3 -- --ignored write_v6_corpus

# Digests consumers pin for a revision
scripts/pin-digests.sh <commit>
```

## Conventions

- Keep dependencies minimal; this repo is a shared library, not an engine.
  `strategy-core-v3`'s dependency allowlist is enforced by its conformance test.
- Consumers pin a git revision plus the `native/strategy_core_v3` archive digest and the
  `conformance/v6/decision-transactions.json` digest. A change to either is a contract
  change: say so in the commit and update the docs.

## Docs

- [docs/contract-map.md](docs/contract-map.md): the kernel contract
- [docs/decision-v6.md](docs/decision-v6.md): Decision V6
- [docs/v3-contract.md](docs/v3-contract.md): the V3 canonical profile
