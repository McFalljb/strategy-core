//! Borrowed kernel views over the canonical model.
//!
//! Every view presents convenience `f64` fields beside the supplied original it was projected
//! from (`supplied`) and an [`ValueOrigin`] that says whether those conveniences came from a
//! supplied original or from the host's derived representation. Whole-contract quantities are
//! explicit floors of the exact hundredths carried next to them.

use std::borrow::Cow;

use chrono::{DateTime, Utc};

use crate::actions::{ContractQuantity, OrderUpdate};
use crate::supplied::{
    SuppliedExtreme, SuppliedForecast, SuppliedForecastModel, SuppliedObservation,
    SuppliedOracleTable, SuppliedReport, SuppliedWeatherEvent,
};

/// Whether a component's convenience values were projected from a supplied original or from
/// the host's derived (fixed-point or historical) representation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ValueOrigin {
    /// Projected from the provider's supplied original at its original precision.
    Supplied,
    /// Projected from a derived host representation; no supplied original exists.
    #[default]
    Derived,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PriceLevelView {
    pub price: f64,
    /// Whole contracts: the floor of `exact`.
    pub quantity: i64,
    /// Exact resting quantity in hundredths of a contract.
    pub exact: ContractQuantity,
}

impl PriceLevelView {
    /// A level whose exact quantity is a whole number of contracts.
    pub const fn whole(price: f64, quantity: i64) -> Self {
        Self {
            price,
            quantity,
            exact: match ContractQuantity::checked_from_whole_contracts(quantity) {
                Some(exact) => exact,
                None => ContractQuantity::from_hundredths(i64::MAX),
            },
        }
    }

