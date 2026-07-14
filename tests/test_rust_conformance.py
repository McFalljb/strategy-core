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
from tests.conformance_cases import (
    CORE_DIRECT_MODELS,
    CORE_FIXTURE_NAMES,
    build_core_fixtures,
    direct_model_dimensions,
    load_core_fixture,
    python_invalid_category,
    python_round_trip_valid_case,
)
from tests.external_conformance_cases import (
    EXTERNAL_DIRECT_MODELS,
    EXTERNAL_FIXTURE_NAMES,
    build_external_fixtures,
    evaluate_python_helper_case,
    load_external_fixture,
    python_external_invalid_category,
    python_external_round_trip_valid_case,
)

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
    for adjudication in manifest.get("mismatch_adjudications", []):
        assert adjudication["classification"] in {
            "python-canonical",
            "coordinated-change",
            "intentional-divergence",
        }
        assert adjudication["canonical_behavior"].strip()
        assert adjudication["characterization"].strip()
        assert adjudication["compatibility_impact"].strip()

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


def test_core_conformance_fixtures_match_python_contract_objects() -> None:
    expected = build_core_fixtures()

    assert set(expected) == set(CORE_FIXTURE_NAMES)
    for name in CORE_FIXTURE_NAMES:
        assert load_core_fixture(name) == expected[name]


@pytest.mark.parametrize(
    ("family", "case"),
    [(family, case) for family, document in build_core_fixtures().items() for case in document["valid"]],
    ids=lambda value: value if isinstance(value, str) else cast("str", value["id"]),
)
def test_python_authored_core_values_round_trip_structurally(family: str, case: dict[str, Any]) -> None:
    assert family in CORE_FIXTURE_NAMES
    assert python_round_trip_valid_case(case) == case["expected"], case["id"]


def _validate_core_fixture_coverage(manifest: dict[str, Any], fixtures: dict[str, dict[str, Any]]) -> None:
    partition = manifest["fixture_partitions"]["core"]
    assert set(partition["files"]) == set(CORE_FIXTURE_NAMES)
    cases = [case for document in fixtures.values() for kind in ("valid", "invalid") for case in document[kind]]
    _validate_case_evidence(manifest, partition, cases, coverage_key="covers")
    covered = {surface for document in fixtures.values() for case in document["valid"] for surface in case["covers"]}
    assert covered == set(partition["surfaces"]), {
        "missing": sorted(set(partition["surfaces"]) - covered),
        "unknown": sorted(covered - set(partition["surfaces"])),
    }
    for name, document in fixtures.items():
        assert document["family"] == name
        assert document["valid"], name
        assert document["invalid"], name
        assert all(
            case["category"] in {"required_field", "type", "enum", "range", "format"} for case in document["invalid"]
        )
    assert {case["wire"]["type"] for case in fixtures["events"]["valid"] if case["rust_type"] == "StrategyEvent"} == {
        "observation",
        "price_update",
        "forecast_updated",
        "forecast_versions",
        "oracle_scores_updated",
        "station_report",
        "weather_event",
        "new_high",
        "new_low",
        "timer_wake",
        "shutdown",
    }


def test_manifest_core_partition_has_complete_fixture_evidence() -> None:
    _validate_core_fixture_coverage(_load_manifest(), build_core_fixtures())


def test_manifest_core_partition_rejects_missing_fixture_evidence() -> None:
    fixtures = build_core_fixtures()
    surface = fixtures["events"]["valid"][0]["covers"][0]
    fixtures["events"]["valid"][0]["evidence"].pop(surface)

    with pytest.raises(AssertionError):
        _validate_core_fixture_coverage(_load_manifest(), fixtures)


def test_manifest_core_partition_rejects_missing_dimension_evidence() -> None:
    fixtures = build_core_fixtures()
    for document in fixtures.values():
        for kind in ("valid", "invalid"):
            for case in document[kind]:
                case["evidence_dimensions"] = [
                    dimension for dimension in case["evidence_dimensions"] if dimension != "explicit_null"
                ]
                case["evidence"] = {
                    surface: [dimension for dimension in dimensions if dimension != "explicit_null"]
                    for surface, dimensions in case["evidence"].items()
                }

    with pytest.raises(AssertionError):
        _validate_core_fixture_coverage(_load_manifest(), fixtures)


