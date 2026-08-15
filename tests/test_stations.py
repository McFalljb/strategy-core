"""Tests for shared station and Kalshi ticker mapping helpers."""

from __future__ import annotations

import pytest

from strategy_core.stations import (
    CITY_TO_ICAO,
    HOURLY_SERIES_BY_PROFILE,
    ICAO_TO_CITY_CODES,
    MARKET_TYPE_PREFIX,
    STATION_TIMEZONES,
    TICKER_PREFIXES,
    hourly_series_for_station,
    primary_city_code_for_series,
    station_from_event_ticker,
    ticker_prefixes_for_station,
)


def test_city_to_icao_complete() -> None:
    for icao, cities in ICAO_TO_CITY_CODES.items():
        for city in cities:
            assert CITY_TO_ICAO[city] == icao


def test_market_type_prefix_values() -> None:
    assert MARKET_TYPE_PREFIX["high"] == "KXHIGH"
    assert MARKET_TYPE_PREFIX["low"] == "KXLOWT"
    assert "KXHIGH" in TICKER_PREFIXES
    assert "KXLOWT" in TICKER_PREFIXES


def test_ticker_prefixes_for_station_and_market_type() -> None:
    assert ticker_prefixes_for_station("KMIA", "high") == ["KXHIGHMIA", "KXHIGHMI"]
    assert ticker_prefixes_for_station("KMIA", "low") == ["KXLOWTMIA", "KXLOWTMI"]
    assert ticker_prefixes_for_station("KAUS", "low") == ["KXLOWTAUS", "KXLOWTAU"]
    assert ticker_prefixes_for_station("KMDW", "high") == ["KXHIGHCHI", "KXHIGHMDW", "KXHIGHMW"]
    assert ticker_prefixes_for_station("KDFW", "low") == ["KXLOWTDAL", "KXLOWTDFW"]


def test_primary_city_code_and_ticker_station_reverse_lookup() -> None:
    assert primary_city_code_for_series("KNYC") == "NY"
    assert primary_city_code_for_series("KMIA") == "MIA"
    assert primary_city_code_for_series("KMDW") == "CHI"
    assert primary_city_code_for_series("KDFW") == "TDAL"
    assert station_from_event_ticker("KXHIGHMI-260403") == "KMIA"
    assert station_from_event_ticker("KXLOWTDAL-260403") == "KDFW"
    assert station_from_event_ticker("OTHER-260403") is None


@pytest.mark.parametrize(
    ("station", "settlement_source", "series"),
    [
        ("KDCA", "weather_company", ["KXTEMPDCH"]),
        ("KNYC", "weather_company", ["KXTEMPNYCH", "KXHIGHNYD"]),
        ("KAUS", "weather_company", ["KXTEMPAUSH"]),
        ("KBOS", "weather_company", ["KXTEMPBOSH"]),
        ("KMDW", "weather_company", ["KXTEMPCHIH"]),
        ("KLAX", "weather_company", ["KXTEMPLAXH"]),
        ("KMIA", "synoptic", ["KXTEMPMIAH"]),
    ],
)
def test_verified_hourly_profiles_are_source_specific(station: str, settlement_source: str, series: list[str]) -> None:
    assert hourly_series_for_station(station, settlement_source) == series


@pytest.mark.parametrize(
    ("station", "settlement_source"),
    [
        ("KMIA", "weather_company"),
        ("KNYC", "synoptic"),
        ("KATL", "weather_company"),
    ],
)
def test_unsupported_hourly_profile_fails_closed(station: str, settlement_source: str) -> None:
    with pytest.raises(ValueError, match="no verified hourly temperature profile"):
        hourly_series_for_station(station, settlement_source)


def test_unknown_hourly_source_fails_closed() -> None:
    with pytest.raises(ValueError, match="unknown settlement_source"):
        hourly_series_for_station("KNYC", "weather.com")


@pytest.mark.parametrize(("station", "source"), HOURLY_SERIES_BY_PROFILE)
def test_every_hourly_series_has_exact_reverse_lookup(station: str, source: str) -> None:
    for series in hourly_series_for_station(station, source):
        assert station_from_event_ticker(series) == station
        assert station_from_event_ticker(f"{series}-26AUG1511") == station
        assert station_from_event_ticker(f"{series}EXTRA-26AUG1511") is None


def test_invalid_market_type_and_timezone_coverage() -> None:
    with pytest.raises(ValueError, match="unknown market_type"):
        ticker_prefixes_for_station("KMIA", "invalid")
    with pytest.raises(ValueError, match="settlement source"):
        ticker_prefixes_for_station("KNYC", "hourly")

    for icao in ICAO_TO_CITY_CODES:
        assert icao in STATION_TIMEZONES, f"missing timezone for {icao}"
        assert STATION_TIMEZONES[icao]
