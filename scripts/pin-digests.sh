#!/bin/sh
# Print the digests a consumer pins for one strategy-core revision (default HEAD):
# the strategy-core-v3 source tree and the Decision V6 conformance corpus.
# The archive digest covers git's pax header, which embeds the commit id, so it changes
# with every commit even when the crate's files do not.
set -eu
rev=$(git rev-parse --verify "${1:-HEAD}^{commit}")
if command -v sha256sum >/dev/null 2>&1; then sha() { sha256sum | cut -d' ' -f1; }; else sha() { shasum -a 256 | cut -d' ' -f1; }; fi
echo "commit $rev"
echo "strategy-core-v3 archive sha256 $(git archive --format=tar "$rev" native/strategy_core_v3 | sha)"
echo "decision-transactions.json sha256 $(git show "$rev:conformance/v6/decision-transactions.json" | sha)"
