"""Python-authored cases for the broad Rust conformance corpus.

The checked-in JSON files are build artifacts of these Python contract objects. Normal tests call
``build_core_fixtures`` and compare the result with disk; they never rewrite fixtures.
"""

from __future__ import annotations

import collections.abc
import inspect
import json
import math
import sys
import types
from collections.abc import Mapping
from dataclasses import MISSING, fields, is_dataclass
from datetime import UTC, date, datetime
from enum import Enum
from pathlib import Path
from typing import Annotated, Any, Literal, TypeAliasType, Union, cast, get_args, get_origin, get_type_hints

from pydantic import BaseModel, TypeAdapter, ValidationError

import strategy_core as strategy_contract
from strategy_core.broker import (
    Action,
    BrokerOrderUpdate,
    BrokerUpdateStatus,
    ContractSide,
    OrderExecutionStyle,
    OrderIntent,
    OrderResult,
    OrderStatus,
    OrderTimePolicy,
    OrderType,
    PendingOrder,
    Position,
)
from strategy_core.capabilities import EventDelivery, RuntimeCapabilities
from strategy_core.events import (
    ForecastUpdated,
    ForecastVersions,
    MarketBracket,
    NewHigh,
    NewLow,
    Observation,
    OracleScoreRow,
    OracleScoresUpdated,
    OracleScoreTable,
    PersistenceStatus,
    PriceUpdate,
    ShutdownEvent,
    StationReport,
    StrategyEvent,
    TimerWake,
    WeatherEvent,
    WeatherEventSource,
    WuDayMode,
)
from strategy_core.minutetemp import OracleRankBy, OracleScoreMode, ReportType, TemperatureDayMode
from strategy_core.models import JSONValue, OrderId
from strategy_core.native import NativeKernelResult, NativeKernelStatus
from strategy_core.queries import (
    ForecastQuery,
    ForecastRunQuery,
    ForecastRunsQuery,
    LatestObservationQuery,
    LatestReportsQuery,
    LimitsQuery,
    OracleScoresQuery,
    ReportHistoryQuery,
    ReportsQuery,
)
from strategy_core.runtime import MarketType, RuntimeMode, SettlementSource, StrategyScope
from strategy_core.state import (
    FeeType,
    ForecastHourly,
    FreshnessDomain,
    FreshnessDomainSummary,
    FreshnessSnapshot,
    FreshnessStatus,
    FreshnessSummary,
    ModelForecast,
    OracleModelScore,
    StationForecast,
    StationOracleScores,
    StationWeather,
    TickerPrices,
)

FIXTURE_ROOT = Path(__file__).parent / "fixtures" / "conformance"
CORE_FIXTURE_NAMES = ("events", "state", "broker", "runtime", "queries")

CORE_DIRECT_MODEL_NAMES = (
    "BrokerOrderUpdate",
    "ForecastHourly",
    "ForecastQuery",
    "ForecastRunQuery",
    "ForecastRunsQuery",
    "ForecastUpdated",
    "ForecastVersions",
    "FreshnessDomainSummary",
    "FreshnessSnapshot",
    "FreshnessSummary",
    "LatestObservationQuery",
    "LatestReportsQuery",
    "LimitsQuery",
    "MarketBracket",
    "ModelForecast",
    "NativeKernelResult",
    "NewHigh",
    "NewLow",
    "Observation",
    "OracleModelScore",
    "OracleScoreRow",
    "OracleScoreTable",
    "OracleScoresQuery",
    "OracleScoresUpdated",
    "OrderIntent",
    "OrderResult",
    "PendingOrder",
    "Position",
    "PriceUpdate",
    "ReportHistoryQuery",
    "ReportsQuery",
    "RuntimeCapabilities",
    "ShutdownEvent",
    "StationForecast",
    "StationOracleScores",
    "StationReport",
    "StationWeather",
    "StrategyScope",
    "TickerPrices",
    "TimerWake",
    "WeatherEvent",
    "WeatherEventSource",
)
CORE_DIRECT_MODELS = {name: cast("type[object]", getattr(strategy_contract, name)) for name in CORE_DIRECT_MODEL_NAMES}
CORE_DIRECT_ENUM_NAMES = (
    "Action",
    "BrokerUpdateStatus",
    "ContractSide",
    "EventDelivery",
    "FeeType",
    "FreshnessDomain",
    "FreshnessStatus",
    "MarketType",
    "NativeKernelStatus",
    "SettlementSource",
    "OracleScoreMode",
    "OrderExecutionStyle",
    "OrderStatus",
    "OrderTimePolicy",
    "OrderType",
    "PersistenceStatus",
    "RuntimeMode",
    "TemperatureDayMode",
    "WuDayMode",
)
CORE_DIRECT_ENUMS = {
    "Action": Action,
    "BrokerUpdateStatus": BrokerUpdateStatus,
    "ContractSide": ContractSide,
    "EventDelivery": EventDelivery,
    "FeeType": FeeType,
    "FreshnessDomain": FreshnessDomain,
    "FreshnessStatus": FreshnessStatus,
    "MarketType": MarketType,
    "NativeKernelStatus": NativeKernelStatus,
    "SettlementSource": SettlementSource,
    "OracleScoreMode": OracleScoreMode,
    "OrderExecutionStyle": OrderExecutionStyle,
    "OrderStatus": OrderStatus,
    "OrderTimePolicy": OrderTimePolicy,
    "OrderType": OrderType,
    "PersistenceStatus": PersistenceStatus,
    "RuntimeMode": RuntimeMode,
    "TemperatureDayMode": TemperatureDayMode,
    "WuDayMode": WuDayMode,
}

_UTC = UTC
_T0 = datetime(2026, 7, 13, 12, 34, 56, 123456, tzinfo=_UTC)
_T1 = datetime(2026, 7, 13, 12, 35, 1, 987654, tzinfo=_UTC)

_NON_DEFAULT_ROUND_TRIP = "non_default_round_trip"
_PORTABLE_INTEGER_BOUNDARY = 9_007_199_254_740_991


def _contains_explicit_null(value: object) -> bool:
    if value is None:
        return True
    if isinstance(value, Mapping):
        return any(_contains_explicit_null(item) for item in value.values())
    if isinstance(value, (tuple, list)):
        return any(_contains_explicit_null(item) for item in value)
    return False


def _contains_timestamp(value: object) -> bool:
    if isinstance(value, str):
        try:
            if "T" in value:
                datetime.fromisoformat(value.replace("Z", "+00:00"))
            else:
                date.fromisoformat(value)
        except ValueError:
            pass
        else:
            return True
    if isinstance(value, Mapping):
        return any(_contains_timestamp(item) for item in value.values())
    if isinstance(value, (tuple, list)):
        return any(_contains_timestamp(item) for item in value)
    return False


def _contains_numeric_boundary(value: object) -> bool:
    if isinstance(value, bool):
        return False
    if isinstance(value, int):
        return abs(value) >= 9_007_199_254_740_991
    if isinstance(value, float):
        return value == 0.0 and math.copysign(1.0, value) < 0.0
    if isinstance(value, Mapping):
        return any(_contains_numeric_boundary(item) for item in value.values())
    if isinstance(value, (tuple, list)):
        return any(_contains_numeric_boundary(item) for item in value)
    return False


def _contains_omission(wire: object, expected: object) -> bool:
    if isinstance(wire, Mapping) and isinstance(expected, Mapping):
        if set(expected) - set(wire):
            return True
        return any(_contains_omission(wire[key], expected[key]) for key in set(wire) & set(expected))
    if isinstance(wire, (tuple, list)) and isinstance(expected, (tuple, list)):
        return any(_contains_omission(left, right) for left, right in zip(wire, expected, strict=False))
    return False


