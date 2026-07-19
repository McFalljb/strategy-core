//! Canonical MinuteTemp ingress identities shared by live and replay runtimes.

use std::collections::BTreeMap;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy)]
pub struct MinuteTempStationReportIdentityInput<'a> {
    pub station_id: &'a str,
    pub report_id: Option<&'a str>,
    pub provider_event_id: Option<&'a str>,
    pub report_revision: Option<u64>,
    pub report_type: Option<&'a str>,
    pub climate_date: Option<&'a str>,
    pub observed_at: Option<DateTime<Utc>>,
    pub max_temp_f: Option<f64>,
    pub min_temp_f: Option<f64>,
    pub max_temp_c: Option<f64>,
    pub min_temp_c: Option<f64>,
    pub max_temp_time_utc: Option<DateTime<Utc>>,
    pub min_temp_time_utc: Option<DateTime<Utc>>,
    pub temp_f: Option<f64>,
    pub temp_c: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MinuteTempDeliveryIdentity {
    pub delivery_id: String,
    pub content_fingerprint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinuteTempIdentityObservation {
    New,
    Duplicate,
    Collision,
}

#[derive(Debug, Default)]
pub struct MinuteTempIdentityRegistry {
    fingerprints: BTreeMap<String, String>,
}

impl MinuteTempIdentityRegistry {
    #[must_use]
    pub fn observe(
        &mut self,
        identity: &MinuteTempDeliveryIdentity,
    ) -> MinuteTempIdentityObservation {
        match self.fingerprints.get(&identity.delivery_id) {
            None => {
                self.fingerprints.insert(
                    identity.delivery_id.clone(),
                    identity.content_fingerprint.clone(),
                );
                MinuteTempIdentityObservation::New
            }
            Some(fingerprint) if fingerprint == &identity.content_fingerprint => {
                MinuteTempIdentityObservation::Duplicate
            }
            Some(_) => MinuteTempIdentityObservation::Collision,
        }
    }
}

/// Derive the stable semantic identity used for a MinuteTemp station report.
///
/// Transport-local sequence, connection, route, and delivery-attempt fields are
/// deliberately excluded. Revisions are distinct occurrences, while matching
/// identities retain a separate content fingerprint for collision detection.
#[must_use]
pub fn minutetemp_station_report_delivery_identity(
    input: MinuteTempStationReportIdentityInput<'_>,
) -> MinuteTempDeliveryIdentity {
    let revision = input.report_revision.filter(|revision| *revision > 0);
    let fingerprint_input = canonical_report_fingerprint(input);
    let content_fingerprint = sha256_hex(fingerprint_input.as_bytes());
    let semantic_key = trimmed(input.report_id)
        .map(|value| {
            revision.map_or_else(
                || canonical_parts(&["report_id", value]),
                |revision| canonical_parts(&["report_id_revision", value, &revision.to_string()]),
            )
        })
        .or_else(|| {
            trimmed(input.provider_event_id).map(|value| {
                revision.map_or_else(
                    || canonical_parts(&["provider_event_id", value]),
                    |revision| {
                        canonical_parts(&[
                            "provider_event_id_revision",
                            value,
                            &revision.to_string(),
                        ])
                    },
                )
            })
        })
        .unwrap_or_else(|| canonical_parts(&["content", &content_fingerprint]));
    let identity_version = if revision.is_some() {
        "minutetemp_station_report_v2"
    } else {
        "minutetemp_station_report_v1"
    };
    let delivery_version = if revision.is_some() { "v2" } else { "v1" };
    let identity_input = canonical_parts(&[
        identity_version,
        &input.station_id.trim().to_ascii_uppercase(),
        &semantic_key,
    ]);
    MinuteTempDeliveryIdentity {
        delivery_id: format!(
            "minutetemp:{delivery_version}:{}",
            sha256_hex(identity_input.as_bytes())
        ),
        content_fingerprint: format!("sha256:{content_fingerprint}"),
    }
}

fn canonical_report_fingerprint(input: MinuteTempStationReportIdentityInput<'_>) -> String {
    let revision = input
        .report_revision
        .filter(|revision| *revision > 0)
        .map_or_else(String::new, |value| value.to_string());
    canonical_parts(&[
        "minutetemp_station_report_content_v3",
        &input.station_id.trim().to_ascii_uppercase(),
        trimmed(input.report_id).unwrap_or(""),
        &revision,
        trimmed(input.report_type).unwrap_or(""),
        trimmed(input.climate_date).unwrap_or(""),
        &canonical_timestamp(input.observed_at),
        &canonical_float(input.max_temp_f),
        &canonical_float(input.min_temp_f),
        &canonical_float(input.max_temp_c),
        &canonical_float(input.min_temp_c),
        &canonical_timestamp(input.max_temp_time_utc),
        &canonical_timestamp(input.min_temp_time_utc),
        &canonical_float(input.temp_f),
        &canonical_float(input.temp_c),
    ])
}

fn trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn canonical_parts(parts: &[&str]) -> String {
    parts
        .iter()
        .map(|part| format!("{}:{part}", part.len()))
        .collect::<Vec<_>>()
        .join("|")
}

fn canonical_timestamp(value: Option<DateTime<Utc>>) -> String {
    value
        .map(|timestamp| timestamp.to_rfc3339_opts(SecondsFormat::Micros, true))
        .unwrap_or_default()
}

fn canonical_float(value: Option<f64>) -> String {
    value.map_or_else(String::new, |number| format!("{:016x}", number.to_bits()))
}

fn sha256_hex(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    fn input() -> MinuteTempStationReportIdentityInput<'static> {
        MinuteTempStationReportIdentityInput {
            station_id: "kaus",
            report_id: Some("DSM-KAUS-20260718"),
            provider_event_id: Some("transport-event-1"),
            report_revision: Some(2),
            report_type: Some("DSM"),
            climate_date: Some("2026-07-18"),
            observed_at: Utc.with_ymd_and_hms(2026, 7, 18, 21, 0, 0).single(),
            max_temp_f: Some(101.0),
            min_temp_f: Some(78.0),
            max_temp_c: None,
            min_temp_c: None,
            max_temp_time_utc: None,
            min_temp_time_utc: None,
            temp_f: None,
            temp_c: None,
        }
    }

    #[test]
    fn transport_metadata_does_not_change_station_report_identity() {
        let first = minutetemp_station_report_delivery_identity(input());
        let mut replayed = input();
        replayed.provider_event_id = Some("transport-event-2");

        assert_eq!(first, minutetemp_station_report_delivery_identity(replayed));
    }

    #[test]
    fn revision_changes_station_report_identity() {
        let first = minutetemp_station_report_delivery_identity(input());
        let mut corrected = input();
        corrected.report_revision = Some(3);

        assert_ne!(
            first.delivery_id,
            minutetemp_station_report_delivery_identity(corrected).delivery_id
        );
    }

    #[test]
    fn same_identity_with_changed_content_is_a_collision() {
        let first = minutetemp_station_report_delivery_identity(input());
        let mut changed = input();
        changed.max_temp_f = Some(102.0);
        let changed = minutetemp_station_report_delivery_identity(changed);
        let mut registry = MinuteTempIdentityRegistry::default();

        assert_eq!(registry.observe(&first), MinuteTempIdentityObservation::New);
        assert_eq!(
            registry.observe(&changed),
            MinuteTempIdentityObservation::Collision
        );
    }

    #[test]
    fn identity_matches_trader_station_report_contract() {
        let received_at = DateTime::<Utc>::UNIX_EPOCH + chrono::Duration::seconds(1_782_015_427);
        let identity =
            minutetemp_station_report_delivery_identity(MinuteTempStationReportIdentityInput {
                station_id: "kphx",
                report_id: Some("DSM-2026-06-21"),
                provider_event_id: Some("transport-1"),
                report_revision: None,
                report_type: Some("dsm"),
                climate_date: Some("2026-06-21"),
                observed_at: Some(received_at),
                max_temp_f: Some(110.0),
                min_temp_f: Some(84.0),
                max_temp_c: Some(43.333),
                min_temp_c: Some(28.889),
                max_temp_time_utc: None,
                min_temp_time_utc: None,
                temp_f: Some(109.0),
                temp_c: Some((109.0 - 32.0) * 5.0 / 9.0),
            });

        assert_eq!(
            identity.delivery_id,
            "minutetemp:v1:0e5b34b127687b9106681b54ea9e78645ff5e808e9cba3561ea219523c8fb6d1"
        );
    }
}
