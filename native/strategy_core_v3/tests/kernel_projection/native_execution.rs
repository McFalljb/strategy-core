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

fn grant_requests(context: &mut DecisionContextV6) {
    context.capabilities.external_requests = vec!["command:echo".to_owned(), "http:jev".to_owned()];
}

fn jev(path: &str, body: Vec<u8>) -> strategy_core_kernel::HttpRequest {
    strategy_core_kernel::HttpRequest {
        endpoint: "jev".to_owned(),
        method: strategy_core_kernel::HttpMethod::Post,
        path: path.to_owned(),
        body,
        timeout_ms: 5_000,
    }
}

fn echo(arg: String) -> strategy_core_kernel::CommandRequest {
    strategy_core_kernel::CommandRequest {
        command: "echo".to_owned(),
        args: vec![arg],
        stdin: Vec::new(),
        timeout_ms: 1_000,
    }
}

/// An order, then requests up to the per-decision bound, then one request past each bound.
fn requests(context: &mut dyn StrategyKernelContext, seen: &mut Vec<String>) -> KernelResult<()> {
    place_yes(context, seen)?;
    let runtime = context.runtime();
    seen.push(
        runtime
            .request_http(jev("/v1/choose", b"{}".to_vec()))?
            .request_id,
    );
    for index in 0..7 {
        seen.push(runtime.request_command(echo(index.to_string()))?.request_id);
    }
    for (label, outcome) in [
        (
            "ungranted",
            runtime.request_http(strategy_core_kernel::HttpRequest {
                endpoint: "other".to_owned(),
                ..jev("/", Vec::new())
            }),
        ),
        (
            "parent segment",
            runtime.request_http(jev("/v1/../x", Vec::new())),
        ),
        (
            "oversized",
            runtime.request_http(jev("/", vec![b'x'; 64 * 1024])),
        ),
        ("ninth", runtime.request_http(jev("/", Vec::new()))),
    ] {
        seen.push(format!("{label}: {}", outcome.unwrap_err().message()));
    }
    Ok(())
}

/// Native execution issues external requests through the same host code as a transaction:
/// the same grant check, bounds and messages, the same tickets (the command id), and the same
/// staged `ExternalRequest` commands in issue order.
#[test]
fn native_external_requests_match_transactional_tickets_bounds_and_commands() {
    let mut context = priced_context();
    grant_requests(&mut context);
    let reference = decide(&context, requests);
    assert_eq!(reference.result.commands.len(), 9);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut kernel = NativeOnly(ScriptKernel {
        step: requests,
        seen: Rc::clone(&seen),
        updates: Rc::default(),
    });
    let native =
        run_native_decision(&mut kernel, &mut Default::default(), &context, |_| Ok(None)).unwrap();
    assert_eq!(native.commands, reference.result.commands);
    assert_eq!(*seen.borrow(), reference.seen);
    let seen = seen.borrow();
    assert_eq!(seen[1], cid(1, 1));
    assert!(
        seen[9].contains("\"http:other\" is not granted"),
        "{}",
        seen[9]
    );
    assert!(
        seen[10].starts_with("parent segment: invalid request"),
        "{}",
        seen[10]
    );
    assert!(
        seen[11].starts_with("oversized: invalid request"),
        "{}",
        seen[11]
    );
    assert!(
        seen[12].contains("at most 8 external requests"),
        "{}",
        seen[12]
    );
    assert!(matches!(
        &native.commands[1],
        StrategyCommandV6::ExternalRequest { command_id, target, .. }
            if *command_id == cid(1, 1) && target == "jev"
    ));
    // Requests are not Broker commands: the host's seen section tracks only the order.
    assert_eq!(native.acknowledged_command_ids, Vec::<String>::new());
}

