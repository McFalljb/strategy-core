use chrono::{TimeZone, Utc};
use strategy_core_kernel::{
    CancelAllOrdersRequest, CancelOrderRequest, ContractSide, KernelAction, KernelResult,
    MarketBracketView, NativeKernel, OrderAction, OrderResult, OrderStatus, OrderType,
    PlaceOrderRequest, PriceLevelView, PriceUpdateView, StrategyEventView, StrategyKernelBroker,
    StrategyKernelContext, StrategyKernelData, StrategyKernelRuntime, StrategyKernelState,
    StrategyKernelTelemetry, TickerPriceView, TimerWakeView, WakeAtRequest,
};

const YES_BID_LEVELS: [PriceLevelView; 1] = [PriceLevelView {
    price: 0.41,
    quantity: 12,
}];
const YES_ASK_LEVELS: [PriceLevelView; 1] = [PriceLevelView {
    price: 0.42,
    quantity: 8,
}];
const NO_BID_LEVELS: [PriceLevelView; 1] = [PriceLevelView {
    price: 0.58,
    quantity: 9,
}];
const NO_ASK_LEVELS: [PriceLevelView; 1] = [PriceLevelView {
    price: 0.59,
    quantity: 7,
}];

#[derive(Default)]
struct NoopKernel {
    events_seen: usize,
}

impl NativeKernel for NoopKernel {
    fn name(&self) -> &str {
        "noop"
    }

    fn on_event(
        &mut self,
        event: StrategyEventView<'_>,
        _ctx: &mut dyn StrategyKernelContext,
    ) -> KernelResult<()> {
        assert_eq!(event.event_type(), "price_update");
        self.events_seen += 1;
        Ok(())
    }
}

#[derive(Default)]
struct OrderKernel;

impl NativeKernel for OrderKernel {
    fn name(&self) -> &str {
        "order"
    }

    fn on_event(
        &mut self,
        _event: StrategyEventView<'_>,
        ctx: &mut dyn StrategyKernelContext,
    ) -> KernelResult<()> {
        ctx.emit(KernelAction::PlaceOrder(PlaceOrderRequest {
            ticker: "KXHIGHMIA-26MAY30-B90".to_string(),
            action: OrderAction::Buy,
            contract_side: ContractSide::Yes,
            order_type: OrderType::Limit,
            quantity: 2,
            limit_price: Some(0.42),
            signal_type: Some("test_signal".to_string()),
            signal_metadata: Some("{\"source\":\"fixture\"}".to_string()),
            client_order_id: Some("kernel-1".to_string()),
        }))
    }
}

#[derive(Default)]
struct FakeState;

impl StrategyKernelState for FakeState {
    fn get_price(&self, _ticker: &str) -> Option<TickerPriceView<'_>> {
        Some(TickerPriceView {
            ticker: "KXHIGHMIA-26MAY30-B90",
            source: "fixture",
            event_ticker: "KXHIGHMIA-26MAY30",
            event_date: "2026-05-30",
            series_ticker: "KXHIGHMIA",
            fee_type: "kalshi",
            fee_multiplier: Some(1.0),
            strike_type: "above",
            floor_strike: Some(90.0),
            cap_strike: None,
            yes_price: 0.42,
            no_price: 0.58,
            yes_bid: Some(0.41),
            yes_ask: Some(0.42),
            no_bid: Some(0.58),
            no_ask: Some(0.59),
            yes_bid_depth: Some(12),
            yes_ask_depth: Some(8),
            no_bid_depth: Some(9),
            no_ask_depth: Some(7),
            yes_bid_levels: &YES_BID_LEVELS,
            yes_ask_levels: &YES_ASK_LEVELS,
            no_bid_levels: &NO_BID_LEVELS,
            no_ask_levels: &NO_ASK_LEVELS,
            orderbook_depth: Some(2),
            volume: Some(100.0),
            peak_yes_ask: Some(0.43),
            last_update: Some(Utc.with_ymd_and_hms(2026, 5, 30, 12, 0, 0).unwrap()),
        })
    }
}

#[derive(Default)]
struct FakeData;

impl StrategyKernelData for FakeData {}

#[derive(Default)]
struct FakeBroker {
    placed: Vec<PlaceOrderRequest>,
}

