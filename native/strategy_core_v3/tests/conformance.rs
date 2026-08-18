use std::{collections::BTreeSet, fs, path::PathBuf};

use serde_json::Value;
use strategy_core_v3::{
    BoundedContextPayload, CanonicalError, CanonicalValue, ContractSide, DecisionContextV3,
    DecisionOutcome, DecisionResultV3, DecisionTrigger, Diagnostic, DiagnosticSeverity,
    EpochNanoseconds, FixedDecimal, MAX_CANONICAL_BYTES, MAX_CANONICAL_NESTING,
    MAX_DIAGNOSTIC_MESSAGE_BYTES, OrderAction, ReasonCode, ScheduleTimerRequest, SourceEvidence,
    StrategyEvidence, StrategyOrderIntent, TimerKey, TriggerKind, calculate_result_profile,
    canonical_bytes, canonical_sha256,
};

fn corpus() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../conformance/v3/vectors.json");
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn integer_text(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}

fn decode_hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).unwrap();
            u8::from_str_radix(text, 16).unwrap()
        })
        .collect()
}

fn node(value: &Value) -> Result<CanonicalValue, CanonicalError> {
    let object = value.as_object().unwrap();
    assert_eq!(object.len(), 1);
    let (kind, payload) = object.iter().next().unwrap();
    match kind.as_str() {
        "null" => Ok(CanonicalValue::Null),
        "bool" => Ok(CanonicalValue::Bool(payload.as_bool().unwrap())),
        "integer" => CanonicalValue::parse_integer(&integer_text(payload)),
        "string" => CanonicalValue::string(payload.as_str().unwrap().to_owned()),
        "bytes" => CanonicalValue::bytes(decode_hex(payload.as_str().unwrap())),
        "decimal" => Ok(CanonicalValue::Decimal(FixedDecimal::parse(
            payload["value"].as_str().unwrap(),
            payload["scale"].as_u64().unwrap().try_into().unwrap(),
        )?)),
        "epoch_ns" => Ok(CanonicalValue::EpochNanoseconds(EpochNanoseconds::new(
            integer_text(payload).parse::<i128>().unwrap(),
        )?)),
        "list" => CanonicalValue::list(
            payload
                .as_array()
                .unwrap()
                .iter()
                .map(node)
                .collect::<Result<_, _>>()?,
        ),
        "map" => CanonicalValue::map(
            payload
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| {
                    let entry = entry.as_array().unwrap();
                    Ok((entry[0].as_str().unwrap().to_owned(), node(&entry[1])?))
                })
                .collect::<Result<_, CanonicalError>>()?,
        ),
        unknown => panic!("unknown vector node kind: {unknown}"),
    }
}

#[test]
fn shared_valid_vectors_match_canonical_bytes_and_digests() {
    for vector in corpus()["valid"].as_array().unwrap() {
        let value = node(&vector["value"]).unwrap();
        let domain = vector["domain"].as_str().unwrap();
        assert_eq!(
            canonical_bytes(domain, &value).unwrap(),
            decode_hex(vector["expected_hex"].as_str().unwrap()),
            "{}",
            vector["id"]
        );
        assert_eq!(
            canonical_sha256(domain, &value).unwrap(),
            vector["expected_sha256"].as_str().unwrap(),
            "{}",
            vector["id"]
        );
    }
}

#[test]
fn shared_invalid_vectors_fail_with_normalized_category() {
    for vector in corpus()["invalid"].as_array().unwrap() {
        let result = node(&vector["value"])
            .and_then(|value| canonical_bytes(vector["domain"].as_str().unwrap(), &value));
        assert_eq!(
            result.unwrap_err().category,
            vector["category"].as_str().unwrap(),
            "{}",
            vector["id"]
        );
    }
}

#[test]
fn canonical_encoding_rejects_size_and_nesting_overflow_during_encoding() {
    let oversized = CanonicalValue::bytes(vec![b'x'; MAX_CANONICAL_BYTES]).unwrap();
    assert_eq!(
        canonical_bytes("strategy.bytes", &oversized)
            .unwrap_err()
            .category,
        "canonical_overflow"
    );

    let mut value = CanonicalValue::Null;
    for _ in 0..MAX_CANONICAL_NESTING {
        value = CanonicalValue::list(vec![value]).unwrap();
    }
    canonical_bytes("strategy.nesting", &value).unwrap();
    let too_deep = CanonicalValue::list(vec![value]).unwrap();
    assert_eq!(
        canonical_bytes("strategy.nesting", &too_deep)
            .unwrap_err()
            .category,
        "canonical_nesting_overflow"
    );
}

