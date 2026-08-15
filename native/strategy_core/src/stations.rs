use std::{collections::HashMap, error::Error, fmt, sync::LazyLock};

pub static ICAO_TO_CITY_CODES: LazyLock<HashMap<&'static str, &'static [&'static str]>> =
    LazyLock::new(|| {
        HashMap::from([
            ("KATL", &["TATL", "ATL"][..]),
            ("KAUS", &["AUS", "AU"][..]),
            ("KBOS", &["TBOS", "BOS"][..]),
            ("KDCA", &["TDC", "DC", "DCA"][..]),
            ("KDEN", &["DEN"][..]),
            ("KDFW", &["TDAL", "DAL", "DFW"][..]),
            ("KJFK", &["JFK"][..]),
            ("KHOU", &["THOU", "HOU"][..]),
            ("KLAS", &["TLV", "LV", "LAS"][..]),
            ("KLAX", &["LAX", "LA"][..]),
            ("KMDW", &["CHI", "MDW", "MW"][..]),
            ("KMIA", &["MIA", "MI"][..]),
            ("KMSP", &["TMIN", "MIN", "MSP"][..]),
            ("KMSY", &["TNOLA", "NOLA", "MSY"][..]),
            ("KNYC", &["NY"][..]),
            ("KOKC", &["TOKC", "OKC"][..]),
            ("KORD", &["ORD"][..]),
            ("KPHL", &["PHIL", "PHL"][..]),
            ("KPHX", &["TPHX", "PHX"][..]),
            ("KSAT", &["TSATX", "SATX", "SAT"][..]),
            ("KSEA", &["TSEA", "SEA"][..]),
            ("KSFO", &["TSFO", "SFO"][..]),
        ])
    });

pub static STATION_TIMEZONES: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("KATL", "America/New_York"),
        ("KAUS", "America/Chicago"),
        ("KBOS", "America/New_York"),
        ("KDCA", "America/New_York"),
        ("KDEN", "America/Denver"),
        ("KDFW", "America/Chicago"),
        ("KJFK", "America/New_York"),
        ("KHOU", "America/Chicago"),
        ("KLAS", "America/Los_Angeles"),
        ("KLAX", "America/Los_Angeles"),
        ("KMDW", "America/Chicago"),
        ("KMIA", "America/New_York"),
        ("KMSP", "America/Chicago"),
        ("KMSY", "America/Chicago"),
        ("KNYC", "America/New_York"),
        ("KOKC", "America/Chicago"),
        ("KORD", "America/Chicago"),
        ("KPHL", "America/New_York"),
        ("KPHX", "America/Phoenix"),
        ("KSAT", "America/Chicago"),
        ("KSEA", "America/Los_Angeles"),
        ("KSFO", "America/Los_Angeles"),
    ])
});

pub static CITY_TO_ICAO: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    ICAO_TO_CITY_CODES
        .iter()
        .flat_map(|(icao, cities)| cities.iter().map(move |city| (*city, *icao)))
        .collect()
});

pub static MARKET_TYPE_PREFIX: LazyLock<HashMap<&'static str, &'static str>> =
    LazyLock::new(|| HashMap::from([("high", "KXHIGH"), ("low", "KXLOWT")]));

pub static HOURLY_SERIES_BY_PROFILE: LazyLock<
    HashMap<(&'static str, &'static str), &'static [&'static str]>,
> = LazyLock::new(|| {
    HashMap::from([
        (("KDCA", "weather_company"), &["KXTEMPDCH"][..]),
        (
            ("KNYC", "weather_company"),
            &["KXTEMPNYCH", "KXHIGHNYD"][..],
        ),
        (("KAUS", "weather_company"), &["KXTEMPAUSH"][..]),
        (("KBOS", "weather_company"), &["KXTEMPBOSH"][..]),
        (("KMDW", "weather_company"), &["KXTEMPCHIH"][..]),
        (("KLAX", "weather_company"), &["KXTEMPLAXH"][..]),
        (("KMIA", "synoptic"), &["KXTEMPMIAH"][..]),
    ])
});

