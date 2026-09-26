//! Resident execution over host-owned canonical views. No transport context, codec,
//! checkpoint, durability, retries or rollback; errors invalidate the kernel instance.

use super::*;

/// The identity needed for deterministic tickets and timer generations, not a state envelope.
#[derive(Clone, Copy)]
pub struct NativeIdentity<'a> {
    pub sleeve_id: &'a str,
    pub incarnation: u64,
    pub delivery_id: &'a str,
}

impl NativeIdentity<'_> {
    pub fn command_id(self, ordinal: usize) -> String {
        u32::try_from(ordinal)
            .ok()
            .and_then(|ordinal| {
                wire::command_id_v6(self.sleeve_id, self.incarnation, self.delivery_id, ordinal)
            })
            .expect("a native host supplies a digest Sleeve id and a bounded ordinal")
    }

    pub fn timer_generation(self) -> String {
        format!("timer.{}", self.delivery_id)
    }
}

/// A borrowed invocation, supplied directly by an in-process host. Canonical state must be
/// scoped and fixed at the invocation's read fence. The host owns its storage and validation.
/// `None` is startup; otherwise the event follows any changed order updates.
pub struct NativeInvocation<'a> {
    pub identity: NativeIdentity<'a>,
    pub now: DateTime<Utc>,
    pub event: Option<&'a StrategyEvent>,
    pub state: &'a dyn StrategyKernelState,
    pub parameters: &'a StrategyParameters,
    pub capabilities: &'a KernelCapabilities,
    pub contributor_stations: &'a [String],
    pub pending_timers: &'a [PendingTimer],
    pub deployment_mode: DeploymentModeV6,
    pub finances: BrokerFinancialState,
    pub broker: &'a BrokerDetailV6,
    pub orders_complete: bool,
    pub receipts: &'a [wire::CommandReceiptV6],
}

/// Effects staged by one successful invocation. The host owns admission and kernel lifetime.
#[derive(Debug)]
pub struct NativeDecision {
    pub commands: Vec<StrategyCommandV6>,
    pub acknowledged_command_ids: Vec<String>,
    pub diagnostics: Vec<ResultDiagnosticV6>,
    pub telemetry: Vec<TelemetryEntryV6>,
}

/// Runs a resident bot against host-owned views and shared ticket/update semantics. No state
/// projection or checkpoint is performed here. On error the host MUST discard the instance;
/// mutations are not rolled back and none of this invocation's effects are returned.
pub fn run_native_decision<K: NativeKernel + ?Sized>(
    kernel: &mut K,
    seen: &mut RunnerSectionV6,
    context: &NativeInvocation<'_>,
    market_buy_cap: impl Fn(&PlaceOrderRequest) -> Result<Option<u64>, KernelTransactionError>,
) -> Result<NativeDecision, KernelTransactionError> {
    let derived = updates::derive_from_section(
        context.broker,
        context.orders_complete,
        context.receipts,
        std::mem::take(seen),
    )?;
    // The request an external-response event answers is acknowledged like a transactional
    // `TriggerV6::ExternalResponse`; the host forgets it once it applies the decision.
    let external_response = match context.event {
        Some(StrategyEvent::ExternalResponse(response)) => Some(response.request_id.as_str()),
        _ => None,
    };
    let acknowledgements =
        updates::any_acknowledgeable(context.broker, context.receipts, external_response);
    let mut host = KernelHost::native(context, derived.entries(), acknowledgements);
    host.market_buy_cap = Some(&market_buy_cap);
    let issued_from = host.runner.len();
    host.derived_entries = issued_from;
    for index in derived.delivery_order().into_iter().flatten() {
        if let Some(update) = &derived.steps[index].update {
            StrategyEvent::OrderUpdate(order_update(update))
                .with_view(|view| kernel.on_event(view, &mut host))
                .map_err(|error| KernelTransactionError::Kernel(error.to_string()))?;
        }
    }
    match context.event {
        Some(event) => event.with_view(|view| kernel.on_event(view, &mut host)),
        None => kernel.on_start(&mut host),
    }
    .map_err(|error| KernelTransactionError::Kernel(error.to_string()))?;

    let newest_view_revision = derived.newest_view_revision;
    let (entries, notes) = derived.finalize(&BTreeMap::new(), host.runner.split_off(issued_from));
    let acknowledged_command_ids = updates::acknowledgements_from_view(
        context.broker,
        context.receipts,
        external_response,
        &entries,
    );
    *seen = RunnerSectionV6 {
        seeded: true,
        newest_view_revision,
        entries,
    };
    let mut diagnostics = Vec::new();
    append_notes(&notes, &mut diagnostics);
    let mut telemetry = Vec::new();
    for output in host.outputs {
        match output {
            HostOutput::Log(log) => {
                let valid_level = matches!(log.level.as_str(), "error" | "warn" | "info" | "debug");
                let message = if valid_level && !log.message.is_empty() {
                    log.message
                } else {
                    serde_json::json!({"level": log.level, "message": log.message}).to_string()
                };
                diagnostics.push(ResultDiagnosticV6 {
                    severity: if valid_level {
                        log.level
                    } else {
                        "info".to_owned()
                    },
                    code: "kernel_log".to_owned(),
                    message,
                });
            }
            HostOutput::Telemetry(entry) => telemetry.push(entry),
            HostOutput::UpdateError(..) | HostOutput::UpdateWarning(..) => unreachable!(),
        }
    }
    Ok(NativeDecision {
        commands: host.commands,
        acknowledged_command_ids,
        diagnostics,
        telemetry,
    })
}