def _case_evidence(
    rust_type: str,
    covers: list[str],
    wire: object,
    expected: object,
    *,
    enum_value: bool = False,
) -> tuple[list[str], dict[str, list[str]]]:
    dimensions: set[str] = set()
    if wire not in ({}, [], (), None):
        dimensions.add(_NON_DEFAULT_ROUND_TRIP)
    if _contains_omission(wire, expected):
        dimensions.update({"defaults", "omission"})
    if _contains_explicit_null(wire):
        dimensions.add("explicit_null")
    if _contains_timestamp(expected):
        dimensions.add("timestamp_formatting")
    if _contains_numeric_boundary(expected):
        dimensions.add("numeric_boundaries")
    if enum_value:
        dimensions.add("enum_values")

    nested_dimensions = [_NON_DEFAULT_ROUND_TRIP] if _NON_DEFAULT_ROUND_TRIP in dimensions else []
    per_surface = {surface: nested_dimensions for surface in covers}
    if rust_type in per_surface:
        per_surface[rust_type] = sorted(dimensions)
    return sorted(dimensions), per_surface


def _json_value(value: object) -> Any:
    if isinstance(value, BaseModel):
        return value.model_dump(mode="json")
    if isinstance(value, Enum):
        return value.value
    if isinstance(value, datetime):
        return value.isoformat().replace("+00:00", "Z")
    if isinstance(value, date):
        return value.isoformat()
    if is_dataclass(value) and not isinstance(value, type):
        return {item.name: _json_value(getattr(value, item.name)) for item in fields(value)}
    if isinstance(value, Mapping):
        return {str(key): _json_value(item) for key, item in value.items()}
    if isinstance(value, (tuple, list)):
        return [_json_value(item) for item in value]
    return value


def _validate_wire(adapter: TypeAdapter[Any], wire: object) -> Any:
    """Validate the declared JSON boundary without Python scalar coercions."""

    return adapter.validate_json(json.dumps(_json_value(wire)), strict=True)


def _valid(
    case_id: str,
    rust_type: str,
    covers: list[str],
    value: object,
    *,
    wire: object | None = None,
) -> dict[str, Any]:
    expected = _json_value(value)
    normalized_wire = expected if wire is None else _json_value(wire)
    dimensions, evidence = _case_evidence(rust_type, covers, normalized_wire, expected)
    return {
        "id": case_id,
        "rust_type": rust_type,
        "covers": covers,
        "wire": normalized_wire,
        "expected": expected,
        "evidence_dimensions": dimensions,
        "evidence": evidence,
    }


def _raw(case_id: str, rust_type: str, covers: list[str], value: object) -> dict[str, Any]:
    case = _valid(case_id, rust_type, covers, value)
    if isinstance(value, str):
        dimensions, evidence = _case_evidence(
            rust_type,
            covers,
            case["wire"],
            case["expected"],
            enum_value=True,
        )
        case["evidence_dimensions"] = dimensions
        case["evidence"] = evidence
    return case


def _invalid(
    case_id: str,
    rust_type: str,
    category: str,
    value: object,
) -> dict[str, Any]:
    return {
        "id": case_id,
        "rust_type": rust_type,
        "covers": [rust_type],
        "category": category,
        "wire": _json_value(value),
        "evidence_dimensions": ["invalid_input"],
        "evidence": {rust_type: ["invalid_input"]},
    }


def _document(
    family: str,
    valid: list[dict[str, Any]],
    invalid: list[dict[str, Any]],
) -> dict[str, Any]:
    return {
        "schema_version": 1,
        "family": family,
        "authority": "python",
        "comparison": "structural-json",
        "wire_policy": {
            "unknown_object_fields": "ignored and absent from canonical output",
            "non_finite_numbers": "excluded because they are not JSON-compatible values",
            "portable_integer_domain": (
                "IEEE-754 exactly represented signed integers (-9007199254740991 through 9007199254740991)"
            ),
            "scalar_coercions": "disabled at the declared JSON wire boundary",
            "optional_fields": "omitted inputs resolve through defaults; explicit null is retained when nullable",
        },
        "valid": valid,
        "invalid": invalid,
    }


def _model_fields(model: type[object]) -> list[tuple[str, object, bool]]:
    if issubclass(model, BaseModel):
        return [(name, field.annotation, field.is_required()) for name, field in model.model_fields.items()]
    module_globals = dict(vars(sys.modules[model.__module__]))
    module_globals.update(vars(strategy_contract))
    module_globals.update(
        {
            "Iterable": collections.abc.Iterable,
            "Mapping": collections.abc.Mapping,
            "date": date,
            "datetime": datetime,
        }
    )
    annotations = get_type_hints(model, globalns=module_globals, include_extras=True)
    return [
        (
            field.name,
            annotations[field.name],
            field.default is MISSING and field.default_factory is MISSING,
        )
        for field in fields(cast("Any", model))
    ]


def _unwrap_annotated(annotation: object) -> object:
    while True:
        if isinstance(annotation, TypeAliasType):
            annotation = annotation.__value__
            continue
        if get_origin(annotation) is Annotated:
            annotation = get_args(annotation)[0]
            continue
        break
    return annotation


def _union_members(annotation: object) -> tuple[object, ...] | None:
    annotation = _unwrap_annotated(annotation)
    if get_origin(annotation) in {types.UnionType, Union}:
        return get_args(annotation)
    return None


def _annotation_contains(
    annotation: object,
    targets: set[object],
    seen: frozenset[int] = frozenset(),
) -> bool:
    marker = id(annotation)
    if marker in seen:
        return False
    seen = seen | {marker}
    annotation = _unwrap_annotated(annotation)
    if annotation in targets:
        return True
    origin = get_origin(annotation)
    if origin is Literal:
        return any(type(value) in targets for value in get_args(annotation))
    return any(_annotation_contains(argument, targets, seen) for argument in get_args(annotation))


def _annotation_allows_none(annotation: object) -> bool:
    members = _union_members(annotation)
    return members is not None and type(None) in members


def _annotation_has_closed_enum(annotation: object, seen: frozenset[int] = frozenset()) -> bool:
    marker = id(annotation)
    if marker in seen:
        return False
    seen = seen | {marker}
    annotation = _unwrap_annotated(annotation)
    if get_origin(annotation) is Literal:
        return True
    if inspect.isclass(annotation) and issubclass(annotation, Enum):
        return True
    return any(_annotation_has_closed_enum(argument, seen) for argument in get_args(annotation))


def _annotation_has_model_or_mapping(annotation: object, seen: frozenset[int] = frozenset()) -> bool:
    marker = id(annotation)
    if marker in seen:
        return False
    seen = seen | {marker}
    annotation = _unwrap_annotated(annotation)
    origin = get_origin(annotation)
    if origin in {dict, Mapping, collections.abc.Mapping}:
        return True
    if inspect.isclass(annotation) and (is_dataclass(annotation) or issubclass(annotation, BaseModel)):
        return True
    return any(_annotation_has_model_or_mapping(argument, seen) for argument in get_args(annotation))


