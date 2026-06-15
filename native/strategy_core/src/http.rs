use std::collections::BTreeMap;
use std::future::Future;

use serde::{Deserialize, Serialize};

use crate::models::JsonValue;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

pub type HttpHeaders = BTreeMap<String, String>;
pub type HttpParams = BTreeMap<String, JsonValue>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    #[serde(default)]
    pub headers: HttpHeaders,
    #[serde(default)]
    pub params: HttpParams,
    pub json_body: Option<JsonValue>,
    pub text_body: Option<String>,
    pub timeout_seconds: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status_code: i64,
    #[serde(default)]
    pub headers: HttpHeaders,
    pub text: Option<String>,
    pub json_body: Option<JsonValue>,
}

pub trait HttpClient {
    type Error;

    fn request(
        &self,
        request: HttpRequest,
    ) -> impl Future<Output = Result<HttpResponse, Self::Error>> + Send;

    fn get(
        &self,
        url: &str,
        headers: Option<HttpHeaders>,
        params: Option<HttpParams>,
        timeout_seconds: Option<f64>,
    ) -> impl Future<Output = Result<HttpResponse, Self::Error>> + Send;

    fn post(
        &self,
        url: &str,
        headers: Option<HttpHeaders>,
        params: Option<HttpParams>,
        json_body: Option<JsonValue>,
        text_body: Option<String>,
        timeout_seconds: Option<f64>,
    ) -> impl Future<Output = Result<HttpResponse, Self::Error>> + Send;
}
