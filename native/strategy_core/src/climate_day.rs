use std::{collections::BTreeMap, error::Error, fmt, str::FromStr};

use chrono::{DateTime, Datelike, Duration, NaiveDate, Offset, TimeZone, Utc};
use chrono_tz::Tz;

use crate::stations::STATION_TIMEZONES;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClimateDayError {
    UnknownStationTimezone(String),
    InvalidStationTimezone {
        station: String,
        timezone_name: String,
    },
    DateOverflow(NaiveDate),
}

impl fmt::Display for ClimateDayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownStationTimezone(station) => {
                write!(formatter, "unknown timezone for station {station}")
            }
            Self::InvalidStationTimezone {
                station,
                timezone_name,
            } => {
                write!(
                    formatter,
                    "invalid timezone '{timezone_name}' for station {station}"
                )
            }
            Self::DateOverflow(date) => write!(formatter, "date overflow after {date}"),
        }
    }
}

impl Error for ClimateDayError {}

pub fn station_timezone(
    station: Option<&str>,
    station_timezones: Option<&BTreeMap<String, String>>,
) -> Result<Tz, ClimateDayError> {
    let Some(station) = station else {
        return Ok(chrono_tz::UTC);
    };
    let normalized = station.to_uppercase();
    let timezone_name = station_timezones
        .and_then(|timezones| timezones.get(&normalized).map(String::as_str))
        .or_else(|| STATION_TIMEZONES.get(normalized.as_str()).copied())
        .ok_or_else(|| ClimateDayError::UnknownStationTimezone(normalized.clone()))?;

    Tz::from_str(timezone_name).map_err(|_| ClimateDayError::InvalidStationTimezone {
        station: normalized,
        timezone_name: timezone_name.to_string(),
    })
}

pub fn parse_climate_date(raw: Option<&str>) -> Option<NaiveDate> {
    let raw = raw?;
    let value = raw.trim();
    if value.len() == 10 && value.as_bytes().get(4) == Some(&b'-') {
        return NaiveDate::parse_from_str(value, "%Y-%m-%d").ok();
    }
    if value.len() == 8 && value.chars().all(|character| character.is_ascii_digit()) {
        let is_yyyymmdd = value[0..4].parse::<i32>().ok()? >= 2000;
        let year = if is_yyyymmdd {
            value[0..4].parse::<i32>().ok()?
        } else {
            format!("20{}", &value[0..2]).parse::<i32>().ok()?
        };
        let month = if is_yyyymmdd {
            value[4..6].parse::<u32>().ok()?
        } else {
            value[2..4].parse::<u32>().ok()?
        };
        let day = if is_yyyymmdd {
            value[6..8].parse::<u32>().ok()?
        } else {
            value[4..6].parse::<u32>().ok()?
        };
        return NaiveDate::from_ymd_opt(year, month, day);
    }
    if value.len() == 6 && value.chars().all(|character| character.is_ascii_digit()) {
        let year = format!("20{}", &value[0..2]).parse::<i32>().ok()?;
        let month = value[2..4].parse::<u32>().ok()?;
        let day = value[4..6].parse::<u32>().ok()?;
        return NaiveDate::from_ymd_opt(year, month, day);
    }
    None
}

pub fn climate_day_date(
    station: Option<&str>,
    now: DateTime<Utc>,
    station_timezones: Option<&BTreeMap<String, String>>,
) -> Result<NaiveDate, ClimateDayError> {
    let timezone = station_timezone(station, station_timezones)?;
    let offset_seconds = standard_utc_offset_seconds(timezone, now.date_naive());
    Ok((now + Duration::seconds(offset_seconds)).date_naive())
}

pub fn climate_day_end(
    station: Option<&str>,
    event_date: NaiveDate,
    station_timezones: Option<&BTreeMap<String, String>>,
) -> Result<DateTime<Utc>, ClimateDayError> {
    let timezone = station_timezone(station, station_timezones)?;
    let offset_seconds = standard_utc_offset_seconds(timezone, event_date);
    let next_day = event_date
        .succ_opt()
        .ok_or(ClimateDayError::DateOverflow(event_date))?;
    let local_midnight = next_day
        .and_hms_opt(0, 0, 0)
        .ok_or(ClimateDayError::DateOverflow(event_date))?;
    Ok(DateTime::from_naive_utc_and_offset(
        local_midnight - Duration::seconds(offset_seconds),
        Utc,
    ))
}

pub fn climate_day_has_ended(
    station: Option<&str>,
    event_date: NaiveDate,
    now: DateTime<Utc>,
    station_timezones: Option<&BTreeMap<String, String>>,
) -> Result<bool, ClimateDayError> {
    Ok(now >= climate_day_end(station, event_date, station_timezones)?)
}

fn standard_utc_offset_seconds(timezone: Tz, reference_date: NaiveDate) -> i64 {
    [1, 7]
        .into_iter()
        .filter_map(|month| {
            timezone
                .with_ymd_and_hms(reference_date.year(), month, 1, 12, 0, 0)
                .single()
                .map(|value| value.offset().fix().local_minus_utc().into())
        })
        .min()
        .unwrap_or(0)
}