impl StrategyKernelBroker for FakeBroker {
    fn buying_power(&self) -> Option<f64> {
        Some(100.0)
    }

    fn position_quantity(&self, _ticker: &str, _side: ContractSide) -> i64 {
        0
    }

    fn position_avg_price(&self, _ticker: &str, _side: ContractSide) -> Option<f64> {
        None
    }

    fn place_order(&mut self, request: PlaceOrderRequest) -> KernelResult<OrderResult> {
        self.placed.push(request);
        Ok(OrderResult {
            order_id: "order-1".to_string(),
            sleeve_id: "demo:KMIA".to_string(),
            status: OrderStatus::Filled,
            filled_quantity: 2,
            fill_price: 0.42,
            fee_cost: 0.01,
            reason: String::new(),
        })
    }
}

#[derive(Default)]
struct FakeRuntime {
    wakes: Vec<WakeAtRequest>,
}

impl StrategyKernelRuntime for FakeRuntime {
    fn wake_at(&mut self, request: WakeAtRequest) -> KernelResult<()> {
        self.wakes.push(request);
        Ok(())
    }
}

#[derive(Default)]
struct FakeTelemetry {
    counters: Vec<(String, f64, Vec<(String, String)>)>,
}

impl StrategyKernelTelemetry for FakeTelemetry {
    fn counter(&mut self, name: &str, value: f64, fields: &[(&str, &str)]) -> KernelResult<()> {
        self.counters.push((
            name.to_string(),
            value,
            fields
                .iter()
                .map(|(key, item)| ((*key).to_string(), (*item).to_string()))
                .collect(),
        ));
        Ok(())
    }
}

#[derive(Default)]
struct FakeContext {
    state: FakeState,
    data: FakeData,
    broker: FakeBroker,
    runtime: FakeRuntime,
    telemetry: FakeTelemetry,
    actions: Vec<KernelAction>,
}

impl StrategyKernelContext for FakeContext {
    fn state(&self) -> &dyn StrategyKernelState {
        &self.state
    }

    fn data(&self) -> &dyn StrategyKernelData {
        &self.data
    }

    fn broker(&mut self) -> &mut dyn StrategyKernelBroker {
        &mut self.broker
    }

    fn runtime(&mut self) -> &mut dyn StrategyKernelRuntime {
        &mut self.runtime
    }

    fn telemetry(&mut self) -> &mut dyn StrategyKernelTelemetry {
        &mut self.telemetry
    }

    fn emit(&mut self, action: KernelAction) -> KernelResult<()> {
        self.actions.push(action);
        Ok(())
    }
}

#[test]
fn no_op_kernel_consumes_events_without_actions() {
    let mut kernel = NoopKernel::default();
    let mut ctx = FakeContext::default();
    let event = price_update_event();

    kernel.on_event(event, &mut ctx).unwrap();

    assert_eq!(kernel.events_seen, 1);
    assert!(ctx.actions.is_empty());
}

#[test]
fn kernel_can_emit_deterministic_place_order_action() {
    let mut kernel = OrderKernel;
    let mut ctx = FakeContext::default();

    kernel.on_event(price_update_event(), &mut ctx).unwrap();

    assert_eq!(ctx.actions.len(), 1);
    let encoded = serde_json::to_string(&ctx.actions[0]).unwrap();
    assert_eq!(
        encoded,
        concat!(
            r#"{"type":"place_order","ticker":"KXHIGHMIA-26MAY30-B90","#,
            r#""action":"buy","contract_side":"yes","order_type":"limit","#,
            r#""quantity":2,"limit_price":0.42,"signal_type":"test_signal","#,
            r#""signal_metadata":"{\"source\":\"fixture\"}","client_order_id":"kernel-1"}"#
        ),
    );
}

#[test]
fn kernel_can_emit_deterministic_cancel_actions() {
    let cancel_one = KernelAction::CancelOrder(CancelOrderRequest {
        order_id: "order-1".to_string(),
    });
    let cancel_all = KernelAction::CancelAllOrders(CancelAllOrdersRequest {});

    assert_eq!(
        serde_json::to_string(&cancel_one).unwrap(),
        r#"{"type":"cancel_order","order_id":"order-1"}"#,
    );
    assert_eq!(
        serde_json::to_string(&cancel_all).unwrap(),
        r#"{"type":"cancel_all_orders"}"#,
    );
}

