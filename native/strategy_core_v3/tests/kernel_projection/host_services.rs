//! Host services a kernel reads or requests beyond Broker calls: its parameters, the host's
//! capabilities, gauge and annotation telemetry, and cancellable timers.

use super::*;
use strategy_core_kernel::{KernelCapabilities, ParameterValue};
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