/// A native host that grants nothing refuses every request, as a transaction does.
#[test]
fn native_requests_need_the_external_requests_grant() {
    fn refused(
        context: &mut dyn StrategyKernelContext,
        seen: &mut Vec<String>,
    ) -> KernelResult<()> {
        let runtime = context.runtime();
        for outcome in [
            runtime.request_http(jev("/", Vec::new())),
            runtime.request_command(echo("x".to_owned())),
        ] {
            seen.push(outcome.unwrap_err().message().to_owned());
        }
        Ok(())
    }
    let context = priced_context();
    let reference = decide(&context, refused);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut kernel = NativeOnly(ScriptKernel {
        step: refused,
        seen: Rc::clone(&seen),
        updates: Rc::default(),
    });
    let native =
        run_native_decision(&mut kernel, &mut Default::default(), &context, |_| Ok(None)).unwrap();
    assert!(native.commands.is_empty());
    assert_eq!(*seen.borrow(), reference.seen);
    assert!(
        seen.borrow()
            .iter()
            .all(|message| message.contains("is not granted"))
    );
}

/// An `ExternalResponse` is a native event like any other: the kernel sees it (after any order
/// updates), may act on it, and the invocation acknowledges the answered request exactly as a
/// completed transactional result does. A failed invocation returns nothing to acknowledge.
#[test]
fn a_native_external_response_is_delivered_and_acknowledged() {
    use strategy_core_v3::decision_v6::{ExternalErrorKindV6, ExternalOutcomeV6};
    fn answer(context: &mut dyn StrategyKernelContext, seen: &mut Vec<String>) -> KernelResult<()> {
        seen.push(
            context
                .runtime()
                .request_command(echo("again".to_owned()))?
                .request_id,
        );
        Ok(())
    }
    let request_id = cid(0, 3);
    for outcome in [
        ExternalOutcomeV6::Ok {
            status: 200,
            body: b"{\"p\":[0.1,0.9]}".to_vec(),
        },
        ExternalOutcomeV6::Err {
            kind: ExternalErrorKindV6::Refused,
            message: "external requests are not configured".to_owned(),
        },
    ] {
        let mut context = priced_context();
        grant_requests(&mut context);
        context.trigger = TriggerV6::ExternalResponse {
            request_id: request_id.clone(),
            outcome,
        };
        context.validate().unwrap();
        let reference = decide(&context, answer);
        assert_eq!(
            reference.result.acknowledged_command_ids,
            [request_id.clone()]
        );
        let seen = Rc::new(RefCell::new(Vec::new()));
        let mut kernel = NativeOnly(ScriptKernel {
            step: answer,
            seen: Rc::clone(&seen),
            updates: Rc::default(),
        });
        let mut section = Default::default();
        let native =
            run_native_decision(&mut kernel, &mut section, &context, |_| Ok(None)).unwrap();
        assert_eq!(*seen.borrow(), reference.seen);
        assert!(
            seen.borrow()[0].starts_with("event=ExternalResponse("),
            "{:?}",
            seen.borrow()
        );
        assert_eq!(native.commands, reference.result.commands);
        assert_eq!(
            native.acknowledged_command_ids,
            reference.result.acknowledged_command_ids
        );

        fn fail(_: &mut dyn StrategyKernelContext, _: &mut Vec<String>) -> KernelResult<()> {
            Err(strategy_core_kernel::KernelError::new("cannot parse"))
        }
        kernel.0.step = fail;
        assert!(run_native_decision(&mut kernel, &mut section, &context, |_| Ok(None)).is_err());
    }
}

/// Runs `step` natively over `seen` and transactionally over the context's checkpoint, and
/// asserts both saw the same updates and refusals and issued the same commands.
fn assert_native_matches_transaction(
    context: &DecisionContextV6,
    mut seen: strategy_core_v3::decision_v6::RunnerSectionV6,
    step: Step,
) -> Decision {
    let reference = decide(context, step);
    let outcomes = Rc::new(RefCell::new(Vec::new()));
    let updates = Rc::new(RefCell::new(Vec::new()));
    let mut kernel = NativeOnly(ScriptKernel {
        step,
        seen: Rc::clone(&outcomes),
        updates: Rc::clone(&updates),
    });
    let native = run_native_decision(&mut kernel, &mut seen, context, |_| Ok(None)).unwrap();
    assert_eq!(*updates.borrow(), reference.updates);
    assert_eq!(*outcomes.borrow(), reference.seen);
    assert_eq!(native.commands, reference.result.commands);
    assert_eq!(
        native.acknowledged_command_ids,
        reference.result.acknowledged_command_ids
    );
    reference
}

