"""Versioned bounds and scalar semantics for the V3 Strategy contract."""

from __future__ import annotations

import re
import unicodedata
from dataclasses import dataclass

CANONICAL_PROFILE = "strategy-core-canonical-v1"
CANONICAL_PROFILE_VERSION = 1
MAX_CANONICAL_BYTES = 1_048_576
MAX_CANONICAL_NESTING = 64
MAX_IDENTIFIER_BYTES = 128
MAX_REASON_CODE_BYTES = 64
MAX_INTENTS = 64
MAX_TIMER_REQUESTS = 64
MAX_EVIDENCE = 128
MAX_DIAGNOSTICS = 64
MAX_DIAGNOSTIC_MESSAGE_BYTES = 512
MAX_TIMER_SEMANTICS_BYTES = 4096
MAX_DECIMAL_SCALE = 18
MAX_DECIMAL_DIGITS = 38

_REASON_CODE = re.compile(r"[a-z][a-z0-9_]{0,63}\Z")
_DECIMAL = re.compile(r"(-?)(0|[1-9][0-9]*)(?:\.([0-9]+))?\Z")


class CanonicalError(ValueError):
    """A stable canonical-profile validation failure."""

    def __init__(self, category: str, message: str) -> None:
        self.category = category
        super().__init__(f"{category}: {message}")


class ReasonCode(str):
    """A deterministic, language-neutral reason code."""

    def __new__(cls, value: str) -> ReasonCode:
        if not _REASON_CODE.fullmatch(value) or len(value.encode("utf-8")) > MAX_REASON_CODE_BYTES:
            raise ValueError("invalid_reason_code")
        return super().__new__(cls, value)


class TimerKey(str):
    """Stable Strategy-owned timer meaning; scheduling identity remains Trader-owned."""

    def __new__(cls, value: str) -> TimerKey:
        normalized = unicodedata.normalize("NFC", value)
        if (
            not value
            or normalized != value
            or len(value.encode("utf-8")) > MAX_IDENTIFIER_BYTES
            or value.strip() != value
        ):
            raise ValueError("invalid_timer_key")
        return super().__new__(cls, value)


@dataclass(frozen=True, slots=True)
class EpochNanoseconds:
    """A persistent UTC instant represented as signed epoch nanoseconds."""

    value: int

    def __post_init__(self) -> None:
        if isinstance(self.value, bool) or not -(2**63) <= self.value < 2**63:
            raise CanonicalError("invalid_time", "epoch nanoseconds must fit signed 64-bit")


@dataclass(frozen=True, slots=True)
class FixedDecimal:
    """A canonical fixed-scale decimal string."""

    value: str
    scale: int

    @classmethod
    def parse(cls, value: str, scale: int) -> FixedDecimal:
        if isinstance(scale, bool) or not 0 <= scale <= MAX_DECIMAL_SCALE:
            raise CanonicalError("invalid_decimal", "scale is outside the profile")
        match = _DECIMAL.fullmatch(value)
        if match is None:
            raise CanonicalError("invalid_decimal", "value is not a canonical decimal")
        sign, whole, fraction = match.groups()
        fraction = fraction or ""
        if len(fraction) != scale:
            raise CanonicalError("invalid_decimal", "value does not have the declared scale")
        if len(whole) + len(fraction) > MAX_DECIMAL_DIGITS:
            raise CanonicalError("decimal_overflow", "value exceeds the digit bound")
        if sign and all(character == "0" for character in whole + fraction):
            raise CanonicalError("invalid_normalization", "negative zero is not canonical")
        return cls(value=value, scale=scale)


def bounded_text(value: str, *, name: str, maximum: int = MAX_IDENTIFIER_BYTES) -> str:
    if not value or unicodedata.normalize("NFC", value) != value or len(value.encode("utf-8")) > maximum:
        raise ValueError(f"invalid_{name}")
    return value
