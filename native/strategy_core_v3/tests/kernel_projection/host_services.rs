//! Host services a kernel reads or requests beyond Broker calls: its parameters, the host's
//! capabilities, gauge and annotation telemetry, cancellable timers, and external requests
//! with their responses.

use super::*;
use strategy_core_kernel::{
    AnnotationValue, CommandRequest, HttpMethod, HttpRequest, KernelCapabilities, ParameterValue,
    RequestTicket, RuntimeMode, TimerHandle, WakeAtRequest,
};
use strategy_core_v3::decision_v4::TimerRecoveryV4;
use strategy_core_v3::decision_v6::{
    AnnotationValueV6, DecisionResultV6, ExternalErrorKindV6, ExternalOutcomeV6,
    ExternalRequestKindV6, HttpMethodV6, StrategyParameterValueV6, TelemetryEntryV6,
};

type Script = fn(&mut dyn StrategyKernelContext, &mut Vec<String>) -> KernelResult<()>;

/// Runs one script from `on_start` (Bootstrap/Recovery) or `on_event`.
#[derive(Clone)]
struct ServicesKernel {
    script: Script,
    seen: Rc<RefCell<Vec<String>>>,
}

impl ServicesKernel {
    fn run(&mut self, context: &mut dyn StrategyKernelContext) -> KernelResult<()> {
        let mut seen = Vec::new();
        let result = (self.script)(context, &mut seen);
        self.seen.borrow_mut().extend(seen);
        result
    }
}

impl NativeKernel for ServicesKernel {
    fn name(&self) -> &str {
        "services"
    }

    fn on_start(&mut self, context: &mut dyn StrategyKernelContext) -> KernelResult<()> {
        self.run(context)
    }

    fn on_event(
        &mut self,
        _event: StrategyEventView<'_>,
        context: &mut dyn StrategyKernelContext,
    ) -> KernelResult<()> {
        self.run(context)
    }
}

impl TransactionKernel for ServicesKernel {
    fn encode_checkpoint_state(&self) -> Result<Vec<u8>, KernelTransactionError> {
        Ok(b"services".to_vec())
    }
}

struct ServicesFactory(ServicesKernel);

impl TransactionKernelFactory for ServicesFactory {
    type Kernel = ServicesKernel;

    fn checkpoint_codec(
        &self,
        _strategy_id: &str,
    ) -> Result<KernelCheckpointCodec, KernelTransactionError> {
        Ok(KernelCheckpointCodec {
            profile: "services.checkpoint.v1".to_owned(),
            version: 1,
        })
    }
    fn create(&self, _context: &DecisionContextV6) -> Result<Self::Kernel, KernelTransactionError> {
        Ok(self.0.clone())
    }
    fn restore(
        &self,
        _context: &DecisionContextV6,
        _checkpoint: &strategy_core_v3::decision_v6::KernelCheckpointV6,
    ) -> Result<Self::Kernel, KernelTransactionError> {
        Ok(self.0.clone())
    }
}

fn run_script(context: &DecisionContextV6, script: Script) -> (Vec<String>, DecisionResultV6) {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let factory = ServicesFactory(ServicesKernel {
        script,
        seen: Rc::clone(&seen),
    });
    let result = run_transaction(&factory, context).unwrap();
    let seen = seen.borrow().clone();
    (seen, result)
}

