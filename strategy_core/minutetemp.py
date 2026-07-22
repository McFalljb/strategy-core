"""MinuteTemp OpenAPI-aligned models for engine-owned data reads."""

from __future__ import annotations

from dataclasses import dataclass, field
from types import MappingProxyType
from typing import TYPE_CHECKING, Literal

if TYPE_CHECKING:
    from collections.abc import Iterable, Mapping
    from datetime import date, datetime

ReportType = Literal["cli", "dsm", "metar_tgroup", "metar_6hr"]
TemperatureUnit = Literal["F", "C"]
PlanTier = Literal["starter", "pro", "clanker"]
DataResolution = Literal["1m", "5m", "10m"]
TemperatureDayMode = Literal["calendar_day", "nws_climate_day"]
WuDayMode = Literal["calendar_day"]
OracleScoreMode = Literal["overall", "day_ahead", "day_of"]
OracleRankBy = Literal["combined", "high", "low"]
ReportScheduleBasis = Literal["utc", "local"]


def _freeze_tuple[T](values: Iterable[T]) -> tuple[T, ...]:
    return tuple(values)


def _freeze_mapping[K, V](values: Mapping[K, V]) -> Mapping[K, V]:
    return MappingProxyType(dict(values))


@dataclass(frozen=True, slots=True)
class CityInfo:
    """Station-city context included in many MinuteTemp read responses."""

    id: str = ""
    slug: str = ""
    name: str = ""
    timezone: str = ""


@dataclass(frozen=True, slots=True)
class StationInfo:
    """Station context included in many MinuteTemp read responses."""

    station_id: str = ""
    name: str = ""
    temperature_unit: TemperatureUnit | str = "F"
    uses_nws_climate_day: bool | None = None


@dataclass(frozen=True, slots=True)
class ObservationRecord:
    """OpenAPI-aligned observation record."""

    observation_time: datetime | None = None
    temperature_f: float | None = None
    temperature_c: float | None = None
    dewpoint: float | None = None
    heat_index: float | None = None
    wind_chill: float | None = None
    relative_humidity: float | None = None
    barometric_pressure: float | None = None
    sea_level_pressure: float | None = None
    wind_speed: float | None = None
    wind_direction: float | None = None
    wind_gust: float | None = None
    text_description: str | None = None
    precipitation_1h: float | None = None
    precipitation_3h: float | None = None
    precipitation_6h: float | None = None
    is_locf: bool = False
    is_from_report: bool = False
    report_type: ReportType | None = None
    source_report_id: str | None = None
    temp_min_f: float | None = None
    temp_max_f: float | None = None
    temp_min_c: float | None = None
    temp_max_c: float | None = None


@dataclass(frozen=True, slots=True)
class StationReportRecord:
    """OpenAPI-aligned station report record."""

    report_id: str = ""
    report_revision: int = 0
    report_updated_at: datetime | None = None
    report_type: ReportType | None = None
    report_date: date | None = None
    issuance_time: datetime | None = None
    fetched_at: datetime | None = None
    baseline: bool = False
    provider_available_at: datetime | None = None
    baseline_cached_at: datetime | None = None
    max_temp_f: float | None = None
    max_temp_c: float | None = None
    max_temp_time_utc: datetime | None = None
    min_temp_f: float | None = None
    min_temp_c: float | None = None
    min_temp_time_utc: datetime | None = None
    temp_f: float | None = None
    temp_c: float | None = None


@dataclass(frozen=True, slots=True)
class ReportClockSchedule:
    """Clock-based report schedule definition."""

    basis: ReportScheduleBasis | str = "utc"
    hour: int | None = None
    minute: int | None = None
    utc_hour: int | None = None
    utc_minute: int | None = None
    local_hour: int | None = None
    local_minute: int | None = None
    label: str = ""


@dataclass(frozen=True, slots=True)
class ReportIntervalSchedule:
    """Interval-based report schedule definition."""

    interval_minutes: int
    utc_minute: int | None = None
    local_minute: int | None = None
    label: str = ""


