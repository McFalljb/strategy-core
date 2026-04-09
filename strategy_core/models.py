"""Shared type aliases and small value helpers for the strategy contract."""

from __future__ import annotations

from collections.abc import Mapping

type JSONPrimitive = str | int | float | bool | None
type JSONValue = JSONPrimitive | list[JSONValue] | dict[str, JSONValue]
type JSONObject = dict[str, JSONValue]

type StrategyConfig = Mapping[str, object]

type OrderId = str

type TelemetryField = str | int | float | bool | None
type TelemetryFields = Mapping[str, TelemetryField]