#[test]
fn kernels_read_exact_parameters_and_the_granted_capabilities() {
    let mut context = observation_context(Some("22.8"), Some("73"));
    context.strategy.parameters = vec![
        ("enabled".to_owned(), StrategyParameterValueV6::Bool(true)),
        (
            "max_price".to_owned(),
            StrategyParameterValueV6::Decimal {
                coefficient: 4_250,
                scale: 4,
            },
        ),
        (
            "mode".to_owned(),
            StrategyParameterValueV6::String("fast".to_owned()),
        ),
        ("offset".to_owned(), StrategyParameterValueV6::I64(-3)),
        ("unset".to_owned(), StrategyParameterValueV6::Null),
        ("window".to_owned(), StrategyParameterValueV6::U64(90)),
    ];
    context.capabilities.external_requests = vec!["http:weather.lookup".to_owned()];
    let (seen, result) = run_script(&context, |context, seen| {
        let parameters = context.parameters();
        seen.push(format!(
            "keys={:?}",
            parameters.iter().map(|(key, _)| key).collect::<Vec<_>>()
        ));
        assert_eq!(parameters.len(), 6);
        assert_eq!(
            parameters.get("enabled").and_then(ParameterValue::as_bool),
            Some(true)
        );
        assert_eq!(
            parameters.get("max_price"),
            Some(&ParameterValue::Decimal {
                coefficient: 4_250,
                scale: 4
            }),
            "configured digits are kept"
        );
        assert_eq!(
            parameters.get("max_price").and_then(ParameterValue::as_f64),
            Some(0.425)
        );
        assert_eq!(
            parameters.get("mode").and_then(ParameterValue::as_str),
            Some("fast")
        );
        assert_eq!(
            parameters.get("offset").and_then(ParameterValue::as_i64),
            Some(-3)
        );
        assert_eq!(
            parameters.get("offset").and_then(ParameterValue::as_u64),
            None
        );
        assert!(parameters.get("unset").is_some_and(ParameterValue::is_null));
        assert_eq!(
            parameters.get("window").and_then(ParameterValue::as_u64),
            Some(90)
        );
        assert!(parameters.get("absent").is_none());

        let capabilities = context.capabilities();
        assert_eq!(capabilities.mode, Some(RuntimeMode::Paper));
        assert!(capabilities.timers);
        assert!(capabilities.timer_handles);
        assert_eq!(capabilities.external_requests, ["http:weather.lookup"]);
        assert_eq!(context.contributor_stations(), [STATION]);
        seen.push(format!("capabilities={capabilities:?}"));
        Ok(())
    });
    assert_eq!(result.disposition, DecisionDispositionV6::Completed);
    assert_eq!(
        seen[0],
        r#"keys=["enabled", "max_price", "mode", "offset", "unset", "window"]"#
    );
    let json = strategy_core_v3::kernel_v6::strategy_parameters_json(&context).unwrap();
    assert_eq!(
        json["max_price"],
        serde_json::json!(0.425),
        "same f64 as the initializer"
    );
    assert!(!KernelCapabilities::default().timers);
}

#[test]
fn gauges_and_annotations_are_recorded_in_order_beside_counters() {
    let context = observation_context(Some("22.8"), Some("73"));
    let (_, result) = run_script(&context, |context, _| {
        assert!(context.capabilities().gauges);
        assert!(context.capabilities().annotations);
        let telemetry = context.telemetry();
        telemetry.counter("orders_considered", 1.0, &[])?;
        telemetry.gauge("buying_power", 12.5, &[("sleeve", "a")])?;
        telemetry.annotate("gate", AnnotationValue::Text("price_moved"), &[])?;
        telemetry.annotate("attempt", AnnotationValue::Integer(-2), &[])?;
        telemetry.annotate("edge", AnnotationValue::Float(0.1), &[("unit", "dollars")])?;
        telemetry.annotate("armed", AnnotationValue::Bool(false), &[])?;
        telemetry.annotate("reason", AnnotationValue::Null, &[])?;
        context.emit(KernelAction::Log(LogAction {
            level: "info".to_owned(),
            message: "after".to_owned(),
        }))?;
        Ok(())
    });
    assert_eq!(result.disposition, DecisionDispositionV6::Completed);
    assert!(result.commands.is_empty());
    let fields = |pairs: &[(&str, &str)]| {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect::<Vec<_>>()
    };
    let annotation = |name: &str, value, pairs: &[(&str, &str)]| TelemetryEntryV6::Annotation {
        name: name.to_owned(),
        value,
        fields: fields(pairs),
    };
    assert_eq!(
        result.telemetry,
        [
            TelemetryEntryV6::Counter {
                name: "orders_considered".to_owned(),
                value_bits: 1.0_f64.to_bits(),
                fields: vec![],
            },
            TelemetryEntryV6::Gauge {
                name: "buying_power".to_owned(),
                value_bits: 12.5_f64.to_bits(),
                fields: fields(&[("sleeve", "a")]),
            },
            annotation(
                "gate",
                AnnotationValueV6::Text("price_moved".to_owned()),
                &[]
            ),
            annotation("attempt", AnnotationValueV6::Integer(-2), &[]),
            annotation(
                "edge",
                AnnotationValueV6::FloatBits(0.1_f64.to_bits()),
                &[("unit", "dollars")]
            ),
            annotation("armed", AnnotationValueV6::Bool(false), &[]),
            annotation("reason", AnnotationValueV6::Null, &[]),
        ]
    );
    let rows = result
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(rows, [("kernel_log", "after")]);
}

