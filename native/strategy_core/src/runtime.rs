use std::future::Future;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::models::JsonValue;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarketType {
    High,
    Low,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeMode {
    Paper,
    Replay,
    Live,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StrategyScope {
    pub sleeve_id: String,
    pub strategy_name: String,
    pub station_id: Option<String>,
    #[serde(default)]
    pub tickers: Vec<String>,
    pub market_type: Option<MarketType>,
    pub event_ticker: Option<String>,
    pub event_date: Option<NaiveDate>,
}

pub trait TimerHandle {
    fn cancelled(&self) -> bool;
    fn cancel(&mut self);
}

pub trait WorkHandle {
    type Error;

    fn cancelled(&self) -> bool;
    fn done(&self) -> bool;
    fn exception(&self) -> Option<&Self::Error>;
    fn cancel(&mut self);
}

pub trait EngineClock {
    fn now(&self) -> DateTime<Utc>;
    fn sleep(&self, seconds: f64) -> impl Future<Output = ()> + Send;
    fn sleep_until(&self, when: DateTime<Utc>) -> impl Future<Output = ()> + Send;
}

pub trait StrategyRuntime {
    type Clock: EngineClock;
    type Timer: TimerHandle;
    type Work: WorkHandle;

    fn mode(&self) -> RuntimeMode;
    fn run_id(&self) -> &str;
    fn scope(&self) -> &StrategyScope;
    fn clock(&self) -> &Self::Clock;
    fn runtime_identity(&self) -> &JsonValue;
    fn wake_at(&mut self, when: DateTime<Utc>, name: Option<&str>) -> Self::Timer;
    fn start_work<F>(&mut self, work: F, name: Option<&str>) -> Self::Work
    where
        F: Future<Output = ()> + Send + 'static;
}
