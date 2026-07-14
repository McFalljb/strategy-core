"""Completeness checks for the cross-language conformance inventory."""

from __future__ import annotations

import copy
import json
import re
from collections import Counter
from pathlib import Path
from typing import Any, cast

import pytest

import strategy_core

ROOT = Path(__file__).parents[1]
MANIFEST_PATH = ROOT / "tests" / "fixtures" / "conformance" / "manifest.json"
RUST_BROAD_LIB = ROOT / "native" / "strategy_core" / "src" / "lib.rs"
RUST_KERNEL_LIB = ROOT / "native" / "strategy_core_kernel" / "src" / "lib.rs"

OWNERSHIP_CLASSES = {
    "broad-parity-required",
    "kernel-only",
    "python-only",
    "consumer-owned",
}
EVIDENCE_MECHANISMS = {
    "fixture",
    "helper-vector",
    "trait-test",
    "intentional-exclusion",
}
EVIDENCE_DIMENSIONS = {
    "non_default_round_trip",
    "defaults",
    "omission",
    "explicit_null",
    "enum_values",
    "timestamp_formatting",
    "numeric_boundaries",
    "invalid_input",
    "helper_or_trait_behavior",
    "consumer_boundaries",
}
LANGUAGES = {"python", "rust_broad", "rust_kernel"}


def _load_manifest() -> dict[str, Any]:
    return cast("dict[str, Any]", json.loads(MANIFEST_PATH.read_text()))


def _rust_exports(path: Path) -> set[str]:
    """Return the explicitly re-exported root symbols from a crate lib.rs."""

    source = path.read_text()
    exports: list[str] = []
    for match in re.finditer(r"pub use\s+[^;]+;", source, re.DOTALL):
        statement = match.group(0)
        if "*" in statement:
            continue
        if "{" in statement:
            body = statement.split("{", 1)[1].rsplit("}", 1)[0]
            parts = body.split(",")
        else:
            body = statement.removeprefix("pub use ").removesuffix(";")
            parts = [body.rsplit("::", 1)[-1]]
        exports.extend(part.strip().split(" as ")[-1] for part in parts if part.strip())
    return set(exports)


def _actual_exports() -> dict[str, set[str]]:
    return {
        "python": set(strategy_core.__all__),
        "rust_broad": _rust_exports(RUST_BROAD_LIB),
        "rust_kernel": _rust_exports(RUST_KERNEL_LIB),
    }


def _validate_manifest(manifest: dict[str, Any], actual: dict[str, set[str]]) -> None:
    assert manifest["schema_version"] == 1
    assert set(manifest["ownership_classes"]) == OWNERSHIP_CLASSES
    assert set(manifest["evidence_mechanisms"]) == EVIDENCE_MECHANISMS
    assert set(manifest["evidence_dimensions"]) == EVIDENCE_DIMENSIONS

    profiles = manifest["evidence_profiles"]
    assert profiles
    for profile_name, profile in profiles.items():
        assert profile["mechanism"] in EVIDENCE_MECHANISMS, profile_name
        assert profile["reference"].strip(), profile_name
        dimensions = profile["dimensions"]
        assert set(dimensions) == EVIDENCE_DIMENSIONS, profile_name
        for dimension_name, policy in dimensions.items():
            assert isinstance(policy["applicable"], bool), (profile_name, dimension_name)
            if not policy["applicable"]:
                assert policy.get("rationale", "").strip(), (profile_name, dimension_name)

    recorded: dict[str, list[str]] = {language: [] for language in LANGUAGES}
    groups = manifest["export_groups"]
    assert groups
    for group in groups:
        assert group["ownership"] in OWNERSHIP_CLASSES, group["id"]
        assert group["evidence_profile"] in profiles, group["id"]
        assert group["rationale"].strip(), group["id"]
        surfaces = group["surfaces"]
        assert surfaces
        assert set(surfaces) <= LANGUAGES, group["id"]
        for language, symbols in surfaces.items():
            assert symbols, (group["id"], language)
            recorded[language].extend(symbols)

    for language, expected in actual.items():
        counts = Counter(recorded[language])
        duplicates = sorted(symbol for symbol, count in counts.items() if count != 1)
        assert not duplicates, f"{language} symbols must appear exactly once: {duplicates}"
        assert set(counts) == expected, {
            "language": language,
            "missing": sorted(expected - set(counts)),
            "unknown": sorted(set(counts) - expected),
        }


def test_parity_manifest_covers_every_public_surface() -> None:
    _validate_manifest(_load_manifest(), _actual_exports())


def test_manifest_rejects_unknown_ownership_class() -> None:
    manifest = _load_manifest()
    manifest["export_groups"][0]["ownership"] = "unclassified"

    with pytest.raises(AssertionError):
        _validate_manifest(manifest, _actual_exports())


def test_manifest_rejects_missing_not_applicable_rationale() -> None:
    manifest = _load_manifest()
    profile = next(
        profile
        for profile in manifest["evidence_profiles"].values()
        if any(not policy["applicable"] for policy in profile["dimensions"].values())
    )
    dimension = next(name for name, policy in profile["dimensions"].items() if not policy["applicable"])
    profile["dimensions"][dimension].pop("rationale")

    with pytest.raises(AssertionError):
        _validate_manifest(manifest, _actual_exports())


def test_manifest_rejects_non_exported_symbol_reference() -> None:
    manifest = copy.deepcopy(_load_manifest())
    manifest["export_groups"][0]["surfaces"]["python"].append("NotAnExport")

    with pytest.raises(AssertionError):
        _validate_manifest(manifest, _actual_exports())