#[test]
fn a_rejected_decision_keeps_its_gauges_and_annotations() {
    let context = observation_context(Some("22.8"), Some("73"));
    let (_, result) = run_script(&context, |context, _| {
        context.telemetry().gauge("depth", 3.0, &[])?;
        context
            .telemetry()
            .annotate("why", AnnotationValue::Text("no_edge"), &[])?;
        Err(strategy_core_kernel::KernelError::new("fixture failure"))
    });
    assert_eq!(result.disposition, DecisionDispositionV6::Rejected);
    assert!(result.commands.is_empty());
    assert_eq!(result.kernel_checkpoint, context.kernel_checkpoint);
    let codes = result
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect::<Vec<_>>();
    assert_eq!(codes, ["kernel_error"]);
    assert!(matches!(
        &result.telemetry[..],
        [
            TelemetryEntryV6::Gauge { .. },
            TelemetryEntryV6::Annotation { .. }
        ]
    ));
}

#[test]
fn telemetry_is_bounded_with_explicit_overflow_accounting() {
    let context = observation_context(Some("22.8"), Some("73"));
    let (_, result) = run_script(&context, |context, _| {
        let message = serde_json::json!({"reason": "large_evidence", "details": "x".repeat(5000)})
            .to_string();
        for _ in 0..80 {
            context.emit(KernelAction::Log(LogAction {
                level: "info".to_owned(),
                message: message.clone(),
            }))?;
        }
        for index in 0..300 {
            context.telemetry().counter("late", f64::from(index), &[])?;
        }
        Ok(())
    });
    assert_eq!(result.disposition, DecisionDispositionV6::Completed);
    let overflow = result
        .diagnostics
        .iter()
        .find(|d| d.code == "kernel_telemetry_overflow")
        .expect("overflow is accounted");
    let lost =
        serde_json::from_str::<serde_json::Value>(&overflow.message).unwrap()["lost_entries"]
            .as_u64()
            .unwrap();
    // 80 logs over the diagnostic bound go to evidence up to its 64 entries; 256 counters fit.
    assert_eq!(lost, 16 + 44);
    assert_eq!(result.telemetry.len(), 256);
    assert!(result.evidence.iter().all(|evidence| {
        serde_json::from_slice::<serde_json::Value>(&evidence.payload).is_ok()
    }));
}

thread_local! {
    /// Stands in for a kernel checkpoint carrying a handle between decisions.
    static KEPT_HANDLE: RefCell<Option<TimerHandle>> = const { RefCell::new(None) };
}

fn wake(name: &str) -> WakeAtRequest {
    WakeAtRequest {
        when: ns(EMITTED_NS) + chrono::Duration::minutes(5),
        name: Some(name.to_owned()),
    }
}

