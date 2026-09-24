//! Pure station, climate-day and freshness helpers. Station and climate-day expectations are
//! the legacy contract's examples (Python `test_stations.py`/`test_climate_day.py` and the
//! legacy Rust contract tests), kept here before the legacy crate is deleted.

use std::collections::BTreeMap;

use chrono::{Duration, NaiveDate, TimeZone, Utc};
use strategy_core_kernel::{
    ComponentAuthority, ComponentMeta,
    climate_day::{
        ClimateDayError, climate_day_date, climate_day_end, climate_day_has_ended,
        parse_climate_date, station_timezone,
    },
    freshness::FreshnessStatus,
    stations::{
        CITY_TO_ICAO, HOURLY_SERIES_BY_PROFILE, ICAO_TO_CITY_CODES, MARKET_TYPE_PREFIX,
        STATION_TIMEZONES, StationError, TICKER_PREFIXES, city_codes_for_market_type,
        hourly_series_for_station, primary_city_code_for_market_type, primary_city_code_for_series,
        station_from_event_ticker, ticker_prefixes_for_station,
    },
};

fn date(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

#[test]
fn station_mappings_match_the_legacy_contract() {
    assert_eq!(MARKET_TYPE_PREFIX["high"], "KXHIGH");
    assert_eq!(MARKET_TYPE_PREFIX["low"], "KXLOWT");
    assert_eq!(TICKER_PREFIXES, ["KXHIGH", "KXLOWT"]);
    assert_eq!(ICAO_TO_CITY_CODES.len(), 22);
    for (icao, cities) in ICAO_TO_CITY_CODES.iter() {
        assert!(STATION_TIMEZONES.contains_key(icao));
        for city in *cities {
            assert_eq!(CITY_TO_ICAO[city], *icao);
        }
    }
    assert_eq!(CITY_TO_ICAO["NY"], "KNYC");
    assert_eq!(CITY_TO_ICAO["CHI"], "KMDW");

    assert_eq!(
        ticker_prefixes_for_station("KMIA", "high").unwrap(),
        ["KXHIGHMIA", "KXHIGHMI"]
    );
    assert_eq!(
        ticker_prefixes_for_station("KMIA", "low").unwrap(),
        ["KXLOWTMIA", "KXLOWTMI"]
    );
    assert_eq!(
        ticker_prefixes_for_station("KAUS", "low").unwrap(),
        ["KXLOWTAUS", "KXLOWTAU"]
    );
    assert_eq!(
        ticker_prefixes_for_station("KMDW", "high").unwrap(),
        ["KXHIGHCHI", "KXHIGHMDW", "KXHIGHMW"]
    );
    assert_eq!(
        ticker_prefixes_for_station("KDFW", "low").unwrap(),
        ["KXLOWTDAL", "KXLOWTDFW"]
    );
    assert_eq!(
        ticker_prefixes_for_station("KMIA", "invalid"),
        Err(StationError::UnknownMarketType("invalid".to_owned()))
    );
    assert_eq!(
        ticker_prefixes_for_station("KNYC", "hourly"),
        Err(StationError::MissingHourlySettlementSource)
    );

    assert_eq!(primary_city_code_for_series("KNYC"), "NY");
    assert_eq!(primary_city_code_for_series("kmia"), "MIA");
    assert_eq!(primary_city_code_for_series("KMDW"), "CHI");
    assert_eq!(primary_city_code_for_series("KDFW"), "TDAL");
    assert_eq!(primary_city_code_for_series("KXYZ"), "XYZ");
    assert_eq!(primary_city_code_for_market_type("KMIA", "low"), "MIA");
    assert_eq!(primary_city_code_for_market_type("KDFW", "low"), "DAL");
    assert_eq!(city_codes_for_market_type("UNKNOWN", "high"), ["UNKNOWN"]);

    assert_eq!(station_from_event_ticker("KXHIGHMI-260403"), Some("KMIA"));
    assert_eq!(station_from_event_ticker("kxlowtdal-260403"), Some("KDFW"));
    assert_eq!(
        station_from_event_ticker("KXHIGHNY-26JUN26-B91.5"),
        Some("KNYC")
    );
    assert_eq!(station_from_event_ticker("OTHER-260403"), None);
}

#[test]
fn hourly_profiles_are_source_specific_and_reverse_exactly() {
    let cases = [
        ("KDCA", "weather_company", &["KXTEMPDCH"][..]),
        ("KNYC", "weather_company", &["KXTEMPNYCH", "KXHIGHNYD"][..]),
        ("KAUS", "weather_company", &["KXTEMPAUSH"][..]),
        ("KBOS", "weather_company", &["KXTEMPBOSH"][..]),
        ("KMDW", "weather_company", &["KXTEMPCHIH"][..]),
        ("KLAX", "weather_company", &["KXTEMPLAXH"][..]),
        ("KMIA", "synoptic", &["KXTEMPMIAH"][..]),
    ];
    for (station, source, expected) in cases {
        assert_eq!(
            hourly_series_for_station(station, source).unwrap(),
            expected
        );
    }
    assert_eq!(HOURLY_SERIES_BY_PROFILE.len(), cases.len());
    assert!(hourly_series_for_station("KMIA", "weather_company").is_err());
    assert!(hourly_series_for_station("KNYC", "synoptic").is_err());
    assert!(hourly_series_for_station("KATL", "weather_company").is_err());
    assert_eq!(
        hourly_series_for_station("KNYC", "weather.com"),
        Err(StationError::UnknownSettlementSource(
            "weather.com".to_owned()
        ))
    );
    for ((station, _), series_tickers) in HOURLY_SERIES_BY_PROFILE.iter() {
        for series in *series_tickers {
            assert_eq!(station_from_event_ticker(series), Some(*station));
            assert_eq!(
                station_from_event_ticker(&format!("{series}-26AUG1511")),
                Some(*station)
            );
            assert_eq!(
                station_from_event_ticker(&format!("{series}EXTRA-26AUG1511")),
                None
            );
        }
    }
}

#[test]
fn climate_day_uses_the_station_standard_time_boundary() {
    for (station, zone) in [
        ("KMIA", "America/New_York"),
        ("KNYC", "America/New_York"),
        ("KPHX", "America/Phoenix"),
        ("KDEN", "America/Denver"),
        ("KSEA", "America/Los_Angeles"),
        ("kdfw", "America/Chicago"),
    ] {
        assert_eq!(station_timezone(Some(station), None).unwrap().name(), zone);
    }
    assert_eq!(station_timezone(None, None).unwrap(), chrono_tz::UTC);
    assert_eq!(
        station_timezone(Some("EGLL"), None),
        Err(ClimateDayError::UnknownStationTimezone("EGLL".to_owned()))
    );
    let overrides = BTreeMap::from([("EGLL".to_owned(), "Europe/London".to_owned())]);
    assert_eq!(
        station_timezone(Some("EGLL"), Some(&overrides))
            .unwrap()
            .name(),
        "Europe/London"
    );
    assert_eq!(
        climate_day_date(
            Some("EGLL"),
            Utc.with_ymd_and_hms(2026, 4, 3, 0, 10, 0).unwrap(),
            Some(&overrides)
        )
        .unwrap(),
        date(2026, 4, 3)
    );
    let invalid = BTreeMap::from([("EGLL".to_owned(), "Mars/Base".to_owned())]);
    assert!(station_timezone(Some("EGLL"), Some(&invalid)).is_err());

    let before = Utc.with_ymd_and_hms(2026, 4, 2, 4, 55, 0).unwrap();
    let after = Utc.with_ymd_and_hms(2026, 4, 2, 5, 5, 0).unwrap();
    assert_eq!(
        climate_day_date(Some("KMIA"), before, None).unwrap(),
        date(2026, 4, 1)
    );
    assert_eq!(
        climate_day_date(Some("KMIA"), after, None).unwrap(),
        date(2026, 4, 2)
    );

    let event_date = date(2026, 7, 4);
    let end = Utc.with_ymd_and_hms(2026, 7, 5, 5, 0, 0).unwrap();
    assert_eq!(
        climate_day_end(Some("KMIA"), event_date, None).unwrap(),
        end
    );
    assert!(
        !climate_day_has_ended(Some("KMIA"), event_date, end - Duration::minutes(1), None).unwrap()
    );
    assert!(climate_day_has_ended(Some("KMIA"), event_date, end, None).unwrap());
    // Phoenix keeps standard time; Denver's summer day still ends at standard midnight.
    assert_eq!(
        climate_day_end(Some("KPHX"), event_date, None).unwrap(),
        Utc.with_ymd_and_hms(2026, 7, 5, 7, 0, 0).unwrap()
    );
    assert_eq!(
        climate_day_end(Some("KDEN"), event_date, None).unwrap(),
        Utc.with_ymd_and_hms(2026, 7, 5, 7, 0, 0).unwrap()
    );
    assert_eq!(
        climate_day_end(Some("KSEA"), date(2026, 1, 15), None).unwrap(),
        Utc.with_ymd_and_hms(2026, 1, 16, 8, 0, 0).unwrap()
    );
}

#[test]
fn climate_dates_parse_the_known_event_formats() {
    assert_eq!(
        parse_climate_date(Some("2026-04-03")),
        Some(date(2026, 4, 3))
    );
    assert_eq!(
        parse_climate_date(Some(" 20260403 ")),
        Some(date(2026, 4, 3))
    );
    assert_eq!(parse_climate_date(Some("260403")), Some(date(2026, 4, 3)));
    assert_eq!(parse_climate_date(Some("bad")), None);
    assert_eq!(parse_climate_date(Some("2026-99-99")), None);
    assert_eq!(parse_climate_date(None), None);
}

#[test]
fn freshness_is_the_component_age_at_the_decision_time() {
    let now = Utc.with_ymd_and_hms(2026, 9, 24, 12, 0, 0).unwrap();
    let limit = Duration::seconds(90);
    let meta = |updated_at| ComponentMeta {
        updated_at,
        ..ComponentMeta::default()
    };

    let fresh = meta(Some(now - Duration::seconds(90)));
    let freshness = fresh.freshness_at(now, limit);
    assert_eq!(freshness.status, FreshnessStatus::Fresh);
    assert!(freshness.is_fresh());
    assert_eq!(freshness.age, Some(Duration::seconds(90)));

    let stale = meta(Some(now - Duration::milliseconds(90_001)));
    assert_eq!(
        stale.freshness_at(now, limit).status,
        FreshnessStatus::Stale
    );
    assert_eq!(stale.age_at(now), Some(Duration::milliseconds(90_001)));

    let ahead = meta(Some(now + Duration::seconds(5)));
    assert_eq!(ahead.age_at(now), Some(Duration::zero()));
    assert!(ahead.freshness_at(now, limit).is_fresh());

    let missing = meta(None);
    assert_eq!(missing.age_at(now), None);
    assert_eq!(
        missing.freshness_at(now, limit).status,
        FreshnessStatus::Missing
    );

    let degraded = ComponentMeta {
        authority: ComponentAuthority::RefreshPending,
        refresh_error: Some("provider timeout".to_owned()),
        ..meta(Some(now))
    };
    let freshness = degraded.freshness_at(now, limit);
    assert_eq!(freshness.status, FreshnessStatus::Fresh);
    assert_eq!(freshness.authority, ComponentAuthority::RefreshPending);
    assert_eq!(freshness.refresh_error, Some("provider timeout"));
}
