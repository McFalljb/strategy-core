"""Typed strategy-visible events shared across trader and backtester runtimes."""

from __future__ import annotations

from datetime import datetime  # noqa: TC003 - pydantic needs datetime available at runtime
from typing import Annotated, Literal

from pydantic import BaseModel, ConfigDict, Field

RequiredText = Annotated[str, Field(min_length=1)]


class MarketBracket(BaseModel):
    """Single bracket within a price update payload."""

    model_config = ConfigDict(frozen=True)

    market_id: str = ""
    ticker: RequiredText
    yes_price: float = 0.0
    no_price: float = 0.0
    event_ticker: str = ""
    event_date: str = ""
    strike_type: str = ""
    floor_strike: float | None = None
    cap_strike: float | None = None
    snapshot_time: datetime | None = None
    yes_bid: float | None = None
    yes_ask: float | None = None
    no_bid: float | None = None
    no_ask: float | None = None
    orderbook_depth: int | None = None
    volume: float | None = None


class WeatherEventSource(BaseModel):
    """Source METAR details attached to weather events."""

    model_config = ConfigDict(frozen=True)

    metar_type: str | None = None
    flight_category: str | None = None
    wx_string: str | None = None
    wx_token: str | None = None
    wind_speed_kt: float | None = None
    wind_gust_kt: float | None = None
    peak_wind_kt: float | None = None
    peak_wind_direction: int | None = None
    visibility_mi: float | None = None
    cb_location: str | None = None


class Observation(BaseModel):
    """Temperature observation update for one station."""

    model_config = ConfigDict(frozen=True)

    type: Literal["observation"] = "observation"
    event_id: str | None = None
    sequence: int | None = None
    city_sequence: int | None = None
    emitted_at: datetime | None = None
    slug: str = ""
    station_id: RequiredText
    observed_at: datetime | None = None
    lag_seconds: int | None = None
    preliminary: bool = False
    temperature_f: float | None = None
    temperature_c: float | None = None
    temp_min_f: float | None = None
    temp_max_f: float | None = None
    temp_min_c: float | None = None
    temp_max_c: float | None = None
    is_from_report: bool = False
    report_type: str | None = None
    source_report_id: str | None = None
    wu_current_temp_f: float | None = None
    wu_current_temp_c: float | None = None
    wu_daily_high_f: float | None = None
    wu_daily_low_f: float | None = None
    wu_daily_high_c: float | None = None
    wu_daily_low_c: float | None = None
    wu_observation_time: datetime | None = None
    wu_fetched_at: datetime | None = None
    dewpoint: float | None = None
    heat_index: float | None = None
    wind_chill: float | None = None
    relative_humidity: float | None = None
    wind_speed: float | None = None
    wind_direction: float | None = None
    wind_gust: float | None = None
    text_description: str | None = None


class PriceUpdate(BaseModel):
    """Market price update for one station/source payload."""

    model_config = ConfigDict(frozen=True)

    type: Literal["price_update"] = "price_update"
    event_id: str | None = None
    sequence: int | None = None
    city_sequence: int | None = None
    emitted_at: datetime | None = None
    source: RequiredText
    slug: str = ""
    station_id: RequiredText
    city_id: str = ""
    timestamp: datetime | None = None
    markets: list[MarketBracket] = Field(default_factory=list)


class ForecastUpdated(BaseModel):
    """Cache-invalidation hint that a forecast model version changed."""

    model_config = ConfigDict(frozen=True)

    type: Literal["forecast_updated"] = "forecast_updated"
    event_id: str | None = None
    sequence: int | None = None
    emitted_at: datetime | None = None
    slug: str = ""
    station_id: RequiredText
    model_id: RequiredText
    version: RequiredText


class ForecastVersions(BaseModel):
    """Bootstrap snapshot of model -> version for one station."""

    model_config = ConfigDict(frozen=True)

    type: Literal["forecast_versions"] = "forecast_versions"
    event_id: str | None = None
    sequence: int | None = None
    emitted_at: datetime | None = None
    slug: str = ""
    station_id: RequiredText
    versions: dict[str, str] = Field(default_factory=dict)


class OracleScoreRow(BaseModel):
    """One oracle score row inside a websocket update."""

    model_config = ConfigDict(frozen=True)

    model_id: RequiredText
    model_name: str = ""
    is_public: bool | None = None
    combined_mae: float | None = None
    high_mae: float | None = None
    low_mae: float | None = None
    high_bias: float | None = None
    low_bias: float | None = None
    day_count: int | None = None