    /// A level from its exact hundredths; `quantity` is the whole-contract floor.
    pub const fn exact(price: f64, exact: ContractQuantity) -> Self {
        Self {
            price,
            quantity: exact.hundredths().div_euclid(100),
            exact,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MarketBracketView<'a> {
    pub market_id: &'a str,
    pub ticker: &'a str,
    pub yes_price: f64,
    pub no_price: f64,
    pub event_ticker: &'a str,
    pub event_date: &'a str,
    pub close_time: Option<DateTime<Utc>>,
    pub strike_type: &'a str,
    pub floor_strike: Option<f64>,
    pub cap_strike: Option<f64>,
    pub snapshot_time: Option<DateTime<Utc>>,
    pub yes_bid: Option<f64>,
    pub yes_ask: Option<f64>,
    pub no_bid: Option<f64>,
    pub no_ask: Option<f64>,
    /// Whole-contract floors of the exact quantities below.
    pub yes_bid_depth: Option<i64>,
    pub yes_ask_depth: Option<i64>,
    pub no_bid_depth: Option<i64>,
    pub no_ask_depth: Option<i64>,
    pub yes_bid_quantity: Option<ContractQuantity>,
    pub yes_ask_quantity: Option<ContractQuantity>,
    pub no_bid_quantity: Option<ContractQuantity>,
    pub no_ask_quantity: Option<ContractQuantity>,
    pub yes_bid_levels: &'a [PriceLevelView],
    pub yes_ask_levels: &'a [PriceLevelView],
    pub no_bid_levels: &'a [PriceLevelView],
    pub no_ask_levels: &'a [PriceLevelView],
    pub orderbook_depth: Option<i64>,
    pub volume: Option<f64>,
    pub volume_exact: Option<ContractQuantity>,
    pub volume_24h: Option<ContractQuantity>,
    pub open_interest: Option<ContractQuantity>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PriceUpdateView<'a> {
    pub event_id: Option<&'a str>,
    pub sequence: Option<i64>,
    pub city_sequence: Option<i64>,
    pub emitted_at: Option<DateTime<Utc>>,
    pub source: &'a str,
    pub slug: &'a str,
    pub station_id: &'a str,
    pub city_id: &'a str,
    pub timestamp: Option<DateTime<Utc>>,
    pub markets: &'a [MarketBracketView<'a>],
}

/// Provider event identity and producer metadata shared by every event view.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EventProvenanceView<'a> {
    pub provider: &'a str,
    pub source: &'a str,
    pub event_key: Option<&'a str>,
    pub source_timestamp: Option<DateTime<Utc>>,
    pub wmo_emit_time: Option<DateTime<Utc>>,
    pub producer_received_at: Option<DateTime<Utc>>,
    pub live_published_at: Option<DateTime<Utc>>,
    pub persistence_status: Option<&'a str>,
    pub producer_sequence: Option<i64>,
    /// Host receipt time; host evidence, never provider data.
    pub received_at: Option<DateTime<Utc>>,
    pub connection_epoch: Option<u64>,
    pub sid: Option<u64>,
    pub received_frame_ordinal: Option<u64>,
    pub acceptance: Option<&'a crate::state::EventAcceptance>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ObservationView<'a> {
    pub event_id: Option<&'a str>,
    pub sequence: Option<i64>,
    pub city_sequence: Option<i64>,
    pub emitted_at: Option<DateTime<Utc>>,
    pub slug: &'a str,
    pub station_id: &'a str,
    pub observed_at: Option<DateTime<Utc>>,
    pub lag_seconds: Option<i64>,
    pub preliminary: bool,
    pub temperature_f: Option<f64>,
    pub temperature_c: Option<f64>,
    pub temp_min_f: Option<f64>,
    pub temp_max_f: Option<f64>,
    pub temp_min_c: Option<f64>,
    pub temp_max_c: Option<f64>,
    pub is_from_report: bool,
    pub report_type: Option<&'a str>,
    pub source_report_id: Option<&'a str>,
    pub temperature_day_mode: Option<&'a str>,
    pub temperature_day_date: Option<&'a str>,
    pub dewpoint: Option<f64>,
    pub heat_index: Option<f64>,
    pub wind_chill: Option<f64>,
    pub relative_humidity: Option<f64>,
    pub wind_speed: Option<f64>,
    pub wind_direction: Option<f64>,
    pub wind_gust: Option<f64>,
    pub text_description: Option<&'a str>,
    pub barometric_pressure: Option<f64>,
    pub sea_level_pressure: Option<f64>,
    pub precipitation_1h: Option<f64>,
    pub precipitation_3h: Option<f64>,
    pub precipitation_6h: Option<f64>,
    pub is_locf: Option<bool>,
    pub provenance: EventProvenanceView<'a>,
    pub origin: ValueOrigin,
    pub supplied: Option<&'a SuppliedObservation>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StationReportView<'a> {
    pub event_id: Option<&'a str>,
    pub sequence: Option<i64>,
    pub city_sequence: Option<i64>,
    pub emitted_at: Option<DateTime<Utc>>,
    pub slug: &'a str,
    pub station_id: &'a str,
    pub report_id: &'a str,
    pub report_type: &'a str,
    pub report_date: &'a str,
    pub report_revision: i64,
    pub report_updated_at: Option<DateTime<Utc>>,
    pub issuance_time: Option<DateTime<Utc>>,
    pub fetched_at: Option<DateTime<Utc>>,
    pub source_url: &'a str,
    pub provider: &'a str,
    pub max_temp_f: Option<f64>,
    pub max_temp_c: Option<f64>,
    pub min_temp_f: Option<f64>,
    pub min_temp_c: Option<f64>,
    pub temp_f: Option<f64>,
    pub temp_c: Option<f64>,
    pub max_temp_time_utc: Option<DateTime<Utc>>,
    pub min_temp_time_utc: Option<DateTime<Utc>>,
    pub report_fingerprint: Option<&'a str>,
    pub provenance: EventProvenanceView<'a>,
    pub origin: ValueOrigin,
    pub supplied: Option<&'a SuppliedReport>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StationWeatherView {
    pub station_id: String,
    pub current_temp: Option<f64>,
    pub running_high: Option<f64>,
    pub running_low: Option<f64>,
    pub last_metar_time: Option<DateTime<Utc>>,
    pub temp_min_f: Option<f64>,
    pub temp_max_f: Option<f64>,
    pub temp_min_c: Option<f64>,
    pub temp_max_c: Option<f64>,
    pub preliminary: bool,
    pub dsm_high: Option<f64>,
    pub dsm_low: Option<f64>,
    pub dsm_high_time: Option<DateTime<Utc>>,
    pub dsm_low_time: Option<DateTime<Utc>>,
    pub six_hr_high: Option<f64>,
    pub six_hr_low: Option<f64>,
    pub last_dsm_time: Option<DateTime<Utc>>,
    pub last_six_hr_time: Option<DateTime<Utc>>,
    pub asos_daily_high_f: Option<f64>,
    pub asos_daily_low_f: Option<f64>,
    pub dewpoint: Option<f64>,
    pub heat_index: Option<f64>,
    pub wind_chill: Option<f64>,
    pub relative_humidity: Option<f64>,
    pub wind_speed: Option<f64>,
    pub wind_direction: Option<f64>,
    pub wind_gust: Option<f64>,
    pub text_description: Option<String>,
    pub lag_seconds: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ForecastHourlySnapshot<'a> {
    pub time: &'a str,
    pub temperature_2m_f: Option<f64>,
    pub temperature_2m_c: Option<f64>,
    pub apparent_temperature_f: Option<f64>,
    pub apparent_temperature_c: Option<f64>,
    pub relative_humidity_2m: Option<f64>,
    pub dew_point_2m: Option<f64>,
    pub pressure_msl: Option<f64>,
    pub wind_speed_10m: Option<f64>,
    pub wind_direction_10m: Option<f64>,
    pub wind_gusts_10m: Option<f64>,
    pub cloud_cover: Option<f64>,
    pub precipitation_probability: Option<f64>,
    pub weather_code: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ForecastModelSnapshot<'a> {
    pub model_id: &'a str,
    /// Derived summary retained for the frozen kernel contract: the maximum point temperature.
    pub value: f64,
    /// The version the provider advertised for this model.
    pub version: &'a str,
    /// Fetch time of the model run, when supplied.
    pub updated_at: Option<DateTime<Utc>>,
    /// Issuance time of the model run, when supplied; independent of `updated_at`.
    pub run_issued_at: Option<DateTime<Utc>>,
    pub run_id: Option<&'a str>,
    pub timezone: Option<&'a str>,
    pub utc_offset_seconds: Option<i64>,
    pub hourly: Cow<'a, [ForecastHourlySnapshot<'a>]>,
    pub origin: ValueOrigin,
    pub supplied: Option<&'a SuppliedForecastModel>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ForecastInputSnapshot<'a> {
    pub station_id: &'a str,
    pub received_at: Option<DateTime<Utc>>,
    pub source: &'a str,
    pub models: Cow<'a, [ForecastModelSnapshot<'a>]>,
    /// Versions the provider advertised per model id, when supplied.
    pub advertised_versions: Cow<'a, [(String, String)]>,
    pub origin: ValueOrigin,
    pub supplied: Option<&'a SuppliedForecast>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct OracleModelScoreSnapshot<'a> {
    pub model_id: &'a str,
    pub model_name: &'a str,
    pub is_public: Option<bool>,
    pub high_mae: Option<f64>,
    pub low_mae: Option<f64>,
    pub combined_mae: Option<f64>,
    pub high_bias: Option<f64>,
    pub low_bias: Option<f64>,
    pub day_count: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct OracleInputSnapshot<'a> {
    pub station_id: &'a str,
    pub received_at: Option<DateTime<Utc>>,
    pub source: &'a str,
    pub score_mode: &'a str,
    pub rank_by: &'a str,
    pub days_requested: &'a str,
    pub range_start: &'a str,
    pub range_end: &'a str,
    pub scores: Cow<'a, [OracleModelScoreSnapshot<'a>]>,
    pub all_time: Option<bool>,
    pub updated_at: Option<DateTime<Utc>>,
    pub notification_modes: &'a [String],
    pub origin: ValueOrigin,
    pub supplied: Option<&'a SuppliedOracleTable>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ForecastUpdatedView<'a> {
    pub event_id: Option<&'a str>,
    pub sequence: Option<i64>,
    pub emitted_at: Option<DateTime<Utc>>,
    pub slug: &'a str,
    pub station_id: &'a str,
    pub model_id: &'a str,
    pub version: &'a str,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct OracleScoresUpdatedView<'a> {
    pub event_id: Option<&'a str>,
    pub sequence: Option<i64>,
    pub emitted_at: Option<DateTime<Utc>>,
    pub slug: &'a str,
    pub station_id: &'a str,
    pub modes: &'a [String],
    pub updated_at: Option<DateTime<Utc>>,
    pub overall: Option<OracleInputSnapshot<'a>>,
    pub day_ahead: Option<OracleInputSnapshot<'a>>,
    pub day_of: Option<OracleInputSnapshot<'a>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WeatherEventSourceView<'a> {
    pub metar_type: Option<&'a str>,
    pub flight_category: Option<&'a str>,
    pub wx_string: Option<&'a str>,
    pub wx_token: Option<&'a str>,
    pub wind_speed_kt: Option<f64>,
    pub wind_gust_kt: Option<f64>,
    pub peak_wind_kt: Option<f64>,
    pub peak_wind_direction: Option<i64>,
    pub visibility_mi: Option<f64>,
    pub cb_location: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WeatherEventView<'a> {
    pub event_id: Option<&'a str>,
    pub sequence: Option<i64>,
    pub city_sequence: Option<i64>,
    pub emitted_at: Option<DateTime<Utc>>,
    pub slug: &'a str,
    pub station_id: &'a str,
    pub id: &'a str,
    pub event_type_name: &'a str,
    pub tier: &'a str,
    pub state: &'a str,
    pub name: &'a str,
    pub badge: &'a str,
    pub detail: &'a str,
    pub summary: &'a str,
    pub started_at: Option<DateTime<Utc>>,
    pub last_confirmed_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub source: Option<WeatherEventSourceView<'a>>,
    pub provenance: EventProvenanceView<'a>,
    pub origin: ValueOrigin,
    pub supplied: Option<&'a SuppliedWeatherEvent>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HighLowView<'a> {
    pub event_id: Option<&'a str>,
    pub sequence: Option<i64>,
    pub city_sequence: Option<i64>,
    pub emitted_at: Option<DateTime<Utc>>,
    pub event_key: &'a str,
    pub source_timestamp: Option<DateTime<Utc>>,
    pub wmo_emit_time: Option<DateTime<Utc>>,
    pub producer_received_at: Option<DateTime<Utc>>,
    pub live_published_at: Option<DateTime<Utc>>,
    pub persistence_status: Option<&'a str>,
    pub producer_sequence: Option<i64>,
    pub slug: &'a str,
    pub station_id: &'a str,
    pub value_f: f64,
    pub value_c: f64,
    pub prev_value_f: Option<f64>,
    pub observed_at: Option<DateTime<Utc>>,
    pub temperature_day_mode: Option<&'a str>,
    pub temperature_day_date: Option<&'a str>,
    pub is_from_report: bool,
    pub report_type: Option<&'a str>,
    pub source_report_id: Option<&'a str>,
    pub provenance: EventProvenanceView<'a>,
    pub origin: ValueOrigin,
    pub supplied: Option<&'a SuppliedExtreme>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TimerWakeView<'a> {
    pub scheduled_for: DateTime<Utc>,
    pub fired_at: Option<DateTime<Utc>>,
    pub name: &'a str,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TickerPriceView<'a> {
    pub ticker: &'a str,
    pub source: &'a str,
    pub event_ticker: &'a str,
    pub event_date: &'a str,
    pub series_ticker: &'a str,
    pub close_time: Option<DateTime<Utc>>,
    pub fee_type: &'a str,
    pub fee_multiplier: Option<f64>,
    pub strike_type: &'a str,
    pub floor_strike: Option<f64>,
    pub cap_strike: Option<f64>,
    pub yes_price: f64,
    pub no_price: f64,
    pub yes_bid: Option<f64>,
    pub yes_ask: Option<f64>,
    pub no_bid: Option<f64>,
    pub no_ask: Option<f64>,
    /// Whole-contract floors of the exact quantities below.
    pub yes_bid_depth: Option<i64>,
    pub yes_ask_depth: Option<i64>,
    pub no_bid_depth: Option<i64>,
    pub no_ask_depth: Option<i64>,
    pub yes_bid_quantity: Option<ContractQuantity>,
    pub yes_ask_quantity: Option<ContractQuantity>,
    pub no_bid_quantity: Option<ContractQuantity>,
    pub no_ask_quantity: Option<ContractQuantity>,
    pub yes_bid_levels: &'a [PriceLevelView],
    pub yes_ask_levels: &'a [PriceLevelView],
    pub no_bid_levels: &'a [PriceLevelView],
    pub no_ask_levels: &'a [PriceLevelView],
    pub orderbook_depth: Option<i64>,
    pub volume: Option<f64>,
    pub volume_exact: Option<ContractQuantity>,
    pub volume_24h: Option<ContractQuantity>,
    pub open_interest: Option<ContractQuantity>,
    pub last_price: Option<f64>,
    pub last_trade_quantity: Option<ContractQuantity>,
    pub expiration_time: Option<DateTime<Utc>>,
    pub lifecycle_status: Option<&'a str>,
    pub lifecycle_result: Option<&'a str>,
    pub peak_yes_ask: Option<f64>,
    pub last_update: Option<DateTime<Utc>>,
}

// Preserve the borrowed callback interface without a new per-event box allocation.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum StrategyEventView<'a> {
    PriceUpdate(PriceUpdateView<'a>),
    Observation(ObservationView<'a>),
    ForecastUpdated(ForecastUpdatedView<'a>),
    OracleScoresUpdated(OracleScoresUpdatedView<'a>),
    StationReport(StationReportView<'a>),
    WeatherEvent(WeatherEventView<'a>),
    NewHigh(HighLowView<'a>),
    NewLow(HighLowView<'a>),
    TimerWake(TimerWakeView<'a>),
    OrderUpdate(&'a OrderUpdate),
    Unknown {
        event_type: &'a str,
        emitted_at: Option<DateTime<Utc>>,
    },
}

impl StrategyEventView<'_> {
    #[must_use]
    pub fn event_type(&self) -> &str {
        match self {
            Self::PriceUpdate(_) => "price_update",
            Self::Observation(_) => "observation",
            Self::ForecastUpdated(_) => "forecast_updated",
            Self::OracleScoresUpdated(_) => "oracle_scores_updated",
            Self::StationReport(_) => "station_report",
            Self::WeatherEvent(_) => "weather_event",
            Self::NewHigh(_) => "new_high",
            Self::NewLow(_) => "new_low",
            Self::TimerWake(_) => "timer_wake",
            Self::OrderUpdate(_) => "order_update",
            Self::Unknown { event_type, .. } => event_type,
        }
    }
}
