//! Host-retained model issuance, distinct from the latest fetched forecast original.

use bincode::{Decode, Encode};

#[derive(Clone, Copy, Debug, Encode, Decode, Eq, PartialEq)]
pub enum ForecastIssuanceBasis {
    SuppliedIssuedAt,
    TimestampedVersion,
    DerivedIssuedAt,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct ForecastIssuance {
    pub model_id: String,
    pub version: String,
    pub at_unix_ns: i64,
    pub basis: ForecastIssuanceBasis,
    pub original_text: Option<String>,
    pub source: String,
    pub received_at_unix_ns: i64,
    /// Station cursor/baseline generation scopes the accepted station revision.
    pub station_generation: u64,
    pub station_revision: u64,
    /// Independent forecast replacement generation, not a transport sequence.
    pub forecast_generation: u64,
}

impl ForecastIssuance {
    pub fn is_valid(&self) -> bool {
        if self.model_id.is_empty()
            || self.model_id.len() > 128
            || self.version.is_empty()
            || self.version.len() > 2048
            || self.source.is_empty()
            || self.source.len() > 2048
            || self.station_generation == 0
            || self.station_revision == 0
            || self.forecast_generation == 0
        {
            return false;
        }
        if let Some(text) = &self.original_text {
            if text.len() > 2048
                || chrono::DateTime::parse_from_rfc3339(text)
                    .ok()
                    .and_then(|time| time.timestamp_nanos_opt())
                    != Some(self.at_unix_ns)
            {
                return false;
            }
        }
        match self.basis {
            ForecastIssuanceBasis::TimestampedVersion => {
                self.original_text.as_deref() == Some(self.version.as_str())
            }
            ForecastIssuanceBasis::DerivedIssuedAt => self.original_text.is_none(),
            ForecastIssuanceBasis::SuppliedIssuedAt => true,
        }
    }
}
