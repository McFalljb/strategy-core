from __future__ import annotations

import ast
import hashlib
import json
from dataclasses import FrozenInstanceError
from pathlib import Path
from typing import Any, cast

import pytest

from strategy_core_v3 import (
    MAX_CANONICAL_BYTES,
    MAX_CANONICAL_NESTING,
    MAX_DIAGNOSTIC_MESSAGE_BYTES,
    MAX_TIMER_REQUESTS,
    BoundedContextPayload,
    CancelTimerRequest,
    CanonicalError,
    ContractSide,
    DecisionContextV3,
    DecisionOutcome,
    DecisionResultV3,
    DecisionTrigger,
    Diagnostic,
    DiagnosticSeverity,
    EpochNanoseconds,
    FixedDecimal,
    OrderAction,
    ReasonCode,
    ScheduleTimerRequest,
    SourceEvidence,
    StrategyEvidence,
    StrategyOrderIntent,
    TimerKey,
    TriggerKind,
    calculate_result_profile,
    canonical_bytes,
    canonical_sha256,
)

ROOT = Path(__file__).parents[2]
VECTORS = ROOT / "conformance" / "v3" / "vectors.json"
ALLOWED_PYTHON_IMPORTS = {
    "__future__",
    "collections",
    "dataclasses",
    "enum",
    "hashlib",
    "re",
    "unicodedata",
}
ALLOWED_RUST_DEPENDENCIES = {"sha2", "unicode-normalization"}
ALLOWED_RUST_DEV_DEPENDENCIES = {"serde_json"}


def _node(value: Any) -> Any:
    if not isinstance(value, dict) or len(value) != 1:
        raise AssertionError(f"invalid vector node: {value!r}")
    kind, payload = next(iter(value.items()))
    if kind == "null":
        return None
    if kind in {"bool", "string", "bytes"}:
        return bytes.fromhex(payload) if kind == "bytes" else payload
    if kind == "integer":
        return int(payload)
    if kind == "decimal":
        return FixedDecimal.parse(payload["value"], payload["scale"])
    if kind == "epoch_ns":
        return EpochNanoseconds(payload)
    if kind == "list":
        return [_node(item) for item in payload]
    if kind == "map":
        return {key: _node(item) for key, item in payload}
    raise AssertionError(f"unknown vector node kind: {kind}")


def _load_vectors() -> dict[str, Any]:
    return cast("dict[str, Any]", json.loads(VECTORS.read_text()))


def test_shared_valid_vectors_match_canonical_bytes_and_digests() -> None:
    for vector in _load_vectors()["valid"]:
        encoded = canonical_bytes(vector["domain"], _node(vector["value"]))
        assert encoded.hex() == vector["expected_hex"], vector["id"]
        assert canonical_sha256(vector["domain"], _node(vector["value"])) == vector["expected_sha256"], vector["id"]


def test_shared_invalid_vectors_fail_with_normalized_category() -> None:
    for vector in _load_vectors()["invalid"]:
        with pytest.raises(CanonicalError) as error:
            canonical_bytes(vector["domain"], _node(vector["value"]))
        assert error.value.category == vector["category"], vector["id"]


def test_canonical_encoding_rejects_size_and_nesting_overflow_during_encoding() -> None:
    with pytest.raises(CanonicalError, match="canonical_overflow"):
        canonical_bytes("strategy.bytes", b"x" * MAX_CANONICAL_BYTES)

    value: Any = None
    for _ in range(MAX_CANONICAL_NESTING):
        value = [value]
    canonical_bytes("strategy.nesting", value)
    with pytest.raises(CanonicalError, match="canonical_nesting_overflow"):
        canonical_bytes("strategy.nesting", [value])


def test_timer_requests_carry_only_stable_strategy_meaning() -> None:
    key = TimerKey("weather.recheck")
    scheduled = ScheduleTimerRequest(
        key=key,
        scheduled_at=EpochNanoseconds(1_800_000_000_000_000_000),
        semantics_version=1,
        semantics=b"re-evaluate forecast",
    )
    cancelled = CancelTimerRequest(key=key)

    assert scheduled.key == cancelled.key
    assert not hasattr(scheduled, "generation")
    assert not hasattr(scheduled, "delivery_id")
    with pytest.raises(FrozenInstanceError):
        scheduled.semantics_version = 2  # type: ignore[misc]


def test_context_result_intent_evidence_and_profile_are_bounded_values() -> None:
    empty_payload = BoundedContextPayload(canonical_bytes("strategy.context", {}))
    context = DecisionContextV3(
        delivery_id="delivery-1",
        sleeve_identity="sleeve-1",
        state_fence="fence-1",
        trigger=DecisionTrigger(TriggerKind.WEATHER, EpochNanoseconds(1)),
        source_evidence=(SourceEvidence("weather", "capture-1", "a" * 64),),
        weather=empty_payload,
        opportunity=empty_payload,
        markets=empty_payload,
        broker=empty_payload,
        authorization=empty_payload,
        delivered_at_monotonic_ns=10,
        hard_expires_at_monotonic_ns=20,
    )
    intent = StrategyOrderIntent(
        market_id="market-1",
        action=OrderAction.BUY,
        side=ContractSide.YES,
        quantity=2,
        limit_price=FixedDecimal.parse("0.42", 2),
        reduce_only=False,
        reason_code=ReasonCode("forecast_threshold_met"),
    )
    result = DecisionResultV3(
        delivery_id=context.delivery_id,
        sleeve_identity=context.sleeve_identity,
        state_fence=context.state_fence,
        outcome=DecisionOutcome.COMPLETED,
        intents=(intent,),
        evidence=(StrategyEvidence(ReasonCode("forecast_used"), b"capture-1"),),
    )

    assert calculate_result_profile(result).intent_count == 1
    with pytest.raises(ValueError, match="too_many_timer_requests"):
        DecisionResultV3(
            delivery_id="delivery-1",
            sleeve_identity="sleeve-1",
            state_fence="fence-1",
            outcome=DecisionOutcome.NO_ACTION,
            timer_requests=tuple(
                CancelTimerRequest(TimerKey(f"timer-{index}")) for index in range(MAX_TIMER_REQUESTS + 1)
            ),
        )


