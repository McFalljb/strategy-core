from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any, cast

from strategy_core_v3 import CanonicalError, EpochNanoseconds, FixedDecimal, canonical_bytes, canonical_sha256


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


def main() -> None:
    vectors = cast("dict[str, Any]", json.loads(Path(sys.argv[1]).read_text()))
    for vector in vectors["valid"]:
        value = _node(vector["value"])
        assert canonical_bytes(vector["domain"], value).hex() == vector["expected_hex"], vector["id"]
        assert canonical_sha256(vector["domain"], value) == vector["expected_sha256"], vector["id"]
    for vector in vectors["invalid"]:
        try:
            canonical_bytes(vector["domain"], _node(vector["value"]))
        except CanonicalError as error:
            assert error.category == vector["category"], vector["id"]
        else:
            raise AssertionError(f"invalid vector passed: {vector['id']}")
    print(f"python clean consumer: {len(vectors['valid'])} valid and {len(vectors['invalid'])} invalid vectors passed")


if __name__ == "__main__":
    main()
