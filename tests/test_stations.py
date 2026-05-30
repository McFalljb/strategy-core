"""Tests for shared station and Kalshi ticker mapping helpers."""

from __future__ import annotations

import pytest

from strategy_core.stations import (
    CITY_TO_ICAO,
    ICAO_TO_CITY_CODES,
    MARKET_TYPE_PREFIX,
    STATION_TIMEZONES,
    TICKER_PREFIXES,
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


def test_invalid_market_type_and_timezone_coverage() -> None:
    with pytest.raises(ValueError, match="unknown market_type"):
        ticker_prefixes_for_station("KMIA", "invalid")

    for icao in ICAO_TO_CITY_CODES:
        assert icao in STATION_TIMEZONES, f"missing timezone for {icao}"
        assert STATION_TIMEZONES[icao]