def test_default_only_cases_do_not_claim_non_default_round_trip() -> None:
    cases = [case for document in build_core_fixtures().values() for case in document["valid"] if case["wire"] == {}]
    assert cases
    assert all("non_default_round_trip" not in case["evidence_dimensions"] for case in cases)


@pytest.mark.parametrize(
    ("family", "case"),
    [(family, case) for family, document in build_core_fixtures().items() for case in document["invalid"]],
    ids=lambda value: value if isinstance(value, str) else cast("str", value["id"]),
)
def test_python_rejects_declared_invalid_core_wire_cases_with_normalized_category(
    family: str, case: dict[str, Any]
) -> None:
    assert family in CORE_FIXTURE_NAMES
    assert python_invalid_category(case) == case["category"], case["id"]


def test_manifest_rejects_non_exported_symbol_reference() -> None:
    manifest = copy.deepcopy(_load_manifest())
    manifest["export_groups"][0]["surfaces"]["python"].append("NotAnExport")

    with pytest.raises(AssertionError):
        _validate_manifest(manifest, _actual_exports())


def test_external_conformance_fixtures_match_python_contract_objects() -> None:
    expected = build_external_fixtures()

    assert set(expected) == set(EXTERNAL_FIXTURE_NAMES)
    for name in EXTERNAL_FIXTURE_NAMES:
        assert load_external_fixture(name) == expected[name]


def _covered_external_surfaces(document: dict[str, Any]) -> set[str]:
    covered: set[str] = set()
    for case in document["valid"]:
        expected = case["expected"]
        for surface, path in case["coverage_paths"].items():
            value = expected
            for component in path:
                value = value[component] if isinstance(component, int) else value.get(component)
            assert value is not None, (case["id"], surface, path)
            if isinstance(value, (list, dict)):
                assert value, (case["id"], surface, path)
            covered.add(surface)
    return covered


def _applicable_dimensions(manifest: dict[str, Any], partition: dict[str, Any]) -> set[str]:
    profile = manifest["evidence_profiles"][partition["evidence_profile"]]
    return {name for name, policy in profile["dimensions"].items() if policy["applicable"]}


def _validate_case_evidence(
    manifest: dict[str, Any],
    partition: dict[str, Any],
    cases: list[dict[str, Any]],
    *,
    coverage_key: str,
) -> None:
    applicable = _applicable_dimensions(manifest, partition)
    linked_dimensions: set[str] = set()
    linked_by_surface: dict[str, set[str]] = {}
    for case in cases:
        case_dimensions = set(case["evidence_dimensions"])
        evidence = case["evidence"]
        covered_surfaces = case.get(coverage_key, case.get("covers"))
        assert case_dimensions
        assert covered_surfaces is not None, case["id"]
        assert set(evidence) == set(covered_surfaces), case["id"]
        assert set().union(*(set(dimensions) for dimensions in evidence.values())) == case_dimensions, case["id"]
        assert case_dimensions <= applicable, case["id"]
        if "category" in case:
            assert case_dimensions == {"invalid_input"}, case["id"]
        elif "helper" not in case:
            assert "invalid_input" not in case_dimensions, case["id"]
        linked_dimensions.update(case_dimensions)
        for surface, dimensions in evidence.items():
            linked_by_surface.setdefault(surface, set()).update(dimensions)

    declared_by_surface: dict[str, set[str]] = {}
    for group in partition["surface_dimension_groups"]:
        dimensions = set(group["dimensions"])
        assert dimensions <= applicable
        for surface in group["surfaces"]:
            assert surface not in declared_by_surface, surface
            declared_by_surface[surface] = dimensions
    assert set(declared_by_surface) == set(partition["surfaces"])

    required_by_surface: dict[str, set[str]] = {surface: set() for surface in partition["surfaces"]}
    required_pairs: set[tuple[str, str]] = set()
    for requirement in partition["surface_dimension_requirements"]:
        dimension = requirement["dimension"]
        assert dimension in applicable
        surfaces = set(requirement["surfaces"])
        assert surfaces
        assert surfaces <= set(required_by_surface)
        rationale = requirement["rationale"]
        assert isinstance(rationale, str) and rationale.strip()
        for surface in surfaces:
            pair = (surface, dimension)
            assert pair not in required_pairs, pair
            required_pairs.add(pair)
            required_by_surface[surface].add(dimension)
    assert all(required_by_surface.values())
    assert declared_by_surface == required_by_surface

    required_exclusions = {
        (surface, dimension)
        for surface, dimensions in required_by_surface.items()
        for dimension in applicable - dimensions
    }
    resolved_exclusions: dict[tuple[str, str], str] = {}
    for group in partition["surface_dimension_exclusion_groups"]:
        dimensions = set(group["dimensions"])
        assert dimensions <= applicable
        rationale = group["rationale"]
        assert isinstance(rationale, str) and rationale.strip()
        assert not {"vector", "corpus", "test"} & set(rationale.lower().split())
        surfaces = set(group["surfaces"])
        assert surfaces
        assert surfaces <= set(declared_by_surface)
        pairs = {(surface, dimension) for surface in surfaces for dimension in dimensions}
        for pair in pairs:
            assert pair in required_exclusions, pair
            assert pair not in resolved_exclusions, pair
            resolved_exclusions[pair] = rationale
    assert set(resolved_exclusions) == required_exclusions

    assert linked_by_surface == declared_by_surface, {
        surface: {
            "missing": sorted(declared_by_surface.get(surface, set()) - linked_by_surface.get(surface, set())),
            "unknown": sorted(linked_by_surface.get(surface, set()) - declared_by_surface.get(surface, set())),
        }
        for surface in set(linked_by_surface) | set(declared_by_surface)
        if linked_by_surface.get(surface, set()) != declared_by_surface.get(surface, set())
    }
    assert linked_dimensions == applicable, {
        "missing": sorted(applicable - linked_dimensions),
        "unknown": sorted(linked_dimensions - applicable),
    }