/// With 256 live entries, an update that ends one does not free its slot for this decision's
/// commands: a transaction keeps it for the entry a failed update would bring back, and
/// native admission reserves the same slot.
#[test]
fn native_admission_reserves_the_runner_slot_of_an_ending_update() {
    use BrokerOrderStatusV6::*;
    fn one_more(
        context: &mut dyn StrategyKernelContext,
        seen: &mut Vec<String>,
    ) -> KernelResult<()> {
        let outcome =
            context
                .broker()
                .place_order(limit_buy("one-more", ContractSide::Yes, 100, 0.01));
        seen.push(format!("{:?}", outcome.map_err(|error| error.to_string())));
        Ok(())
    }
    // 191 open orders and 65 cancels waiting for their receipts: 256 live entries. One order
    // is now cancelled.
    let (_, checkpoint) = crowded_with(191, 65, 0);
    let orders = (0..191)
        .map(|index| {
            let (status, revision) = if index == 0 {
                (Cancelled, 2)
            } else {
                (Resting, 1)
            };
            order(
                &format!("command.old.{index:03}"),
                &format!("old-{index:03}"),
                status,
                0,
                revision,
            )
        })
        .collect::<Vec<_>>();
    let mut context = follow_up(&priced_context(), None, 2, orders, vec![]);
    context.kernel_checkpoint = Some(checkpoint.clone());
    context.validate().unwrap();
    let reference = assert_native_matches_transaction(&context, checkpoint.runner, one_more);
    assert_eq!(reference.updates.len(), 1, "the cancellation is reported");
    assert_eq!(
        reference.seen.last().unwrap(),
        r#"Err("the runner tracks at most 256 orders and commands")"#
    );
}

/// Near the result size bound, native admission reserves the bytes a transaction's result
/// carries besides its commands (the order-update evidence and the acknowledgeable ids), so
/// the same orders fit.
#[test]
fn native_admission_reserves_the_result_bytes_of_evidence_and_acknowledgements() {
    use BrokerOrderStatusV6::*;
    /// Fills the result with orders whose metadata halves on each refusal, down to one byte.
    fn fill(context: &mut dyn StrategyKernelContext, seen: &mut Vec<String>) -> KernelResult<()> {
        let mut size = 60 * 1024;
        let mut index = 0;
        while size > 0 {
            let mut request = limit_buy(&format!("big-{index}"), ContractSide::Yes, 100, 0.01);
            request.signal_metadata = Some("m".repeat(size));
            index += 1;
            match context.broker().place_order(request) {
                Ok(ticket) => seen.push(format!("{size}: {}", ticket.command_id)),
                Err(error) => {
                    seen.push(format!("{size}: {error}"));
                    size /= 2;
                }
            }
        }
        Ok(())
    }
    let context = priced_context();
    let mut seen = Default::default();
    let mut kernel = NativeOnly(ScriptKernel {
        step: place_yes,
        seen: Rc::default(),
        updates: Rc::default(),
    });
    run_native_decision(&mut kernel, &mut seen, &context, |_| Ok(None)).unwrap();
    let placed = decide(&context, place_yes).result;
    // The placed order filled, 255 other terminal orders and 256 receipts to acknowledge.
    let terminal = std::iter::once(order(&cid(1, 0), "yes-1", Filled, 300, 2))
        .chain((0..255).map(|index| {
            order(
                &format!("t.{index:03}"),
                &format!("t{index:03}"),
                Filled,
                300,
                1,
            )
        }))
        .collect();
    let receipts = (0..256)
        .map(|index| CommandReceiptV6 {
            command_id: format!("c.{index:03}"),
            kind: BrokerCommandKindV6::CancelOrder,
            outcome: CommandOutcomeV6::Accepted,
        })
        .collect();
    let next = follow_up(&context, Some(&placed), 2, terminal, receipts);
    let reference = assert_native_matches_transaction(&next, seen, fill);
    assert_eq!(reference.updates.len(), 1, "the fill is reported");
    assert!(
        reference.seen.iter().any(|outcome| outcome
            .ends_with("the decision's commands would exceed the result size bound")),
        "{:?}",
        reference.seen
    );
}
