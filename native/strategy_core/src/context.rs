use std::future::Future;

use crate::{
    Broker, HttpClient, MarketStateView, RuntimeCapabilities, StrategyConfig, StrategyDataClient,
    StrategyEvent, StrategyRuntime, Telemetry,
};

pub trait StrategyContext {
    type State: MarketStateView;
    type Data: StrategyDataClient;
    type Broker: Broker;
    type Http: HttpClient;
    type Runtime: StrategyRuntime;
    type Telemetry: Telemetry;

    fn state(&self) -> &Self::State;
    fn data(&self) -> &Self::Data;
    fn broker(&mut self) -> &mut Self::Broker;
    fn http(&self) -> &Self::Http;
    fn runtime(&mut self) -> &mut Self::Runtime;
    fn capabilities(&self) -> &RuntimeCapabilities;
    fn config(&self) -> &StrategyConfig;
    fn telemetry(&mut self) -> &mut Self::Telemetry;
    fn next_event(&mut self) -> impl Future<Output = Option<StrategyEvent>> + Send;
}

pub trait StrategyHandler<C: StrategyContext> {
    fn run(&mut self, ctx: &mut C) -> impl Future<Output = ()> + Send;
}