def _validate_external_fixture_coverage(manifest: dict[str, Any], fixtures: dict[str, dict[str, Any]]) -> None:
    external = manifest["fixture_partitions"]["external"]
    helpers = manifest["fixture_partitions"]["helpers"]
    assert set(external["files"]) == {"minutetemp", "kalshi", "http-data"}
    assert set(helpers["files"]) == {"helpers"}

    external_cases = [
        case for name in external["files"] for kind in ("valid", "invalid") for case in fixtures[name][kind]
    ]
    _validate_case_evidence(manifest, external, external_cases, coverage_key="coverage_paths")
    _validate_case_evidence(manifest, helpers, fixtures["helpers"]["cases"], coverage_key="covers")

    covered = set().union(*(_covered_external_surfaces(fixtures[name]) for name in external["files"]))
    assert covered == set(external["surfaces"]), {
        "missing": sorted(set(external["surfaces"]) - covered),
        "unknown": sorted(covered - set(external["surfaces"])),
    }

    helper_surfaces = {surface for case in fixtures["helpers"]["cases"] for surface in case["covers"]}
    assert helper_surfaces == set(helpers["surfaces"]), {
        "missing": sorted(set(helpers["surfaces"]) - helper_surfaces),
        "unknown": sorted(helper_surfaces - set(helpers["surfaces"])),
    }

    serializable = next(group for group in manifest["export_groups"] if group["id"] == "shared-serializable-contract")
    shared_serializable = set(serializable["surfaces"]["python"])
    assert (
        (set(manifest["fixture_partitions"]["core"]["surfaces"]) & shared_serializable)
        | set(external["surfaces"])
        | (set(helpers["surfaces"]) & shared_serializable)
    ) == shared_serializable


def test_manifest_external_partitions_have_complete_evidence() -> None:
    _validate_external_fixture_coverage(_load_manifest(), build_external_fixtures())


def test_manifest_rejects_claimed_nested_surface_without_evidence() -> None:
    fixtures = build_external_fixtures()
    case = next(case for case in fixtures["kalshi"]["valid"] if len(case["coverage_paths"]) > 1)
    nested_surface = next(iter(set(case["coverage_paths"]) - {case["rust_type"]}))
    case["coverage_paths"][nested_surface] = ["missing"]

    with pytest.raises(AssertionError):
        _validate_external_fixture_coverage(_load_manifest(), fixtures)