def direct_model_dimensions(model: type[object]) -> set[str]:
    """Return minimum evidence dimensions implied by a public model's direct fields."""

    model_fields = _model_fields(model)
    dimensions = {_NON_DEFAULT_ROUND_TRIP, "invalid_input"}
    if any(not required for _, _, required in model_fields):
        dimensions.update({"defaults", "omission"})
    if any(_annotation_allows_none(annotation) for _, annotation, _ in model_fields):
        dimensions.add("explicit_null")
    if any(_annotation_contains(annotation, {date, datetime}) for _, annotation, _ in model_fields):
        dimensions.add("timestamp_formatting")
    if any(_annotation_contains(annotation, {int, float}) for _, annotation, _ in model_fields):
        dimensions.add("numeric_boundaries")
    return dimensions


def _minimal_wire_value(annotation: object) -> object:
    annotation = _unwrap_annotated(annotation)
    members = _union_members(annotation)
    if members is not None:
        return _minimal_wire_value(next(member for member in members if member is not type(None)))
    origin = get_origin(annotation)
    arguments = get_args(annotation)
    if origin is Literal:
        return arguments[0]
    if origin in {list, set, frozenset, tuple, collections.abc.Sequence}:
        return []
    if origin in {dict, Mapping, collections.abc.Mapping}:
        return {}
    if annotation is str:
        return "value"
    if annotation is bool:
        return False
    if annotation is int:
        return 1
    if annotation is float:
        return 1.0
    if annotation is datetime:
        return _T0.isoformat().replace("+00:00", "Z")
    if annotation is date:
        return _T0.date().isoformat()
    if inspect.isclass(annotation) and issubclass(annotation, Enum):
        return next(iter(annotation)).value
    if inspect.isclass(annotation) and (is_dataclass(annotation) or issubclass(annotation, BaseModel)):
        return _minimal_model_wire(annotation)
    return {}


def _minimal_model_wire(model: type[object]) -> dict[str, object]:
    return {name: _minimal_wire_value(annotation) for name, annotation, required in _model_fields(model) if required}


def _dimension_wire_value(annotation: object, dimension: str) -> object:
    annotation = _unwrap_annotated(annotation)
    members = _union_members(annotation)
    if members is not None:
        member = next(
            candidate
            for candidate in members
            if candidate is not type(None) and _annotation_contains(candidate, {date, datetime, int, float})
        )
        return _dimension_wire_value(member, dimension)
    origin = get_origin(annotation)
    arguments = get_args(annotation)
    if origin in {list, set, frozenset, tuple, collections.abc.Sequence}:
        return [_dimension_wire_value(arguments[0], dimension)]
    if origin in {dict, Mapping, collections.abc.Mapping}:
        return {"key": _dimension_wire_value(arguments[-1], dimension)}
    if dimension == "timestamp_formatting":
        if annotation is datetime:
            return _T0.isoformat().replace("+00:00", "Z")
        if annotation is date:
            return _T0.date().isoformat()
    if annotation is float:
        return -0.0
    if annotation is int:
        return _PORTABLE_INTEGER_BOUNDARY
    raise AssertionError((annotation, dimension))


def _non_default_wire_value(annotation: object) -> object:
    annotation = _unwrap_annotated(annotation)
    members = _union_members(annotation)
    if members is not None:
        return _non_default_wire_value(next(member for member in members if member is not type(None)))
    origin = get_origin(annotation)
    arguments = get_args(annotation)
    if origin is Literal:
        return arguments[-1]
    if origin in {list, set, frozenset, tuple, collections.abc.Sequence}:
        return [_non_default_wire_value(arguments[0])]
    if origin in {dict, Mapping, collections.abc.Mapping}:
        return {"key": _non_default_wire_value(arguments[-1])}
    if annotation is str:
        return "non-default"
    if annotation is bool:
        return True
    if annotation is int:
        return 2
    if annotation is float:
        return 1.5
    if annotation is datetime:
        return _T0.isoformat().replace("+00:00", "Z")
    if annotation is date:
        return _T0.date().isoformat()
    if inspect.isclass(annotation) and issubclass(annotation, Enum):
        return list(annotation)[-1].value
    if inspect.isclass(annotation) and (is_dataclass(annotation) or issubclass(annotation, BaseModel)):
        return _non_default_model_wire(annotation)
    return {"value": True}


def _non_default_model_wire(model: type[object]) -> dict[str, object]:
    wire = _minimal_model_wire(model)
    for name, annotation, _ in _model_fields(model):
        candidate = _non_default_wire_value(annotation)
        if name not in wire or wire[name] != candidate:
            return {**wire, name: candidate}
    return wire


def _validation_category(error: ValidationError) -> str:
    error_types = {item["type"] for item in error.errors()}
    if error_types & {"missing", "union_tag_not_found"}:
        return "required_field"
    if error_types <= {"enum", "literal_error", "union_tag_invalid"}:
        return "enum"
    if error_types & {"string_too_short", "greater_than", "less_than"}:
        return "range"
    if all("parsing" in error_type for error_type in error_types):
        return "format"
    return "type"


