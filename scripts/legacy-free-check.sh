#!/usr/bin/env bash
# Legacy-free gate (Phase 9 of traderv3 docs/plans/2026-09-24-strategy-core-parity-and-legacy-removal.md).
# Fails on any legacy Strategy contract left in this repository. strategies runs the same script;
# traderv3 runs the same rules in `cargo xtask repository-policy` (xtask/src/legacy_free.rs).
# Keep the lists in step. docs/ and Markdown record history and are not checked. Needs cargo.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

# The old `strategy-core` crate (native/strategy_core) and the old trader repository's crates.
legacy_crates=(strategy-core trader-core trader-bot-ipc)
legacy_features=(legacy-kernels v2-bot)
# The Decision V4 IPC and Decision V5 continuation and checkpoint symbols the V6 cutover and
# Phases 7, 8 and 8b deleted, matched as whole tokens. The V4 types V6 embeds
# (`strategy_core_v3::decision_v4::*`, `DecisionContextV6.owner_state`) are not listed.
forbidden_symbols=(
  # Decision V4 IPC and its production profile.
  trader-sleeve-ipc/4 encode_production_decision_v4 decode_production_result
  PRODUCTION_PROFILE PRODUCTION_PROFILE_DIGEST PRODUCTION_CAPABILITIES
  PRODUCTION_CAPABILITIES_DIGEST PRODUCTION_CALCULATOR_PROFILE
  DecisionV4ObservedAssertions ReleaseTopologyDecisionV4
  # Decision V5 wire, continuations and checkpoints.
  trader-sleeve-ipc/5 IPC_PROTOCOL_V5 PRODUCTION_CAPABILITIES_V5
  DecisionV5 DecisionV5Error DecisionContextV5 DecisionResultV5
  encode_decision_context_v5 decode_decision_context_v5
  encode_decision_result_v5 decode_decision_result_v5
  DECISION_CONTEXT_V5_MAGIC DECISION_RESULT_V5_MAGIC SDCTXV5 SDRESV5
  StrategyCommandV5 ContinuationCommitmentV5 KernelCheckpointV5
  KernelCheckpointV5Layout convert_v5_kernel_checkpoint decode_v5_checkpoint
  encode_v5_checkpoint StoredStrategyCheckpoint strategy_continuations
  migrate_decision_v5_ledger
)
# Where a forbidden symbol may stay: "symbol exact-file-path audited-count". More occurrences in
# that file fail.
allowed=(
  # The V5 kernel checkpoint converter in the V6 crate and its tests. traderv3 deleted its only
  # caller in Phase 8; removing it changes the pinned crate digest.
  "KernelCheckpointV5Layout native/strategy_core_v3/src/decision_v6.rs 2"
  "KernelCheckpointV5Layout native/strategy_core_v3/src/decision_v6/corpus_tests.rs 1"
  "KernelCheckpointV5Layout native/strategy_core_v3/src/decision_v6/tests.rs 2"
  "KernelCheckpointV5Layout native/strategy_core_v3/tests/kernel_projection/decisions.rs 3"
  "convert_v5_kernel_checkpoint native/strategy_core_v3/src/decision_v6.rs 2"
  "convert_v5_kernel_checkpoint native/strategy_core_v3/src/decision_v6/corpus_tests.rs 1"
  "convert_v5_kernel_checkpoint native/strategy_core_v3/src/decision_v6/tests.rs 2"
  "convert_v5_kernel_checkpoint native/strategy_core_v3/tests/kernel_projection/decisions.rs 2"
)

self=scripts/legacy-free-check.sh
scope=(-- . ':(exclude)docs' ':(exclude)*.md' ":(exclude)$self")
status=0

fail() {
  printf '%s:\n%s\n' "$1" "$(sed 's/^/  /' <<<"$2")" >&2
  status=1
}

# check MESSAGE GIT_GREP_ARGUMENTS...: reports every match of `git grep` over tracked and
# untracked (not ignored) files, binary ones included (-a).
check() {
  local message=$1 hits rc=0
  shift
  hits=$(git grep --untracked -n -a "$@") || rc=$?
  if ((rc == 0)); then
    fail "$message" "$hits"
  elif ((rc != 1)); then
    echo "git grep failed for: $message" >&2
    exit 2
  fi
}

# Legacy crates: `cargo metadata` resolves each lockfile's workspace with every feature and
# target, so it names every crate any `cargo tree` can show. A lockfile naming one as a quoted
# string in any spelling (`name = "…"`, `name='…'`, `"… 0.1.0"`) fails too.
while IFS= read -r lock; do
  for crate in "${legacy_crates[@]}"; do
    check "legacy crate \`$crate\` is in $lock" \
      -E -e "[\"']$crate( [^\"']*)?[\"']" -- "$lock"
  done
  if ! metadata=$(cargo metadata --format-version 1 --locked --all-features \
    --manifest-path "$(dirname "$lock")/Cargo.toml" </dev/null); then
    fail "cargo metadata failed" "$lock"
    continue
  fi
  for crate in "${legacy_crates[@]}"; do
    if grep -q "\"name\":\"$crate\"" <<<"$metadata"; then
      fail "legacy crate \`$crate\` is in the dependency graph" "cargo metadata for $lock"
    fi
  done
done < <(git ls-files --cached --others --exclude-standard -- '*Cargo.lock' ':(exclude)docs')

# No Python: strategy-core deleted its Python packages in Phase 7.
python=$(git ls-files --cached --others --exclude-standard -- \
  '*.py' 'pyproject.toml' '*/pyproject.toml' 'setup.py' '*/setup.py')
if [[ -n $python ]]; then
  fail 'Python file (the repository has no Python since Phase 7)' "$python"
fi

check 'legacy `strategy_core::` import; the kernel is `strategy_core_kernel::`' \
  -E -e '(^|[^A-Za-z0-9_])strategy_core::' -- '*.rs' ':(exclude)docs'
check 'path dependency on the legacy `../trader` repository' \
  -E -e "[\"']([^\"']*/)?\\.\\./trader(/[^\"']*)?[\"']" -- '*Cargo.toml' ':(exclude)docs'
for feature in "${legacy_features[@]}"; do
  check "legacy feature \`$feature\`" -w -F -e "$feature" "${scope[@]}"
done
for symbol in "${forbidden_symbols[@]}"; do
  excludes=()
  for entry in ${allowed[@]+"${allowed[@]}"}; do
    read -r allowed_symbol allowed_path audited <<<"$entry"
    if [[ $allowed_symbol == "$symbol" ]]; then
      excludes+=(":(exclude)$allowed_path")
      hits=$(git grep --untracked -o -a -w -F -e "$symbol" -- "$allowed_path") || [[ $? == 1 ]]
      count=$(grep -c . <<<"$hits" || true)
      if ((count > audited)); then
        fail "\`$symbol\` beyond its $audited audited occurrence(s)" "$allowed_path: $count"
      fi
    fi
  done
  check "deleted Decision V4 IPC or V5 symbol \`$symbol\`" \
    -w -F -e "$symbol" "${scope[@]}" ${excludes[@]+"${excludes[@]}"}
done

if ((status != 0)); then
  echo 'legacy-free gate failed' >&2
  exit 1
fi
echo 'legacy-free gate passed'