def test_manifest_rejects_nested_surface_missing_declared_dimension() -> None:
    fixtures = build_external_fixtures()
    for case in fixtures["minutetemp"]["valid"]:
        if "CityInfo" not in case["evidence"]:
            continue
        case["evidence"]["CityInfo"] = [
            dimension for dimension in case["evidence"]["CityInfo"] if dimension != "defaults"
        ]
        case["evidence_dimensions"] = sorted(
            set().union(*(set(dimensions) for dimensions in case["evidence"].values()))
        )

    with pytest.raises(AssertionError):
        _validate_external_fixture_coverage(_load_manifest(), fixtures)


def test_manifest_rejects_empty_surface_dimension_exclusion_reason() -> None:
    manifest = copy.deepcopy(_load_manifest())
    manifest["fixture_partitions"]["core"]["surface_dimension_exclusion_groups"][0]["rationale"] = ""

    with pytest.raises(AssertionError):
        _validate_core_fixture_coverage(manifest, build_core_fixtures())


def test_manifest_rejects_missing_surface_dimension_exclusion() -> None:
    manifest = copy.deepcopy(_load_manifest())
    groups = manifest["fixture_partitions"]["external"]["surface_dimension_exclusion_groups"]
    groups[:] = [group for group in groups if "defaults" not in group["dimensions"]]

    with pytest.raises(AssertionError):
        _validate_external_fixture_coverage(manifest, build_external_fixtures())


def test_manifest_uses_only_explicit_surface_dimension_exclusions() -> None:
    manifest = _load_manifest()
    for partition in manifest["fixture_partitions"].values():
        for exclusion in partition.get("surface_dimension_exclusion_groups", []):
            assert isinstance(exclusion["surfaces"], list)
            assert exclusion["surfaces"]


def test_manifest_rejects_semantic_obligation_and_evidence_removed_together() -> None:
    manifest = copy.deepcopy(_load_manifest())
    partition = manifest["fixture_partitions"]["core"]
    surface = "StationWeather"
    dimension = "numeric_boundaries"

    declaration = next(group for group in partition["surface_dimension_groups"] if surface in group["surfaces"])
    declaration["surfaces"].remove(surface)
    partition["surface_dimension_groups"].append(
        {
            "surfaces": [surface],
            "dimensions": [item for item in declaration["dimensions"] if item != dimension],
        }
    )
    requirement = next(item for item in partition["surface_dimension_requirements"] if item["dimension"] == dimension)
    requirement["surfaces"].remove(surface)

    fixtures = build_core_fixtures()
    for document in fixtures.values():
        for kind in ("valid", "invalid"):
            for case in document[kind]:
                if surface not in case["evidence"]:
                    continue
                case["evidence"][surface] = [item for item in case["evidence"][surface] if item != dimension]
                case["evidence_dimensions"] = sorted(
                    set().union(*(set(dimensions) for dimensions in case["evidence"].values()))
                )

    with pytest.raises(AssertionError):
        _validate_core_fixture_coverage(manifest, fixtures)


def test_manifest_rejects_evidence_claim_removed_from_semantic_requirements() -> None:
    manifest = copy.deepcopy(_load_manifest())
    observation = next(
        group
        for group in manifest["fixture_partitions"]["core"]["surface_dimension_groups"]
        if "Observation" in group["surfaces"]
    )
    observation["dimensions"].remove("defaults")

    with pytest.raises(AssertionError):
        _validate_core_fixture_coverage(manifest, build_core_fixtures())


