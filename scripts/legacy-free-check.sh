#!/usr/bin/env bash
# Legacy-free gate (Phase 9 of traderv3 docs/plans/2026-09-24-strategy-core-parity-and-legacy-removal.md).
# Fails on any legacy Strategy contract left in this repository. strategies runs the same script;
# traderv3 runs the same rules in `cargo xtask repository-policy` (xtask/src/legacy_free.rs).
# Keep the lists in step. docs/ and Markdown record history and are not checked.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

# The old `strategy-core` crate (native/strategy_core) and the old trader repository's crates.
# Cargo.lock resolves every feature and target, so it holds every crate any `cargo tree` can show.
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
# Where a forbidden symbol may stay: "symbol path-or-directory".
allowed=(
  # The V5 kernel checkpoint converter in the V6 crate. traderv3 deleted its only caller in
  # Phase 8; removing it changes the pinned crate digest.
  "KernelCheckpointV5Layout native/strategy_core_v3"
  "convert_v5_kernel_checkpoint native/strategy_core_v3"
)

self=scripts/legacy-free-check.sh
scope=(-- . ':(exclude)docs' ':(exclude)*.md' ":(exclude)$self")
status=0

# check MESSAGE GIT_GREP_ARGUMENTS...: reports every match of `git grep` over tracked and
# untracked (not ignored) files.
check() {
  local message=$1 hits rc=0
  shift
  hits=$(git grep --untracked -n -I "$@") || rc=$?
  if ((rc == 0)); then
    printf '%s:\n%s\n' "$message" "$(sed 's/^/  /' <<<"$hits")" >&2
    status=1
  elif ((rc != 1)); then
    echo "git grep failed for: $message" >&2
    exit 2
  fi
}

for crate in "${legacy_crates[@]}"; do
  check "legacy crate \`$crate\` is in the dependency graph" \
    -E -e "^name = \"$crate\"\$" -- '*Cargo.lock' ':(exclude)docs'
done
check 'legacy `strategy_core::` import; the kernel is `strategy_core_kernel::`' \
  -E -e '(^|[^A-Za-z0-9_])strategy_core::' -- '*.rs' ':(exclude)docs'
check 'legacy Python `strategy_core` import' \
  -E -e '^[[:space:]]*(import|from)[[:space:]]+strategy_core' -- '*.py' ':(exclude)docs'
check 'path dependency on the legacy `../trader` repository' \
  -E -e "[\"']([^\"']*/)?\\.\\./trader(/[^\"']*)?[\"']" -- '*Cargo.toml' ':(exclude)docs'
for feature in "${legacy_features[@]}"; do
  check "legacy feature \`$feature\`" -w -F -e "$feature" "${scope[@]}"
done
for symbol in "${forbidden_symbols[@]}"; do
  excludes=()
  for entry in ${allowed[@]+"${allowed[@]}"}; do
    if [[ ${entry%% *} == "$symbol" ]]; then
      excludes+=(":(exclude)${entry#* }")
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
