//! Pure, bounded, runtime-neutral Strategy Core V3 semantics.

use std::fmt;

use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

pub const CANONICAL_PROFILE: &str = "strategy-core-canonical-v1";
pub const CANONICAL_PROFILE_VERSION: u8 = 1;
pub const MAX_CANONICAL_BYTES: usize = 1_048_576;
pub const MAX_CANONICAL_NESTING: usize = 64;
pub const MAX_IDENTIFIER_BYTES: usize = 128;
pub const MAX_REASON_CODE_BYTES: usize = 64;
pub const MAX_INTENTS: usize = 64;
pub const MAX_TIMER_REQUESTS: usize = 64;
pub const MAX_EVIDENCE: usize = 128;
pub const MAX_DIAGNOSTICS: usize = 64;
pub const MAX_DIAGNOSTIC_MESSAGE_BYTES: usize = 512;
pub const MAX_TIMER_SEMANTICS_BYTES: usize = 4096;
pub const MAX_DECIMAL_SCALE: u8 = 18;
pub const MAX_DECIMAL_DIGITS: usize = 38;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalError {
    pub category: &'static str,
    message: String,
}

impl CanonicalError {
    fn new(category: &'static str, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
        }
    }
}

impl fmt::Display for CanonicalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.category, self.message)
    }
}

impl std::error::Error for CanonicalError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedDecimal {
    value: String,
    scale: u8,
}

