//! Decision Context V4: the bounded, self-contained Trader-to-Strategy state contract.
//!
//! The representation is canonical because every collection is ordered before construction and
//! bincode uses a fixed, versioned configuration. It contains named typed fields rather than
//! provider payloads or lookup references.

use bincode::{Decode, Encode};

pub const DECISION_CONTEXT_V4_MAGIC: &[u8; 8] = b"SDCTXV4\0";
pub const MAX_DECISION_CONTEXT_V4_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_STATIONS: usize = 5;
pub const MAX_MARKETS: usize = 128;
pub const MAX_MODELS_PER_STATION: usize = 32;
pub const MAX_POINTS_PER_MODEL: usize = 100;
pub const MAX_BOOK_LEVELS_PER_SIDE: usize = 128;
pub const MAX_ORACLE_ROWS: usize = 32;
pub const MAX_REPORTS: usize = 8;
pub const MAX_WEATHER_EVENTS: usize = 32;
pub const MAX_EVIDENCE_REFERENCES: usize = 16;
pub const MAX_PROVENANCE_PER_COMPONENT: usize = 4;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecisionV4Error {
    Encode,
    Decode,
    BoundExceeded,
    TrailingBytes,
    InvalidContract,
}
impl core::fmt::Display for DecisionV4Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for DecisionV4Error {}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum TriggerV4 {
    Weather {
        station_id: String,
        source_generation: u64,
        source_sequence: u64,
    },
    StationReport {
        station_id: String,
        report_id: String,
        report_type: String,
        report_revision: u64,
        provider: String,
        source_generation: u64,
        source_sequence: u64,
    },
    MarketPrice {
        market_id: String,
        price_revision: u64,
    },
    Timer {
        key: String,
    },
    Bootstrap,
    Recovery,
}
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum AuthorityV4 {
    Warming,
    Current,
    RefreshPending,
    Uncertain,
    Unavailable,
}
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum RankByV4 {
    High,
    Low,
}
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum LifecycleV4 {
    Discovered,
    Hydrating,
    Eligible,
    Uncertain,
    Ineligible,
    Terminal,
}
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum MarketAuthorityV4 {
    Current,
    Uncertain,
}
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub enum GateStatusV4 {
    Open,
    ClosedFinanceUnavailable,
    ClosedAdmissionDenied,
    ClosedRecoveryRequired,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct ProvenanceV4 {
    pub provider: String,
    pub source: String,
    pub event_id: Option<String>,
    pub connection_epoch: Option<u64>,
    pub sid: Option<u64>,
    pub sequence: Option<u64>,
    pub city_sequence: Option<u64>,
    pub producer_sequence: Option<u64>,
    pub received_frame_ordinal: Option<u64>,
    pub provider_at_unix_ms: Option<i64>,
    pub received_at_unix_ms: i64,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct EvidenceV4 {
    pub kind: String,
    pub sha256: [u8; 32],
    pub reference: String,
    pub byte_count: u64,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct ComponentMetaV4 {
    pub authority: AuthorityV4,
    pub revision: u64,
    pub generation: u64,
    pub updated_at_unix_ms: Option<i64>,
    pub expected_version: Option<String>,
    pub provenance: Vec<ProvenanceV4>,
    pub refresh_error: Option<String>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct StationIdentityV4 {
    pub station_id: String,
    pub city_id: Option<String>,
    pub city_slug: Option<String>,
    pub logical_location: String,
    pub name: Option<String>,
    pub latitude_micros: Option<i64>,
    pub longitude_micros: Option<i64>,
    pub timezone: String,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct ObservationV4 {
    pub station_id: String,
    pub observed_at_unix_ms: i64,
    pub source_timestamp_unix_ms: Option<i64>,
    pub producer_received_at_unix_ms: Option<i64>,
    pub live_published_at_unix_ms: Option<i64>,
    pub lag_ms: Option<i64>,
    pub preliminary: bool,
    pub persistence_status: Option<String>,
    pub temperature_milli_c: Option<i32>,
    pub temperature_min_milli_c: Option<i32>,
    pub temperature_max_milli_c: Option<i32>,
    pub wu_current_temperature_milli_c: Option<i32>,
    pub wu_daily_high_milli_c: Option<i32>,
    pub wu_daily_low_milli_c: Option<i32>,
    pub wu_observation_at_unix_ms: Option<i64>,
    pub wu_fetched_at_unix_ms: Option<i64>,
    pub temperature_day_mode: Option<String>,
    pub temperature_day_date: Option<String>,
    pub wu_day_mode: Option<String>,
    pub wu_day_date: Option<String>,
    pub is_from_report: bool,
    pub report_type: Option<String>,
    pub source_report_id: Option<String>,
    pub dewpoint_micros: Option<i64>,
    pub heat_index_micros: Option<i64>,
    pub wind_chill_micros: Option<i64>,
    pub relative_humidity_micros: Option<i64>,
    pub wind_speed_micros: Option<i64>,
    pub wind_direction_micros: Option<i64>,
    pub wind_gust_micros: Option<i64>,
    pub text_description: Option<String>,
    pub provenance: ProvenanceV4,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct WeatherV4 {
    pub station_id: String,
    pub current_temperature_milli_c: Option<i32>,
    pub running_high_milli_c: Option<i32>,
    pub running_low_milli_c: Option<i32>,
    pub last_metar_at_unix_ms: Option<i64>,
    pub dsm_high_milli_c: Option<i32>,
    pub dsm_low_milli_c: Option<i32>,
    pub dsm_high_at_unix_ms: Option<i64>,
    pub dsm_low_at_unix_ms: Option<i64>,
    pub six_hour_high_milli_c: Option<i32>,
    pub six_hour_low_milli_c: Option<i32>,
    pub asos_daily_high_milli_c: Option<i32>,
    pub asos_daily_low_milli_c: Option<i32>,
    pub wu_current_temperature_milli_c: Option<i32>,
    pub wu_daily_high_milli_c: Option<i32>,
    pub wu_daily_low_milli_c: Option<i32>,
    pub dewpoint_micros: Option<i64>,
    pub heat_index_micros: Option<i64>,
    pub wind_chill_micros: Option<i64>,
    pub relative_humidity_micros: Option<i64>,
    pub wind_speed_micros: Option<i64>,
    pub wind_direction_micros: Option<i64>,
    pub wind_gust_micros: Option<i64>,
    pub text_description: Option<String>,
    pub preliminary: bool,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct ExtremeV4 {
    pub value_milli_c: i32,
    pub previous_value_milli_c: Option<i32>,
    pub observed_at_unix_ms: Option<i64>,
    pub temperature_day_mode: Option<String>,
    pub temperature_day_date: Option<String>,
    pub is_from_report: bool,
    pub report_type: Option<String>,
    pub source_report_id: Option<String>,
    pub provenance: ProvenanceV4,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct ExtremaV4 {
    pub high: Option<ExtremeV4>,
    pub low: Option<ExtremeV4>,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct ReportV4 {
    pub report_id: String,
    pub report_type: String,
    pub report_date: String,
    pub authority: AuthorityV4,
    pub revision: u64,
    pub generation: u64,
    pub issued_at_unix_ms: Option<i64>,
    pub updated_at_unix_ms: Option<i64>,
    pub fetched_at_unix_ms: Option<i64>,
    pub provider: String,
    pub source_url: String,
    pub max_temperature_milli_c: Option<i32>,
    pub min_temperature_milli_c: Option<i32>,
    pub temperature_milli_c: Option<i32>,
    pub temperature_milli_f: Option<i32>,
    pub max_temperature_at_unix_ms: Option<i64>,
    pub min_temperature_at_unix_ms: Option<i64>,
    pub provenance: ProvenanceV4,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct WeatherEventSourceV4 {
    pub metar_type: Option<String>,
    pub flight_category: Option<String>,
    pub wx_string: Option<String>,
    pub wx_token: Option<String>,
    pub wind_speed_knots_micros: Option<i64>,
    pub wind_gust_knots_micros: Option<i64>,
    pub peak_wind_knots_micros: Option<i64>,
    pub peak_wind_direction: Option<i64>,
    pub visibility_miles_micros: Option<i64>,
    pub cb_location: Option<String>,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct WeatherEventV4 {
    pub event_id: String,
    pub event_type: String,
    pub tier: String,
    pub state: String,
    pub authority: AuthorityV4,
    pub revision: u64,
    pub generation: u64,
    pub name: String,
    pub badge: String,
    pub detail: String,
    pub summary: String,
    pub started_at_unix_ms: Option<i64>,
    pub last_confirmed_at_unix_ms: Option<i64>,
    pub ended_at_unix_ms: Option<i64>,
    pub source: Option<WeatherEventSourceV4>,
    pub provenance: ProvenanceV4,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct ForecastPointV4 {
    pub at_unix_ms: i64,
    pub temperature_milli_c: Option<i32>,
    pub apparent_temperature_milli_c: Option<i32>,
    pub humidity_millionths: Option<i64>,
    pub dew_point_milli_c: Option<i32>,
    pub pressure_msl_micros: Option<i64>,
    pub wind_speed_micros: Option<i64>,
    pub wind_direction_degrees: Option<i32>,
    pub wind_gust_micros: Option<i64>,
    pub cloud_cover_millionths: Option<i64>,
    pub precipitation_probability_millionths: Option<i64>,
    pub weather_code: Option<i32>,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct ForecastModelV4 {
    pub model_id: String,
    pub version: String,
    pub run_id: Option<String>,
    pub fetched_at_unix_ms: Option<i64>,
    pub issued_at_unix_ms: Option<i64>,
    pub timezone: Option<String>,
    pub utc_offset_seconds: Option<i32>,
    pub hourly: Vec<ForecastPointV4>,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct ForecastV4 {
    pub advertised_versions: Vec<(String, String)>,
    pub models: Vec<ForecastModelV4>,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct OracleQueryV4 {
    pub station_id: String,
    pub mode: String,
    pub rank_by: RankByV4,
    pub days: u8,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct OracleRowV4 {
    pub rank: u8,
    pub model_id: String,
    pub model_name: String,
    pub is_public: Option<bool>,
    pub high_mae_millionths: Option<i64>,
    pub low_mae_millionths: Option<i64>,
    pub combined_mae_millionths: Option<i64>,
    pub high_bias_millionths: Option<i64>,
    pub low_bias_millionths: Option<i64>,
    pub day_count: Option<u16>,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct OracleTableV4 {
    pub query: OracleQueryV4,
    pub range_start: String,
    pub range_end: String,
    pub updated_at_unix_ms: Option<i64>,
    pub rows: Vec<OracleRowV4>,
    pub provenance: ProvenanceV4,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct CursorV4 {
    pub connection_generation: u64,
    pub event_id: String,
    pub sequence: u64,
    pub city_sequence: Option<u64>,
    pub emitted_at_unix_ms: i64,
    pub received_at_unix_ms: i64,
    pub snapshot_complete: bool,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct StationV4 {
    pub contract_version: String,
    pub revision: u64,
    pub climate_event_date: String,
    pub climate_day_start_utc_unix_ms: i64,
    pub climate_day_end_utc_unix_ms: i64,
    pub identity: StationIdentityV4,
    pub observation_meta: ComponentMetaV4,
    pub observation: ObservationV4,
    pub weather_meta: ComponentMetaV4,
    pub weather: WeatherV4,
    pub extrema_meta: ComponentMetaV4,
    pub extrema: ExtremaV4,
    pub reports_meta: ComponentMetaV4,
    pub reports: Vec<ReportV4>,
    pub weather_events_meta: ComponentMetaV4,
    pub weather_events: Vec<WeatherEventV4>,
    pub forecast_meta: ComponentMetaV4,
    pub forecast: ForecastV4,
    pub oracle_meta: ComponentMetaV4,
    pub oracle: OracleTableV4,
    pub provider_cursor: CursorV4,
    pub evidence: Vec<EvidenceV4>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct MarketMetaV4 {
    pub authority: AuthorityV4,
    pub revision: u64,
    pub generation: u64,
    pub updated_at_unix_ms: Option<i64>,
    pub expected_version: Option<String>,
    pub refresh_error: Option<String>,
    pub provenance: Vec<ProvenanceV4>,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct MarketIdentityV4 {
    pub market_id: String,
    pub opportunity_id: String,
    pub venue: String,
    pub event_ticker: String,
    pub series_ticker: String,
    pub strike_type: String,
    pub fee_type: String,
    pub fee_multiplier_millionths: Option<i64>,
    pub floor_strike_milli_c: Option<i64>,
    pub floor_strike_milli_f: Option<i64>,
    pub cap_strike_milli_c: Option<i64>,
    pub close_at_unix_ms: Option<i64>,
    pub expiration_at_unix_ms: Option<i64>,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct MarketLifecycleV4 {
    pub status: String,
    pub result: Option<String>,
    pub connection_epoch: Option<u64>,
    pub received_frame_ordinal: Option<u64>,
    pub open_at_unix_ms: Option<i64>,
    pub close_at_unix_ms: Option<i64>,
    pub settled_at_unix_ms: Option<i64>,
    pub updated_at_unix_ms: Option<i64>,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct TickerV4 {
    pub yes_bid_micros: Option<u64>,
    pub yes_ask_micros: Option<u64>,
    pub no_bid_micros: Option<u64>,
    pub no_ask_micros: Option<u64>,
    pub yes_bid_quantity_hundredths: Option<u64>,
    pub yes_ask_quantity_hundredths: Option<u64>,
    pub no_bid_quantity_hundredths: Option<u64>,
    pub no_ask_quantity_hundredths: Option<u64>,
    pub last_price_micros: Option<u64>,
    pub last_trade_quantity_hundredths: Option<u64>,
    pub volume_hundredths: Option<u64>,
    pub volume_24h_hundredths: Option<u64>,
    pub open_interest_hundredths: Option<u64>,
    pub provider_at_unix_ms: Option<i64>,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct BookLevelV4 {
    pub price_micros: u64,
    pub quantity_hundredths: u64,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct BookV4 {
    pub connection_epoch: Option<u64>,
    pub sid: Option<u64>,
    pub sequence: Option<u64>,
    pub snapshot_at_unix_ms: Option<i64>,
    pub resync_required: bool,
    pub yes_bids: Vec<BookLevelV4>,
    pub no_bids: Vec<BookLevelV4>,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct LastTradeV4 {
    pub trade_id: String,
    pub yes_price_micros: u64,
    pub no_price_micros: u64,
    pub quantity_hundredths: u64,
    pub taker_side: Option<String>,
    pub traded_at_unix_ms: i64,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct FinalFactV4 {
    pub status: String,
    pub result: String,
    pub settlement_value_micros: Option<i64>,
    pub settled_price_micros: Option<u64>,
    pub provider_at_unix_ms: Option<i64>,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct MarketComparisonV4 {
    pub source: String,
    pub event_id: String,
    pub event_date: String,
    pub sequence: u64,
    pub provider_at_unix_ms: i64,
    pub yes_bid_micros: Option<u64>,
    pub yes_ask_micros: Option<u64>,
    pub no_bid_micros: Option<u64>,
    pub no_ask_micros: Option<u64>,
    pub volume_hundredths: Option<u64>,
    pub evidence_sha256: [u8; 32],
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct MarketV4 {
    pub contract_version: String,
    pub revision: u64,
    pub identity: MarketIdentityV4,
    pub lifecycle_meta: MarketMetaV4,
    pub lifecycle: Option<MarketLifecycleV4>,
    pub ticker_meta: MarketMetaV4,
    pub ticker: Option<TickerV4>,
    pub book_meta: MarketMetaV4,
    pub book: Option<BookV4>,
    pub last_trade_meta: MarketMetaV4,
    pub last_trade: Option<LastTradeV4>,
    pub final_fact_meta: MarketMetaV4,
    pub final_fact: Option<FinalFactV4>,
    pub field_provenance: Vec<(String, ProvenanceV4)>,
    pub evidence: Vec<EvidenceV4>,
    pub minutetemp_comparison: Option<MarketComparisonV4>,
    pub uncertain_fields: Vec<String>,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct OpportunityV4 {
    pub opportunity_id: String,
    pub venue_id: String,
    pub match_profile: String,
    pub resolved_scope: String,
    pub lifecycle: LifecycleV4,
    pub close_time_unix_seconds: u64,
    pub market_ids: Vec<String>,
    pub contributor_stations: Vec<String>,
    pub authority: MarketAuthorityV4,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct BrokerV4 {
    pub revision: u64,
    pub venue_account_id: String,
    pub sleeve_id: String,
    pub provider_available_balance: u64,
    pub locally_reserved_cash: u64,
    pub portfolio_value: u64,
    pub allowance_limit: u64,
    pub current_commitment: u64,
    pub exposure_increase: GateStatusV4,
    pub reduce_only_protective: GateStatusV4,
    pub cancellation: GateStatusV4,
    pub reconciliation_allocation: GateStatusV4,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct ConfigV4 {
    pub revision: u64,
    pub profile_and_calculator_digest: String,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct SupervisorV4 {
    pub sleeve_id: String,
    pub incarnation: u64,
    pub process_attempt: u64,
    pub route_epoch: u64,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct ResourceRevisionV4 {
    pub identity: String,
    pub revision: u64,
    pub generation: u64,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct FenceV4 {
    pub sleeve_incarnation: u64,
    pub config_revision: u64,
    pub profile_and_calculator_digest: String,
    pub route_epoch: u64,
    pub route_plan_sha256: [u8; 32],
    pub source_revision: u64,
    pub source_generation: u64,
    pub catalog_revision: u64,
    pub market_authority_generation: u64,
    pub price_revision: u64,
    pub broker_revision: u64,
    pub station_resources: Vec<ResourceRevisionV4>,
    pub market_revisions: Vec<ResourceRevisionV4>,
}
#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct TimerRecoveryV4 {
    pub key: String,
    pub scheduled_at: u64,
    pub generation: String,
    pub admission_state: u8,
}

#[derive(Clone, Debug, Default, Encode, Decode, Eq, PartialEq)]
pub struct DecisionContextV4 {
    pub delivery_id: String,
    pub sleeve: SupervisorV4,
    pub trigger: TriggerV4,
    pub fence: FenceV4,
    pub config: ConfigV4,
    pub stations: Vec<StationV4>,
    pub opportunity: OpportunityV4,
    pub markets: Vec<MarketV4>,
    pub broker: BrokerV4,
    pub timer_recovery: Option<Vec<TimerRecoveryV4>>,
    pub delivered_at_monotonic_ns: u64,
    pub hard_expires_at_monotonic_ns: u64,
}

impl DecisionContextV4 {
    pub fn validate(&self) -> Result<(), DecisionV4Error> {
        if self.delivery_id.is_empty()
            || self.stations.is_empty()
            || self.stations.len() > MAX_STATIONS
            || self.markets.is_empty()
            || self.markets.len() > MAX_MARKETS
            || self.fence.route_plan_sha256 == [0; 32]
            || self.hard_expires_at_monotonic_ns < self.delivered_at_monotonic_ns
        {
            return Err(DecisionV4Error::InvalidContract);
        }
        for station in &self.stations {
            if station.identity.timezone.is_empty()
                || station.climate_event_date.is_empty()
                || station.climate_day_end_utc_unix_ms <= station.climate_day_start_utc_unix_ms
                || station.forecast.models.len() > MAX_MODELS_PER_STATION
                || station.reports.len() > MAX_REPORTS
                || station.weather_events.len() > MAX_WEATHER_EVENTS
                || station.evidence.len() > MAX_EVIDENCE_REFERENCES
                || station.oracle.rows.len() > MAX_ORACLE_ROWS
                || station.forecast.models.iter().any(|model| {
                    model.hourly.is_empty()
                        || model.hourly.len() > MAX_POINTS_PER_MODEL
                        || model.hourly.iter().any(|point| {
                            point.at_unix_ms < station.climate_day_start_utc_unix_ms
                                || point.at_unix_ms >= station.climate_day_end_utc_unix_ms
                        })
                })
            {
                return Err(DecisionV4Error::BoundExceeded);
            }
        }
        if self.markets.iter().any(|market| {
            market
                .minutetemp_comparison
                .as_ref()
                .is_none_or(|comparison| comparison.event_date.is_empty())
                || market.book.as_ref().is_some_and(|book| {
                    book.yes_bids.len() > MAX_BOOK_LEVELS_PER_SIDE
                        || book.no_bids.len() > MAX_BOOK_LEVELS_PER_SIDE
                })
                || market.evidence.len() > MAX_EVIDENCE_REFERENCES
        }) {
            return Err(DecisionV4Error::BoundExceeded);
        }
        Ok(())
    }
}

pub fn encode_decision_context_v4(context: &DecisionContextV4) -> Result<Vec<u8>, DecisionV4Error> {
    context.validate()?;
    let config = bincode::config::standard()
        .with_big_endian()
        .with_fixed_int_encoding();
    let body = bincode::encode_to_vec(context, config).map_err(|_| DecisionV4Error::Encode)?;
    let total = DECISION_CONTEXT_V4_MAGIC
        .len()
        .checked_add(body.len())
        .ok_or(DecisionV4Error::BoundExceeded)?;
    if total > MAX_DECISION_CONTEXT_V4_BYTES {
        return Err(DecisionV4Error::BoundExceeded);
    }
    let mut encoded = Vec::with_capacity(total);
    encoded.extend_from_slice(DECISION_CONTEXT_V4_MAGIC);
    encoded.extend_from_slice(&body);
    Ok(encoded)
}

pub fn decision_fence_v4_sha256(fence: &FenceV4) -> Result<String, DecisionV4Error> {
    use sha2::{Digest, Sha256};
    let config = bincode::config::standard()
        .with_big_endian()
        .with_fixed_int_encoding();
    let bytes = bincode::encode_to_vec(fence, config).map_err(|_| DecisionV4Error::Encode)?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub fn decode_decision_context_v4(bytes: &[u8]) -> Result<DecisionContextV4, DecisionV4Error> {
    if bytes.len() > MAX_DECISION_CONTEXT_V4_BYTES || !bytes.starts_with(DECISION_CONTEXT_V4_MAGIC)
    {
        return Err(DecisionV4Error::InvalidContract);
    }
    let config = bincode::config::standard()
        .with_big_endian()
        .with_fixed_int_encoding();
    let (context, consumed): (DecisionContextV4, usize) =
        bincode::decode_from_slice(&bytes[DECISION_CONTEXT_V4_MAGIC.len()..], config)
            .map_err(|_| DecisionV4Error::Decode)?;
    if consumed != bytes.len() - DECISION_CONTEXT_V4_MAGIC.len() {
        return Err(DecisionV4Error::TrailingBytes);
    }
    context.validate()?;
    Ok(context)
}

impl Default for TriggerV4 {
    fn default() -> Self {
        Self::Bootstrap
    }
}
impl Default for AuthorityV4 {
    fn default() -> Self {
        Self::Warming
    }
}
impl Default for RankByV4 {
    fn default() -> Self {
        Self::High
    }
}
impl Default for LifecycleV4 {
    fn default() -> Self {
        Self::Discovered
    }
}
impl Default for MarketAuthorityV4 {
    fn default() -> Self {
        Self::Uncertain
    }
}
impl Default for GateStatusV4 {
    fn default() -> Self {
        Self::ClosedFinanceUnavailable
    }
}
