"""NWS climate-day helpers for station-scoped weather markets."""

from __future__ import annotations

from datetime import UTC, date, datetime, time, timedelta, timezone
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

from strategy_core.stations import STATION_TIMEZONES

_DEFAULT_TIMEZONE = ZoneInfo("UTC")


def station_timezone(
    station: str | None,
    *,
    station_timezones: dict[str, str] | None = None,
) -> ZoneInfo:
    """Return the best-known IANA timezone for a station."""
    if station is None:
        return _DEFAULT_TIMEZONE

    normalized = station.upper()
    timezone_name = None
    if station_timezones is not None:
        timezone_name = station_timezones.get(normalized)
    if timezone_name is None:
        timezone_name = STATION_TIMEZONES.get(normalized)
    if timezone_name is None:
        msg = f"unknown timezone for station {normalized}"
        raise ValueError(msg)
    try:
        return ZoneInfo(timezone_name)
    except ZoneInfoNotFoundError as exc:
        msg = f"invalid timezone '{timezone_name}' for station {normalized}"
        raise ValueError(msg) from exc


def parse_climate_date(raw: str | date | None) -> date | None:
    """Parse Kalshi/NWS event-date strings into a calendar date."""
    if raw is None:
        return None
    if isinstance(raw, date):
        return raw
    s = raw.strip()
    if len(s) == 10 and s[4] == "-" and s[7] == "-":
        try:
            return date.fromisoformat(s)
        except ValueError:
            return None
    if len(s) == 8 and s.isdigit():
        is_yyyymmdd = int(s[0:4]) >= 2000
        year = int(s[0:4]) if is_yyyymmdd else int(f"20{s[0:2]}")
        month = int(s[4:6]) if is_yyyymmdd else int(s[2:4])
        day = int(s[6:8]) if is_yyyymmdd else int(s[4:6])
        try:
            return date(year, month, day)
        except ValueError:
            return None
    if len(s) == 6 and s.isdigit():
        try:
            return date(int(f"20{s[0:2]}"), int(s[2:4]), int(s[4:6]))
        except ValueError:
            return None
    return None


def climate_day_date(
    station: str | None,
    now: datetime | None = None,
    *,
    station_timezones: dict[str, str] | None = None,
) -> date:
    """Return the active NWS climate-day date for a station."""
    anchor = now or datetime.now(UTC)
    if anchor.tzinfo is None:
        anchor = anchor.replace(tzinfo=UTC)
    tz = station_timezone(station, station_timezones=station_timezones)
    standard_tz = _standard_timezone(tz, anchor.date())
    return anchor.astimezone(standard_tz).date()


def climate_day_end(
    station: str | None,
    event_date: date,
    *,
    station_timezones: dict[str, str] | None = None,
) -> datetime:
    """Return the UTC instant when an event date's NWS climate day ends."""
    tz = station_timezone(station, station_timezones=station_timezones)
    standard_tz = _standard_timezone(tz, event_date)
    end_standard = datetime.combine(event_date + timedelta(days=1), time.min, tzinfo=standard_tz)
    return end_standard.astimezone(UTC)


def climate_day_has_ended(
    station: str | None,
    event_date: date,
    now: datetime | None = None,
    *,
    station_timezones: dict[str, str] | None = None,
) -> bool:
    """Return whether the station's NWS climate day has ended for event_date."""
    anchor = now or datetime.now(UTC)
    if anchor.tzinfo is None:
        anchor = anchor.replace(tzinfo=UTC)
    return anchor.astimezone(UTC) >= climate_day_end(
        station,
        event_date,
        station_timezones=station_timezones,
    )


def _standard_timezone(tz: ZoneInfo, reference_date: date) -> timezone:
    offset = _standard_utc_offset(tz, reference_date)
    return timezone(offset, name=f"{tz.key}-standard")


def _standard_utc_offset(tz: ZoneInfo, reference_date: date) -> timedelta:
    candidates: list[timedelta] = []
    for month, day in ((1, 1), (7, 1)):
        offset = datetime(reference_date.year, month, day, 12, tzinfo=tz).utcoffset()
        if offset is not None:
            candidates.append(offset)
    if not candidates:
        return timedelta(0)
    return min(candidates)
