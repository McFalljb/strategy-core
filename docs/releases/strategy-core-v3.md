# Strategy Core V3 0.1.0 release

Strategy Core V3 0.1.0 is the immutable Python/Rust semantic release for the `strategy-core-canonical-v1` profile. It contains the architecture-neutral Python wheel, normalized Rust source crate, shared conformance corpus, exact toolchain record, artifact manifest, qualification consumer lock, and SHA-256 inventory.

## Local qualification

The checked qualification environment is CPython 3.12.12, uv 0.9.28, Rust 1.85.0 with rustfmt, and the GitHub-hosted `ubuntu-24.04` x64 runner family. The workflow verifies those facts. On that runner, run:

```bash
python scripts/build_v3_release.py --check --check-toolchains
uv run --frozen pytest tests/v3/test_release_manifest.py -q
```

For development on another OS, omit `--check-toolchains`; this checks the payload but does not qualify a different environment. The builder makes two artifact builds in one recorded environment and requires byte-identical results. This is not a claim that different operating systems or compressor implementations produce identical bytes. The Rust `.crate` uses CPython's stdlib gzip level 9 and USTAR writer with fixed metadata. The Python wheel is produced by exact Hatchling through exact uv.

Each artifact is extracted into a clean temporary consumer. The Rust proof copies the checked `rust-consumer.Cargo.lock` into an empty `CARGO_HOME`, permits `cargo fetch --locked` to resolve its exact registry checksums, and only then runs `cargo test --locked --offline`. It never generates a lock from ambient cache state. The `.crate` has exact direct dependency versions and no Cargo-generated VCS metadata; the protected release tag and immutable GitHub release provide source identity.

## Qualification consumer rule

`release/v3/consumer-lock.json` is the only supported qualification-resolution record. Consumers must:

1. use its exact `strategy-core-v3-v0.1.0` GitHub release URLs;
2. verify SHA-256 before extracting or installing an artifact;
3. retain the matching released corpus and manifest digests.

Moving Git branches or branch archives, Git dependencies with `branch`, sibling checkout paths, editable installs, `file:` URLs, and unverified cache entries are forbidden qualification inputs. Local extraction paths used only after a locked artifact's digest is verified do not become dependency sources.

## External immutability prerequisite

Repository files cannot establish or prove GitHub repository settings. Before a tag may be pushed, a repository administrator must externally verify both of these controls:

1. GitHub immutable releases are enabled for `McFalljb/strategy-core`.
2. A GitHub tag ruleset matching `strategy-core-v3-v*` blocks tag updates and deletion, with no bypass actor that can retarget or delete a release tag.

After that independent verification, set these repository variables as the workflow's external attestation:

- `STRATEGY_CORE_V3_IMMUTABLE_RELEASES_ATTESTATION=immutable-releases-enabled`
- `STRATEGY_CORE_V3_PROTECTED_TAG_ATTESTATION=strategy-core-v3-tags-update-delete-blocked-no-bypass`

The release workflow fails before checkout or publication when either attestation is absent or different. These variables do not configure the controls; administrators must not set them until the corresponding GitHub settings have been inspected. Until both controls are verified and attested, publication is externally blocked and U-003 is not an immutable published release.

## Publish procedure

The U-003 implementation commit itself must be the tagged object. After the external prerequisite is satisfied, run qualification on the recorded runner and create the annotated tag for that exact commit:

```bash
python scripts/build_v3_release.py --check --check-toolchains
git tag -a strategy-core-v3-v0.1.0 -m "Strategy Core V3 0.1.0 immutable semantic release" <U-003-COMMIT>
git push origin strategy-core-v3-v0.1.0
```

Pushing the annotated tag starts `.github/workflows/release-v3.yml`; it requires the external attestations, rechecks the exact tag target, tools, runner, reproducibility, corpus, digests, and clean consumers before creating the GitHub release. Do not retarget or reuse this tag. If qualification fails, fix forward with a new version and tag.

This implementation intentionally creates neither a local tag nor a GitHub release and does not claim the external controls are currently configured. The caller must replace `<U-003-COMMIT>` with the orchestrator-owned U-003 commit hash only after an administrator supplies the attestations.