def direct_model_cases(
    models: Mapping[str, type[object]],
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """Build direct root cases for every field-shape dimension owned by exported models."""

    valid: list[dict[str, Any]] = []
    invalid: list[dict[str, Any]] = []
    for surface, model in models.items():
        adapter: TypeAdapter[Any] = TypeAdapter(model)
        adapter.rebuild(
            _types_namespace={
                **vars(strategy_contract),
                **vars(sys.modules[model.__module__]),
                **globals(),
            }
        )
        model_fields = _model_fields(model)
        minimal_wire = _minimal_model_wire(model)
        baseline = _validate_wire(adapter, minimal_wire)
        baseline_json = _json_value(baseline)
        dimensions = direct_model_dimensions(model)

        for name, annotation, _ in model_fields:
            wire = {**minimal_wire, name: _non_default_wire_value(annotation)}
            try:
                decoded = _validate_wire(adapter, wire)
            except ValidationError:
                continue
            if _json_value(decoded) != baseline_json:
                valid.append(_valid(f"direct-{surface}-non-default", surface, [surface], decoded, wire=wire))
                break
        else:
            raise AssertionError(f"{surface} has no constructible non-default field")

        if "defaults" in dimensions:
            valid.append(_valid(f"direct-{surface}-defaults", surface, [surface], baseline, wire=minimal_wire))

        if "explicit_null" in dimensions:
            name, _, _ = next(field for field in model_fields if _annotation_allows_none(field[1]))
            wire = {**minimal_wire, name: None}
            valid.append(
                _valid(
                    f"direct-{surface}-explicit-null",
                    surface,
                    [surface],
                    _validate_wire(adapter, wire),
                    wire=wire,
                )
            )

        dimension_targets: tuple[tuple[str, set[object]], ...] = (
            ("timestamp_formatting", {date, datetime}),
            ("numeric_boundaries", {int, float}),
        )
        for dimension, targets in dimension_targets:
            if dimension not in dimensions:
                continue
            name, annotation, _ = next(field for field in model_fields if _annotation_contains(field[1], targets))
            wire = {**minimal_wire, name: _dimension_wire_value(annotation, dimension)}
            valid.append(
                _valid(
                    f"direct-{surface}-{dimension.replace('_', '-')}",
                    surface,
                    [surface],
                    _validate_wire(adapter, wire),
                    wire=wire,
                )
            )

        for name, annotation, _ in model_fields:
            bad_values: tuple[object, ...] = (
                ("invalid", [], {}, None)
                if _annotation_has_closed_enum(annotation) or _annotation_has_model_or_mapping(annotation)
                else ([], {}, None, "invalid")
            )
            for bad_value in bad_values:
                wire = {**minimal_wire, name: bad_value}
                try:
                    _validate_wire(adapter, wire)
                except ValidationError as error:
                    invalid.append(
                        _invalid(
                            f"direct-{surface}-invalid-{name}",
                            surface,
                            _validation_category(error),
                            wire,
                        )
                    )
                    break
            else:
                continue
            break
        else:
            raise AssertionError(f"{surface} has no rejected direct field input")
    return valid, invalid


def _events_fixture() -> dict[str, Any]:
    score_table = OracleScoreTable(
        station_id="KMIA",
        range_start="2026-06-01",
        range_end="2026-07-12",
        days_requested=30,
        all_time=False,
        score_mode="day_of",
        rank_by="high",
        scores=[
            OracleScoreRow(
                model_id="ncep_hrrr_conus",
                model_name="HRRR",
                is_public=True,
                combined_mae=1.25,
                high_mae=1.0,
                low_mae=1.5,
                high_bias=-0.0,
                low_bias=-0.25,
                day_count=30,
            )
        ],
    )
    events: list[tuple[str, object, list[str]]] = [
        (
            "observation-full",
            Observation(
                event_id="obs-1",
                sequence=9_007_199_254_740_991,
                city_sequence=0,
                emitted_at=_T0,
                slug="miami",
                station_id="KMIA",
                observed_at=_T1,
                lag_seconds=0,
                preliminary=True,
                temperature_f=90.5,
                temperature_c=32.5,
                temp_min_f=88.0,
                temp_max_f=91.0,
                temp_min_c=31.1,
                temp_max_c=32.8,
                is_from_report=True,
                report_type="cli",
                source_report_id="cli-1",
                wu_current_temp_f=90.0,
                wu_current_temp_c=32.2,
                wu_daily_high_f=91.0,
                wu_daily_low_f=77.0,
                wu_daily_high_c=32.8,
                wu_daily_low_c=25.0,
                wu_observation_time=_T0,
                wu_fetched_at=_T1,
                temperature_day_mode="nws_climate_day",
                temperature_day_date="2026-07-13",
                wu_day_mode="calendar_day",
                wu_day_date="2026-07-13",
                dewpoint=73.0,
                heat_index=101.0,
                wind_chill=-0.0,
                relative_humidity=60.0,
                wind_speed=8.0,
                wind_direction=180.0,
                wind_gust=15.0,
                text_description="Partly Cloudy",
            ),
            ["Observation", "TemperatureDayMode", "WuDayMode"],
        ),
        (
            "price-update-full",
            PriceUpdate(
                event_id="prices-1",
                sequence=2,
                city_sequence=3,
                emitted_at=_T0,
                source="kalshi",
                slug="miami",
                station_id="KMIA",
                city_id="MIA",
                timestamp=_T1,
                markets=[
                    MarketBracket(
                        market_id="market-1",
                        ticker="KXHIGHMIA-26JUL13-B90.5",
                        yes_price=0.61,
                        no_price=0.39,
                        event_ticker="KXHIGHMIA-26JUL13",
                        event_date="2026-07-13",
                        close_time=_T1,
                        strike_type="between",
                        floor_strike=89.5,
                        cap_strike=90.5,
                        snapshot_time=_T0,
                        yes_bid=0.60,
                        yes_ask=0.62,
                        no_bid=0.38,
                        no_ask=0.40,
                        yes_bid_depth=1_000_000,
                        yes_ask_depth=20,
                        no_bid_depth=30,
                        no_ask_depth=40,
                        yes_bid_levels=[(0.60, 10), (0.59, 20)],
                        yes_ask_levels=[(0.62, 12)],
                        no_bid_levels=[],
                        no_ask_levels=[(0.40, 8)],
                        orderbook_depth=50,
                        volume=0.0,
                    )
                ],
            ),
            ["PriceUpdate", "MarketBracket", "PriceLevel"],
        ),
        (
            "forecast-updated",
            ForecastUpdated(
                event_id="forecast-1",
                sequence=3,
                emitted_at=_T0,
                slug="miami",
                station_id="KMIA",
                model_id="ncep_hrrr_conus",
                version="2026-07-13T12:00:00Z",
            ),
            ["ForecastUpdated"],
        ),
        (
            "forecast-versions",
            ForecastVersions(
                event_id="versions-1",
                sequence=4,
                emitted_at=_T0,
                station_id="KMIA",
                versions={"ncep_hrrr_conus": "v2", "ncep_gfs": "v1"},
            ),
            ["ForecastVersions"],
        ),
        (
            "oracle-scores-updated",
            OracleScoresUpdated(
                event_id="oracle-1",
                sequence=5,
                emitted_at=_T0,
                station_id="KMIA",
                modes=["overall", "day_ahead", "day_of"],
                updated_at=_T1,
                overall=score_table,
                day_ahead=score_table.model_copy(update={"score_mode": "day_ahead"}),
                day_of=score_table,
            ),
            ["OracleScoresUpdated", "OracleScoreTable", "OracleScoreRow", "OracleScoreMode"],
        ),
        (
            "station-report-full",
            StationReport(
                event_id="report-event-1",
                sequence=6,
                city_sequence=7,
                emitted_at=_T0,
                slug="miami",
                station_id="KMIA",
                report_id="cli-20260713",
                report_revision=2,
                report_updated_at=_T1,
                report_type="cli",
                report_date="2026-07-13",
                issuance_time=_T0,
                fetched_at=_T1,
                source_url="https://example.test/report",
                provider="nws",
                max_temp_f=91.0,
                max_temp_c=32.8,
                max_temp_time_utc=_T0,
                min_temp_f=77.0,
                min_temp_c=25.0,
                min_temp_time_utc=_T1,
                temp_f=90.0,
                temp_c=32.2,
            ),
            ["StationReport"],
        ),
        (
            "weather-event-full",
            WeatherEvent(
                event_id="weather-1",
                sequence=8,
                city_sequence=9,
                emitted_at=_T0,
                slug="miami",
                station_id="KMIA",
                id="wx-1",
                event_type="thunderstorm",
                tier="severe",
                state="active",
                name="Thunderstorm",
                badge="TS",
                detail="Nearby cell",
                summary="Storm observed",
                started_at=_T0,
                last_confirmed_at=_T1,
                ended_at=None,
                source=WeatherEventSource(
                    metar_type="SPECI",
                    flight_category="IFR",
                    wx_string="+TSRA",
                    wx_token="TSRA",
                    wind_speed_kt=25.0,
                    wind_gust_kt=40.0,
                    peak_wind_kt=45.0,
                    peak_wind_direction=270,
                    visibility_mi=0.5,
                    cb_location="NW",
                ),
            ),
            ["WeatherEvent", "WeatherEventSource"],
        ),
        (
            "new-high-full",
            NewHigh(
                event_id="high-1",
                sequence=10,
                city_sequence=11,
                emitted_at=_T0,
                event_key="KMIA:2026-07-13:high",
                source_timestamp=_T0,
                wmo_emit_time=_T0,
                producer_received_at=_T1,
                live_published_at=_T1,
                persistence_status="committed",
                producer_sequence=12,
                slug="miami",
                station_id="KMIA",
                value_f=91.0,
                value_c=32.7777777778,
                prev_value_f=90.0,
                observed_at=_T0,
                temperature_day_mode="calendar_day",
                temperature_day_date="2026-07-13",
                is_from_report=True,
                report_type="cli",
                source_report_id="cli-1",
            ),
            ["NewHigh", "PersistenceStatus"],
        ),
        (
            "new-low-full",
            NewLow(
                event_id="low-1",
                sequence=13,
                city_sequence=14,
                emitted_at=_T0,
                event_key="KMIA:2026-07-13:low",
                source_timestamp=_T0,
                wmo_emit_time=_T0,
                producer_received_at=_T1,
                live_published_at=_T1,
                persistence_status="failed",
                producer_sequence=15,
                slug="miami",
                station_id="KMIA",
                value_f=-0.0,
                value_c=-17.7777777778,
                prev_value_f=1.0,
                observed_at=_T0,
                temperature_day_mode="nws_climate_day",
                temperature_day_date="2026-07-13",
                is_from_report=False,
            ),
            ["NewLow"],
        ),
        (
            "timer-wake",
            TimerWake(scheduled_for=_T0, fired_at=_T1, name="rebalance"),
            ["TimerWake"],
        ),
        ("shutdown-defaults", ShutdownEvent(), ["ShutdownEvent"]),
    ]
    trader_events: list[tuple[str, object, list[str]]] = [
        (
            "trader-price-update-knyc",
            PriceUpdate(
                event_id="evt-price-1",
                sequence=12,
                city_sequence=None,
                emitted_at=datetime(2026, 6, 16, 12, 0, 1, tzinfo=_UTC),
                source="kalshi",
                slug="nyc",
                station_id="KNYC",
                city_id="nyc",
                timestamp=datetime(2026, 6, 16, 12, 0, 1, tzinfo=_UTC),
                markets=[
                    MarketBracket(
                        market_id="KXHIGHNY-26JUN16-T70",
                        ticker="KXHIGHNY-26JUN16-T70",
                        yes_price=0.43,
                        no_price=0.57,
                        event_ticker="KXHIGHNY-26JUN16",
                        event_date="2026-06-16",
                        close_time=datetime(2026, 6, 16, 19, 0, tzinfo=_UTC),
                        snapshot_time=datetime(2026, 6, 16, 12, 0, 1, tzinfo=_UTC),
                        yes_bid=0.42,
                        yes_ask=0.44,
                        no_bid=0.56,
                        no_ask=0.58,
                        yes_bid_depth=120,
                        yes_ask_depth=80,
                        orderbook_depth=2,
                    )
                ],
            ),
            ["PriceUpdate", "MarketBracket", "PriceLevel"],
        ),
        (
            "trader-forecast-updated-knyc",
            ForecastUpdated(
                event_id="mt-knyc-forecast-1",
                sequence=91,
                emitted_at=datetime(2026, 6, 16, 12, 0, tzinfo=_UTC),
                slug="nyc",
                station_id="KNYC",
                model_id="gfs",
                version="2026-06-16T12:00:00Z",
            ),
            ["ForecastUpdated"],
        ),
        (
            "trader-oracle-scores-updated-kmia",
            OracleScoresUpdated(
                event_id="oracle-1",
                sequence=5,
                emitted_at=_T0,
                station_id="KMIA",
                modes=["overall", "day_ahead", "day_of"],
                updated_at=_T1,
                overall=score_table.model_copy(update={"score_mode": "overall"}),
                day_ahead=score_table.model_copy(update={"score_mode": "day_ahead"}),
                day_of=score_table,
            ),
            ["OracleScoresUpdated", "OracleScoreTable", "OracleScoreRow", "OracleScoreMode"],
        ),
        (
            "trader-station-report-kphx",
            StationReport(
                event_id="mt-kphx-dsm-1",
                sequence=90,
                city_sequence=12,
                emitted_at=datetime(2026, 6, 21, 4, 16, tzinfo=_UTC),
                slug="phoenix",
                station_id="KPHX",
                report_id="latest-dsm",
                report_revision=0,
                report_updated_at=datetime(2026, 6, 21, 4, 16, tzinfo=_UTC),
                report_type="dsm",
                report_date="2026-06-20",
                issuance_time=datetime(2026, 6, 21, 4, 16, tzinfo=_UTC),
                fetched_at=datetime(2026, 6, 21, 4, 17, 7, tzinfo=_UTC),
                source_url="",
                provider="minutetemp",
                max_temp_f=114.0,
                max_temp_c=45.6,
                max_temp_time_utc=datetime(2026, 6, 21, 4, 16, tzinfo=_UTC),
                min_temp_f=91.0,
                min_temp_c=32.8,
                min_temp_time_utc=datetime(2026, 6, 21, 4, 16, tzinfo=_UTC),
                temp_f=109.0,
                temp_c=42.8,
            ),
            ["StationReport"],
        ),
    ]
    events.extend(trader_events)
    valid = [
        _valid(case_id, "StrategyEvent", [*covers, "StrategyEvent", "EngineEvent"], event)
        for case_id, event, covers in events
    ]
    for case in valid:
        variant = cast("list[str]", case["covers"])[0]
        case["evidence"][variant] = case["evidence_dimensions"]
    observation_defaults = _valid(
        "observation-defaults-and-null",
        "StrategyEvent",
        ["Observation", "TemperatureDayMode", "WuDayMode", "StrategyEvent", "EngineEvent"],
        Observation(station_id="KMIA"),
        wire={"type": "observation", "station_id": "KMIA", "event_id": None},
    )
    observation_defaults["evidence"]["Observation"] = observation_defaults["evidence_dimensions"]
    valid.append(observation_defaults)
    enum_values: dict[str, list[str]] = {
        "OracleScoreMode": ["overall", "day_ahead", "day_of"],
        "PersistenceStatus": ["uncommitted", "committed", "failed"],
        "TemperatureDayMode": ["calendar_day", "nws_climate_day"],
        "WuDayMode": ["calendar_day"],
    }
    for rust_type, values in enum_values.items():
        valid.extend(_raw(f"{rust_type}-{value}", rust_type, [rust_type], value) for value in values)
    blank_observation = _invalid(
        "blank-required-station",
        "StrategyEvent",
        "range",
        {"type": "observation", "station_id": ""},
    )
    blank_observation["covers"].append("Observation")
    blank_observation["evidence"]["Observation"] = ["invalid_input"]
    return _document(
        "events",
        valid,
        [
            _invalid("missing-discriminator", "StrategyEvent", "required_field", {"station_id": "KMIA"}),
            _invalid("unknown-discriminator", "StrategyEvent", "enum", {"type": "unknown"}),
            blank_observation,
            _invalid(
                "malformed-nested-market",
                "StrategyEvent",
                "type",
                {"type": "price_update", "source": "kalshi", "station_id": "KMIA", "markets": ["bad"]},
            ),
            *[_invalid(f"unknown-{rust_type}", rust_type, "enum", "unknown") for rust_type in enum_values],
        ],
    )


def _state_fixture() -> dict[str, Any]:
    forecast = StationForecast(
        model_forecasts={
            "ncep_hrrr_conus": ModelForecast(
                model_id="ncep_hrrr_conus",
                value=91.25,
                version="v2",
                updated_at=_T1,
                run_issued_at=_T0,
                hourly=(
                    ForecastHourly(
                        time="2026-07-13T13:00:00-04:00",
                        temperature_2m_f=90.0,
                        temperature_2m_c=32.2,
                        apparent_temperature_f=101.0,
                        relative_humidity_2m=70.0,
                        dew_point_2m=75.0,
                        pressure_msl=1012.3,
                        wind_speed_10m=12.0,
                        wind_direction_10m=180.0,
                        wind_gusts_10m=20.0,
                        cloud_cover=0.0,
                        precipitation_probability=25.0,
                    ),
                ),
            )
        },
        updated_at=_T1,
    )
    scores = StationOracleScores(
        station_id="KMIA",
        scores=(
            OracleModelScore(
                model_id="ncep_hrrr_conus",
                model_name="HRRR",
                combined_mae=1.1,
                high_mae=1.0,
                low_mae=1.2,
                high_bias=-0.0,
                low_bias=-0.1,
                day_count=30,
                is_public=True,
            ),
        ),
        rank_by="high",
        score_mode="day_of",
        days_requested="30",
        range_start="2026-06-01",
        range_end="2026-07-12",
        updated_at=_T1,
    )
    weather = StationWeather(
        current_temp=90.0,
        running_high=91.0,
        running_low=77.0,
        last_metar_time=_T0,
        temp_min_f=89.0,
        temp_max_f=91.0,
        temp_min_c=31.7,
        temp_max_c=32.8,
        preliminary=True,
        lag_seconds=0,
        wu_current_temp_f=90.0,
        wu_current_temp_c=32.2,
        wu_daily_high_f=91.0,
        wu_daily_low_f=77.0,
        wu_daily_high_c=32.8,
        wu_daily_low_c=25.0,
        wu_observation_time=_T0,
        wu_fetched_at=_T1,
        asos_daily_high_f=91.0,
        asos_daily_low_f=77.0,
        dewpoint=73.0,
        heat_index=101.0,
        wind_chill=-0.0,
        relative_humidity=60.0,
        wind_speed=8.0,
        wind_direction=180.0,
        wind_gust=15.0,
        text_description="Partly Cloudy",
        dsm_high=91.0,
        dsm_low=77.0,
        dsm_high_time=_T0,
        dsm_low_time=_T0,
        six_hr_high=91.0,
        six_hr_low=77.0,
        last_dsm_time=_T0,
        last_six_hr_time=_T1,
    )
    prices = TickerPrices(
        ticker="KXHIGHMIA-26JUL13-B90.5",
        source="kalshi",
        event_ticker="KXHIGHMIA-26JUL13",
        event_date="2026-07-13",
        series_ticker="KXHIGHMIA",
        close_time=_T1,
        fee_type="quadratic",
        fee_multiplier=1.0,
        strike_type="between",
        floor_strike=89.5,
        cap_strike=90.5,
        yes_price=0.61,
        no_price=0.39,
        yes_bid=0.60,
        yes_ask=0.62,
        no_bid=0.38,
        no_ask=0.40,
        yes_bid_depth=10,
        yes_ask_depth=20,
        no_bid_depth=30,
        no_ask_depth=40,
        yes_bid_levels=((0.60, 10),),
        yes_ask_levels=((0.62, 20),),
        no_bid_levels=(),
        no_ask_levels=((0.40, 40),),
        orderbook_depth=50,
        volume=1_000_000.0,
        peak_yes_ask=0.99,
        last_update=_T1,
    )
    freshness = FreshnessSnapshot(
        domain=FreshnessDomain.PRICE,
        key=prices.ticker,
        status=FreshnessStatus.STALE,
        source="kalshi",
        updated_at=_T0,
        observed_at=_T0,
        stale_after_seconds=30.0,
        age_seconds=31.25,
        invalidation_reason="forecast_updated",
        detail="waiting for price refresh",
    )
    summary = FreshnessSummary(
        as_of=_T1,
        domains=(
            FreshnessDomainSummary(FreshnessDomain.WEATHER, 1, 1, 0, 0.0),
            FreshnessDomainSummary(FreshnessDomain.PRICE, 2, 1, 1, 31.25),
        ),
    )
    valid = [
        _valid(
            "freshness-full",
            "FreshnessSnapshot",
            ["FreshnessSnapshot", "FreshnessDomain", "FreshnessStatus"],
            freshness,
        ),
        _valid("freshness-summary-full", "FreshnessSummary", ["FreshnessSummary", "FreshnessDomainSummary"], summary),
        _valid("forecast-full", "StationForecast", ["StationForecast", "ModelForecast", "ForecastHourly"], forecast),
        _valid("oracle-scores-full", "StationOracleScores", ["StationOracleScores", "OracleModelScore"], scores),
        _valid("weather-full", "StationWeather", ["StationWeather"], weather),
        _valid("weather-defaults", "StationWeather", ["StationWeather"], StationWeather(), wire={}),
        _valid("ticker-prices-full", "TickerPrices", ["TickerPrices", "PriceLevel", "FeeType"], prices),
        _valid("ticker-prices-defaults", "TickerPrices", ["TickerPrices"], TickerPrices(), wire={}),
    ]
    valid.extend(
        _raw(f"freshness-status-{value.value}", "FreshnessStatus", ["FreshnessStatus"], value.value)
        for value in FreshnessStatus
    )
    valid.extend(
        _raw(f"freshness-domain-{value.value}", "FreshnessDomain", ["FreshnessDomain"], value.value)
        for value in FreshnessDomain
    )
    valid.extend(
        _raw(f"fee-type-{value}", "FeeType", ["FeeType"], value)
        for value in ("quadratic", "quadratic_with_maker_fees", "flat")
    )
    return _document(
        "state",
        valid,
        [
            _invalid(
                "unknown-freshness-status",
                "FreshnessSnapshot",
                "enum",
                {**_json_value(freshness), "status": "unknown"},
            ),
            _invalid(
                "malformed-freshness-count",
                "FreshnessSummary",
                "format",
                {"as_of": "not-a-time", "domains": []},
            ),
            _invalid("wrong-hourly-shape", "StationForecast", "type", {"model_forecasts": []}),
        ],
    )


def _broker_fixture() -> dict[str, Any]:
    position = Position("KXHIGHMIA-26JUL13-B90.5", "yes", 1_000_000, 0.61)
    pending = PendingOrder(
        order_id="order-1",
        sleeve_id="demo:KMIA",
        ticker=position.ticker,
        action="buy",
        contract_side="yes",
        limit_price=0.61,
        requested_quantity=1_000_000,
        filled_quantity=0,
        reserved_global=610_000.0,
        reserved_sleeve=610_000.0,
        fee_type="quadratic",
        fee_multiplier=1.5,
        fee_accumulator=-0.0,
        signal_type="forecast_edge",
        signal_metadata='{"model":"hrrr"}',
        created_at="2026-07-13T08:34:56.123456-04:00",
        client_order_id="client-1",
        expires_at="2026-07-13T09:34:56.123456-04:00",
    )
    result = OrderResult(
        order_id="order-1",
        sleeve_id="demo:KMIA",
        status="partial",
        filled_quantity=500_000,
        fill_price=0.60,
        fee_cost=123.45,
        reason="partial liquidity",
    )
    intent = OrderIntent(
        ticker=position.ticker,
        action="sell",
        contract_side="no",
        order_type="limit",
        quantity=1_000_000,
        limit_price=0.39,
        max_price=0.40,
        max_cost=400_000.0,
        execution_style="sweep",
        time_policy="fill_or_kill",
        reduce_only=True,
        post_only=False,
        signal_type="risk_exit",
        signal_metadata='{"reason":"limit"}',
        client_order_id="client-2",
        expires_after_ms=86_400_000,
    )
    update = BrokerOrderUpdate(
        order_id="order-1",
        sleeve_id="demo:KMIA",
        ticker=position.ticker,
        status="reconciled",
        action="sell",
        contract_side="no",
        requested_quantity=1_000_000,
        filled_quantity=500_000,
        remaining_quantity=500_000,
        fill_price=0.60,
        average_fill_price=0.605,
        fee_cost=123.45,
        reason="provider replay",
        client_order_id="client-2",
        provider_order_id="provider-1",
        provider_sequence="999999999999999999",
        updated_at="2026-07-13T08:35:01.987-04:00",
        expires_at=None,
    )
    valid = [
        _valid("position", "Position", ["Position"], position),
        _valid("pending-order-full", "PendingOrder", ["PendingOrder", "OrderId"], pending),
        _valid(
            "pending-order-defaults-and-null",
            "PendingOrder",
            ["PendingOrder"],
            PendingOrder("order-2", "demo:KMIA", position.ticker, "buy", "yes", 0.50, 1),
            wire={
                "order_id": "order-2",
                "sleeve_id": "demo:KMIA",
                "ticker": position.ticker,
                "action": "buy",
                "contract_side": "yes",
                "limit_price": 0.50,
                "requested_quantity": 1,
                "client_order_id": None,
            },
        ),
        _valid("order-result", "OrderResult", ["OrderResult", "OrderStatus"], result),
        _valid(
            "order-intent-full",
            "OrderIntent",
            ["OrderIntent", "OrderType", "OrderExecutionStyle", "OrderTimePolicy"],
            intent,
        ),
        _valid("broker-order-update", "BrokerOrderUpdate", ["BrokerOrderUpdate", "BrokerUpdateStatus"], update),
    ]
    enum_values: dict[str, list[str]] = {
        "Action": ["buy", "sell"],
        "ContractSide": ["yes", "no"],
        "OrderType": ["market", "limit"],
        "OrderStatus": ["filled", "partial", "pending", "rejected", "cancelled"],
        "OrderExecutionStyle": ["resting_limit", "direct", "sweep"],
        "OrderTimePolicy": ["good_till_canceled", "immediate_or_cancel", "fill_or_kill"],
        "BrokerUpdateStatus": [
            "accepted",
            "rejected",
            "submitted",
            "resting",
            "partially_filled",
            "filled",
            "cancel_requested",
            "cancelled",
            "expired",
            "closed",
            "submission_unknown",
            "reconciled",
        ],
    }
    for rust_type, values in enum_values.items():
        valid.extend(_raw(f"{rust_type}-{value}", rust_type, [rust_type], value) for value in values)
    document = _document(
        "broker",
        valid,
        [
            _invalid(
                "unknown-action",
                "OrderIntent",
                "enum",
                {**_json_value(intent), "action": "hold"},
            ),
            _invalid("missing-ticker", "OrderIntent", "required_field", {"action": "buy"}),
            _invalid(
                "wrong-quantity-type",
                "BrokerOrderUpdate",
                "type",
                {**_json_value(update), "requested_quantity": []},
            ),
        ],
    )
    document["drift_decisions"] = [
        {
            "id": "optional-expiry-null-serialization",
            "classification": "python-canonical",
            "python_before": "Dataclass serialization includes optional expiry fields as explicit null.",
            "rust_before": "Serde omitted optional expiry fields when their value was None.",
            "consumer_impact": (
                "Omitted and null inputs both decode as None; emitting null is an additive "
                "structural-output correction."
            ),
            "resolution": "Rust serializes expires_at and expires_after_ms as null when unset.",
        }
    ]
    return document


def _runtime_fixture() -> dict[str, Any]:
    capabilities = RuntimeCapabilities(
        supports_http=True,
        supports_data_queries=False,
        supports_one_shot_timers=True,
        supports_recurring_timers=True,
        supports_native_kernels=True,
        queue_is_durable=True,
        replay_controls_event_progression=True,
        event_delivery="decision",
    )
    scope = StrategyScope(
        sleeve_id="demo:KMIA",
        strategy_name="demo",
        station_id="KMIA",
        tickers=("KXHIGHMIA-26JUL13-B90.5", "KXLOWTMIA-26JUL13-B75.5"),
        market_type="high",
        event_ticker="KXHIGHMIA-26JUL13",
        event_date=date(2026, 7, 13),
    )
    native_result = NativeKernelResult(
        status="fallback_completed",
        events_handled=1_000_000,
        actions_emitted=0,
        fallback_used=True,
        metadata={"reason": "unsupported", "attempt": 1, "ratio": -0.0, "active": False, "detail": None},
    )
    valid = [
        _valid("capabilities-full", "RuntimeCapabilities", ["RuntimeCapabilities", "EventDelivery"], capabilities),
        _valid("capabilities-defaults", "RuntimeCapabilities", ["RuntimeCapabilities"], RuntimeCapabilities(), wire={}),
        _valid(
            "capabilities-unknown-field-normalized",
            "RuntimeCapabilities",
            ["RuntimeCapabilities"],
            RuntimeCapabilities(),
            wire={"future_flag": True},
        ),
        _valid("strategy-scope", "StrategyScope", ["StrategyScope", "MarketType"], scope),
        _valid(
            "strategy-scope-defaults",
            "StrategyScope",
            ["StrategyScope"],
            StrategyScope("demo:KMIA", "demo"),
            wire={"sleeve_id": "demo:KMIA", "strategy_name": "demo"},
        ),
        _valid(
            "native-result-full",
            "NativeKernelResult",
            ["NativeKernelResult", "NativeKernelStatus", "JSONValue", "JSONObject"],
            native_result,
        ),
        _valid("native-result-defaults", "NativeKernelResult", ["NativeKernelResult"], NativeKernelResult(), wire={}),
        _raw(
            "strategy-config",
            "StrategyConfig",
            ["StrategyConfig"],
            {"station": "KMIA", "retries": 3, "enabled": True, "nested": {"value": None}},
        ),
        _raw(
            "telemetry-fields",
            "TelemetryFields",
            ["TelemetryField", "TelemetryFields"],
            {"text": "ok", "count": 3, "ratio": -0.0, "active": True, "empty": None},
        ),
    ]
    valid.extend(_raw(f"runtime-mode-{mode.value}", "RuntimeMode", ["RuntimeMode"], mode.value) for mode in RuntimeMode)
    valid.extend(
        _raw(f"market-type-{value}", "MarketType", ["MarketType"], value) for value in ("high", "low", "hourly")
    )
    valid.extend(
        _raw(
            f"settlement-source-{source.value}",
            "SettlementSource",
            ["SettlementSource"],
            source.value,
        )
        for source in SettlementSource
    )
    valid.extend(
        _raw(f"event-delivery-{value}", "EventDelivery", ["EventDelivery"], value) for value in ("wake", "decision")
    )
    valid.extend(
        _raw(f"native-status-{value}", "NativeKernelStatus", ["NativeKernelStatus"], value)
        for value in ("completed", "fallback_completed")
    )
    return _document(
        "runtime",
        valid,
        [
            _invalid(
                "unknown-event-delivery",
                "RuntimeCapabilities",
                "enum",
                {**_json_value(capabilities), "event_delivery": "batch"},
            ),
            _invalid(
                "unknown-market-type",
                "StrategyScope",
                "enum",
                {**_json_value(scope), "market_type": "mid"},
            ),
            _invalid(
                "coerced-supports-http",
                "RuntimeCapabilities",
                "type",
                {"supports_http": "true"},
            ),
            _invalid("invalid-telemetry-field", "TelemetryFields", "type", {"nested": []}),
        ],
    )


def _queries_fixture() -> dict[str, Any]:
    valid = [
        _valid("limits-defaults", "LimitsQuery", ["LimitsQuery"], LimitsQuery(), wire={}),
        _valid("forecast-full", "ForecastQuery", ["ForecastQuery"], ForecastQuery("ncep_hrrr_conus", True)),
        _valid("forecast-defaults", "ForecastQuery", ["ForecastQuery"], ForecastQuery(), wire={}),
        _valid(
            "oracle-full",
            "OracleScoresQuery",
            ["OracleScoresQuery"],
            OracleScoresQuery("30", "day_of", "combined", True),
        ),
        _valid("oracle-defaults", "OracleScoresQuery", ["OracleScoresQuery"], OracleScoresQuery(), wire={}),
        _valid(
            "forecast-runs-full",
            "ForecastRunsQuery",
            ["ForecastRunsQuery", "DateLike"],
            ForecastRunsQuery("ncep_hrrr_conus", _T0, "2026-07-14T12:00:00Z", 1_000_000, "cursor-1", True),
        ),
        _valid("forecast-runs-defaults", "ForecastRunsQuery", ["ForecastRunsQuery"], ForecastRunsQuery(), wire={}),
        _valid("forecast-run", "ForecastRunQuery", ["ForecastRunQuery"], ForecastRunQuery("run-1", True)),
        _valid("latest-reports-defaults", "LatestReportsQuery", ["LatestReportsQuery"], LatestReportsQuery(), wire={}),
        _valid(
            "latest-reports-baseline",
            "LatestReportsQuery",
            ["LatestReportsQuery"],
            LatestReportsQuery(include_baseline=True),
        ),
        _valid(
            "reports-full",
            "ReportsQuery",
            ["ReportsQuery", "LocalDateLike"],
            ReportsQuery("cli", date(2026, 7, 13), True),
        ),
        _valid("reports-defaults", "ReportsQuery", ["ReportsQuery"], ReportsQuery(), wire={}),
        _valid(
            "report-history-full",
            "ReportHistoryQuery",
            ["ReportHistoryQuery", "LocalDateLike"],
            ReportHistoryQuery("cli", date(2026, 7, 1), "2026-07-13", 1_000_000, "cursor-2", True),
        ),
        _valid("report-history-defaults", "ReportHistoryQuery", ["ReportHistoryQuery"], ReportHistoryQuery(), wire={}),
        _valid(
            "latest-observation-full",
            "LatestObservationQuery",
            ["LatestObservationQuery", "TemperatureDayMode"],
            LatestObservationQuery("nws_climate_day", True),
        ),
        _valid(
            "latest-observation-defaults",
            "LatestObservationQuery",
            ["LatestObservationQuery"],
            LatestObservationQuery(),
            wire={},
        ),
    ]
    return _document(
        "queries",
        valid,
        [
            _invalid("wrong-refresh-type", "LimitsQuery", "type", {"refresh": []}),
            _invalid("missing-run-id", "ForecastRunQuery", "required_field", {"refresh": False}),
            _invalid("wrong-limit-type", "ForecastRunsQuery", "type", {"limit": []}),
            _invalid("coerced-limit-type", "ForecastRunsQuery", "type", {"limit": "10"}),
        ],
    )


def build_core_fixtures() -> dict[str, dict[str, Any]]:
    """Build the canonical corpus from Python contract objects."""

    documents = {
        "events": _events_fixture(),
        "state": _state_fixture(),
        "broker": _broker_fixture(),
        "runtime": _runtime_fixture(),
        "queries": _queries_fixture(),
    }
    direct_valid, direct_invalid = direct_model_cases(CORE_DIRECT_MODELS)
    documents["runtime"]["valid"].extend(direct_valid)
    documents["runtime"]["invalid"].extend(direct_invalid)
    documents["runtime"]["invalid"].extend(
        _invalid(f"direct-{name}-unknown", name, "enum", "unknown") for name in CORE_DIRECT_ENUM_NAMES
    )
    return documents


def load_core_fixture(name: str) -> dict[str, Any]:
    """Load one checked-in fixture without mutating it."""

    if name not in CORE_FIXTURE_NAMES:
        msg = f"unknown core conformance fixture: {name}"
        raise ValueError(msg)
    return cast("dict[str, Any]", json.loads((FIXTURE_ROOT / f"{name}.json").read_text()))


def write_core_fixtures() -> None:
    """Explicitly regenerate checked-in fixtures for an intentional contract update."""

    for name, document in build_core_fixtures().items():
        path = FIXTURE_ROOT / f"{name}.json"
        path.write_text(f"{json.dumps(document, indent=2, sort_keys=True)}\n")


_PYTHON_WIRE_TYPES: dict[str, object] = {
    "Action": str,
    "BrokerOrderUpdate": BrokerOrderUpdate,
    "BrokerUpdateStatus": str,
    "ContractSide": str,
    "EventDelivery": str,
    "FeeType": str,
    "ForecastQuery": ForecastQuery,
    "ForecastRunQuery": ForecastRunQuery,
    "ForecastRunsQuery": ForecastRunsQuery,
    "FreshnessDomain": FreshnessDomain,
    "StrategyEvent": StrategyEvent,
    "FreshnessSnapshot": FreshnessSnapshot,
    "FreshnessStatus": FreshnessStatus,
    "FreshnessSummary": FreshnessSummary,
    "LatestObservationQuery": LatestObservationQuery,
    "LatestReportsQuery": LatestReportsQuery,
    "LimitsQuery": LimitsQuery,
    "MarketType": str,
    "NativeKernelResult": NativeKernelResult,
    "NativeKernelStatus": str,
    "OracleScoresQuery": OracleScoresQuery,
    "OrderExecutionStyle": str,
    "OrderIntent": OrderIntent,
    "OrderResult": OrderResult,
    "OrderStatus": str,
    "OrderTimePolicy": str,
    "OrderType": str,
    "OracleScoreMode": OracleScoreMode,
    "PendingOrder": PendingOrder,
    "Position": Position,
    "PersistenceStatus": PersistenceStatus,
    "ReportHistoryQuery": ReportHistoryQuery,
    "ReportsQuery": ReportsQuery,
    "RuntimeCapabilities": RuntimeCapabilities,
    "RuntimeMode": RuntimeMode,
    "StationForecast": StationForecast,
    "StationOracleScores": StationOracleScores,
    "StationWeather": StationWeather,
    "StrategyConfig": dict[str, object],
    "StrategyScope": StrategyScope,
    "TelemetryFields": dict[str, str | int | float | bool | None],
    "TemperatureDayMode": TemperatureDayMode,
    "TickerPrices": TickerPrices,
    "WuDayMode": WuDayMode,
}
_PYTHON_WIRE_TYPES.update(CORE_DIRECT_MODELS)
_PYTHON_WIRE_TYPES.update(CORE_DIRECT_ENUMS)
_TYPE_NAMESPACE: dict[str, object] = {
    "JSONValue": JSONValue,
    "OracleRankBy": OracleRankBy,
    "OracleScoreMode": OracleScoreMode,
    "OrderId": OrderId,
    "ReportType": ReportType,
    "TemperatureDayMode": TemperatureDayMode,
}


def _wire_adapter(rust_type: str) -> TypeAdapter[Any]:
    python_type = _PYTHON_WIRE_TYPES[rust_type]
    adapter: TypeAdapter[Any] = TypeAdapter(python_type)
    adapter.rebuild(_types_namespace={**globals(), **_TYPE_NAMESPACE})
    return adapter


def python_round_trip_valid_case(case: Mapping[str, Any]) -> Any:
    """Decode and canonicalize a valid case through its Python contract type."""

    rust_type = cast("str", case["rust_type"])
    return _json_value(_validate_wire(_wire_adapter(rust_type), case["wire"]))


def python_invalid_category(case: Mapping[str, Any]) -> str | None:
    """Return Python's normalized rejection category for an invalid wire case."""

    adapter = _wire_adapter(cast("str", case["rust_type"]))
    try:
        _validate_wire(adapter, case["wire"])
    except ValidationError as error:
        return _validation_category(error)
    return None


if __name__ == "__main__":
    write_core_fixtures()