fn timer_commands(result: &DecisionResultV6) -> Vec<(String, String, String, &'static str)> {
    result
        .commands
        .iter()
        .filter_map(|command| match command {
            StrategyCommandV6::ScheduleTimer {
                command_id,
                key,
                generation,
                ..
            } => Some((
                command_id.clone(),
                key.clone(),
                generation.clone(),
                "schedule",
            )),
            StrategyCommandV6::CancelTimer {
                command_id,
                key,
                generation,
            } => Some((
                command_id.clone(),
                key.clone(),
                generation.clone(),
                "cancel",
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn a_timer_handle_cancels_with_the_generation_the_timer_was_scheduled_under() {
    // Decision 1 schedules and keeps the handle.
    let first = observation_context(Some("22.8"), Some("73"));
    let (_, scheduled) = run_script(&first, |context, _| {
        assert!(context.capabilities().timer_handles);
        let handle = context.runtime().wake_at(wake("exit.check"))?;
        KEPT_HANDLE.with(|kept| *kept.borrow_mut() = Some(handle));
        Ok(())
    });
    let handle = KEPT_HANDLE.with(|kept| kept.borrow_mut().take()).unwrap();
    assert_eq!(
        handle,
        TimerHandle {
            key: "exit.check".to_owned(),
            generation: "timer.delivery.daily.1".to_owned(),
        }
    );
    let [(_, key, generation, "schedule")] = &timer_commands(&scheduled)[..] else {
        panic!("one schedule: {:?}", scheduled.commands);
    };
    assert_eq!((key, generation), (&handle.key, &handle.generation));

    // Decision 2, a later delivery: the host reports the pending timer as traderv3 recorded
    // it, and the kept handle cancels it under the scheduling decision's generation.
    let mut second = observation_context(Some("22.8"), Some("73"));
    second.owner_state.delivery_id = "delivery.daily.2".to_owned();
    second.owner_state.timer_recovery = Some(vec![
        TimerRecoveryV4 {
            key: "exit.check".to_owned(),
            scheduled_at: u64::try_from(EMITTED_NS).unwrap(),
            generation: generation.clone(),
            admission_state: 0,
        },
        TimerRecoveryV4 {
            key: "old.fired".to_owned(),
            scheduled_at: 1,
            generation: "timer.delivery.daily.0".to_owned(),
            admission_state: 2,
        },
    ]);
    KEPT_HANDLE.with(|kept| *kept.borrow_mut() = Some(handle.clone()));
    let (seen, cancelled) = run_script(&second, |context, seen| {
        let pending = context.runtime().pending_timers();
        seen.push(format!("{pending:?}"));
        let kept = KEPT_HANDLE.with(|kept| kept.borrow_mut().take()).unwrap();
        assert_eq!(pending.len(), 1, "only active timers are pending");
        assert_eq!(pending[0].handle, kept);
        assert_eq!(pending[0].scheduled_for, ns(EMITTED_NS));
        context.runtime().cancel_timer(&kept)
    });
    assert_eq!(cancelled.disposition, DecisionDispositionV6::Completed);
    assert_eq!(
        timer_commands(&cancelled),
        [(
            cid(2, 0),
            "exit.check".to_owned(),
            "timer.delivery.daily.1".to_owned(),
            "cancel"
        )]
    );
    assert!(seen[0].contains("exit.check"), "{seen:?}");
}

#[test]
fn a_decision_carries_at_most_one_timer_operation_per_key() {
    let context = observation_context(Some("22.8"), Some("73"));
    let (_, result) = run_script(&context, |context, seen| {
        let runtime = context.runtime();
        let handle = runtime.wake_at(wake("a"))?;
        seen.push("scheduled".to_owned());
        assert!(
            runtime.cancel_timer(&handle).is_err(),
            "schedule then cancel"
        );
        assert!(runtime.wake_at(wake("a")).is_err(), "two schedules");
        runtime.wake_at(wake("b"))?;
        let other = TimerHandle {
            key: "c".to_owned(),
            generation: "timer.delivery.daily.0".to_owned(),
        };
        runtime.cancel_timer(&other)?;
        assert!(runtime.cancel_timer(&other).is_err(), "two cancels");
        assert!(runtime.wake_at(wake("c")).is_err(), "cancel then schedule");
        assert!(
            runtime
                .cancel_timer(&TimerHandle {
                    key: "bad key".to_owned(),
                    generation: "timer.x".to_owned(),
                })
                .is_err()
        );
        assert!(runtime.wake_at(wake("")).is_err());
        let unnamed = runtime.wake_at(WakeAtRequest {
            when: ns(EMITTED_NS),
            name: None,
        })?;
        assert_eq!(unnamed.key, "kernel.wake");
        Ok(())
    });
    let mut untimed = context.clone();
    untimed.capabilities.timers = false;
    let (_, refused) = run_script(&untimed, |context, _| {
        assert!(!context.capabilities().timers);
        assert!(context.runtime().wake_at(wake("a")).is_err());
        Ok(())
    });
    assert!(refused.commands.is_empty(), "no timers without the grant");
    assert_eq!(result.disposition, DecisionDispositionV6::Completed);
    assert_eq!(
        timer_commands(&result),
        [
            (
                cid(1, 0),
                "a".to_owned(),
                "timer.delivery.daily.1".to_owned(),
                "schedule"
            ),
            (
                cid(1, 1),
                "b".to_owned(),
                "timer.delivery.daily.1".to_owned(),
                "schedule"
            ),
            (
                cid(1, 2),
                "c".to_owned(),
                "timer.delivery.daily.0".to_owned(),
                "cancel"
            ),
            (
                cid(1, 3),
                "kernel.wake".to_owned(),
                "timer.delivery.daily.1".to_owned(),
                "schedule"
            ),
        ]
    );
}

fn request_grants(context: &mut DecisionContextV6) {
    context.capabilities.external_requests = vec!["command:echo".to_owned(), "http:jev".to_owned()];
}

fn jev(path: &str) -> HttpRequest {
    HttpRequest {
        endpoint: "jev".to_owned(),
        method: HttpMethod::Post,
        path: path.to_owned(),
        body: b"{\"options\":6}".to_vec(),
        timeout_ms: 5_000,
    }
}

fn echo(args: &[&str]) -> CommandRequest {
    CommandRequest {
        command: "echo".to_owned(),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        stdin: Vec::new(),
        timeout_ms: 1_000,
    }
}

/// A request is one more command of the decision, beside its orders; its ticket's request id
/// is its command id.
#[test]
fn external_requests_share_a_decision_with_orders_and_return_tickets() {
    let mut context = observation_context(Some("22.8"), Some("73"));
    request_grants(&mut context);
    let (seen, result) = run_script(&context, |context, seen| {
        let order = context.broker().place_order(PlaceOrderRequest {
            ticker: MARKET.to_owned(),
            action: OrderAction::Buy,
            contract_side: ContractSide::Yes,
            order_type: OrderType::Limit,
            quantity: ContractQuantity::from_hundredths(100),
            limit_price: Some(0.42),
            expires_after_ms: None,
            reduce_only: false,
            signal_type: None,
            signal_metadata: None,
            client_order_id: Some("value-yes-1".to_owned()),
        })?;
        let http = context.runtime().request_http(jev("/v1/choose"))?;
        let command = context.runtime().request_command(echo(&["hello"]))?;
        seen.extend([order.command_id, http.request_id, command.request_id]);
        Ok(())
    });
    assert_eq!(result.disposition, DecisionDispositionV6::Completed);
    assert_eq!(seen, [cid(1, 0), cid(1, 1), cid(1, 2)]);
    assert_eq!(result.commands.len(), 3);
    assert!(matches!(
        result.commands[0],
        StrategyCommandV6::PlaceOrder(_)
    ));
    assert_eq!(
        result.commands[1],
        StrategyCommandV6::ExternalRequest {
            command_id: cid(1, 1),
            kind: ExternalRequestKindV6::Http {
                method: HttpMethodV6::Post,
                path: "/v1/choose".to_owned(),
            },
            target: "jev".to_owned(),
            payload: b"{\"options\":6}".to_vec(),
            timeout_ms: 5_000,
        }
    );
    assert_eq!(
        result.commands[2],
        StrategyCommandV6::ExternalRequest {
            command_id: cid(1, 2),
            kind: ExternalRequestKindV6::Command {
                args: vec!["hello".to_owned()],
            },
            target: "echo".to_owned(),
            payload: Vec::new(),
            timeout_ms: 1_000,
        }
    );
    assert!(result.commands.iter().all(StrategyCommandV6::leaves_host));
    // Only the order is tracked by the runner section.
    let runner = &result.kernel_checkpoint.as_ref().unwrap().runner;
    assert_eq!(runner.entries.len(), 1);
    assert_eq!(runner.entries[0].command_id, cid(1, 0));
}

/// Everything outside the grant or the bounds is a local error, and the decision goes on.
#[test]
fn an_ungranted_or_unbounded_request_is_a_local_error() {
    let mut context = observation_context(Some("22.8"), Some("73"));
    request_grants(&mut context);
    let (seen, result) = run_script(&context, |context, seen| {
        let runtime = context.runtime();
        let mut refuse = |label: &str, outcome: KernelResult<RequestTicket>| {
            let error = outcome.expect_err(label);
            seen.push(format!("{label}: {}", error.message()));
        };
        refuse(
            "unknown endpoint",
            runtime.request_http(HttpRequest {
                endpoint: "other".to_owned(),
                ..jev("/")
            }),
        );
        refuse(
            "a command name as an endpoint",
            runtime.request_http(HttpRequest {
                endpoint: "echo".to_owned(),
                ..jev("/")
            }),
        );
        refuse(
            "an endpoint name as a command",
            runtime.request_command(CommandRequest {
                command: "jev".to_owned(),
                ..echo(&[])
            }),
        );
        refuse(
            "a URL instead of a name",
            runtime.request_http(HttpRequest {
                endpoint: "https://api.example".to_owned(),
                ..jev("/")
            }),
        );
        refuse(
            "a zero timeout",
            runtime.request_http(HttpRequest {
                timeout_ms: 0,
                ..jev("/")
            }),
        );
        refuse(
            "a timeout over two minutes",
            runtime.request_command(CommandRequest {
                timeout_ms: 120_001,
                ..echo(&[])
            }),
        );
        refuse(
            "an absolute URL path",
            runtime.request_http(jev("//evil/x")),
        );
        refuse(
            "a parent segment",
            runtime.request_http(jev("/v1/../admin")),
        );
        refuse(
            "a payload over 64 KiB",
            runtime.request_http(HttpRequest {
                body: vec![b'x'; 64 * 1024],
                ..jev("/")
            }),
        );
        for index in 0..8 {
            runtime.request_command(echo(&[&index.to_string()]))?;
        }
        refuse("a ninth request", runtime.request_http(jev("/")));
        Ok(())
    });
    assert_eq!(result.disposition, DecisionDispositionV6::Completed);
    assert_eq!(result.commands.len(), 8);
    assert_eq!(seen.len(), 10);
    assert!(
        seen[0].contains("\"http:other\" is not granted"),
        "{}",
        seen[0]
    );
    assert!(
        seen[9].contains("at most 8 external requests"),
        "{}",
        seen[9]
    );

    // A host that grants nothing refuses every request.
    let context = observation_context(Some("22.8"), Some("73"));
    let (_, result) = run_script(&context, |context, _| {
        assert!(context.runtime().request_http(jev("/")).is_err());
        assert!(context.runtime().request_command(echo(&[])).is_err());
        Ok(())
    });
    assert!(result.commands.is_empty());
}

/// Records the event a kernel sees; fails on it when asked to.
#[derive(Clone)]
struct ResponseKernel {
    seen: Rc<RefCell<Vec<String>>>,
    fail: bool,
}

impl NativeKernel for ResponseKernel {
    fn name(&self) -> &str {
        "response"
    }

    fn on_event(
        &mut self,
        event: StrategyEventView<'_>,
        _context: &mut dyn StrategyKernelContext,
    ) -> KernelResult<()> {
        if let StrategyEventView::ExternalResponse(response) = &event {
            self.seen
                .borrow_mut()
                .push(format!("{} {:?}", response.request_id, response.outcome));
        }
        self.seen.borrow_mut().push(event.event_type().to_owned());
        if self.fail {
            return Err(strategy_core_kernel::KernelError::new("cannot parse"));
        }
        Ok(())
    }
}

impl TransactionKernel for ResponseKernel {
    fn encode_checkpoint_state(&self) -> Result<Vec<u8>, KernelTransactionError> {
        Ok(b"response".to_vec())
    }
}

struct ResponseFactory(ResponseKernel);

impl TransactionKernelFactory for ResponseFactory {
    type Kernel = ResponseKernel;

    fn checkpoint_codec(
        &self,
        _strategy_id: &str,
    ) -> Result<KernelCheckpointCodec, KernelTransactionError> {
        Ok(KernelCheckpointCodec {
            profile: "response.checkpoint.v1".to_owned(),
            version: 1,
        })
    }
    fn create(&self, _context: &DecisionContextV6) -> Result<Self::Kernel, KernelTransactionError> {
        Ok(self.0.clone())
    }
    fn restore(
        &self,
        _context: &DecisionContextV6,
        _checkpoint: &strategy_core_v3::decision_v6::KernelCheckpointV6,
    ) -> Result<Self::Kernel, KernelTransactionError> {
        Ok(self.0.clone())
    }
}

/// The answer is the decision's event; a completed result acknowledges the request, so the
/// host forgets it once that is durable. A rejected one acknowledges nothing.
#[test]
fn an_external_response_is_the_event_and_the_completed_result_acknowledges_it() {
    let mut context = base_context();
    let request_id = cid(0, 3);
    for (outcome, shown) in [
        (
            ExternalOutcomeV6::Ok {
                status: 200,
                body: b"{\"p\":[0.1,0.9]}".to_vec(),
            },
            "Ok { status: 200, body: [123, 34, 112, 34, 58, 91, 48, 46, 49, 44, 48, 46, 57, 93, 125] }",
        ),
        (
            ExternalOutcomeV6::Err {
                kind: ExternalErrorKindV6::Abandoned,
                message: "traderd restarted while the request was in flight".to_owned(),
            },
            "Err { kind: Abandoned, message: \"traderd restarted while the request was in flight\" }",
        ),
    ] {
        context.trigger = TriggerV6::ExternalResponse {
            request_id: request_id.clone(),
            outcome,
        };
        for fail in [false, true] {
            let seen = Rc::new(RefCell::new(Vec::new()));
            let factory = ResponseFactory(ResponseKernel {
                seen: Rc::clone(&seen),
                fail,
            });
            let result = run_transaction(&factory, &context).unwrap();
            assert_eq!(
                *seen.borrow(),
                [
                    format!("{request_id} {shown}"),
                    "external_response".to_owned()
                ]
            );
            if fail {
                assert_eq!(result.disposition, DecisionDispositionV6::Rejected);
                assert!(result.acknowledged_command_ids.is_empty());
            } else {
                assert_eq!(result.disposition, DecisionDispositionV6::Completed);
                assert_eq!(result.acknowledged_command_ids, [request_id.clone()]);
            }
        }
    }
}