@dataclass(frozen=True, slots=True)
class ReportMultiHourSchedule:
    """Multi-hour report schedule definition."""

    utc_hours: tuple[int, ...] = field(default_factory=tuple)
    local_hours: tuple[int, ...] = field(default_factory=tuple)
    utc_minute: int | None = None
    local_minute: int | None = None
    label: str = ""

    def __post_init__(self) -> None:
        object.__setattr__(self, "utc_hours", _freeze_tuple(self.utc_hours))
        object.__setattr__(self, "local_hours", _freeze_tuple(self.local_hours))


type ReportSchedule = ReportClockSchedule | ReportIntervalSchedule | ReportMultiHourSchedule
type ReportScheduleEntry = ReportSchedule | tuple[ReportSchedule, ...]


def _freeze_report_schedules(
    values: Mapping[str, ReportScheduleEntry],
) -> Mapping[str, ReportScheduleEntry]:
    normalized: dict[str, ReportScheduleEntry] = {}
    for key, value in values.items():
        if isinstance(value, (list, tuple)):
            normalized[key] = _freeze_tuple(value)
        else:
            normalized[key] = value
    return MappingProxyType(normalized)


@dataclass(frozen=True, slots=True)
class HourlyForecastRecord:
    """OpenAPI-aligned hourly forecast point."""

    time: datetime | None = None
    temperature_2m_f: float | None = None
    temperature_2m_c: float | None = None
    apparent_temperature_f: float | None = None
    relative_humidity_2m: float | None = None
    dew_point_2m: float | None = None
    pressure_msl: float | None = None
    wind_speed_10m: float | None = None
    wind_direction_10m: float | None = None
    wind_gusts_10m: float | None = None
    cloud_cover: float | None = None
    precipitation_probability: float | None = None


@dataclass(frozen=True, slots=True)
class ForecastBundleRun:
    """Forecast-run metadata embedded in the latest-forecast response."""

    id: str = ""
    fetched_at: datetime | None = None
    timezone: str = ""
    utc_offset_seconds: int | None = None


@dataclass(frozen=True, slots=True)
class ForecastBundle:
    """One model-specific forecast bundle."""

    model_id: str = ""
    forecast_run: ForecastBundleRun | None = None
    hourly: tuple[HourlyForecastRecord, ...] = field(default_factory=tuple)

    def __post_init__(self) -> None:
        object.__setattr__(self, "hourly", _freeze_tuple(self.hourly))


@dataclass(frozen=True, slots=True)
class OracleModelScoreRecord:
    """OpenAPI-aligned oracle score row."""

    model_id: str = ""
    model_name: str = ""
    is_public: bool | None = None
    high_mae: float | None = None
    low_mae: float | None = None
    high_bias: float | None = None
    low_bias: float | None = None
    combined_mae: float | None = None
    day_count: int | None = None


@dataclass(frozen=True, slots=True)
class OracleScoreData:
    """Typed oracle score response payload."""

    station_id: str = ""
    range_start: date | None = None
    range_end: date | None = None
    days_requested: int | None = None
    all_time: bool | None = None
    score_mode: OracleScoreMode | str = "overall"
    rank_by: OracleRankBy | str = "combined"
    scores: tuple[OracleModelScoreRecord, ...] = field(default_factory=tuple)

    def __post_init__(self) -> None:
        object.__setattr__(self, "scores", _freeze_tuple(self.scores))


@dataclass(frozen=True, slots=True)
class CursorPage:
    """Cursor pagination metadata."""

    limit: int | None = None
    next_cursor: str | None = None


@dataclass(frozen=True, slots=True)
class ForecastRunSummary:
    """OpenAPI-aligned forecast run summary."""

    id: str = ""
    station_id: str = ""
    model_id: str = ""
    forecast_time: datetime | None = None
    fetched_at: datetime | None = None
    timezone: str = ""
    utc_offset_seconds: int | None = None
    data_hash: str = ""


