"""Immutable, bounded V3 Strategy decision results."""

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum

from .profile import (
    MAX_CANONICAL_BYTES,
    MAX_DIAGNOSTIC_MESSAGE_BYTES,
    MAX_DIAGNOSTICS,
    MAX_EVIDENCE,
    MAX_INTENTS,
    MAX_TIMER_REQUESTS,
    MAX_TIMER_SEMANTICS_BYTES,
    EpochNanoseconds,
    FixedDecimal,
    ReasonCode,
    TimerKey,
    bounded_text,
)


class DecisionOutcome(StrEnum):
    COMPLETED = "completed"
    NO_ACTION = "no_action"
    REJECTED = "rejected"


class OrderAction(StrEnum):
    BUY = "buy"
    SELL = "sell"


class ContractSide(StrEnum):
    YES = "yes"
    NO = "no"


class DiagnosticSeverity(StrEnum):
    INFO = "info"
    WARNING = "warning"
    ERROR = "error"


@dataclass(frozen=True, slots=True)
class StrategyOrderIntent:
    market_id: str
    action: OrderAction
    side: ContractSide
    quantity: int
    limit_price: FixedDecimal | None
    reduce_only: bool
    reason_code: ReasonCode
    metadata: bytes = b""

    def __post_init__(self) -> None:
        object.__setattr__(self, "metadata", bytes(self.metadata))
        bounded_text(self.market_id, name="market_id")
        if isinstance(self.quantity, bool) or not 0 < self.quantity < 2**63:
            raise ValueError("invalid_order_quantity")
        if self.limit_price is not None:
            FixedDecimal.parse(self.limit_price.value, self.limit_price.scale)
        if len(self.metadata) > MAX_CANONICAL_BYTES:
            raise ValueError("order_metadata_too_large")


@dataclass(frozen=True, slots=True)
class ScheduleTimerRequest:
    key: TimerKey
    scheduled_at: EpochNanoseconds
    semantics_version: int
    semantics: bytes

    def __post_init__(self) -> None:
        object.__setattr__(self, "semantics", bytes(self.semantics))
        if isinstance(self.semantics_version, bool) or not 0 <= self.semantics_version < 2**32:
            raise ValueError("invalid_timer_semantics_version")
        if len(self.semantics) > MAX_TIMER_SEMANTICS_BYTES:
            raise ValueError("timer_semantics_too_large")


@dataclass(frozen=True, slots=True)
class CancelTimerRequest:
    key: TimerKey


TimerRequest = ScheduleTimerRequest | CancelTimerRequest


@dataclass(frozen=True, slots=True)
class StrategyEvidence:
    code: ReasonCode
    payload: bytes

    def __post_init__(self) -> None:
        object.__setattr__(self, "payload", bytes(self.payload))
        if len(self.payload) > MAX_CANONICAL_BYTES:
            raise ValueError("evidence_payload_too_large")


@dataclass(frozen=True, slots=True)
class Diagnostic:
    severity: DiagnosticSeverity
    code: ReasonCode
    message: str

    def __post_init__(self) -> None:
        if len(self.message.encode("utf-8")) > MAX_DIAGNOSTIC_MESSAGE_BYTES:
            raise ValueError("diagnostic_message_too_long")


@dataclass(frozen=True, slots=True)
class DecisionResultProfile:
    intent_count: int
    timer_request_count: int
    evidence_count: int
    diagnostic_count: int
    diagnostic_bytes: int


@dataclass(frozen=True, slots=True)
class DecisionResultV3:
    delivery_id: str
    sleeve_identity: str
    state_fence: str
    outcome: DecisionOutcome
    intents: tuple[StrategyOrderIntent, ...] = ()
    timer_requests: tuple[TimerRequest, ...] = ()
    evidence: tuple[StrategyEvidence, ...] = ()
    diagnostics: tuple[Diagnostic, ...] = ()

    def __post_init__(self) -> None:
        object.__setattr__(self, "intents", tuple(self.intents))
        object.__setattr__(self, "timer_requests", tuple(self.timer_requests))
        object.__setattr__(self, "evidence", tuple(self.evidence))
        object.__setattr__(self, "diagnostics", tuple(self.diagnostics))
        bounded_text(self.delivery_id, name="delivery_id")
        bounded_text(self.sleeve_identity, name="sleeve_identity")
        bounded_text(self.state_fence, name="state_fence")
        profile = calculate_result_profile(self)
        if profile.intent_count > MAX_INTENTS:
            raise ValueError("too_many_order_intents")
        if profile.timer_request_count > MAX_TIMER_REQUESTS:
            raise ValueError("too_many_timer_requests")
        if profile.evidence_count > MAX_EVIDENCE:
            raise ValueError("too_many_evidence_items")
        if profile.diagnostic_count > MAX_DIAGNOSTICS:
            raise ValueError("too_many_diagnostics")


def calculate_result_profile(result: DecisionResultV3) -> DecisionResultProfile:
    """Purely calculate the bounded semantic footprint of a result."""

    return DecisionResultProfile(
        intent_count=len(result.intents),
        timer_request_count=len(result.timer_requests),
        evidence_count=len(result.evidence),
        diagnostic_count=len(result.diagnostics),
        diagnostic_bytes=sum(len(item.message.encode("utf-8")) for item in result.diagnostics),
    )
