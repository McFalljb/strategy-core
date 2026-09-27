#!/bin/sh
# Print the digests a consumer pins for one strategy-core revision (default HEAD):
# the strategy-core-v3 source tree and the Decision V6 conformance corpus.
# The archive digest covers git's pax header, which embeds the commit id, so it changes
# with every commit even when the crate's files do not.
set -eu
cd "$(git rev-parse --show-toplevel)"
rev=$(git rev-parse --verify "${1:-HEAD}^{commit}")
if command -v sha256sum >/dev/null 2>&1; then sha() { sha256sum | cut -d' ' -f1; }; else sha() { shasum -a 256 | cut -d' ' -f1; }; fi
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
# Write each input to a file first, so a git failure stops the script instead of hashing
# empty input.
git archive --format=tar "$rev" native/strategy_core_v3 >"$tmp/crate.tar"
git show "$rev:conformance/v6/decision-transactions.json" >"$tmp/corpus.json"
crate=$(sha <"$tmp/crate.tar")
corpus=$(sha <"$tmp/corpus.json")
echo "commit $rev"
echo "strategy-core-v3 archive sha256 $crate"
echo "decision-transactions.json sha256 $corpus"