def test_manifest_semantic_requirements_cover_direct_models_and_closed_enums() -> None:
    manifest = _load_manifest()

    def requirements(partition_name: str) -> dict[str, set[str]]:
        partition = manifest["fixture_partitions"][partition_name]
        result: dict[str, set[str]] = {surface: set() for surface in partition["surfaces"]}
        for requirement in partition["surface_dimension_requirements"]:
            for surface in requirement["surfaces"]:
                result[surface].add(requirement["dimension"])
        return result

    core = requirements("core")
    for surface, model in CORE_DIRECT_MODELS.items():
        assert direct_model_dimensions(model) <= core[surface], surface
    assert {
        "defaults",
        "explicit_null",
        "invalid_input",
        "non_default_round_trip",
        "numeric_boundaries",
        "omission",
        "timestamp_formatting",
    } <= core["Observation"]
    for surface in ("OracleScoreMode", "PersistenceStatus", "TemperatureDayMode", "WuDayMode"):
        assert {"enum_values", "invalid_input", "non_default_round_trip"} <= core[surface]

    external = requirements("external")
    for surface, model in EXTERNAL_DIRECT_MODELS.items():
        assert direct_model_dimensions(model) <= external[surface], surface
    assert {"defaults", "non_default_round_trip", "omission"} <= external["CityInfo"]
    for surface in (
        "DataResolution",
        "OracleRankBy",
        "PlanTier",
        "ReportType",
        "TemperatureUnit",
        "KalshiMarketSide",
        "KalshiOrderAction",
    ):
        assert {"enum_values", "invalid_input", "non_default_round_trip"} <= external[surface]


def test_manifest_rejects_helper_dimension_not_allowed_by_profile() -> None:
    fixtures = build_external_fixtures()
    case = fixtures["helpers"]["cases"][0]
    case["evidence_dimensions"].append("defaults")
    for dimensions in case["evidence"].values():
        dimensions.append("defaults")

    with pytest.raises(AssertionError):
        _validate_external_fixture_coverage(_load_manifest(), fixtures)


def test_helper_numeric_boundary_evidence_matches_vector_semantics() -> None:
    cases = {case["id"]: case for case in build_external_fixtures()["helpers"]["cases"]}
    assert "numeric_boundaries" not in cases["fee-taker-default"]["evidence_dimensions"]
    assert "numeric_boundaries" not in cases["fee-unknown-type"]["evidence_dimensions"]
    assert "numeric_boundaries" in cases["fee-rounding-boundary"]["evidence_dimensions"]
    assert "numeric_boundaries" in cases["fee-negative-quantity"]["evidence_dimensions"]
    assert "numeric_boundaries" in cases["fee-negative-multiplier"]["evidence_dimensions"]
    assert "numeric_boundaries" in cases["fee-rounding-signed"]["evidence_dimensions"]
    assert "numeric_boundaries" in cases["fee-positive-infinity"]["evidence_dimensions"]


def test_helper_invalid_literal_evidence_matches_vector_semantics() -> None:
    cases = {case["id"]: case for case in build_external_fixtures()["helpers"]["cases"]}
    assert cases["fee-unknown-liquidity-role"]["expected"] == {"error": "unknown_liquidity_role"}
    assert cases["fill-fee-unknown-action"]["expected"] == {"error": "unknown_action"}
    assert "invalid_input" in cases["fee-unknown-liquidity-role"]["evidence_dimensions"]
    assert "invalid_input" in cases["fill-fee-unknown-action"]["evidence_dimensions"]


@pytest.mark.parametrize(
    ("family", "case"),
    [
        (family, case)
        for family, document in build_external_fixtures().items()
        if family != "helpers"
        for case in document["valid"]
    ],
    ids=lambda value: value if isinstance(value, str) else cast("str", value["id"]),
)
def test_python_authored_external_values_round_trip_structurally(family: str, case: dict[str, Any]) -> None:
    assert family in EXTERNAL_FIXTURE_NAMES
    assert python_external_round_trip_valid_case(case) == case["expected"], case["id"]


@pytest.mark.parametrize(
    ("family", "case"),
    [
        (family, case)
        for family, document in build_external_fixtures().items()
        if family != "helpers"
        for case in document["invalid"]
    ],
    ids=lambda value: value if isinstance(value, str) else cast("str", value["id"]),
)
def test_python_rejects_declared_invalid_external_cases(family: str, case: dict[str, Any]) -> None:
    assert family in EXTERNAL_FIXTURE_NAMES
    assert python_external_invalid_category(case) == case["category"], case["id"]


@pytest.mark.parametrize(
    "case",
    build_external_fixtures()["helpers"]["cases"],
    ids=lambda case: cast("str", case["id"]),
)
def test_python_helper_vectors_match_canonical_results(case: dict[str, Any]) -> None:
    assert evaluate_python_helper_case(case) == case["expected"], case["id"]