impl FixedDecimal {
    pub fn parse(value: &str, scale: u8) -> Result<Self, CanonicalError> {
        if scale > MAX_DECIMAL_SCALE {
            return Err(CanonicalError::new(
                "invalid_decimal",
                "scale is outside the profile",
            ));
        }
        let (negative, unsigned) = value
            .strip_prefix('-')
            .map_or((false, value), |item| (true, item));
        let mut parts = unsigned.split('.');
        let whole = parts.next().unwrap_or_default();
        let fraction_part = parts.next();
        let fraction = fraction_part.unwrap_or_default();
        if parts.next().is_some()
            || (scale == 0 && fraction_part.is_some())
            || (scale > 0 && fraction_part.is_none())
            || whole.is_empty()
            || !whole.bytes().all(|item| item.is_ascii_digit())
            || !fraction.bytes().all(|item| item.is_ascii_digit())
            || (whole.len() > 1 && whole.starts_with('0'))
            || fraction.len() != usize::from(scale)
        {
            return Err(CanonicalError::new(
                "invalid_decimal",
                "value is not a canonical decimal at the declared scale",
            ));
        }
        if whole.len() + fraction.len() > MAX_DECIMAL_DIGITS {
            return Err(CanonicalError::new(
                "decimal_overflow",
                "value exceeds the digit bound",
            ));
        }
        if negative
            && whole
                .bytes()
                .chain(fraction.bytes())
                .all(|item| item == b'0')
        {
            return Err(CanonicalError::new(
                "invalid_normalization",
                "negative zero is not canonical",
            ));
        }
        Ok(Self {
            value: value.to_owned(),
            scale,
        })
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn scale(&self) -> u8 {
        self.scale
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpochNanoseconds(i64);

impl EpochNanoseconds {
    pub fn new(value: i128) -> Result<Self, CanonicalError> {
        i64::try_from(value).map(Self).map_err(|_| {
            CanonicalError::new("invalid_time", "epoch nanoseconds must fit signed 64-bit")
        })
    }

    pub fn value(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalString(String);

impl CanonicalString {
    pub fn new(value: String) -> Result<Self, CanonicalError> {
        if value.chars().count() > MAX_CANONICAL_BYTES {
            return Err(CanonicalError::new(
                "canonical_overflow",
                "text exceeds the profile bound",
            ));
        }
        let normalized_length = value.nfc().map(char::len_utf8).sum::<usize>();
        if normalized_length > MAX_CANONICAL_BYTES {
            return Err(CanonicalError::new(
                "canonical_overflow",
                "text exceeds the profile bound",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedCanonicalBytes(Vec<u8>);

impl BoundedCanonicalBytes {
    pub fn new(value: Vec<u8>) -> Result<Self, CanonicalError> {
        if value.len() <= MAX_CANONICAL_BYTES {
            Ok(Self(value))
        } else {
            Err(CanonicalError::new(
                "canonical_overflow",
                "bytes exceed the profile bound",
            ))
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalList(Vec<CanonicalValue>);

impl CanonicalList {
    pub fn new(values: Vec<CanonicalValue>) -> Result<Self, CanonicalError> {
        if values.len() > (MAX_CANONICAL_BYTES - 4) / 5 {
            Err(CanonicalError::new(
                "canonical_overflow",
                "list has too many items",
            ))
        } else {
            Ok(Self(values))
        }
    }

    pub fn as_slice(&self) -> &[CanonicalValue] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalMap(Vec<(Vec<u8>, CanonicalValue)>);

impl CanonicalMap {
    pub fn new(values: Vec<(String, CanonicalValue)>) -> Result<Self, CanonicalError> {
        if values.len() > (MAX_CANONICAL_BYTES - 4) / 10 {
            return Err(CanonicalError::new(
                "canonical_overflow",
                "map has too many items",
            ));
        }
        let mut key_bytes = 0_usize;
        let mut entries = Vec::with_capacity(values.len());
        for (key, value) in values {
            let normalized = normalized(&key)?;
            key_bytes = key_bytes.checked_add(normalized.len() + 5).ok_or_else(|| {
                CanonicalError::new("canonical_overflow", "map keys are too large")
            })?;
            if key_bytes > MAX_CANONICAL_BYTES {
                return Err(CanonicalError::new(
                    "canonical_overflow",
                    "map keys exceed the profile bound",
                ));
            }
            entries.push((normalized, value));
        }
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        if entries.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(CanonicalError::new(
                "invalid_normalization",
                "map keys collide after NFC normalization",
            ));
        }
        Ok(Self(entries))
    }

    pub fn entries(&self) -> impl ExactSizeIterator<Item = (&[u8], &CanonicalValue)> {
        self.0.iter().map(|(key, value)| (key.as_slice(), value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalValue {
    Null,
    Bool(bool),
    Integer(i128),
    String(CanonicalString),
    Bytes(BoundedCanonicalBytes),
    Decimal(FixedDecimal),
    EpochNanoseconds(EpochNanoseconds),
    List(CanonicalList),
    Map(CanonicalMap),
}

impl CanonicalValue {
    pub fn parse_integer(value: &str) -> Result<Self, CanonicalError> {
        value
            .parse::<i128>()
            .map(Self::Integer)
            .map_err(|_| CanonicalError::new("integer_overflow", "integer must fit signed 128-bit"))
    }

    pub fn string(value: String) -> Result<Self, CanonicalError> {
        CanonicalString::new(value).map(Self::String)
    }

    pub fn bytes(value: Vec<u8>) -> Result<Self, CanonicalError> {
        BoundedCanonicalBytes::new(value).map(Self::Bytes)
    }

    pub fn list(values: Vec<Self>) -> Result<Self, CanonicalError> {
        CanonicalList::new(values).map(Self::List)
    }

    pub fn map(values: Vec<(String, Self)>) -> Result<Self, CanonicalError> {
        CanonicalMap::new(values).map(Self::Map)
    }
}

struct Encoder {
    output: Vec<u8>,
}

impl Encoder {
    fn new() -> Self {
        Self { output: Vec::new() }
    }

    fn append(&mut self, value: &[u8]) -> Result<(), CanonicalError> {
        if value.len() > MAX_CANONICAL_BYTES - self.output.len() {
            return Err(CanonicalError::new(
                "canonical_overflow",
                "encoded value exceeds the profile bound",
            ));
        }
        self.output.extend_from_slice(value);
        Ok(())
    }

    fn frame(&mut self, tag: u8, payload: &[u8]) -> Result<(), CanonicalError> {
        let length = u32::try_from(payload.len())
            .map_err(|_| CanonicalError::new("canonical_overflow", "framed value is too large"))?;
        self.append(&[tag])?;
        self.append(&length.to_be_bytes())?;
        self.append(payload)
    }

    fn begin_frame(&mut self, tag: u8) -> Result<usize, CanonicalError> {
        self.append(&[tag])?;
        let length_offset = self.output.len();
        self.append(&[0; 4])?;
        Ok(length_offset)
    }

    fn end_frame(&mut self, length_offset: usize) -> Result<(), CanonicalError> {
        let payload_length = self.output.len() - length_offset - 4;
        let length = u32::try_from(payload_length)
            .map_err(|_| CanonicalError::new("canonical_overflow", "framed value is too large"))?;
        self.output[length_offset..length_offset + 4].copy_from_slice(&length.to_be_bytes());
        Ok(())
    }
}

fn normalized(value: &str) -> Result<Vec<u8>, CanonicalError> {
    if value.chars().count() > MAX_CANONICAL_BYTES
        || value.nfc().map(char::len_utf8).sum::<usize>() > MAX_CANONICAL_BYTES
    {
        return Err(CanonicalError::new(
            "canonical_overflow",
            "text exceeds the profile bound",
        ));
    }
    Ok(value.nfc().collect::<String>().into_bytes())
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_IDENTIFIER_BYTES && value.nfc().eq(value.chars())
}

fn encode(
    encoder: &mut Encoder,
    value: &CanonicalValue,
    depth: usize,
) -> Result<(), CanonicalError> {
    if depth > MAX_CANONICAL_NESTING {
        return Err(CanonicalError::new(
            "canonical_nesting_overflow",
            "value exceeds the nesting bound",
        ));
    }
    match value {
        CanonicalValue::Null => encoder.frame(b'n', &[]),
        CanonicalValue::Bool(value) => encoder.frame(b'b', if *value { b"1" } else { b"0" }),
        CanonicalValue::Integer(value) => encoder.frame(b'i', value.to_string().as_bytes()),
        CanonicalValue::String(value) => encoder.frame(b's', &normalized(value.as_str())?),
        CanonicalValue::Bytes(value) => encoder.frame(b'x', value.as_bytes()),
        CanonicalValue::Decimal(value) => {
            let value = FixedDecimal::parse(value.value(), value.scale())?;
            let length_offset = encoder.begin_frame(b'd')?;
            encoder.append(&[value.scale()])?;
            encoder.append(value.value().as_bytes())?;
            encoder.end_frame(length_offset)
        }
        CanonicalValue::EpochNanoseconds(value) => {
            encoder.frame(b't', &value.value().to_be_bytes())
        }
        CanonicalValue::List(values) => {
            let length_offset = encoder.begin_frame(b'l')?;
            let count = u32::try_from(values.as_slice().len()).map_err(|_| {
                CanonicalError::new("canonical_overflow", "list has too many items")
            })?;
            encoder.append(&count.to_be_bytes())?;
            for value in values.as_slice() {
                encode(encoder, value, depth + 1)?;
            }
            encoder.end_frame(length_offset)
        }
        CanonicalValue::Map(values) => {
            let length_offset = encoder.begin_frame(b'm')?;
            let count = u32::try_from(values.entries().len())
                .map_err(|_| CanonicalError::new("canonical_overflow", "map has too many items"))?;
            encoder.append(&count.to_be_bytes())?;
            for (key, value) in values.entries() {
                encoder.frame(b's', key)?;
                encode(encoder, value, depth + 1)?;
            }
            encoder.end_frame(length_offset)
        }
    }
}

pub fn canonical_bytes(domain: &str, value: &CanonicalValue) -> Result<Vec<u8>, CanonicalError> {
    let domain = normalized(domain)?;
    if domain.is_empty() || domain.len() > MAX_IDENTIFIER_BYTES {
        return Err(CanonicalError::new(
            "invalid_domain",
            "domain is empty or exceeds its bound",
        ));
    }
    let mut encoder = Encoder::new();
    encoder.append(b"SCV3")?;
    encoder.append(&[CANONICAL_PROFILE_VERSION])?;
    encoder.frame(b'D', &domain)?;
    encode(&mut encoder, value, 0)?;
    Ok(encoder.output)
}

pub fn canonical_sha256(domain: &str, value: &CanonicalValue) -> Result<String, CanonicalError> {
    let digest = Sha256::digest(canonical_bytes(domain, value)?);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasonCode(String);

impl ReasonCode {
    pub fn new(value: &str) -> Result<Self, &'static str> {
        let mut characters = value.chars();
        let valid = characters
            .next()
            .is_some_and(|item| item.is_ascii_lowercase())
            && characters
                .all(|item| item.is_ascii_lowercase() || item.is_ascii_digit() || item == '_')
            && value.len() <= MAX_REASON_CODE_BYTES;
        if valid {
            Ok(Self(value.to_owned()))
        } else {
            Err("invalid_reason_code")
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimerKey(String);

impl TimerKey {
    pub fn new(value: &str) -> Result<Self, &'static str> {
        let normalized: String = value.nfc().collect();
        if !value.is_empty()
            && value.len() <= MAX_IDENTIFIER_BYTES
            && value.trim() == value
            && normalized == value
        {
            Ok(Self(value.to_owned()))
        } else {
            Err("invalid_timer_key")
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleTimerRequest {
    key: TimerKey,
    scheduled_at: EpochNanoseconds,
    semantics_version: u32,
    semantics: Vec<u8>,
}

impl ScheduleTimerRequest {
    pub fn new(
        key: TimerKey,
        scheduled_at: EpochNanoseconds,
        semantics_version: u32,
        semantics: Vec<u8>,
    ) -> Result<Self, &'static str> {
        if semantics.len() > MAX_TIMER_SEMANTICS_BYTES {
            return Err("timer_semantics_too_large");
        }
        Ok(Self {
            key,
            scheduled_at,
            semantics_version,
            semantics,
        })
    }

    pub fn key(&self) -> &TimerKey {
        &self.key
    }

    pub fn scheduled_at(&self) -> EpochNanoseconds {
        self.scheduled_at
    }

    pub fn semantics_version(&self) -> u32 {
        self.semantics_version
    }

    pub fn semantics(&self) -> &[u8] {
        &self.semantics
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelTimerRequest {
    key: TimerKey,
}

impl CancelTimerRequest {
    pub fn new(key: TimerKey) -> Self {
        Self { key }
    }

    pub fn key(&self) -> &TimerKey {
        &self.key
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimerRequest {
    Schedule(ScheduleTimerRequest),
    Cancel(CancelTimerRequest),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    severity: DiagnosticSeverity,
    code: ReasonCode,
    message: String,
}

impl Diagnostic {
    pub fn new(
        severity: DiagnosticSeverity,
        code: ReasonCode,
        message: String,
    ) -> Result<Self, &'static str> {
        if message.len() > MAX_DIAGNOSTIC_MESSAGE_BYTES {
            return Err("diagnostic_message_too_long");
        }
        Ok(Self {
            severity,
            code,
            message,
        })
    }

    pub fn severity(&self) -> DiagnosticSeverity {
        self.severity
    }

    pub fn code(&self) -> &ReasonCode {
        &self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionOutcome {
    Completed,
    NoAction,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderAction {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractSide {
    Yes,
    No,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyOrderIntent {
    market_id: String,
    action: OrderAction,
    side: ContractSide,
    quantity: u64,
    limit_price: Option<FixedDecimal>,
    reduce_only: bool,
    reason_code: ReasonCode,
    metadata: Vec<u8>,
}

impl StrategyOrderIntent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        market_id: String,
        action: OrderAction,
        side: ContractSide,
        quantity: u64,
        limit_price: Option<FixedDecimal>,
        reduce_only: bool,
        reason_code: ReasonCode,
        metadata: Vec<u8>,
    ) -> Result<Self, &'static str> {
        if !valid_identifier(&market_id) {
            return Err("invalid_market_id");
        }
        if quantity == 0 || quantity > i64::MAX as u64 {
            return Err("invalid_order_quantity");
        }
        if let Some(value) = &limit_price {
            FixedDecimal::parse(value.value(), value.scale()).map_err(|_| "invalid_limit_price")?;
        }
        if metadata.len() > MAX_CANONICAL_BYTES {
            return Err("order_metadata_too_large");
        }
        Ok(Self {
            market_id,
            action,
            side,
            quantity,
            limit_price,
            reduce_only,
            reason_code,
            metadata,
        })
    }

    pub fn market_id(&self) -> &str {
        &self.market_id
    }
    pub fn action(&self) -> OrderAction {
        self.action
    }
    pub fn side(&self) -> ContractSide {
        self.side
    }
    pub fn quantity(&self) -> u64 {
        self.quantity
    }
    pub fn limit_price(&self) -> Option<&FixedDecimal> {
        self.limit_price.as_ref()
    }
    pub fn reduce_only(&self) -> bool {
        self.reduce_only
    }
    pub fn reason_code(&self) -> &ReasonCode {
        &self.reason_code
    }
    pub fn metadata(&self) -> &[u8] {
        &self.metadata
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyEvidence {
    code: ReasonCode,
    payload: Vec<u8>,
}

impl StrategyEvidence {
    pub fn new(code: ReasonCode, payload: Vec<u8>) -> Result<Self, &'static str> {
        if payload.len() > MAX_CANONICAL_BYTES {
            Err("evidence_payload_too_large")
        } else {
            Ok(Self { code, payload })
        }
    }

    pub fn code(&self) -> &ReasonCode {
        &self.code
    }
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionResultV3 {
    delivery_id: String,
    sleeve_identity: String,
    state_fence: String,
    outcome: DecisionOutcome,
    intents: Vec<StrategyOrderIntent>,
    timer_requests: Vec<TimerRequest>,
    evidence: Vec<StrategyEvidence>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecisionResultProfile {
    pub intent_count: usize,
    pub timer_request_count: usize,
    pub evidence_count: usize,
    pub diagnostic_count: usize,
    pub diagnostic_bytes: usize,
}

pub fn calculate_result_profile(result: &DecisionResultV3) -> DecisionResultProfile {
    DecisionResultProfile {
        intent_count: result.intents.len(),
        timer_request_count: result.timer_requests.len(),
        evidence_count: result.evidence.len(),
        diagnostic_count: result.diagnostics.len(),
        diagnostic_bytes: result
            .diagnostics
            .iter()
            .map(|item| item.message.len())
            .sum(),
    }
}

impl DecisionResultV3 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        delivery_id: String,
        sleeve_identity: String,
        state_fence: String,
        outcome: DecisionOutcome,
        intents: Vec<StrategyOrderIntent>,
        timer_requests: Vec<TimerRequest>,
        evidence: Vec<StrategyEvidence>,
        diagnostics: Vec<Diagnostic>,
    ) -> Result<Self, &'static str> {
        if !valid_identifier(&delivery_id) {
            return Err("invalid_delivery_id");
        }
        if !valid_identifier(&sleeve_identity) {
            return Err("invalid_sleeve_identity");
        }
        if !valid_identifier(&state_fence) {
            return Err("invalid_state_fence");
        }
        if intents.len() > MAX_INTENTS {
            return Err("too_many_order_intents");
        }
        if timer_requests.len() > MAX_TIMER_REQUESTS {
            return Err("too_many_timer_requests");
        }
        if evidence.len() > MAX_EVIDENCE {
            return Err("too_many_evidence_items");
        }
        if diagnostics.len() > MAX_DIAGNOSTICS {
            return Err("too_many_diagnostics");
        }
        Ok(Self {
            delivery_id,
            sleeve_identity,
            state_fence,
            outcome,
            intents,
            timer_requests,
            evidence,
            diagnostics,
        })
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        Ok(())
    }
    pub fn delivery_id(&self) -> &str {
        &self.delivery_id
    }
    pub fn sleeve_identity(&self) -> &str {
        &self.sleeve_identity
    }
    pub fn state_fence(&self) -> &str {
        &self.state_fence
    }
    pub fn outcome(&self) -> DecisionOutcome {
        self.outcome
    }
    pub fn intents(&self) -> &[StrategyOrderIntent] {
        &self.intents
    }
    pub fn timer_requests(&self) -> &[TimerRequest] {
        &self.timer_requests
    }
    pub fn evidence(&self) -> &[StrategyEvidence] {
        &self.evidence
    }
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerKind {
    Bootstrap,
    Recovery,
    Weather,
    Timer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionTrigger {
    kind: TriggerKind,
    occurred_at: EpochNanoseconds,
    timer_key: Option<TimerKey>,
}

impl DecisionTrigger {
    pub fn new(
        kind: TriggerKind,
        occurred_at: EpochNanoseconds,
        timer_key: Option<TimerKey>,
    ) -> Result<Self, &'static str> {
        if matches!(kind, TriggerKind::Timer) != timer_key.is_some() {
            return Err("invalid_trigger_timer_key");
        }
        Ok(Self {
            kind,
            occurred_at,
            timer_key,
        })
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        Ok(())
    }
    pub fn kind(&self) -> TriggerKind {
        self.kind
    }
    pub fn occurred_at(&self) -> EpochNanoseconds {
        self.occurred_at
    }
    pub fn timer_key(&self) -> Option<&TimerKey> {
        self.timer_key.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEvidence {
    kind: String,
    reference: String,
    payload_sha256: String,
}

impl SourceEvidence {
    pub fn new(
        kind: String,
        reference: String,
        payload_sha256: String,
    ) -> Result<Self, &'static str> {
        if !valid_identifier(&kind) {
            return Err("invalid_evidence_kind");
        }
        if !valid_identifier(&reference) {
            return Err("invalid_evidence_reference");
        }
        if payload_sha256.len() != 64
            || !payload_sha256
                .bytes()
                .all(|item| item.is_ascii_digit() || (b'a'..=b'f').contains(&item))
        {
            return Err("invalid_evidence_digest");
        }
        Ok(Self {
            kind,
            reference,
            payload_sha256,
        })
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }
    pub fn reference(&self) -> &str {
        &self.reference
    }
    pub fn payload_sha256(&self) -> &str {
        &self.payload_sha256
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedContextPayload(Vec<u8>);

impl BoundedContextPayload {
    pub fn new(value: Vec<u8>) -> Result<Self, &'static str> {
        if value.len() <= MAX_CANONICAL_BYTES {
            Ok(Self(value))
        } else {
            Err("context_payload_too_large")
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionContextV3 {
    delivery_id: String,
    sleeve_identity: String,
    state_fence: String,
    trigger: DecisionTrigger,
    source_evidence: Vec<SourceEvidence>,
    weather: BoundedContextPayload,
    opportunity: BoundedContextPayload,
    markets: BoundedContextPayload,
    broker: BoundedContextPayload,
    authorization: BoundedContextPayload,
    delivered_at_monotonic_ns: u64,
    hard_expires_at_monotonic_ns: u64,
}

impl DecisionContextV3 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        delivery_id: String,
        sleeve_identity: String,
        state_fence: String,
        trigger: DecisionTrigger,
        source_evidence: Vec<SourceEvidence>,
        weather: BoundedContextPayload,
        opportunity: BoundedContextPayload,
        markets: BoundedContextPayload,
        broker: BoundedContextPayload,
        authorization: BoundedContextPayload,
        delivered_at_monotonic_ns: u64,
        hard_expires_at_monotonic_ns: u64,
    ) -> Result<Self, &'static str> {
        if !valid_identifier(&delivery_id) {
            return Err("invalid_delivery_id");
        }
        if !valid_identifier(&sleeve_identity) {
            return Err("invalid_sleeve_identity");
        }
        if !valid_identifier(&state_fence) {
            return Err("invalid_state_fence");
        }
        if source_evidence.len() > MAX_EVIDENCE {
            return Err("too_many_source_evidence_items");
        }
        if hard_expires_at_monotonic_ns < delivered_at_monotonic_ns {
            return Err("invalid_hard_expiry");
        }
        Ok(Self {
            delivery_id,
            sleeve_identity,
            state_fence,
            trigger,
            source_evidence,
            weather,
            opportunity,
            markets,
            broker,
            authorization,
            delivered_at_monotonic_ns,
            hard_expires_at_monotonic_ns,
        })
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        Ok(())
    }
    pub fn delivery_id(&self) -> &str {
        &self.delivery_id
    }
    pub fn sleeve_identity(&self) -> &str {
        &self.sleeve_identity
    }
    pub fn state_fence(&self) -> &str {
        &self.state_fence
    }
    pub fn trigger(&self) -> &DecisionTrigger {
        &self.trigger
    }
    pub fn source_evidence(&self) -> &[SourceEvidence] {
        &self.source_evidence
    }
    pub fn weather(&self) -> &BoundedContextPayload {
        &self.weather
    }
    pub fn opportunity(&self) -> &BoundedContextPayload {
        &self.opportunity
    }
    pub fn markets(&self) -> &BoundedContextPayload {
        &self.markets
    }
    pub fn broker(&self) -> &BoundedContextPayload {
        &self.broker
    }
    pub fn authorization(&self) -> &BoundedContextPayload {
        &self.authorization
    }
    pub fn delivered_at_monotonic_ns(&self) -> u64 {
        self.delivered_at_monotonic_ns
    }
    pub fn hard_expires_at_monotonic_ns(&self) -> u64 {
        self.hard_expires_at_monotonic_ns
    }
}
