//! Delivery of order updates at the edges: acknowledgements over truncated views, revived and
//! adopted orders past the issue bound, capacity refusals inside update handlers, abandoned
//! updates, and kernels whose state cannot be saved or restored mid-decision.

use std::cell::Cell;

use super::*;
use strategy_core_v3::decision_v6::KernelCheckpointV6;

fn codes(result: &DecisionResultV6) -> Vec<(&str, &str)> {
    result
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.severity.as_str(), diagnostic.code.as_str()))
        .collect()
}

fn entry<'a>(
    result: &'a DecisionResultV6,
    command_id: &str,
) -> Option<&'a strategy_core_v3::decision_v6::RunnerEntryV6> {
    result
        .kernel_checkpoint
        .as_ref()
        .unwrap()
        .runner
        .entries
        .iter()
        .find(|entry| entry.command_id == command_id)
}

fn state(result: &DecisionResultV6) -> String {
    String::from_utf8(result.kernel_checkpoint.as_ref().unwrap().state.clone()).unwrap()
}

/// A view at `revision`, below the one `follow_up` gives its delivery.
fn at_revision(mut context: DecisionContextV6, revision: u64) -> DecisionContextV6 {
    context.broker.revision = revision;
    context.owner_state.broker.revision = revision;
    context.owner_state.fence.broker_revision = revision;
    context.trigger = TriggerV6::BrokerState {
        broker_revision: revision,
    };
    context.validate().unwrap();
    context
}

#[test]
fn a_final_order_is_acknowledged_once_delivered_even_over_truncated_views() {
    use BrokerOrderStatusV6::*;
    let context = priced_context();
    let mut first = follow_up(&context, None, 1, vec![], vec![]);
    first.orders_complete = false;
    first.trigger = TriggerV6::Owner(OwnerTriggerV6::Recovery);
    let placed = decide(&first, place_yes);
    assert!(
        placed
            .result
            .kernel_checkpoint
            .as_ref()
            .unwrap()
            .runner
            .seeded
    );
    let command_id = cid(1, 0);
    let mut filled = follow_up(
        &context,
        Some(&placed.result),
        2,
        vec![order(&command_id, "yes-1", Filled, 300, 2)],
        vec![],
    );
    filled.orders_complete = false;
    let decision = decide(&filled, nothing);
    assert_eq!(
        decision.updates.iter().map(summary).collect::<Vec<_>>(),
        [(OrderUpdateStatus::Filled, 300, 0, true)]
    );
    assert_eq!(decision.result.acknowledged_command_ids, [command_id]);
}

#[test]
fn a_revived_tombstone_may_take_the_live_entries_past_256() {
    use BrokerOrderStatusV6::*;
    // 190 open orders, 66 cancels waiting (256 live entries) and one tombstone whose order is
    // back.
    let (_, checkpoint) = crowded_with(190, 66, 1);
    let mut orders = (0..190)
        .map(|index| {
            order(
                &format!("command.old.{index:03}"),
                &format!("old-{index:03}"),
                Resting,
                0,
                1,
            )
        })
        .collect::<Vec<_>>();
    orders.push(order("command.gone.000", "gone-000", Resting, 0, 2));
    let mut context = follow_up(&priced_context(), None, 2, orders, vec![]);
    context.kernel_checkpoint = Some(checkpoint);
    context.validate().unwrap();
    let decision = decide(&context, |context, seen| {
        seen.push(
            context
                .broker()
                .place_order(limit_buy("one-more", ContractSide::Yes, 100, 0.01))
                .unwrap_err()
                .to_string(),
        );
        Ok(())
    });
    assert_eq!(decision.updates.len(), 1, "the returned order is reported");
    assert_eq!(
        decision.seen.last().unwrap(),
        "the runner tracks at most 256 orders and commands"
    );
    let entries = &decision.result.kernel_checkpoint.unwrap().runner.entries;
    assert_eq!(entries.iter().filter(|entry| entry.is_live()).count(), 257);
}

/// Hedges every fill at once with `?`: a refused place fails the update.
#[derive(Clone)]
struct HedgeKernel {
    counted: i64,
}

impl NativeKernel for HedgeKernel {
    fn name(&self) -> &str {
        "hedge"
    }
    fn on_event(
        &mut self,
        event: StrategyEventView<'_>,
        context: &mut dyn StrategyKernelContext,
    ) -> KernelResult<()> {
        if let StrategyEventView::OrderUpdate(update) = event {
            self.counted += update.newly_filled.hundredths();
            if update.newly_filled.is_positive() {
                context.broker().place_order(limit_buy(
                    &format!("hedge-{}", update.filled.hundredths()),
                    ContractSide::No,
                    100,
                    0.4,
                ))?;
            }
        }
        Ok(())
    }
}

