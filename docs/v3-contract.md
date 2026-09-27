# Strategy Core V3 semantic contract

The root module of the Rust `strategy-core-v3` crate (`native/strategy_core_v3`) defines the runtime-neutral V3 semantics: the canonical profile and the bounded V3 value types. The same crate carries the Decision V6 wire and, with the `kernel` feature, its runner ([Decision V6](decision-v6.md)), plus the frozen V4 owner-projection codec (`decision_v4`) that V6 contexts embed. The kernel API itself is `strategy-core-kernel` ([contract map](contract-map.md)).

## Ownership boundary

Strategy Core owns immutable bounded decision context/result values, Strategy order-intent meaning, deterministic reason codes, evidence and diagnostics, the pure result-profile calculator, stable `TimerKey` schedule/cancel requests, and canonical semantic bytes.

Trader owns lookup and state assembly, broker behavior, provider/HTTP integration, persistence, process runtime/supervision, IPC framing, runtime identities, delivery causality, state fences, monotonic deadlines, and timer generation, ordering, scheduling, reconstruction, delivery identity, deduplication, and terminal accounting. V3 values contain no callback, service locator, client, or runtime adapter.

A `ScheduleTimerRequest` is only Strategy meaning: key, signed UTC scheduled epoch nanoseconds, semantics version, and bounded semantic bytes. Schedule is an idempotent upsert and cancel is idempotent. Neither request carries a Trader-generated timer generation or delivery identity.

## Bounds

The profile constants the crate exports are normative:

| Item | Bound |
|---|---:|
| canonical value or context/evidence payload | 1,048,576 bytes |
| canonical list/map nesting | 64 levels |
| canonical integer | signed 128-bit |
| identifier / `TimerKey` | 128 UTF-8 bytes |
| reason code | 64 ASCII bytes |
| order intents per result | 64 |
| timer requests per result | 64 |
| evidence items per context/result | 128 |
| diagnostics per result | 64 |
| diagnostic message | 512 UTF-8 bytes |
| timer semantics | 4,096 bytes |
| decimal digits / scale | 38 / 18 |

Reason codes match `[a-z][a-z0-9_]{0,63}`. Persistent UTC instants are signed 64-bit epoch nanoseconds. Envelope deadlines remain unsigned host-local monotonic nanoseconds and are not persistent instants.

## Canonical profile `strategy-core-canonical-v1`

Canonical bytes are independent from IPC framing. Every value is encoded as:

1. ASCII magic `SCV3`;
2. one profile-version byte (`0x01`);
3. a domain frame;
4. one canonical value frame.

A frame is one ASCII type byte, a four-byte unsigned big-endian payload length, and payload bytes. Type bytes are `D` domain, `n` null, `b` boolean (`0`/`1`), `i` signed i128 integer ASCII, `t` signed big-endian i64 epoch nanoseconds, `d` one scale byte plus fixed-scale decimal ASCII, `s` UTF-8 string, `x` bytes, `l` list, and `m` map. Strings and map keys use Unicode NFC. Map entries sort by normalized UTF-8 key bytes; keys colliding after normalization fail. Lists and maps prefix their payload with a four-byte item count. Decimal strings have exactly the declared scale, no leading zeroes, and no negative zero. Digests are lowercase SHA-256 hex over the complete domain-separated bytes.

Canonical encoding enforces the complete byte bound incrementally and allows at most 64 nested list/map levels (a root scalar is depth 0). Exceeding either limit fails with a normalized canonical error before recursive growth can become unbounded.

`conformance/v3/vectors.json` is the profile's corpus, checked by `native/strategy_core_v3/tests/conformance.rs`. It includes Unicode, map order, signed i128 boundaries and overflow, negative/positive instants, decimal, timer schedule/cancel, diagnostic boundary, size/nesting overflow, and normalization cases.

## Dependency exclusions

The crate's required dependencies are exactly `bincode`, `sha2`, `strategy-core-kernel` and `unicode-normalization`. Its only feature, `kernel`, adds the optional `chrono` and `serde_json`, and `serde_json` is its sole dev dependency. The conformance suite enforces this allowlist and rejects any other dependency, such as a Trader runtime, reqwest or tokio edge.
