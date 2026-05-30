"""Shared ICAO station, timezone, and Kalshi city-code mappings."""

from __future__ import annotations

ICAO_TO_CITY_CODES: dict[str, list[str]] = {
    "KATL": ["TATL", "ATL"],
    "KAUS": ["AUS", "AU"],
    "KBOS": ["TBOS", "BOS"],
    "KDCA": ["TDC", "DC", "DCA"],
    "KDEN": ["DEN"],
    "KDFW": ["TDAL", "DAL", "DFW"],
    "KJFK": ["JFK"],
    "KHOU": ["THOU", "HOU"],
    "KLAS": ["TLV", "LV", "LAS"],
    "KLAX": ["LAX", "LA"],
    "KMDW": ["CHI", "MDW", "MW"],
    "KMIA": ["MIA", "MI"],
    "KMSP": ["TMIN", "MIN", "MSP"],
    "KMSY": ["TNOLA", "NOLA", "MSY"],
    "KNYC": ["NY"],
    "KOKC": ["TOKC", "OKC"],
    "KORD": ["ORD"],
    "KPHL": ["PHIL", "PHL"],
    "KPHX": ["TPHX", "PHX"],
    "KSAT": ["TSATX", "SATX", "SAT"],
    "KSEA": ["TSEA", "SEA"],
    "KSFO": ["TSFO", "SFO"],
}

STATION_TIMEZONES: dict[str, str] = {
    "KATL": "America/New_York",
    "KAUS": "America/Chicago",
    "KBOS": "America/New_York",
    "KDCA": "America/New_York",
    "KDEN": "America/Denver",
    "KDFW": "America/Chicago",
    "KJFK": "America/New_York",
    "KHOU": "America/Chicago",
    "KLAS": "America/Los_Angeles",
    "KLAX": "America/Los_Angeles",
    "KMDW": "America/Chicago",
    "KMIA": "America/New_York",
    "KMSP": "America/Chicago",
    "KMSY": "America/Chicago",
    "KNYC": "America/New_York",
    "KOKC": "America/Chicago",
    "KORD": "America/Chicago",
    "KPHL": "America/New_York",
    "KPHX": "America/Phoenix",
    "KSAT": "America/Chicago",
    "KSEA": "America/Los_Angeles",
    "KSFO": "America/Los_Angeles",
}

CITY_TO_ICAO: dict[str, str] = {city: icao for icao, cities in ICAO_TO_CITY_CODES.items() for city in cities}

MARKET_TYPE_PREFIX: dict[str, str] = {
    "high": "KXHIGH",
    "low": "KXLOWT",
}

TICKER_PREFIXES: tuple[str, ...] = tuple(MARKET_TYPE_PREFIX.values())


def primary_city_code_for_series(station: str) -> str:
    """Kalshi series ticker city suffix for one ICAO station."""
    station_upper = station.upper()
    cities = ICAO_TO_CITY_CODES.get(station_upper)
    if cities:
        return cities[0]
    return station_upper.lstrip("K") if station_upper.startswith("K") else station_upper


def city_codes_for_market_type(station: str, market_type: str) -> list[str]:
    """Kalshi city-code suffixes for a station and market type."""
    station_upper = station.upper()
    city_codes = ICAO_TO_CITY_CODES.get(station_upper)
    if city_codes is None:
        city = station_upper.lstrip("K") if station_upper.startswith("K") else station_upper
        return [city]
    if market_type != "low":
        return list(city_codes)

    normalized: list[str] = []
    for city in city_codes:
        normalized_city = city[1:] if city.startswith("T") else city
        if normalized_city not in normalized:
            normalized.append(normalized_city)
    return normalized


def primary_city_code_for_market_type(station: str, market_type: str) -> str:
    """Primary Kalshi city-code suffix for one station and market type."""
    return city_codes_for_market_type(station, market_type)[0]


def ticker_prefixes_for_station(station: str, market_type: str) -> list[str]:
    """Generate Kalshi event-ticker prefixes for a station and market type."""
    prefix = MARKET_TYPE_PREFIX.get(market_type)
    if prefix is None:
        msg = f"unknown market_type: {market_type!r} (expected 'high' or 'low')"
        raise ValueError(msg)

    return [f"{prefix}{city}" for city in city_codes_for_market_type(station, market_type)]


def station_from_event_ticker(event_ticker: str) -> str | None:
    """Derive an ICAO station from a Kalshi weather event ticker."""
    upper = event_ticker.upper()
    for prefix in TICKER_PREFIXES:
        if upper.startswith(prefix):
            rest = upper[len(prefix) :]
            dash_idx = rest.find("-")
            city = rest if dash_idx == -1 else rest[:dash_idx]
            return CITY_TO_ICAO.get(city)
    return None
