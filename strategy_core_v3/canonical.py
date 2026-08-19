"""Canonical strategy-semantic bytes for profile version 1."""

from __future__ import annotations

import hashlib
import unicodedata
from collections.abc import Mapping, Sequence

from .profile import (
    CANONICAL_PROFILE_VERSION,
    MAX_CANONICAL_BYTES,
    MAX_CANONICAL_NESTING,
    MAX_IDENTIFIER_BYTES,
    CanonicalError,
    EpochNanoseconds,
    FixedDecimal,
)

type CanonicalScalar = None | bool | int | str | bytes | FixedDecimal | EpochNanoseconds
type CanonicalValue = CanonicalScalar | Sequence[CanonicalValue] | Mapping[str, CanonicalValue]
_MAGIC = b"SCV3"
_MIN_I128 = -(2**127)
_MAX_I128 = 2**127 - 1


class _Encoder:
    def __init__(self) -> None:
        self.output = bytearray()

    def append(self, value: bytes) -> None:
        if len(value) > MAX_CANONICAL_BYTES - len(self.output):
            raise CanonicalError("canonical_overflow", "encoded value exceeds the profile bound")
        self.output.extend(value)

    def frame(self, tag: bytes, payload: bytes) -> None:
        if len(payload) >= 2**32:
            raise CanonicalError("canonical_overflow", "framed value is too large")
        self.append(tag)
        self.append(len(payload).to_bytes(4, "big"))
        self.append(payload)

    def begin_frame(self, tag: bytes) -> int:
        self.append(tag)
        length_offset = len(self.output)
        self.append(b"\0\0\0\0")
        return length_offset

    def end_frame(self, length_offset: int) -> None:
        payload_length = len(self.output) - length_offset - 4
        if payload_length >= 2**32:
            raise CanonicalError("canonical_overflow", "framed value is too large")
        self.output[length_offset : length_offset + 4] = payload_length.to_bytes(4, "big")


def _normalized_utf8(value: str) -> bytes:
    if len(value) > MAX_CANONICAL_BYTES:
        raise CanonicalError("canonical_overflow", "text exceeds the profile bound")
    normalized = unicodedata.normalize("NFC", value)
    encoded_length = sum(
        1 if ord(character) < 0x80 else 2 if ord(character) < 0x800 else 3 if ord(character) < 0x10000 else 4
        for character in normalized
    )
    if encoded_length > MAX_CANONICAL_BYTES:
        raise CanonicalError("canonical_overflow", "text exceeds the profile bound")
    return normalized.encode("utf-8")


def _encode(encoder: _Encoder, value: CanonicalValue, depth: int) -> None:
    if depth > MAX_CANONICAL_NESTING:
        raise CanonicalError("canonical_nesting_overflow", "value exceeds the nesting bound")
    if value is None:
        encoder.frame(b"n", b"")
    elif isinstance(value, bool):
        encoder.frame(b"b", b"1" if value else b"0")
    elif isinstance(value, EpochNanoseconds):
        encoder.frame(b"t", value.value.to_bytes(8, "big", signed=True))
    elif isinstance(value, FixedDecimal):
        validated = FixedDecimal.parse(value.value, value.scale)
        encoder.frame(b"d", bytes([validated.scale]) + validated.value.encode("ascii"))
    elif isinstance(value, int):
        if not _MIN_I128 <= value <= _MAX_I128:
            raise CanonicalError("integer_overflow", "integer must fit signed 128-bit")
        encoder.frame(b"i", str(value).encode("ascii"))
    elif isinstance(value, str):
        encoder.frame(b"s", _normalized_utf8(value))
    elif isinstance(value, bytes):
        encoder.frame(b"x", value)
    elif isinstance(value, Mapping):
        if len(value) > (MAX_CANONICAL_BYTES - 4) // 10:
            raise CanonicalError("canonical_overflow", "map has too many items")
        entries: list[tuple[bytes, CanonicalValue]] = []
        seen: set[bytes] = set()
        key_bytes = 0
        for key, item in value.items():
            if not isinstance(key, str):
                raise CanonicalError("invalid_map_key", "map keys must be strings")
            normalized = _normalized_utf8(key)
            key_bytes += len(normalized) + 5
            if key_bytes > MAX_CANONICAL_BYTES:
                raise CanonicalError("canonical_overflow", "map keys exceed the profile bound")
            if normalized in seen:
                raise CanonicalError("invalid_normalization", "map keys collide after NFC normalization")
            seen.add(normalized)
            entries.append((normalized, item))
        entries.sort(key=lambda entry: entry[0])
        length_offset = encoder.begin_frame(b"m")
        encoder.append(len(entries).to_bytes(4, "big"))
        for normalized_key, item in entries:
            encoder.frame(b"s", normalized_key)
            _encode(encoder, item, depth + 1)
        encoder.end_frame(length_offset)
    elif isinstance(value, Sequence):
        if len(value) > (MAX_CANONICAL_BYTES - 4) // 5:
            raise CanonicalError("canonical_overflow", "list has too many items")
        length_offset = encoder.begin_frame(b"l")
        encoder.append(len(value).to_bytes(4, "big"))
        for item in value:
            _encode(encoder, item, depth + 1)
        encoder.end_frame(length_offset)
    else:
        raise CanonicalError("unsupported_type", f"unsupported canonical value: {type(value).__name__}")


def canonical_bytes(domain: str, value: CanonicalValue) -> bytes:
    """Encode a value with domain separation and the frozen V1 profile."""

    domain_bytes = _normalized_utf8(domain)
    if not domain_bytes or len(domain_bytes) > MAX_IDENTIFIER_BYTES:
        raise CanonicalError("invalid_domain", "domain is empty or exceeds its bound")
    encoder = _Encoder()
    encoder.append(_MAGIC)
    encoder.append(bytes([CANONICAL_PROFILE_VERSION]))
    encoder.frame(b"D", domain_bytes)
    _encode(encoder, value, 0)
    return bytes(encoder.output)


def canonical_sha256(domain: str, value: CanonicalValue) -> str:
    """Return lowercase SHA-256 hex for canonical strategy-semantic bytes."""

    return hashlib.sha256(canonical_bytes(domain, value)).hexdigest()