#[test]
fn runtime_timer_requests_stay_engine_owned() {
    let mut ctx = FakeContext::default();
    let wake = WakeAtRequest {
        when: Utc.with_ymd_and_hms(2026, 5, 30, 13, 0, 0).unwrap(),
        name: Some("recheck".to_string()),
    };

    ctx.runtime().wake_at(wake.clone()).unwrap();

    assert_eq!(ctx.runtime.wakes, vec![wake]);
}

#[test]
fn timer_wake_view_preserves_missing_fired_at() {
    let wake = StrategyEventView::TimerWake(TimerWakeView {
        scheduled_for: Utc.with_ymd_and_hms(2026, 5, 30, 13, 0, 0).unwrap(),
        fired_at: None,
        name: "recheck",
    });

    assert_eq!(wake.event_type(), "timer_wake");
    let StrategyEventView::TimerWake(wake) = wake else {
        panic!("expected timer wake");
    };
    assert!(wake.fired_at.is_none());
}

#[test]
fn event_views_preserve_price_update_fields() {
    let StrategyEventView::PriceUpdate(update) = price_update_event() else {
        panic!("expected price update");
    };
    let market = &update.markets[0];

    assert_eq!(update.event_id, Some("evt-1"));
    assert_eq!(update.sequence, Some(42));
    assert_eq!(update.station_id, "KMIA");
    assert_eq!(
        update.timestamp,
        Some(Utc.with_ymd_and_hms(2026, 5, 30, 12, 0, 0).unwrap())
    );
    assert_eq!(market.ticker, "KXHIGHMIA-26MAY30-B90");
    assert_eq!(market.yes_bid, Some(0.41));
    assert_eq!(market.yes_ask, Some(0.42));
    assert_eq!(
        market.yes_bid_levels,
        &[PriceLevelView {
            price: 0.41,
            quantity: 12
        }]
    );
    assert_eq!(
        market.no_ask_levels,
        &[PriceLevelView {
            price: 0.59,
            quantity: 7
        }]
    );
    assert_eq!(market.orderbook_depth, Some(2));
}

#[test]
fn contract_crate_does_not_depend_on_runtime_crates() {
    let manifest = include_str!("../Cargo.toml");

    assert!(!manifest.contains("backtester"));
    assert!(!manifest.contains("trader"));
}

fn price_update_event<'a>() -> StrategyEventView<'a> {
    let markets = Box::leak(
        vec![MarketBracketView {
            market_id: "market-1",
            ticker: "KXHIGHMIA-26MAY30-B90",
            yes_price: 0.42,
            no_price: 0.58,
            event_ticker: "KXHIGHMIA-26MAY30",
            event_date: "2026-05-30",
            strike_type: "above",
            floor_strike: Some(90.0),
            cap_strike: None,
            snapshot_time: None,
            yes_bid: Some(0.41),
            yes_ask: Some(0.42),
            no_bid: Some(0.58),
            no_ask: Some(0.59),
            yes_bid_depth: Some(12),
            yes_ask_depth: Some(8),
            no_bid_depth: Some(9),
            no_ask_depth: Some(7),
            yes_bid_levels: &YES_BID_LEVELS,
            yes_ask_levels: &YES_ASK_LEVELS,
            no_bid_levels: &NO_BID_LEVELS,
            no_ask_levels: &NO_ASK_LEVELS,
            orderbook_depth: Some(2),
            volume: Some(123.0),
        }]
        .into_boxed_slice(),
    );

    StrategyEventView::PriceUpdate(PriceUpdateView {
        event_id: Some("evt-1"),
        sequence: Some(42),
        city_sequence: Some(7),
        emitted_at: Some(Utc.with_ymd_and_hms(2026, 5, 30, 12, 0, 0).unwrap()),
        source: "kalshi",
        slug: "miami",
        station_id: "KMIA",
        city_id: "mia",
        timestamp: Some(Utc.with_ymd_and_hms(2026, 5, 30, 12, 0, 0).unwrap()),
        markets,
    })
}