class OracleScoreTable(BaseModel):
    """One mode-specific oracle score table."""

    model_config = ConfigDict(frozen=True)

    station_id: RequiredText
    range_start: str = ""
    range_end: str = ""
    days_requested: int | None = None
    all_time: bool | None = None
    score_mode: str = ""
    rank_by: str = ""
    scores: list[OracleScoreRow] = Field(default_factory=list)


class OracleScoresUpdated(BaseModel):
    """Oracle score update hint or payload for one station."""

    model_config = ConfigDict(frozen=True)

    type: Literal["oracle_scores_updated"] = "oracle_scores_updated"
    event_id: str | None = None
    sequence: int | None = None
    emitted_at: datetime | None = None
    slug: str = ""
    station_id: RequiredText
    modes: list[Literal["overall", "day_ahead"]] = Field(default_factory=list)
    updated_at: datetime | None = None
    overall: OracleScoreTable | None = None
    day_ahead: OracleScoreTable | None = None


class StationReport(BaseModel):
    """Official station report publication event."""

    model_config = ConfigDict(frozen=True)

    type: Literal["station_report"] = "station_report"
    event_id: str | None = None
    sequence: int | None = None
    city_sequence: int | None = None
    emitted_at: datetime | None = None
    slug: str = ""
    station_id: RequiredText
    report_id: RequiredText
    report_revision: int = 0
    report_updated_at: datetime | None = None
    report_type: str = ""
    report_date: str = ""
    issuance_time: datetime | None = None
    fetched_at: datetime | None = None
    source_url: str = ""
    provider: str = ""
    max_temp_f: float | None = None
    max_temp_c: float | None = None
    max_temp_time_utc: datetime | None = None
    min_temp_f: float | None = None
    min_temp_c: float | None = None
    min_temp_time_utc: datetime | None = None
    temp_f: float | None = None
    temp_c: float | None = None


class WeatherEvent(BaseModel):
    """Weather event lifecycle update."""

    model_config = ConfigDict(frozen=True)

    type: Literal["weather_event"] = "weather_event"
    event_id: str | None = None
    sequence: int | None = None
    city_sequence: int | None = None
    emitted_at: datetime | None = None
    slug: str = ""
    station_id: RequiredText
    id: RequiredText
    event_type: RequiredText
    tier: str = ""
    state: str = ""
    name: str = ""
    badge: str = ""
    detail: str = ""
    summary: str = ""
    started_at: datetime | None = None
    last_confirmed_at: datetime | None = None
    ended_at: datetime | None = None
    source: WeatherEventSource | None = None


class NewHigh(BaseModel):
    """New running daily high value for a station."""

    model_config = ConfigDict(frozen=True)

    type: Literal["new_high"] = "new_high"
    event_id: str | None = None
    sequence: int | None = None
    city_sequence: int | None = None
    emitted_at: datetime | None = None
    slug: str = ""
    station_id: RequiredText
    value_f: float
    value_c: float
    prev_value_f: float | None = None
    observed_at: datetime | None = None
    is_from_report: bool = False
    report_type: str | None = None
    source_report_id: str | None = None


class NewLow(BaseModel):
    """New running daily low value for a station."""

    model_config = ConfigDict(frozen=True)

    type: Literal["new_low"] = "new_low"
    event_id: str | None = None
    sequence: int | None = None
    city_sequence: int | None = None
    emitted_at: datetime | None = None
    slug: str = ""
    station_id: RequiredText
    value_f: float
    value_c: float
    prev_value_f: float | None = None
    observed_at: datetime | None = None
    is_from_report: bool = False
    report_type: str | None = None
    source_report_id: str | None = None


class ShutdownEvent(BaseModel):
    """Runtime sentinel signaling clean strategy shutdown."""

    model_config = ConfigDict(frozen=True)

    type: Literal["shutdown"] = "shutdown"
    reason: str = ""


StrategyEvent = Annotated[
    Observation
    | PriceUpdate
    | ForecastUpdated
    | ForecastVersions
    | OracleScoresUpdated
    | StationReport
    | WeatherEvent
    | NewHigh
    | NewLow
    | ShutdownEvent,
    Field(discriminator="type"),
]

# Compatibility alias for later trader adoption.
EngineEvent = StrategyEvent