def test_mutable_inputs_are_normalized_to_immutable_owned_values() -> None:
    payload_source = bytearray(b"payload")
    payload = BoundedContextPayload(payload_source)  # type: ignore[arg-type]
    evidence_source = [SourceEvidence("weather", "capture-1", "a" * 64)]
    context = DecisionContextV3(
        delivery_id="delivery-1",
        sleeve_identity="sleeve-1",
        state_fence="fence-1",
        trigger=DecisionTrigger(TriggerKind.WEATHER, EpochNanoseconds(1)),
        source_evidence=evidence_source,  # type: ignore[arg-type]
        weather=payload,
        opportunity=payload,
        markets=payload,
        broker=payload,
        authorization=payload,
        delivered_at_monotonic_ns=1,
        hard_expires_at_monotonic_ns=2,
    )
    semantics_source = bytearray(b"meaning")
    request = ScheduleTimerRequest(
        TimerKey("weather.recheck"),
        EpochNanoseconds(2),
        1,
        semantics_source,  # type: ignore[arg-type]
    )
    requests_source = [request]
    result = DecisionResultV3(
        delivery_id="delivery-1",
        sleeve_identity="sleeve-1",
        state_fence="fence-1",
        outcome=DecisionOutcome.NO_ACTION,
        timer_requests=requests_source,  # type: ignore[arg-type]
    )

    payload_source[0] = ord("X")
    evidence_source.clear()
    semantics_source[0] = ord("X")
    requests_source.clear()
    assert payload.canonical_bytes == b"payload"
    assert len(context.source_evidence) == 1
    assert request.semantics == b"meaning"
    assert len(result.timer_requests) == 1
    assert isinstance(context.source_evidence, tuple)
    assert isinstance(result.timer_requests, tuple)


def test_reason_codes_and_diagnostics_have_executable_bounds() -> None:
    reason = ReasonCode("forecast_threshold_met")
    diagnostic = Diagnostic(
        severity=DiagnosticSeverity.INFO,
        code=ReasonCode("profile_selected"),
        message="x" * MAX_DIAGNOSTIC_MESSAGE_BYTES,
    )

    assert str(reason) == "forecast_threshold_met"
    assert len(diagnostic.message.encode()) == MAX_DIAGNOSTIC_MESSAGE_BYTES
    with pytest.raises(ValueError, match="diagnostic_message_too_long"):
        Diagnostic(
            severity=DiagnosticSeverity.ERROR,
            code=ReasonCode("invalid_input"),
            message="x" * (MAX_DIAGNOSTIC_MESSAGE_BYTES + 1),
        )


@pytest.mark.parametrize("invalid", ["UPPER", "contains space", "", "a" * 65])
def test_reason_codes_reject_noncanonical_values(invalid: str) -> None:
    with pytest.raises(ValueError, match="invalid_reason_code"):
        ReasonCode(invalid)


def _unexpected_python_imports(source: str) -> set[str]:
    tree = ast.parse(source)
    imported_roots = {
        alias.name.split(".", 1)[0] for node in ast.walk(tree) if isinstance(node, ast.Import) for alias in node.names
    }
    imported_roots.update(
        node.module.split(".", 1)[0]
        for node in ast.walk(tree)
        if isinstance(node, ast.ImportFrom) and node.level == 0 and node.module
    )
    return imported_roots - ALLOWED_PYTHON_IMPORTS


def _cargo_dependencies(manifest: str, section: str) -> set[str]:
    body = manifest.split(f"[{section}]", 1)[1]
    body = body.split("\n[", 1)[0]
    return {
        line.split("=", 1)[0].strip()
        for line in body.splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    }


def test_v3_python_and_rust_packages_use_exact_dependency_allowlists() -> None:
    for path in (ROOT / "strategy_core_v3").glob("*.py"):
        assert not _unexpected_python_imports(path.read_text()), path

    cargo = (ROOT / "native" / "strategy_core_v3" / "Cargo.toml").read_text()
    assert _cargo_dependencies(cargo, "dependencies") == ALLOWED_RUST_DEPENDENCIES
    assert _cargo_dependencies(cargo, "dev-dependencies") == ALLOWED_RUST_DEV_DEPENDENCIES


def test_dependency_policy_rejects_realistic_runtime_and_network_mutations() -> None:
    assert _unexpected_python_imports("import requests\nfrom trader.runtime import Engine") == {"requests", "trader"}
    cargo = "[dependencies]\nsha2='0.10'\nunicode-normalization='0.1'\nreqwest='0.12'\ntokio='1'\n"
    assert _cargo_dependencies(cargo, "dependencies") - ALLOWED_RUST_DEPENDENCIES == {"reqwest", "tokio"}


def test_vector_manifest_pins_profile_and_corpus_digest() -> None:
    manifest = json.loads((ROOT / "conformance" / "v3" / "manifest.json").read_text())
    corpus = VECTORS.read_bytes()

    assert manifest["schema_version"] == 1
    assert manifest["canonical_profile"] == "strategy-core-canonical-v1"
    assert manifest["vectors_sha256"] == hashlib.sha256(corpus).hexdigest()
