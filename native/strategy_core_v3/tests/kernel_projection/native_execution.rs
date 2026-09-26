//! Native execution requires only NativeKernel, never a checkpoint codec or factory.

use super::*;
use strategy_core_v3::kernel_v6::{
    KernelEvent, KernelHost, KernelSnapshot, NativeDecision, NativeIdentity, NativeInvocation,
};

// Test-only adapter: the transaction fixture supplies the oracle values. Runtime callers
// supply their own canonical state directly, without constructing a DecisionContextV6.
fn run_native_decision(
    kernel: &mut NativeOnly,
    seen: &mut strategy_core_v3::decision_v6::RunnerSectionV6,
    context: &DecisionContextV6,
    cap: impl Fn(&PlaceOrderRequest) -> Result<Option<u64>, KernelTransactionError>,
) -> Result<NativeDecision, KernelTransactionError> {
    let mut reference = KernelHost::new(context, Vec::new(), true)?;
    let finances = reference.broker().financial_state();
    let timers = reference.runtime().pending_timers();
    let now = reference.runtime().now().unwrap();
    let capabilities = reference.capabilities();
    let event = KernelEvent::from_context(context)?;
    strategy_core_v3::kernel_v6::run_native_decision(
        kernel,
        seen,
        &NativeInvocation {
            identity: NativeIdentity {
                sleeve_id: &context.owner_state.sleeve.sleeve_id,
                incarnation: context.owner_state.sleeve.incarnation,
                delivery_id: &context.owner_state.delivery_id,
            },
            now,
            event: event.event(),
            state: reference.state(),
            parameters: reference.parameters(),
            capabilities: &capabilities,
            contributor_stations: reference.contributor_stations(),
            pending_timers: &timers,
            deployment_mode: context.deployment_mode,
            finances,
            broker: &context.broker,
            orders_complete: context.orders_complete,
            receipts: &context.command_receipts,
        },
        cap,
    )
}

struct NativeOnly(ScriptKernel);

impl NativeKernel for NativeOnly {
    fn name(&self) -> &str {
        self.0.name()
    }
    fn on_start(&mut self, context: &mut dyn StrategyKernelContext) -> KernelResult<()> {
        self.0.on_start(context)
    }
    fn on_event(
        &mut self,
        event: StrategyEventView<'_>,
        context: &mut dyn StrategyKernelContext,
    ) -> KernelResult<()> {
        self.0.on_event(event, context)
    }
}

#[test]
fn native_error_does_not_return_staged_commands_or_advance_delivery_bookkeeping() {
    fn fail(context: &mut dyn StrategyKernelContext, seen: &mut Vec<String>) -> KernelResult<()> {
        place_yes(context, seen)?;
        Err(strategy_core_kernel::KernelError::new("after staged order"))
    }
    let mut kernel = NativeOnly(ScriptKernel {
        step: fail,
        seen: Rc::default(),
        updates: Rc::default(),
    });
    let mut seen = Default::default();
    let error =
        run_native_decision(&mut kernel, &mut seen, &priced_context(), |_| Ok(None)).unwrap_err();
    assert!(error.to_string().contains("after staged order"));
    assert!(!seen.seeded);
    assert!(seen.entries.is_empty());
}

