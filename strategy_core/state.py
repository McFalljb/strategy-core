"""Read-only state view protocols and shared strategy-visible value objects."""

from __future__ import annotations

from dataclasses import dataclass, field
from types import MappingProxyType
from typing import TYPE_CHECKING, Literal, Protocol, runtime_checkable

if TYPE_CHECKING:
    from collections.abc import Mapping
    from datetime import datetime

FeeType = Literal["quadratic", "quadratic_with_maker_fees", "flat"]


@dataclass(frozen=True, slots=True)
class ForecastHourly:
    """Single hourly forecast point from a weather forecast response."""

    time: str = ""
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
class ModelForecast:
    """Single model forecast summary and its optional hourly detail."""

    model_id: str
    value: float
    version: str = ""
    updated_at: datetime | None = None
    run_issued_at: datetime | None = None
    hourly: tuple[ForecastHourly, ...] = field(default_factory=tuple)

    def __post_init__(self) -> None:
        object.__setattr__(self, "hourly", tuple(self.hourly))


@dataclass(frozen=True, slots=True)
class StationForecast:
    """Latest known forecast snapshot for one station."""

    model_forecasts: Mapping[str, ModelForecast] = field(default_factory=dict)
    updated_at: datetime | None = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "model_forecasts", MappingProxyType(dict(self.model_forecasts)))


@dataclass(frozen=True, slots=True)
class OracleModelScore:
    """One oracle score row for a station/model combination."""

    model_id: str
    model_name: str = ""
    combined_mae: float | None = None
    high_mae: float | None = None
    low_mae: float | None = None
    high_bias: float | None = None
    low_bias: float | None = None
    day_count: int | None = None
    is_public: bool | None = None


@dataclass(frozen=True, slots=True)
class StationOracleScores:
    """Most recent fetched oracle ranking table for one station."""

    station_id: str
    scores: tuple[OracleModelScore, ...] = field(default_factory=tuple)
    rank_by: str = ""
    score_mode: str = ""
    days_requested: str = ""
    range_start: str = ""
    range_end: str = ""
    updated_at: datetime | None = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "scores", tuple(self.scores))


@dataclass(frozen=True, slots=True)
class StationWeather:
    """Latest known weather summary for a station."""

    current_temp: float | None = None
    running_high: float | None = None
    running_low: float | None = None
    last_metar_time: datetime | None = None
    temp_min_f: float | None = None
    temp_max_f: float | None = None
    temp_min_c: float | None = None
    temp_max_c: float | None = None
    preliminary: bool = False
    lag_seconds: int | None = None
    wu_current_temp_f: float | None = None
    wu_current_temp_c: float | None = None
    wu_daily_high_f: float | None = None
    wu_daily_low_f: float | None = None
    wu_daily_high_c: float | None = None
    wu_daily_low_c: float | None = None
    wu_observation_time: datetime | None = None
    wu_fetched_at: datetime | None = None
    asos_daily_high_f: float | None = None
    asos_daily_low_f: float | None = None
    dewpoint: float | None = None
    heat_index: float | None = None
    wind_chill: float | None = None
    relative_humidity: float | None = None
    wind_speed: float | None = None
    wind_direction: float | None = None
    wind_gust: float | None = None
    text_description: str | None = None
    dsm_high: float | None = None
    dsm_low: float | None = None
    dsm_high_time: datetime | None = None
    dsm_low_time: datetime | None = None
    six_hr_high: float | None = None
    six_hr_low: float | None = None
    last_dsm_time: datetime | None = None
    last_six_hr_time: datetime | None = None


@dataclass(frozen=True, slots=True)
class TickerPrices:
    """Latest known market snapshot for one bracket ticker."""

    ticker: str = ""
    source: str = ""
    event_ticker: str = ""
    event_date: str = ""
    series_ticker: str = ""
    fee_type: FeeType | str = ""
    fee_multiplier: float | None = None
    strike_type: str = ""
    floor_strike: float | None = None
    cap_strike: float | None = None
    yes_price: float = 0.0
    no_price: float = 0.0
    yes_bid: float | None = None
    yes_ask: float | None = None
    no_bid: float | None = None
    no_ask: float | None = None
    orderbook_depth: int | None = None
    volume: float | None = None
    peak_yes_ask: float | None = None
    last_update: datetime | None = None


@runtime_checkable
class MarketStateView(Protocol):
    """Read-only latest-known state surface exposed to strategies."""

    def get_weather(self, station: str) -> StationWeather | None: ...

    def get_forecast(self, station: str) -> StationForecast | None: ...

    def get_oracle_scores(self, station: str) -> StationOracleScores | None: ...

    def get_prices(self, ticker: str) -> TickerPrices | None: ...