impl TransactionKernel for HedgeKernel {
    fn encode_checkpoint_state(&self) -> Result<Vec<u8>, KernelTransactionError> {
        Ok(self.counted.to_string().into_bytes())
    }
}

struct HedgeFactory;

impl TransactionKernelFactory for HedgeFactory {
    type Kernel = HedgeKernel;
    fn checkpoint_codec(&self, _: &str) -> Result<KernelCheckpointCodec, KernelTransactionError> {
        Ok(KernelCheckpointCodec {
            profile: "script.checkpoint.v1".to_owned(),
            version: 1,
        })
    }
    fn create(&self, _: &DecisionContextV6) -> Result<HedgeKernel, KernelTransactionError> {
        Ok(HedgeKernel { counted: 0 })
    }
    fn restore(
        &self,
        _: &DecisionContextV6,
        checkpoint: &KernelCheckpointV6,
    ) -> Result<HedgeKernel, KernelTransactionError> {
        Ok(HedgeKernel {
            counted: String::from_utf8(checkpoint.state.clone())
                .unwrap()
                .parse()
                .unwrap(),
        })
    }
}

#[test]
fn a_capacity_refusal_inside_an_update_is_not_counted_against_it() {
    use BrokerOrderStatusV6::*;
    // 192 open orders: the Sleeve is at its open-order cap.
    let crowded = crowded_context(192);
    let seeded = run_transaction(&HedgeFactory, &crowded).unwrap();
    let orders = |filled: u64, status: BrokerOrderStatusV6, revision: u64| {
        (0..192)
            .map(|index| {
                let (status, filled, revision) = if index == 0 {
                    (status, filled, revision)
                } else {
                    (Resting, 0, 1)
                };
                order(
                    &format!("command.old.{index:03}"),
                    &format!("old-{index:03}"),
                    status,
                    filled,
                    revision,
                )
            })
            .collect::<Vec<_>>()
    };
    // The first order fills partly; the hedge is refused at the cap in every decision.
    let mut previous = seeded;
    for delivery in 2..7 {
        let next = follow_up(
            &crowded,
            Some(&previous),
            delivery,
            orders(100, PartiallyFilled, 2),
            vec![],
        );
        let result = run_transaction(&HedgeFactory, &next).unwrap();
        validate_decision_result_v6(&next, &result).unwrap();
        assert_eq!(state(&result), "0");
        let waiting = entry(&result, "command.old.000").unwrap();
        assert_eq!(waiting.filled_quantity_hundredths, 0, "the update waits");
        assert_eq!(waiting.delivery_failures, 0, "and is not counted");
        assert_eq!(codes(&result), [("warn", "order_update_deferred")]);
        previous = result;
    }
    // It fills completely, which frees a slot: the hedge goes out and every fill is counted.
    let next = follow_up(&crowded, Some(&previous), 7, orders(300, Filled, 3), vec![]);
    let result = run_transaction(&HedgeFactory, &next).unwrap();
    validate_decision_result_v6(&next, &result).unwrap();
    assert_eq!(state(&result), "300");
    assert_eq!(result.commands.len(), 1);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
}

/// Fails on every order update.
struct Refusing;

impl NativeKernel for Refusing {
    fn name(&self) -> &str {
        "refusing"
    }
    fn on_event(
        &mut self,
        event: StrategyEventView<'_>,
        _: &mut dyn StrategyKernelContext,
    ) -> KernelResult<()> {
        match event {
            StrategyEventView::OrderUpdate(_) => {
                Err(strategy_core_kernel::KernelError::new("cannot handle it"))
            }
            _ => Ok(()),
        }
    }
}

impl TransactionKernel for Refusing {
    fn encode_checkpoint_state(&self) -> Result<Vec<u8>, KernelTransactionError> {
        Ok(b"refusing".to_vec())
    }
}

struct RefusingFactory;

impl TransactionKernelFactory for RefusingFactory {
    type Kernel = Refusing;
    fn checkpoint_codec(&self, _: &str) -> Result<KernelCheckpointCodec, KernelTransactionError> {
        Ok(KernelCheckpointCodec {
            profile: "script.checkpoint.v1".to_owned(),
            version: 1,
        })
    }
    fn create(&self, _: &DecisionContextV6) -> Result<Refusing, KernelTransactionError> {
        Ok(Refusing)
    }
    fn restore(
        &self,
        _: &DecisionContextV6,
        _: &KernelCheckpointV6,
    ) -> Result<Refusing, KernelTransactionError> {
        Ok(Refusing)
    }
}