#[test]
fn native_invocation_needs_no_transport_context_or_projection() {
    use strategy_core_kernel::{MarketState, StationState, StrategyKernelState};
    struct State(StationState);
    impl StrategyKernelState for State {
        fn station(&self, id: &str) -> Option<&StationState> {
            (id == "KMIA").then_some(&self.0)
        }
        fn market(&self, _: &str) -> Option<&MarketState> {
            None
        }
    }
    fn read(context: &mut dyn StrategyKernelContext, _: &mut Vec<String>) -> KernelResult<()> {
        assert_eq!(
            context
                .state()
                .station("KMIA")
                .unwrap()
                .weather
                .running_high,
            Some(87.25)
        );
        Ok(())
    }
    let mut state = State(StationState::default());
    state.0.weather.running_high = Some(87.25);
    let mut kernel = NativeOnly(ScriptKernel {
        step: read,
        seen: Rc::default(),
        updates: Rc::default(),
    });
    let result = strategy_core_v3::kernel_v6::run_native_decision(
        &mut kernel,
        &mut Default::default(),
        &NativeInvocation {
            identity: NativeIdentity {
                sleeve_id: "unused-no-commands",
                incarnation: 1,
                delivery_id: "native.1",
            },
            now: chrono::DateTime::from_timestamp(123, 0).unwrap(),
            event: None,
            state: &state,
            parameters: &Default::default(),
            capabilities: &Default::default(),
            contributor_stations: &[],
            pending_timers: &[],
            deployment_mode: strategy_core_v3::decision_v6::DeploymentModeV6::Paper,
            finances: Default::default(),
            broker: &strategy_core_v3::decision_v6::BrokerDetailV6 {
                revision: 1,
                reserved_cash_micros: 0,
                positions: Vec::new(),
                orders: Vec::new(),
            },
            orders_complete: true,
            receipts: &[],
        },
        |_| Ok(None),
    )
    .unwrap();
    assert!(result.commands.is_empty());
}

#[test]
fn native_borrowed_views_are_the_same_complete_canonical_views() {
    fn check(context: &mut dyn StrategyKernelContext, _: &mut Vec<String>) -> KernelResult<()> {
        let expected = KernelSnapshot::from_context(&priced_context()).unwrap();
        for station in expected.station_states() {
            let actual = context.state().station(station.station_id()).unwrap();
            assert_eq!(actual, station);
            assert!(std::ptr::eq(
                actual,
                context.state().station(station.station_id()).unwrap()
            ));
        }
        for market in expected.market_states() {
            let actual = context.state().market(&market.market_id).unwrap();
            assert_eq!(actual, market);
            assert!(std::ptr::eq(
                actual,
                context.state().market(&market.market_id).unwrap()
            ));
        }
        assert!(context.state().station("out-of-scope").is_none());
        assert!(context.state().market("out-of-scope").is_none());
        Ok(())
    }
    let mut kernel = NativeOnly(ScriptKernel {
        step: check,
        seen: Rc::default(),
        updates: Rc::default(),
    });
    run_native_decision(
        &mut kernel,
        &mut Default::default(),
        &priced_context(),
        |_| Ok(None),
    )
    .unwrap();
}

#[test]
fn resident_execution_matches_transactional_tickets_and_fill_updates_without_a_codec() {
    use BrokerOrderStatusV6::*;
    let context = priced_context();
    let updates = Rc::new(RefCell::new(Vec::new()));
    let mut kernel = NativeOnly(ScriptKernel {
        step: place_yes,
        seen: Rc::default(),
        updates: Rc::clone(&updates),
    });
    let mut seen = Default::default();
    let native = run_native_decision(&mut kernel, &mut seen, &context, |_| Ok(None)).unwrap();
    let mut previous = decide(&context, place_yes).result;
    assert_eq!(native.commands, previous.commands);
    kernel.0.step = nothing;
    for (index, (status, filled)) in [
        (DurablyAccepted, 0),
        (Resting, 0),
        (PartiallyFilled, 100),
        (PartiallyFilled, 100),
        (Filled, 300),
        (Filled, 300),
    ]
    .into_iter()
    .enumerate()
    {
        let delivery = index as u32 + 2;
        let mut next = follow_up(
            &context,
            Some(&previous),
            delivery,
            vec![order(
                &cid(1, 0),
                "yes-1",
                status,
                filled,
                u64::from(delivery),
            )],
            vec![],
        );
        let reference = decide(&next, nothing);
        // Deliberately supply no checkpoint: all native state belongs to the host.
        next.kernel_checkpoint = None;
        updates.borrow_mut().clear();
        let native = run_native_decision(&mut kernel, &mut seen, &next, |_| Ok(None)).unwrap();
        assert_eq!(*updates.borrow(), reference.updates);
        assert_eq!(native.commands, reference.result.commands);
        assert_eq!(
            native.acknowledged_command_ids,
            reference.result.acknowledged_command_ids
        );
        previous = reference.result;
    }
}
