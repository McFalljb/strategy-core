"""Immutable, bounded V3 Strategy decision context values."""

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum

from .profile import MAX_CANONICAL_BYTES, MAX_EVIDENCE, EpochNanoseconds, TimerKey, bounded_text


class TriggerKind(StrEnum):
    BOOTSTRAP = "bootstrap"
    RECOVERY = "recovery"
    WEATHER = "weather"
    TIMER = "timer"


@dataclass(frozen=True, slots=True)
class DecisionTrigger:
    kind: TriggerKind
    occurred_at: EpochNanoseconds
    timer_key: TimerKey | None = None

    def __post_init__(self) -> None:
        if (self.kind is TriggerKind.TIMER) != (self.timer_key is not None):
            raise ValueError("invalid_trigger_timer_key")


@dataclass(frozen=True, slots=True)
class SourceEvidence:
    kind: str
    reference: str
    payload_sha256: str

    def __post_init__(self) -> None:
        bounded_text(self.kind, name="evidence_kind")
        bounded_text(self.reference, name="evidence_reference")
        invalid_character = any(character not in "0123456789abcdef" for character in self.payload_sha256)
        if len(self.payload_sha256) != 64 or invalid_character:
            raise ValueError("invalid_evidence_digest")


@dataclass(frozen=True, slots=True)
class BoundedContextPayload:
    """Complete canonical snapshot bytes supplied in the decision envelope."""

    canonical_bytes: bytes

    def __post_init__(self) -> None:
        object.__setattr__(self, "canonical_bytes", bytes(self.canonical_bytes))
        if len(self.canonical_bytes) > MAX_CANONICAL_BYTES:
            raise ValueError("context_payload_too_large")


@dataclass(frozen=True, slots=True)
class DecisionContextV3:
    delivery_id: str
    sleeve_identity: str
    state_fence: str
    trigger: DecisionTrigger
    source_evidence: tuple[SourceEvidence, ...]
    weather: BoundedContextPayload
    opportunity: BoundedContextPayload
    markets: BoundedContextPayload
    broker: BoundedContextPayload
    authorization: BoundedContextPayload
    delivered_at_monotonic_ns: int
    hard_expires_at_monotonic_ns: int

    def __post_init__(self) -> None:
        object.__setattr__(self, "source_evidence", tuple(self.source_evidence))
        bounded_text(self.delivery_id, name="delivery_id")
        bounded_text(self.sleeve_identity, name="sleeve_identity")
        bounded_text(self.state_fence, name="state_fence")
        if len(self.source_evidence) > MAX_EVIDENCE:
            raise ValueError("too_many_source_evidence_items")
        if not 0 <= self.delivered_at_monotonic_ns < 2**64:
            raise ValueError("invalid_delivered_at_monotonic_ns")
        if not self.delivered_at_monotonic_ns <= self.hard_expires_at_monotonic_ns < 2**64:
            raise ValueError("invalid_hard_expiry")