pub const TICKER_PREFIXES: [&str; 2] = ["KXHIGH", "KXLOWT"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StationError {
    UnknownMarketType(String),
    UnknownSettlementSource(String),
    MissingHourlySettlementSource,
    UnsupportedHourlyProfile {
        station: String,
        settlement_source: String,
    },
}

impl fmt::Display for StationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownMarketType(value) => {
                write!(
                    formatter,
                    "unknown market_type: {value:?} (expected 'high', 'low', or 'hourly')"
                )
            }
            Self::UnknownSettlementSource(value) => write!(
                formatter,
                "unknown settlement_source: {value:?} (expected 'weather_company' or 'synoptic')"
            ),
            Self::MissingHourlySettlementSource => write!(
                formatter,
                "hourly market type requires a settlement source; use hourly_series_for_station"
            ),
            Self::UnsupportedHourlyProfile {
                station,
                settlement_source,
            } => write!(
                formatter,
                "no verified hourly temperature profile for station {station} and settlement_source {settlement_source:?}"
            ),
        }
    }
}

impl Error for StationError {}

pub fn primary_city_code_for_series(station: &str) -> String {
    let station_upper = station.to_uppercase();
    match ICAO_TO_CITY_CODES.get(station_upper.as_str()) {
        Some(cities) => cities[0].to_string(),
        None => fallback_city_code(&station_upper),
    }
}

pub fn city_codes_for_market_type(station: &str, market_type: &str) -> Vec<String> {
    let station_upper = station.to_uppercase();
    let Some(city_codes) = ICAO_TO_CITY_CODES.get(station_upper.as_str()) else {
        return vec![fallback_city_code(&station_upper)];
    };

    if market_type != "low" {
        return city_codes.iter().map(|city| (*city).to_string()).collect();
    }

    let mut normalized = Vec::new();
    for city in *city_codes {
        let normalized_city = city.strip_prefix('T').unwrap_or(city);
        if !normalized
            .iter()
            .any(|existing| existing == normalized_city)
        {
            normalized.push(normalized_city.to_string());
        }
    }
    normalized
}

pub fn primary_city_code_for_market_type(station: &str, market_type: &str) -> String {
    city_codes_for_market_type(station, market_type)
        .into_iter()
        .next()
        .unwrap_or_default()
}

pub fn ticker_prefixes_for_station(
    station: &str,
    market_type: &str,
) -> Result<Vec<String>, StationError> {
    if market_type == "hourly" {
        return Err(StationError::MissingHourlySettlementSource);
    }
    let Some(prefix) = MARKET_TYPE_PREFIX.get(market_type) else {
        return Err(StationError::UnknownMarketType(market_type.to_string()));
    };

    Ok(city_codes_for_market_type(station, market_type)
        .into_iter()
        .map(|city| format!("{prefix}{city}"))
        .collect())
}

pub fn hourly_series_for_station(
    station: &str,
    settlement_source: &str,
) -> Result<Vec<String>, StationError> {
    if !matches!(settlement_source, "weather_company" | "synoptic") {
        return Err(StationError::UnknownSettlementSource(
            settlement_source.to_owned(),
        ));
    }

    let station_upper = station.to_ascii_uppercase();
    HOURLY_SERIES_BY_PROFILE
        .get(&(station_upper.as_str(), settlement_source))
        .map(|series| series.iter().map(|ticker| (*ticker).to_owned()).collect())
        .ok_or(StationError::UnsupportedHourlyProfile {
            station: station_upper,
            settlement_source: settlement_source.to_owned(),
        })
}

pub fn station_from_event_ticker(event_ticker: &str) -> Option<&'static str> {
    let upper = event_ticker.to_uppercase();
    let series = upper
        .split_once('-')
        .map_or(upper.as_str(), |(series, _)| series);
    if let Some(((station, _), _)) = HOURLY_SERIES_BY_PROFILE
        .iter()
        .find(|(_, hourly_series)| hourly_series.contains(&series))
    {
        return Some(*station);
    }
    for prefix in TICKER_PREFIXES {
        if let Some(rest) = upper.strip_prefix(prefix) {
            let city = rest.split_once('-').map_or(rest, |(city, _)| city);
            return CITY_TO_ICAO.get(city).copied();
        }
    }
    None
}

fn fallback_city_code(station_upper: &str) -> String {
    station_upper
        .strip_prefix('K')
        .unwrap_or(station_upper)
        .to_string()
}