#[test]
fn timer_requests_carry_stable_strategy_meaning_without_mutable_collection_access() {
    let request = ScheduleTimerRequest::new(
        TimerKey::new("weather.recheck").unwrap(),
        EpochNanoseconds::new(1_800_000_000_000_000_000).unwrap(),
        1,
        b"re-evaluate forecast".to_vec(),
    )
    .unwrap();

    assert_eq!(request.key().as_str(), "weather.recheck");
    assert_eq!(request.semantics_version(), 1);
    assert_eq!(request.semantics(), b"re-evaluate forecast");
}

#[test]
fn context_result_intent_evidence_and_profile_validate_as_bounded_values() {
    let payload = BoundedContextPayload::new(
        canonical_bytes("strategy.context", &CanonicalValue::map(vec![]).unwrap()).unwrap(),
    )
    .unwrap();
    let context = DecisionContextV3::new(
        "delivery-1".to_owned(),
        "sleeve-1".to_owned(),
        "fence-1".to_owned(),
        DecisionTrigger::new(
            TriggerKind::Weather,
            EpochNanoseconds::new(1).unwrap(),
            None,
        )
        .unwrap(),
        vec![
            SourceEvidence::new("weather".to_owned(), "capture-1".to_owned(), "a".repeat(64))
                .unwrap(),
        ],
        payload.clone(),
        payload.clone(),
        payload.clone(),
        payload.clone(),
        payload,
        10,
        20,
    )
    .unwrap();
    assert_eq!(context.validate(), Ok(()));

    let result = DecisionResultV3::new(
        context.delivery_id().to_owned(),
        context.sleeve_identity().to_owned(),
        context.state_fence().to_owned(),
        DecisionOutcome::Completed,
        vec![
            StrategyOrderIntent::new(
                "market-1".to_owned(),
                OrderAction::Buy,
                ContractSide::Yes,
                2,
                Some(FixedDecimal::parse("0.42", 2).unwrap()),
                false,
                ReasonCode::new("forecast_threshold_met").unwrap(),
                vec![],
            )
            .unwrap(),
        ],
        vec![],
        vec![
            StrategyEvidence::new(
                ReasonCode::new("forecast_used").unwrap(),
                b"capture-1".to_vec(),
            )
            .unwrap(),
        ],
        vec![],
    )
    .unwrap();
    assert_eq!(result.validate(), Ok(()));
    assert_eq!(calculate_result_profile(&result).intent_count, 1);
    assert_eq!(result.intents()[0].metadata(), b"");
}

#[test]
fn diagnostics_enforce_the_shared_utf8_byte_bound() {
    let code = ReasonCode::new("profile_selected").unwrap();
    assert!(
        Diagnostic::new(
            DiagnosticSeverity::Info,
            code.clone(),
            "x".repeat(MAX_DIAGNOSTIC_MESSAGE_BYTES),
        )
        .is_ok()
    );
    assert_eq!(
        Diagnostic::new(
            DiagnosticSeverity::Error,
            code,
            "x".repeat(MAX_DIAGNOSTIC_MESSAGE_BYTES + 1),
        )
        .unwrap_err(),
        "diagnostic_message_too_long"
    );
}

fn dependencies(manifest: &str, section: &str) -> BTreeSet<String> {
    let body = manifest.split(&format!("[{section}]")).nth(1).unwrap();
    body.lines()
        .skip(1)
        .take_while(|line| !line.starts_with('['))
        .filter_map(|line| {
            let line = line.trim();
            (!line.is_empty() && !line.starts_with('#'))
                .then(|| line.split('=').next().unwrap().trim().to_owned())
        })
        .collect()
}

#[test]
fn crate_dependencies_match_exact_pure_allowlists() {
    let manifest = include_str!("../Cargo.toml");
    assert_eq!(
        dependencies(manifest, "dependencies"),
        BTreeSet::from(["sha2".to_owned(), "unicode-normalization".to_owned()])
    );
    assert_eq!(
        dependencies(manifest, "dev-dependencies"),
        BTreeSet::from(["serde_json".to_owned()])
    );
}

#[test]
fn dependency_policy_rejects_realistic_network_and_runtime_mutations() {
    let fixture =
        "[dependencies]\nsha2='0.10'\nunicode-normalization='0.1'\nreqwest='0.12'\ntokio='1'\n";
    let allowed = BTreeSet::from(["sha2".to_owned(), "unicode-normalization".to_owned()]);
    let actual = dependencies(fixture, "dependencies");
    assert_eq!(
        actual
            .difference(&allowed)
            .cloned()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["reqwest".to_owned(), "tokio".to_owned()])
    );
}