@dataclass(frozen=True, slots=True)
class IpGuardLimits:
    """Per-IP guard limits nested under effective limits."""

    requests_per_second: int | None = None
    burst: int | None = None


@dataclass(frozen=True, slots=True)
class EffectiveLimits:
    """Typed limits response payload."""

    tier: PlanTier | str = "starter"
    requests_per_minute: int | None = None
    daily_max: int | None = None
    max_history_days: int | None = None
    ip_guard: IpGuardLimits | None = None
    rate_limit_remaining: int | None = None
    rate_limit_reset_seconds: int | None = None


@dataclass(frozen=True, slots=True)
class LatestObservationData:
    """Typed latest-observation response payload."""

    city: CityInfo | None = None
    station: StationInfo | None = None
    observation: ObservationRecord | None = None
    daily_high_f: float | None = None
    daily_low_f: float | None = None
    daily_high_c: float | None = None
    daily_low_c: float | None = None
    asos_daily_high_f: float | None = None
    asos_daily_low_f: float | None = None
    asos_daily_high_c: float | None = None
    asos_daily_low_c: float | None = None
    wu_current_temp_f: float | None = None
    wu_current_temp_c: float | None = None
    wu_daily_high_f: float | None = None
    wu_daily_low_f: float | None = None
    wu_daily_high_c: float | None = None
    wu_daily_low_c: float | None = None
    wu_observation_time: datetime | None = None
    wu_fetched_at: datetime | None = None
    temperature_day_mode: TemperatureDayMode | str | None = None
    temperature_day_date: date | None = None
    wu_day_mode: WuDayMode | str | None = None
    wu_day_date: date | None = None


@dataclass(frozen=True, slots=True)
class StationForecastData:
    """Typed latest-forecast response payload."""

    city: CityInfo | None = None
    station: StationInfo | None = None
    forecasts: tuple[ForecastBundle, ...] = field(default_factory=tuple)
    count: int | None = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "forecasts", _freeze_tuple(self.forecasts))


@dataclass(frozen=True, slots=True)
class LatestReportsData:
    """Typed latest-reports response payload."""

    reports: tuple[StationReportRecord, ...] = field(default_factory=tuple)
    report_schedules: Mapping[str, ReportScheduleEntry] | None = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "reports", _freeze_tuple(self.reports))
        if self.report_schedules is not None:
            object.__setattr__(self, "report_schedules", _freeze_report_schedules(self.report_schedules))


@dataclass(frozen=True, slots=True)
class StationReportsData:
    """Typed station-reports response payload for filtered non-history reads."""

    reports: tuple[StationReportRecord, ...] = field(default_factory=tuple)

    def __post_init__(self) -> None:
        object.__setattr__(self, "reports", _freeze_tuple(self.reports))


@dataclass(frozen=True, slots=True)
class StationReportHistoryPage:
    """Typed report-history page payload."""

    city: CityInfo | None = None
    station: StationInfo | None = None
    reports: tuple[StationReportRecord, ...] = field(default_factory=tuple)
    count: int | None = None
    page: CursorPage | None = None
    report_schedules: Mapping[str, ReportScheduleEntry] | None = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "reports", _freeze_tuple(self.reports))
        if self.report_schedules is not None:
            object.__setattr__(self, "report_schedules", _freeze_report_schedules(self.report_schedules))


@dataclass(frozen=True, slots=True)
class ForecastRunsPage:
    """Typed forecast-runs page payload."""

    city: CityInfo | None = None
    station: StationInfo | None = None
    runs: tuple[ForecastRunSummary, ...] = field(default_factory=tuple)
    count: int | None = None
    page: CursorPage | None = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "runs", _freeze_tuple(self.runs))


@dataclass(frozen=True, slots=True)
class ForecastRunData:
    """Typed single forecast-run detail payload."""

    city: CityInfo | None = None
    station: StationInfo | None = None
    forecast_run: ForecastRunSummary | None = None
    hourly: tuple[HourlyForecastRecord, ...] = field(default_factory=tuple)
    count: int | None = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "hourly", _freeze_tuple(self.hourly))