#[test]
fn an_abandoned_update_is_an_error_naming_the_fill_it_carried() {
    use BrokerOrderStatusV6::*;
    let context = priced_context();
    let placed = decide(&context, place_yes);
    let command_id = cid(1, 0);
    let filled = |previous: &DecisionResultV6, delivery| {
        follow_up(
            &context,
            Some(previous),
            delivery,
            vec![order(&command_id, "yes-1", PartiallyFilled, 100, 2)],
            vec![],
        )
    };
    let mut previous = placed.result;
    for delivery in 2..5 {
        let next = filled(&previous, delivery);
        previous = run_transaction(&RefusingFactory, &next).unwrap();
        validate_decision_result_v6(&next, &previous).unwrap();
    }
    let abandoned = previous
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "order_update_abandoned")
        .expect("abandoned after three counted failures");
    assert_eq!(abandoned.severity, "error");
    let message: serde_json::Value = serde_json::from_str(&abandoned.message).unwrap();
    assert_eq!(message["command_ids"], serde_json::json!([command_id]));
    assert_eq!(
        message["lost_newly_filled_hundredths"],
        serde_json::json!([100])
    );
    assert_eq!(message["total_lost_newly_filled_hundredths"], 100);
    assert_eq!(
        entry(&previous, &command_id)
            .unwrap()
            .filled_quantity_hundredths,
        100,
        "the fill counts as seen"
    );
}

#[test]
fn an_expired_order_that_returns_open_is_adopted_from_a_newer_view() {
    use BrokerOrderStatusV6::*;
    let context = priced_context();
    let placed = decide(&context, place_yes);
    let command_id = cid(1, 0);
    let mut previous = decide(
        &follow_up(
            &context,
            Some(&placed.result),
            2,
            vec![order(&command_id, "yes-1", Resting, 0, 1)],
            vec![],
        ),
        nothing,
    )
    .result;
    // Vanished at delivery 3, expired at 19.
    for delivery in 3..20 {
        previous = decide(
            &follow_up(&context, Some(&previous), delivery, vec![], vec![]),
            nothing,
        )
        .result;
    }
    assert!(entry(&previous, &command_id).is_none());
    // A stale view (below the newest the runner saw) showing it is not trusted.
    let stale = decide(
        &at_revision(
            follow_up(
                &context,
                Some(&previous),
                20,
                vec![order(&command_id, "yes-1", Resting, 0, 1)],
                vec![],
            ),
            50,
        ),
        nothing,
    );
    assert!(stale.updates.is_empty());
    assert!(entry(&stale.result, &command_id).is_none());
    // A newer one adopts it from its current state; its updates follow.
    let mut all = Vec::new();
    previous = stale.result;
    for (delivery, status, filled, revision) in [
        (21, PartiallyFilled, 100, 4),
        (22, PartiallyFilled, 200, 5),
        (23, Filled, 300, 6),
    ] {
        let decision = decide(
            &follow_up(
                &context,
                Some(&previous),
                delivery,
                vec![order(&command_id, "yes-1", status, filled, revision)],
                vec![],
            ),
            nothing,
        );
        if delivery == 21 {
            assert_eq!(codes(&decision.result), [("warn", "runner_order_adopted")]);
        }
        all.extend(decision.updates.iter().map(summary));
        previous = decision.result;
    }
    assert_eq!(
        all,
        [
            (OrderUpdateStatus::PartiallyFilled, 100, 100, false),
            (OrderUpdateStatus::Filled, 100, 0, true),
        ]
    );
    assert_eq!(previous.acknowledged_command_ids, [command_id]);
}

/// Counts the updates it handles; its codec and its factory fail when the test says so.
struct Fragile {
    counted: u32,
    /// Fail on updates of this client order id.
    fail_on: Option<&'static str>,
    plan: Rc<FragilePlan>,
}

#[derive(Default)]
struct FragilePlan {
    /// Encodes of the kernel's state that fail, next first.
    failing_encodes: Cell<u32>,
    /// Restores that fail once the kernel failed on an update.
    failing_restores: Cell<u32>,
    armed: Cell<bool>,
    seen: RefCell<Vec<String>>,
}

impl NativeKernel for Fragile {
    fn name(&self) -> &str {
        "fragile"
    }
    fn on_event(
        &mut self,
        event: StrategyEventView<'_>,
        _: &mut dyn StrategyKernelContext,
    ) -> KernelResult<()> {
        self.plan
            .seen
            .borrow_mut()
            .push(event.event_type().to_owned());
        if let StrategyEventView::OrderUpdate(update) = event {
            self.counted += 1;
            if self.fail_on == Some(update.client_order_id.as_str()) {
                self.plan.armed.set(true);
                return Err(strategy_core_kernel::KernelError::new("boom"));
            }
        }
        Ok(())
    }
}

impl TransactionKernel for Fragile {
    fn encode_checkpoint_state(&self) -> Result<Vec<u8>, KernelTransactionError> {
        let failing = self.plan.failing_encodes.get();
        if failing > 0 {
            self.plan.failing_encodes.set(failing - 1);
            return Err(KernelTransactionError::Checkpoint(
                "encode failed".to_owned(),
            ));
        }
        Ok(self.counted.to_string().into_bytes())
    }
}

