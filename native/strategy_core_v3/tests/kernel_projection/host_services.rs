//! Host services a kernel reads or requests beyond Broker calls: its parameters, the host's
//! capabilities, gauge and annotation telemetry, and cancellable timers.

use super::*;
use strategy_core_kernel::{
    AnnotationValue, KernelCapabilities, ParameterValue, TimerHandle, WakeAtRequest,
};
use strategy_core_v3::decision_v4::TimerRecoveryV4;
use strategy_core_v3::decision_v5::{DecisionResultV5, StrategyParameterValueV5};

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
    fn create(&self, _context: &DecisionContextV5) -> Result<Self::Kernel, KernelTransactionError> {
        Ok(self.0.clone())
    }
    fn restore(
        &self,
        _context: &DecisionContextV5,
        _checkpoint: &strategy_core_v3::decision_v5::KernelCheckpointV5,
    ) -> Result<Self::Kernel, KernelTransactionError> {
        Ok(self.0.clone())
    }
}

fn run_script(context: &DecisionContextV5, script: Script) -> (Vec<String>, DecisionResultV5) {
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
        ("enabled".to_owned(), StrategyParameterValueV5::Bool(true)),
        (
            "max_price".to_owned(),
            StrategyParameterValueV5::Decimal {
                coefficient: 4_250,
                scale: 4,
            },
        ),
        (
            "mode".to_owned(),
            StrategyParameterValueV5::String("fast".to_owned()),
        ),
        ("offset".to_owned(), StrategyParameterValueV5::I64(-3)),
        ("unset".to_owned(), StrategyParameterValueV5::Null),
        ("window".to_owned(), StrategyParameterValueV5::U64(90)),
    ];
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
        assert_eq!(capabilities.mode, None, "V5 contexts do not carry the mode");
        assert!(capabilities.timers);
        seen.push(format!("capabilities={capabilities:?}"));
        Ok(())
    });
    assert_eq!(result.disposition, DecisionDispositionV5::Completed);
    assert_eq!(
        seen[0],
        r#"keys=["enabled", "max_price", "mode", "offset", "unset", "window"]"#
    );
    let json = strategy_core_v3::kernel_v5::strategy_parameters_json(&context).unwrap();
    assert_eq!(
        json["max_price"],
        serde_json::json!(0.425),
        "same f64 as the initializer"
    );
    assert_ne!(KernelCapabilities::default().timers, true);
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
    assert_eq!(result.disposition, DecisionDispositionV5::Completed);
    assert!(result.commands.is_empty());
    let rows = result
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            (
                "kernel_telemetry",
                r#"{"fields":[],"name":"orders_considered","value":1.0,"value_bits":"3ff0000000000000"}"#
            ),
            (
                "kernel_gauge",
                r#"{"fields":[["sleeve","a"]],"name":"buying_power","value":12.5,"value_bits":"4029000000000000"}"#
            ),
            (
                "kernel_annotation",
                r#"{"fields":[],"name":"gate","value":"price_moved"}"#
            ),
            (
                "kernel_annotation",
                r#"{"fields":[],"name":"attempt","value":-2}"#
            ),
            (
                "kernel_annotation",
                r#"{"fields":[["unit","dollars"]],"name":"edge","value":{"bits":"3fb999999999999a","float":0.1}}"#
            ),
            (
                "kernel_annotation",
                r#"{"fields":[],"name":"armed","value":false}"#
            ),
            (
                "kernel_annotation",
                r#"{"fields":[],"name":"reason","value":null}"#
            ),
            ("kernel_log", "after"),
        ]
    );
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
    assert_eq!(result.disposition, DecisionDispositionV5::Rejected);
    let codes = result
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect::<Vec<_>>();
    assert_eq!(codes, ["kernel_error", "kernel_gauge", "kernel_annotation"]);
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

fn timer_commands(result: &DecisionResultV5) -> Vec<(String, String, String, &'static str)> {
    result
        .commands
        .iter()
        .filter_map(|command| match command {
            StrategyCommandV5::ScheduleTimer {
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
            StrategyCommandV5::CancelTimer {
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
        let handle = context.runtime().schedule_timer(wake("exit.check"))?;
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
    assert_eq!(cancelled.disposition, DecisionDispositionV5::Completed);
    assert_eq!(
        timer_commands(&cancelled),
        [(
            "command.delivery.daily.2.100".to_owned(),
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
        let handle = runtime.schedule_timer(wake("a"))?;
        seen.push("scheduled".to_owned());
        assert!(
            runtime.cancel_timer(&handle).is_err(),
            "schedule then cancel"
        );
        assert!(runtime.schedule_timer(wake("a")).is_err(), "two schedules");
        runtime.wake_at(wake("b"))?;
        assert!(
            runtime.schedule_timer(wake("b")).is_err(),
            "wake_at then schedule"
        );
        let other = TimerHandle {
            key: "c".to_owned(),
            generation: "timer.delivery.daily.0".to_owned(),
        };
        runtime.cancel_timer(&other)?;
        assert!(runtime.cancel_timer(&other).is_err(), "two cancels");
        assert!(
            runtime.schedule_timer(wake("c")).is_err(),
            "cancel then schedule"
        );
        assert!(
            runtime
                .cancel_timer(&TimerHandle {
                    key: "bad key".to_owned(),
                    generation: "timer.x".to_owned(),
                })
                .is_err()
        );
        assert!(runtime.schedule_timer(wake("")).is_err());
        let unnamed = runtime.schedule_timer(WakeAtRequest {
            when: ns(EMITTED_NS),
            name: None,
        })?;
        assert_eq!(unnamed.key, "kernel.wake");
        Ok(())
    });
    assert_eq!(result.disposition, DecisionDispositionV5::Completed);
    assert_eq!(
        timer_commands(&result),
        [
            (
                "command.delivery.daily.1.100".to_owned(),
                "a".to_owned(),
                "timer.delivery.daily.1".to_owned(),
                "schedule"
            ),
            (
                "command.delivery.daily.1.101".to_owned(),
                "b".to_owned(),
                "timer.delivery.daily.1".to_owned(),
                "schedule"
            ),
            (
                "command.delivery.daily.1.102".to_owned(),
                "c".to_owned(),
                "timer.delivery.daily.0".to_owned(),
                "cancel"
            ),
            (
                "command.delivery.daily.1.103".to_owned(),
                "kernel.wake".to_owned(),
                "timer.delivery.daily.1".to_owned(),
                "schedule"
            ),
        ]
    );
}
