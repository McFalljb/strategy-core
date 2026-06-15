use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub type JsonValue = serde_json::Value;
pub type JsonObject = serde_json::Map<String, JsonValue>;
#[allow(non_camel_case_types)]
pub type JSONValue = JsonValue;
#[allow(non_camel_case_types)]
pub type JSONObject = JsonObject;
pub type StrategyConfig = BTreeMap<String, JsonValue>;
pub type OrderId = String;
pub type TelemetryFields = BTreeMap<String, TelemetryField>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TelemetryField {
    String(String),
    Integer(i64),
    Float(f64),
    Bool(bool),
    Null,
}