struct FragileFactory {
    fail_on: Option<&'static str>,
    plan: Rc<FragilePlan>,
}

impl TransactionKernelFactory for FragileFactory {
    type Kernel = Fragile;
    fn checkpoint_codec(&self, _: &str) -> Result<KernelCheckpointCodec, KernelTransactionError> {
        Ok(KernelCheckpointCodec {
            profile: "script.checkpoint.v1".to_owned(),
            version: 1,
        })
    }
    fn create(&self, _: &DecisionContextV6) -> Result<Fragile, KernelTransactionError> {
        Ok(Fragile {
            counted: 0,
            fail_on: self.fail_on,
            plan: Rc::clone(&self.plan),
        })
    }
    fn restore(
        &self,
        _: &DecisionContextV6,
        checkpoint: &KernelCheckpointV6,
    ) -> Result<Fragile, KernelTransactionError> {
        let failing = self.plan.failing_restores.get();
        if self.plan.armed.get() && failing > 0 {
            self.plan.failing_restores.set(failing - 1);
            return Err(KernelTransactionError::Checkpoint(
                "restore failed".to_owned(),
            ));
        }
        Ok(Fragile {
            // The first decision's checkpoint is a script kernel's.
            counted: String::from_utf8(checkpoint.state.clone())
                .unwrap()
                .parse()
                .unwrap_or(0),
            fail_on: self.fail_on,
            plan: Rc::clone(&self.plan),
        })
    }
}

/// Three placed orders (clients a, b, c) and a context where each has news.
fn three_updates() -> (DecisionContextV6, [String; 3]) {
    use BrokerOrderStatusV6::*;
    let context = priced_context();
    let placed = decide(&context, |context, _| {
        for client in ["a", "b", "c"] {
            context
                .broker()
                .place_order(limit_buy(client, ContractSide::Yes, 300, 0.4))?;
        }
        Ok(())
    });
    let ids = [cid(1, 0), cid(1, 1), cid(1, 2)];
    let orders = ids
        .iter()
        .zip(["a", "b", "c"])
        .map(|(command_id, client)| order(command_id, client, Resting, 0, 1))
        .collect();
    (
        follow_up(&context, Some(&placed.result), 2, orders, vec![]),
        ids,
    )
}

#[test]
fn a_snapshot_the_kernel_cannot_take_fails_that_update_only() {
    let plan = Rc::new(FragilePlan::default());
    let factory = FragileFactory {
        fail_on: None,
        plan: Rc::clone(&plan),
    };
    let (next, ids) = three_updates();
    plan.seen.borrow_mut().clear();
    // The snapshot before the first update fails.
    plan.failing_encodes.set(1);
    let result = run_transaction(&factory, &next).unwrap();
    validate_decision_result_v6(&next, &result).unwrap();
    assert_eq!(
        *plan.seen.borrow(),
        ["order_update", "order_update", "broker_state"]
    );
    assert_eq!(state(&result), "2");
    assert_eq!(entry(&result, &ids[0]).unwrap().delivery_failures, 1);
    assert_eq!(entry(&result, &ids[0]).unwrap().last_status, None);
    assert_eq!(codes(&result), [("error", "kernel_error")]);
    assert!(result.diagnostics[0].message.contains("could not be saved"));
}

#[test]
fn a_kernel_the_factory_cannot_restore_goes_back_to_the_decision_start() {
    let plan = Rc::new(FragilePlan::default());
    let factory = FragileFactory {
        fail_on: Some("b"),
        plan: Rc::clone(&plan),
    };
    let (next, ids) = three_updates();
    plan.seen.borrow_mut().clear();
    plan.failing_restores.set(1);
    let result = run_transaction(&factory, &next).unwrap();
    validate_decision_result_v6(&next, &result).unwrap();
    // a is handled, b fails and cannot be restored: the decision goes back to its start, c
    // waits, and the trigger is delivered.
    assert_eq!(
        *plan.seen.borrow(),
        ["order_update", "order_update", "broker_state"]
    );
    assert_eq!(state(&result), "0", "a's handling is undone");
    let failures = ids
        .iter()
        .map(|id| {
            let entry = entry(&result, id).unwrap();
            (entry.last_status.is_some(), entry.delivery_failures)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        failures,
        [(false, 0), (false, 1), (false, 0)],
        "every update is delivered again; only b's failure counts"
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("could not be restored"))
    );

    // When even the decision's start cannot be restored, the decision fails.
    plan.armed.set(false);
    plan.failing_restores.set(2);
    assert!(run_transaction(&factory, &next).is_err());
}
