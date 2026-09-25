//! One run per event: order updates derived from the runner section, the provisional view
//! inside a decision, and the local bounds on a decision's commands.

use super::*;
use strategy_core_kernel::{
    BrokerCommandKind, BrokerOrderStatus, CancelOrderRequest, CancelTarget, OrderUpdate,
    OrderUpdateStatus, fees,
};
use strategy_core_v3::decision_v6::{
    BrokerCommandKindV6, BrokerOrderStatusV6, BrokerOrderV6, CancelTargetV6, CommandOutcomeV6,
    CommandReceiptV6, ContractSideV6, DecisionResultV6, KernelCheckpointV5Layout,
    MAX_DECISION_PLAN_ROWS, MAX_STRATEGY_COMMANDS, OrderActionV6, OrderTypeV6,
    convert_v5_kernel_checkpoint, validate_decision_result_v6,
};

type Step = fn(&mut dyn StrategyKernelContext, &mut Vec<String>) -> KernelResult<()>;

/// Runs `step` for the decision's own event and records every order update it is shown.
#[derive(Clone)]
struct ScriptKernel {
    step: Step,
    seen: Rc<RefCell<Vec<String>>>,
    updates: Rc<RefCell<Vec<OrderUpdate>>>,
}

impl ScriptKernel {
    fn run(&mut self, context: &mut dyn StrategyKernelContext) -> KernelResult<()> {
        let mut seen = Vec::new();
        let result = (self.step)(context, &mut seen);
        self.seen.borrow_mut().extend(seen);
        result
    }
}

impl NativeKernel for ScriptKernel {
    fn name(&self) -> &str {
        "script"
    }

    fn on_start(&mut self, context: &mut dyn StrategyKernelContext) -> KernelResult<()> {
        self.run(context)
    }

    fn on_event(
        &mut self,
        event: StrategyEventView<'_>,
        context: &mut dyn StrategyKernelContext,
    ) -> KernelResult<()> {
        match event {
            StrategyEventView::OrderUpdate(update) => {
                self.updates.borrow_mut().push(update.clone());
                Ok(())
            }
            other => {
                self.seen.borrow_mut().push(format!("event={other:?}"));
                self.run(context)
            }
        }
    }
}

impl TransactionKernel for ScriptKernel {
    fn encode_checkpoint_state(&self) -> Result<Vec<u8>, KernelTransactionError> {
        Ok(b"script".to_vec())
    }
}

struct ScriptFactory(ScriptKernel);

impl TransactionKernelFactory for ScriptFactory {
    type Kernel = ScriptKernel;

    fn checkpoint_codec(
        &self,
        _strategy_id: &str,
    ) -> Result<KernelCheckpointCodec, KernelTransactionError> {
        Ok(KernelCheckpointCodec {
            profile: "script.checkpoint.v1".to_owned(),
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

struct Decision {
    seen: Vec<String>,
    updates: Vec<OrderUpdate>,
    result: DecisionResultV6,
}

fn decide(context: &DecisionContextV6, step: Step) -> Decision {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let updates = Rc::new(RefCell::new(Vec::new()));
    let factory = ScriptFactory(ScriptKernel {
        step,
        seen: Rc::clone(&seen),
        updates: Rc::clone(&updates),
    });
    let result = run_transaction(&factory, context).unwrap();
    validate_decision_result_v6(context, &result).unwrap();
    let seen = seen.borrow().clone();
    let updates = updates.borrow().clone();
    Decision {
        seen,
        updates,
        result,
    }
}

fn nothing(_: &mut dyn StrategyKernelContext, _: &mut Vec<String>) -> KernelResult<()> {
    Ok(())
}

fn limit_buy(client: &str, side: ContractSide, hundredths: i64, price: f64) -> PlaceOrderRequest {
    PlaceOrderRequest {
        ticker: MARKET.to_owned(),
        action: OrderAction::Buy,
        contract_side: side,
        order_type: OrderType::Limit,
        quantity: ContractQuantity::from_hundredths(hundredths),
        limit_price: Some(price),
        expires_after_ms: None,
        reduce_only: false,
        signal_type: None,
        signal_metadata: None,
        client_order_id: Some(client.to_owned()),
    }
}

/// A Sleeve whose Market carries the Broker's fee authority.
fn priced_context() -> DecisionContextV6 {
    let mut context = base_context();
    let identity = &mut context.owner_state.markets[0].identity;
    identity.fee_type = "quadratic".to_owned();
    identity.fee_multiplier_millionths = Some(1_000_000);
    context
}

/// One order of 3 contracts at 0.40 on the fixture Market.
fn order(
    command_id: &str,
    client: &str,
    status: BrokerOrderStatusV6,
    filled: u64,
    revision: u64,
) -> BrokerOrderV6 {
    let quantity = 300;
    let remaining = match status {
        BrokerOrderStatusV6::Filled | BrokerOrderStatusV6::Cancelled => 0,
        _ => quantity - filled,
    };
    let reserved = if status.is_terminal() {
        0
    } else {
        remaining * 400_000 / 100
    };
    BrokerOrderV6 {
        command_id: command_id.to_owned(),
        intent_id: format!("intent.{client}"),
        order_id: format!("order.{client}"),
        provider_order_id: None,
        provider_client_id: client.to_owned(),
        market_id: MARKET.to_owned(),
        action: OrderActionV6::Buy,
        side: ContractSideV6::Yes,
        order_type: OrderTypeV6::Limit,
        quantity_hundredths: quantity,
        filled_quantity_hundredths: filled,
        remaining_quantity_hundredths: remaining,
        limit_price_micros: Some(400_000),
        average_fill_price_micros: (filled > 0).then_some(400_000),
        reserved_principal_micros: reserved,
        reserved_fee_micros: 0,
        fees_micros: filled * 70,
        rejection_reason: None,
        created_at_unix_ms: Some(DECISION_MS),
        updated_at_unix_ms: Some(DECISION_MS),
        signal_type: None,
        signal_metadata: None,
        status,
        revision,
    }
}

fn refused(command_id: &str, kind: BrokerCommandKindV6, code: &str) -> CommandReceiptV6 {
    CommandReceiptV6 {
        command_id: command_id.to_owned(),
        kind,
        outcome: CommandOutcomeV6::Refused {
            code: code.to_owned(),
            reason: format!("refused: {code}"),
        },
    }
}

/// The context of a later Broker-state delivery: the previous decision's checkpoint and the
/// Broker state and receipts the host now reports.
fn follow_up(
    context: &DecisionContextV6,
    checkpoint: Option<&DecisionResultV6>,
    delivery: u32,
    mut orders: Vec<BrokerOrderV6>,
    mut receipts: Vec<CommandReceiptV6>,
) -> DecisionContextV6 {
    let mut next = context.clone();
    next.owner_state.delivery_id = format!("delivery.daily.{delivery}");
    next.kernel_checkpoint = checkpoint.and_then(|result| result.kernel_checkpoint.clone());
    orders.sort_by(|left, right| left.order_id.cmp(&right.order_id));
    receipts.sort_by(|left, right| left.command_id.cmp(&right.command_id));
    let reserved = orders
        .iter()
        .map(|order| order.reserved_principal_micros + order.reserved_fee_micros)
        .sum::<u64>();
    let revision = u64::from(delivery) * 10;
    next.broker.orders = orders;
    next.broker.reserved_cash_micros = reserved;
    next.broker.revision = revision;
    next.owner_state.broker.revision = revision;
    next.owner_state.fence.broker_revision = revision;
    next.owner_state.broker.locally_reserved_cash = reserved;
    next.owner_state.broker.current_commitment = reserved;
    next.command_receipts = receipts;
    next.trigger = TriggerV6::BrokerState {
        broker_revision: revision,
    };
    next.validate().unwrap();
    next
}

fn place_yes(context: &mut dyn StrategyKernelContext, seen: &mut Vec<String>) -> KernelResult<()> {
    let ticket = context
        .broker()
        .place_order(limit_buy("yes-1", ContractSide::Yes, 300, 0.4))?;
    seen.push(format!("{ticket:?}"));
    Ok(())
}

fn summary(update: &OrderUpdate) -> (OrderUpdateStatus, i64, i64, bool) {
    (
        update.status.clone(),
        update.newly_filled.hundredths(),
        update.remaining.hundredths(),
        update.is_final,
    )
}

#[test]
fn order_updates_report_each_transition_once_and_again_after_a_checkpoint_regression() {
    use BrokerOrderStatusV6::*;
    use OrderUpdateStatus as Seen;
    let context = priced_context();
    let placed = decide(&context, place_yes);
    assert_eq!(
        placed.seen,
        [format!(
            r#"OrderTicket {{ command_id: "{}", client_order_id: "yes-1" }}"#,
            cid(1, 0)
        )]
    );
    let command_id = cid(1, 0);
    let command = command_id.as_str();

    let steps = [
        (DurablyAccepted, 0, Some((Seen::Accepted, 0, 300, false))),
        (Dispatched, 0, None),
        (Resting, 0, Some((Seen::Resting, 0, 300, false))),
        (CancellationRequested, 0, None),
        (
            PartiallyFilled,
            100,
            Some((Seen::PartiallyFilled, 100, 200, false)),
        ),
        (PartiallyFilled, 100, None),
        (Filled, 300, Some((Seen::Filled, 200, 0, true))),
        (Filled, 300, None),
    ];
    let mut previous = placed.result;
    let mut checkpoints = Vec::new();
    for (index, (status, filled, expected)) in steps.into_iter().enumerate() {
        let delivery = u32::try_from(index).unwrap() + 2;
        let next = follow_up(
            &context,
            Some(&previous),
            delivery,
            vec![order(command, "yes-1", status, filled, u64::from(delivery))],
            vec![],
        );
        let decision = decide(&next, nothing);
        let seen = decision.updates.iter().map(summary).collect::<Vec<_>>();
        assert_eq!(
            seen,
            expected.clone().into_iter().collect::<Vec<_>>(),
            "step {index}"
        );
        if let Some(update) = decision.updates.first() {
            assert_eq!(update.command_kind, BrokerCommandKind::PlaceOrder);
            assert_eq!(update.command_id, command);
            assert_eq!(update.client_order_id, "yes-1");
            assert_eq!(update.order_id.as_deref(), Some("order.yes-1"));
            assert_eq!(update.ticker, MARKET);
            assert_eq!(update.action, Some(OrderAction::Buy));
            assert_eq!(update.contract_side, Some(ContractSide::Yes));
            assert_eq!(update.requested.hundredths(), 300);
            assert_eq!(update.filled.hundredths(), filled as i64);
            assert_eq!(update.fee_cost, filled as f64 * 70.0 / 1_000_000.0);
        }
        let entries = &decision
            .result
            .kernel_checkpoint
            .as_ref()
            .unwrap()
            .runner
            .entries;
        if status == Filled {
            assert!(
                entries.is_empty(),
                "pruned once the terminal status is seen"
            );
            assert_eq!(decision.result.acknowledged_command_ids, [command]);
        } else {
            assert_eq!(entries.len(), 1);
            assert!(decision.result.acknowledged_command_ids.is_empty());
        }
        assert_eq!(
            decision.result.evidence.len(),
            usize::from(expected.is_some()),
            "the derived updates are recorded as evidence"
        );
        checkpoints.push((next, decision.result.clone()));
        previous = decision.result;
    }

    // The host goes back to the checkpoint that last saw the order resting: the fill is
    // reported again, as one update with everything newly filled.
    let (filled_context, _) = &checkpoints[6];
    let (_, resting) = &checkpoints[2];
    let mut regressed = filled_context.clone();
    regressed.kernel_checkpoint = resting.kernel_checkpoint.clone();
    let decision = decide(&regressed, nothing);
    assert_eq!(
        decision.updates.iter().map(summary).collect::<Vec<_>>(),
        [(Seen::Filled, 300, 0, true)]
    );
}

#[test]
fn refusals_rejections_and_vanished_orders_arrive_as_final_updates() {
    use BrokerCommandKindV6::*;
    let context = priced_context();
    let placed = decide(&context, place_yes);
    let command_id = cid(1, 0);
    let command = command_id.as_str();
    let accepted = follow_up(
        &context,
        Some(&placed.result),
        2,
        vec![order(command, "yes-1", BrokerOrderStatusV6::Resting, 0, 1)],
        vec![],
    );
    let resting = decide(&accepted, |context, _| {
        context.broker().cancel_order(CancelOrderRequest {
            target: CancelTarget::ClientOrderId("yes-1".to_owned()),
        })?;
        context.broker().cancel_all_orders()?;
        Ok(())
    });
    let StrategyCommandV6::CancelOrder { target, .. } = &resting.result.commands[0] else {
        panic!("expected a cancel");
    };
    assert_eq!(
        *target,
        CancelTargetV6::Order {
            order_id: "order.yes-1".to_owned(),
            expected_order_revision: 1,
        },
        "a client id of an order the Broker reports names that order"
    );

    type Case = (
        &'static str,
        Vec<BrokerOrderV6>,
        Vec<CommandReceiptV6>,
        Vec<(BrokerCommandKind, OrderUpdateStatus, bool)>,
        usize,
    );
    let refusal = |code: &str| OrderUpdateStatus::Refused {
        code: code.to_owned(),
        reason: format!("refused: {code}"),
    };
    let cases: Vec<Case> = vec![
        (
            "a refused cancel and cancel-all leave the order tracked",
            vec![order(command, "yes-1", BrokerOrderStatusV6::Resting, 0, 1)],
            vec![
                refused(&cid(2, 0), CancelOrder, "stale_order_revision"),
                refused(&cid(2, 1), CancelAllOrders, "live_dispatch_unarmed"),
            ],
            vec![
                (
                    BrokerCommandKind::CancelOrder,
                    refusal("stale_order_revision"),
                    true,
                ),
                (
                    BrokerCommandKind::CancelAllOrders,
                    refusal("live_dispatch_unarmed"),
                    true,
                ),
            ],
            1,
        ),
        (
            "an admitted cancel shows as the order's own update",
            vec![order(
                command,
                "yes-1",
                BrokerOrderStatusV6::Cancelled,
                0,
                2,
            )],
            vec![
                CommandReceiptV6 {
                    command_id: cid(2, 0),
                    kind: CancelOrder,
                    outcome: CommandOutcomeV6::Accepted,
                },
                CommandReceiptV6 {
                    command_id: cid(2, 1),
                    kind: CancelAllOrders,
                    outcome: CommandOutcomeV6::Accepted,
                },
            ],
            vec![(
                BrokerCommandKind::PlaceOrder,
                OrderUpdateStatus::Cancelled,
                true,
            )],
            0,
        ),
        (
            "a provider rejection is a refusal",
            vec![order(command, "yes-1", BrokerOrderStatusV6::Rejected, 0, 2)],
            vec![],
            vec![(BrokerCommandKind::PlaceOrder, refusal_rejected(), true)],
            // The cancel of a final order is settled; the cancel-all waits for its receipt.
            1,
        ),
        (
            "a vanished order is reported once with its last status",
            vec![],
            vec![],
            vec![(
                BrokerCommandKind::PlaceOrder,
                OrderUpdateStatus::Resting,
                true,
            )],
            // The place, the cancel and the cancel-all stay as tombstones.
            3,
        ),
    ];
    for (name, orders, receipts, expected, expected_tracked) in cases {
        let next = follow_up(&context, Some(&resting.result), 3, orders, receipts.clone());
        let decision = decide(&next, nothing);
        let seen = decision
            .updates
            .iter()
            .map(|update| (update.command_kind, update.status.clone(), update.is_final))
            .collect::<Vec<_>>();
        assert_eq!(seen, expected, "{name}");
        for update in &decision.updates {
            if update.command_kind == BrokerCommandKind::CancelOrder {
                assert_eq!(
                    update.client_order_id, "yes-1",
                    "{name}: the cancel's target"
                );
                assert_eq!(update.order_id.as_deref(), Some("order.yes-1"), "{name}");
            }
            if update.command_kind == BrokerCommandKind::CancelAllOrders {
                assert!(
                    update.client_order_id.is_empty() && update.action.is_none(),
                    "{name}"
                );
            }
            if update.status == OrderUpdateStatus::Resting {
                assert_eq!(update.remaining.hundredths(), 0, "{name}: nothing remains");
            }
        }
        let acknowledged = &decision.result.acknowledged_command_ids;
        for receipt in &receipts {
            assert!(acknowledged.contains(&receipt.command_id), "{name}");
        }
        let tracked = decision
            .result
            .kernel_checkpoint
            .as_ref()
            .unwrap()
            .runner
            .entries
            .len();
        assert_eq!(tracked, expected_tracked, "{name}");
        for update in &decision.updates {
            assert_eq!(update.vanished, name.contains("vanished"), "{name}");
        }
    }

    // A refused place has no order record, only its receipt.
    let refused_place = follow_up(
        &context,
        Some(&placed.result),
        2,
        vec![],
        vec![refused(command, PlaceOrder, "price_moved")],
    );
    let decision = decide(&refused_place, nothing);
    assert_eq!(
        decision.updates.iter().map(summary).collect::<Vec<_>>(),
        [(refusal("price_moved"), 0, 0, true)]
    );
    assert_eq!(decision.result.acknowledged_command_ids, [command]);
}

fn refusal_rejected() -> OrderUpdateStatus {
    OrderUpdateStatus::Refused {
        code: strategy_core_v3::kernel_v6::PROVIDER_REJECTED_CODE.to_owned(),
        reason: "the provider rejected the order".to_owned(),
    }
}

#[test]
fn a_kernel_error_on_one_update_is_recorded_and_the_decision_goes_on() {
    /// Places an order and then fails on a refused cancel-all; handles everything else.
    #[derive(Clone)]
    struct FailsOnCancelAllRefusal {
        seen: Rc<RefCell<Vec<String>>>,
    }
    impl NativeKernel for FailsOnCancelAllRefusal {
        fn name(&self) -> &str {
            "fails-on-cancel-all-refusal"
        }
        fn on_start(&mut self, context: &mut dyn StrategyKernelContext) -> KernelResult<()> {
            context
                .broker()
                .place_order(limit_buy("yes-1", ContractSide::Yes, 300, 0.4))?;
            context.broker().cancel_all_orders()?;
            Ok(())
        }
        fn on_event(
            &mut self,
            event: StrategyEventView<'_>,
            context: &mut dyn StrategyKernelContext,
        ) -> KernelResult<()> {
            if let StrategyEventView::OrderUpdate(update) = &event {
                if update.command_kind == BrokerCommandKind::CancelAllOrders {
                    context.broker().place_order(limit_buy(
                        "retry",
                        ContractSide::Yes,
                        100,
                        0.4,
                    ))?;
                    return Err(strategy_core_kernel::KernelError::new("cannot handle it"));
                }
            }
            self.seen.borrow_mut().push(event.event_type().to_owned());
            Ok(())
        }
    }
    impl TransactionKernel for FailsOnCancelAllRefusal {
        fn encode_checkpoint_state(&self) -> Result<Vec<u8>, KernelTransactionError> {
            Ok(format!("seen={}", self.seen.borrow().len()).into_bytes())
        }
    }
    struct Factory(FailsOnCancelAllRefusal);
    impl TransactionKernelFactory for Factory {
        type Kernel = FailsOnCancelAllRefusal;
        fn checkpoint_codec(
            &self,
            _strategy_id: &str,
        ) -> Result<KernelCheckpointCodec, KernelTransactionError> {
            Ok(KernelCheckpointCodec {
                profile: "script.checkpoint.v1".to_owned(),
                version: 1,
            })
        }
        fn create(&self, _: &DecisionContextV6) -> Result<Self::Kernel, KernelTransactionError> {
            Ok(self.0.clone())
        }
        fn restore(
            &self,
            _: &DecisionContextV6,
            _: &strategy_core_v3::decision_v6::KernelCheckpointV6,
        ) -> Result<Self::Kernel, KernelTransactionError> {
            Ok(self.0.clone())
        }
    }
    let seen = Rc::new(RefCell::new(Vec::new()));
    let factory = Factory(FailsOnCancelAllRefusal {
        seen: Rc::clone(&seen),
    });
    let context = priced_context();
    let issued = run_transaction(&factory, &context).unwrap();
    assert_eq!(issued.commands.len(), 2);
    let next = follow_up(
        &context,
        Some(&issued),
        2,
        vec![order(
            &cid(1, 0),
            "yes-1",
            BrokerOrderStatusV6::Resting,
            0,
            1,
        )],
        vec![refused(
            &cid(1, 1),
            BrokerCommandKindV6::CancelAllOrders,
            "live_dispatch_unarmed",
        )],
    );
    let result = run_transaction(&factory, &next).unwrap();
    validate_decision_result_v6(&next, &result).unwrap();
    assert_eq!(result.disposition, DecisionDispositionV6::Completed);
    assert_eq!(
        *seen.borrow(),
        ["order_update", "broker_state"],
        "the other update and the trigger are still delivered"
    );
    assert!(
        result.commands.is_empty(),
        "the failed handler's order is discarded"
    );
    let errors = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "kernel_error")
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].message.contains(&cid(1, 1)),
        "{}",
        errors[0].message
    );
    // The failed update is delivered again, up to three attempts; then it counts as seen.
    let mut previous = result;
    for attempt in 2..=4 {
        let mut again = next.clone();
        again.owner_state.delivery_id = format!("delivery.daily.{}", 10 + attempt);
        again.kernel_checkpoint = previous.kernel_checkpoint.clone();
        seen.borrow_mut().clear();
        let later = run_transaction(&factory, &again).unwrap();
        validate_decision_result_v6(&again, &later).unwrap();
        let codes = later
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>();
        match attempt {
            2 => assert_eq!(codes, ["kernel_error"]),
            3 => assert_eq!(codes, ["order_update_abandoned", "kernel_error"]),
            _ => assert!(codes.is_empty(), "{codes:?}"),
        }
        assert_eq!(
            later.acknowledged_command_ids.contains(&cid(1, 1)),
            attempt >= 3,
            "the refusal's receipt is acknowledged only once the update is given up"
        );
        assert_eq!(*seen.borrow(), ["broker_state"]);
        previous = later;
    }
}

#[test]
fn the_provisional_view_reserves_yes_then_no_with_the_broker_commitment() {
    let context = priced_context();
    let decision = decide(&context, |context, seen| {
        let before = context.broker().financial_state();
        let yes = context
            .broker()
            .place_order(limit_buy("yes-1", ContractSide::Yes, 300, 0.4))?;
        let after_yes = context.broker().financial_state();
        // Spend what YES left on NO at 0.55, as dsm_reaction_v12 does.
        let remaining = context.broker().buying_power().unwrap();
        let contracts = (remaining / 0.6).floor().min(5.0) as i64;
        context
            .broker()
            .place_order(limit_buy("no-1", ContractSide::No, contracts * 100, 0.55))?;
        let after_no = context.broker().financial_state();
        seen.push(format!(
            "{} {} {}",
            before.buying_power_micros(),
            after_yes.buying_power_micros(),
            after_no.buying_power_micros()
        ));
        seen.push(format!(
            "{} {}",
            after_yes.current_commitment_micros - before.current_commitment_micros,
            after_no.current_commitment_micros - after_yes.current_commitment_micros
        ));
        let status = context.broker().order_status(&yes.client_order_id).unwrap();
        assert_eq!(status.status, BrokerOrderStatus::Submitted);
        assert_eq!(
            status.order_id, "",
            "no order id before the Broker admits it"
        );
        let pending = context.broker().pending_orders();
        assert_eq!(pending.len(), 2);
        assert!(pending.iter().all(|order| order.status == "submitted"));
        seen.push(format!("{}", pending[0].reserved_cost));
        assert_eq!(
            context
                .broker()
                .position_quantity(MARKET, ContractSide::Yes),
            ContractQuantity::ZERO,
            "positions are unchanged until the Broker says so"
        );
        Ok(())
    });
    let terms = fees::FeeTerms::new(fees::FeeType::Quadratic, 1_000_000);
    let yes = fees::buy_commitment_micros(400_000, ContractQuantity::from_hundredths(300), terms)
        .unwrap();
    let no = fees::buy_commitment_micros(550_000, ContractQuantity::from_hundredths(500), terms)
        .unwrap();
    assert_eq!(decision.seen[1], format!("{yes} {no}"));
    let start = 100_000_000;
    assert_eq!(
        decision.seen[0],
        format!("{start} {} {}", start - yes, start - yes - no)
    );
    assert_eq!(decision.seen[2], format!("{}", yes as f64 / 1_000_000.0));
    assert_eq!(decision.result.commands.len(), 2);
}

#[test]
fn a_same_decision_cancel_names_the_client_order_and_marks_it() {
    let context = priced_context();
    let decision = decide(&context, |context, seen| {
        let before = context.broker().buying_power().unwrap();
        let yes = context
            .broker()
            .place_order(limit_buy("yes-1", ContractSide::Yes, 300, 0.4))?;
        let cancel = context.broker().cancel_order(CancelOrderRequest {
            target: CancelTarget::ClientOrderId(yes.client_order_id.clone()),
        })?;
        seen.push(cancel.command_id);
        let status = context.broker().order_status("yes-1").unwrap();
        assert_eq!(status.status, BrokerOrderStatus::CancellationRequested);
        assert_eq!(
            context.broker().pending_orders()[0].status,
            "cancellation_requested"
        );
        assert!(
            context.broker().buying_power().unwrap() < before,
            "the reservation stays until the Broker releases it"
        );
        for target in [
            CancelTarget::ClientOrderId("unknown".to_owned()),
            CancelTarget::OrderId("order.unknown".to_owned()),
        ] {
            assert!(
                context
                    .broker()
                    .cancel_order(CancelOrderRequest { target })
                    .is_err(),
                "a cancel of an order nobody reports is a local error"
            );
        }
        Ok(())
    });
    assert_eq!(decision.seen, [cid(1, 1)]);
    assert_eq!(
        decision.result.commands[1],
        StrategyCommandV6::CancelOrder {
            command_id: cid(1, 1),
            target: CancelTargetV6::SameDecision {
                provider_client_id: "yes-1".to_owned(),
            },
        }
    );
    let entries = &decision
        .result
        .kernel_checkpoint
        .as_ref()
        .unwrap()
        .runner
        .entries;
    assert_eq!(
        entries.len(),
        2,
        "the place and its cancel are both tracked"
    );
    assert_eq!(entries[1].client_order_id.as_deref(), Some("yes-1"));

    // The Broker admits and cancels the pair in one write; the bot sees Cancelled.
    let next = follow_up(
        &context,
        Some(&decision.result),
        2,
        vec![order(
            &cid(1, 0),
            "yes-1",
            BrokerOrderStatusV6::Cancelled,
            0,
            1,
        )],
        vec![CommandReceiptV6 {
            command_id: cid(1, 1),
            kind: BrokerCommandKindV6::CancelOrder,
            outcome: CommandOutcomeV6::Accepted,
        }],
    );
    let seen = decide(&next, nothing);
    assert_eq!(
        seen.updates.iter().map(summary).collect::<Vec<_>>(),
        [(OrderUpdateStatus::Cancelled, 0, 0, true)]
    );
    assert!(
        seen.result
            .kernel_checkpoint
            .unwrap()
            .runner
            .entries
            .is_empty()
    );
}

#[test]
fn a_client_order_id_is_derived_when_the_kernel_sets_none() {
    let context = priced_context();
    let decision = decide(&context, |context, seen| {
        let mut request = limit_buy("unused", ContractSide::Yes, 100, 0.4);
        request.client_order_id = None;
        let ticket = context.broker().place_order(request)?;
        seen.push(ticket.client_order_id);
        Ok(())
    });
    let expected = strategy_core_v3::decision_v6::derive_provider_client_id_v6(
        DeploymentModeV6::Paper,
        &context.owner_state.sleeve.sleeve_id,
        context.owner_state.sleeve.incarnation,
        &context.owner_state.delivery_id,
        0,
    )
    .unwrap();
    assert!(expected.starts_with("tv3paper_"));
    assert_eq!(decision.seen, [expected]);
}

/// A context whose Sleeve already has `count` resting orders.
fn crowded_context(count: usize) -> DecisionContextV6 {
    let context = priced_context();
    let orders = (0..count)
        .map(|index| {
            order(
                &format!("command.old.{index:03}"),
                &format!("old-{index:03}"),
                BrokerOrderStatusV6::Resting,
                0,
                1,
            )
        })
        .collect();
    let mut crowded = follow_up(&context, None, 1, orders, vec![]);
    crowded.trigger = TriggerV6::Owner(OwnerTriggerV6::Recovery);
    crowded
}

#[test]
fn broker_commands_over_the_plan_row_limit_are_a_local_error() {
    // 128 resting orders; 63 places cost 4 + 315 rows; a cancel-all over 191 orders is 194.
    let context = crowded_context(128);
    let decision = decide(&context, |context, seen| {
        for index in 0..63 {
            context.broker().place_order(limit_buy(
                &format!("new-{index}"),
                ContractSide::Yes,
                100,
                0.01,
            ))?;
        }
        let error = context.broker().cancel_all_orders().unwrap_err();
        seen.push(error.message().to_owned());
        Ok(())
    });
    assert_eq!(
        decision.seen,
        [format!(
            "the decision's Broker commands need 513 plan rows, over the limit of {MAX_DECISION_PLAN_ROWS}"
        )]
    );
    assert_eq!(
        decision.result.commands.len(),
        63,
        "nothing leaves for the refused call"
    );
}

#[test]
fn a_decision_carries_at_most_64_commands() {
    let context = priced_context();
    let decision = decide(&context, |context, seen| {
        for index in 0..MAX_STRATEGY_COMMANDS {
            context.broker().place_order(limit_buy(
                &format!("new-{index}"),
                ContractSide::Yes,
                100,
                0.01,
            ))?;
        }
        seen.push(
            context
                .broker()
                .place_order(limit_buy("one-too-many", ContractSide::Yes, 100, 0.01))
                .unwrap_err()
                .to_string(),
        );
        seen.push(
            context
                .runtime()
                .wake_at(strategy_core_kernel::WakeAtRequest {
                    when: ns(EMITTED_NS),
                    name: None,
                })
                .unwrap_err()
                .to_string(),
        );
        Ok(())
    });
    assert_eq!(decision.seen, ["a decision carries at most 64 commands"; 2]);
    assert_eq!(decision.result.commands.len(), MAX_STRATEGY_COMMANDS);
    assert_eq!(decision.result.commands[63].command_id(), cid(1, 63));
}

#[test]
fn a_sleeve_holds_at_most_192_open_orders() {
    let context = crowded_context(129);
    let decision = decide(&context, |context, seen| {
        for index in 0..63 {
            context.broker().place_order(limit_buy(
                &format!("new-{index}"),
                ContractSide::Yes,
                100,
                0.01,
            ))?;
        }
        seen.push(
            context
                .broker()
                .place_order(limit_buy("one-more", ContractSide::Yes, 100, 0.01))
                .unwrap_err()
                .to_string(),
        );
        Ok(())
    });
    assert_eq!(decision.seen, ["a Sleeve holds at most 192 open orders"]);
    let tracked = decision
        .result
        .kernel_checkpoint
        .unwrap()
        .runner
        .entries
        .len();
    assert_eq!(tracked, 192, "the 129 open orders and the 63 places");
}

/// The crowded Sleeve's seeded checkpoint with `live` cancels still waiting for their
/// receipts and `tombstones` places that vanished.
fn crowded_with(
    open: usize,
    live: usize,
    tombstones: usize,
) -> (
    DecisionContextV6,
    strategy_core_v3::decision_v6::KernelCheckpointV6,
) {
    let crowded = crowded_context(open);
    let seeded = decide(&crowded, nothing).result;
    let mut checkpoint = seeded.kernel_checkpoint.clone().unwrap();
    let waiting = strategy_core_v3::decision_v6::RunnerEntryV6 {
        command_id: String::new(),
        kind: BrokerCommandKindV6::CancelOrder,
        client_order_id: None,
        order_id: None,
        market_id: None,
        action: None,
        side: None,
        requested_quantity_hundredths: 0,
        last_status: None,
        filled_quantity_hundredths: 0,
        order_revision: 0,
        // Issued above the view's revision: not yet visible.
        issued_broker_revision: 1_000,
        vanished: false,
        vanished_revision: 0,
        absent_views: 0,
        delivery_failures: 0,
    };
    checkpoint.runner.entries.extend((0..live).map(|index| {
        strategy_core_v3::decision_v6::RunnerEntryV6 {
            command_id: format!("command.wait.{index:03}"),
            ..waiting.clone()
        }
    }));
    let template = checkpoint.runner.entries[0].clone();
    checkpoint
        .runner
        .entries
        .extend(
            (0..tombstones).map(|index| strategy_core_v3::decision_v6::RunnerEntryV6 {
                command_id: format!("command.gone.{index:03}"),
                client_order_id: Some(format!("gone-{index:03}")),
                order_id: None,
                vanished: true,
                vanished_revision: 5 + index as u64,
                ..template.clone()
            }),
        );
    let checkpoint = checkpoint.seal();
    let mut context = crowded;
    context.kernel_checkpoint = Some(checkpoint.clone());
    context.validate().unwrap();
    (context, checkpoint)
}

#[test]
fn the_runner_tracks_at_most_256_orders_and_commands() {
    // 191 open orders and 65 cancels waiting for their receipts: 256 live entries.
    let (context, _) = crowded_with(191, 65, 0);
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
    assert_eq!(
        decision.seen,
        ["the runner tracks at most 256 orders and commands"]
    );
}

#[test]
fn tombstones_never_keep_the_strategy_from_cancelling() {
    // 191 open orders, 63 cancels waiting and the full 32 tombstones: a cancel and a
    // cancel-all still fit in the 256 live entries.
    let (context, _) = crowded_with(191, 63, 32);
    let decision = decide(&context, |context, seen| {
        let cancel = context.broker().cancel_order(CancelOrderRequest {
            target: CancelTarget::ClientOrderId("old-000".to_owned()),
        });
        seen.push(format!("{:?}", cancel.map(drop).map_err(|e| e.to_string())));
        let cancel_all = context.broker().cancel_all_orders();
        seen.push(format!(
            "{:?}",
            cancel_all.map(drop).map_err(|e| e.to_string())
        ));
        Ok(())
    });
    assert_eq!(decision.seen, ["Ok(())", "Ok(())"]);
    let entries = &decision.result.kernel_checkpoint.unwrap().runner.entries;
    assert_eq!(entries.iter().filter(|entry| entry.vanished).count(), 32);

    // A 33rd tombstone evicts the oldest, with a diagnostic.
    let crowded = crowded_context(191);
    let (_, mut checkpoint) = crowded_with(191, 0, 32);
    let placed = strategy_core_v3::decision_v6::RunnerEntryV6 {
        command_id: "command.will-vanish".to_owned(),
        client_order_id: Some("will-vanish".to_owned()),
        issued_broker_revision: 0,
        vanished: false,
        vanished_revision: 0,
        ..checkpoint.runner.entries[0].clone()
    };
    checkpoint.runner.entries.push(placed);
    let mut context = crowded;
    context.kernel_checkpoint = Some(checkpoint.seal());
    let decision = decide(&context, nothing);
    assert_eq!(decision.updates.len(), 1, "the vanish is reported");
    assert!(decision.updates[0].vanished);
    let evicted = decision
        .result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "runner_tombstone_evicted")
        .expect("the oldest tombstone is evicted");
    assert!(
        evicted.message.contains("command.gone.000"),
        "{}",
        evicted.message
    );
    let entries = &decision.result.kernel_checkpoint.unwrap().runner.entries;
    assert_eq!(entries.iter().filter(|entry| entry.vanished).count(), 32);
}

#[test]
fn a_tombstone_expires_after_16_complete_newer_views_without_it() {
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
    let mut expired_at = None;
    for delivery in 3..30 {
        let decision = decide(
            &follow_up(&context, Some(&previous), delivery, vec![], vec![]),
            nothing,
        );
        if decision
            .result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "runner_tombstone_expired")
        {
            expired_at.get_or_insert(delivery);
        }
        previous = decision.result;
    }
    // Vanished at delivery 3, then 16 newer complete views without it.
    assert_eq!(expired_at, Some(3 + 16));
    assert!(
        previous
            .kernel_checkpoint
            .unwrap()
            .runner
            .entries
            .is_empty()
    );
}

#[test]
fn a_cancel_waits_for_its_receipt_through_a_newer_view_without_it() {
    use BrokerOrderStatusV6::*;
    let context = priced_context();
    let placed = decide(&context, place_yes);
    let command_id = cid(1, 0);
    let resting = vec![order(&command_id, "yes-1", Resting, 0, 1)];
    let r2 = decide(
        &follow_up(&context, Some(&placed.result), 2, resting.clone(), vec![]),
        |context, _| {
            context.broker().cancel_order(CancelOrderRequest {
                target: CancelTarget::ClientOrderId("yes-1".to_owned()),
            })?;
            Ok(())
        },
    );
    let cancel = cid(2, 0);
    // A newer view that predates the cancel's write: no receipt yet.
    let r3 = decide(
        &follow_up(&context, Some(&r2.result), 3, resting.clone(), vec![]),
        nothing,
    );
    assert!(r3.updates.is_empty());
    // The refusal arrives.
    let r4 = decide(
        &follow_up(
            &context,
            Some(&r3.result),
            4,
            resting,
            vec![refused(
                &cancel,
                BrokerCommandKindV6::CancelOrder,
                "stale_order_revision",
            )],
        ),
        nothing,
    );
    assert_eq!(r4.updates.len(), 1, "the refusal reaches the kernel");
    assert_eq!(r4.updates[0].command_kind, BrokerCommandKind::CancelOrder);
    assert!(r4.result.acknowledged_command_ids.contains(&cancel));
}

#[test]
fn seeding_waits_for_a_complete_view() {
    use BrokerOrderStatusV6::*;
    let context = priced_context();
    let hidden_open = order("command.v5-hidden-open", "v5-hidden-open", Resting, 0, 3);
    let hidden_done = order("command.v5-hidden-done", "v5-hidden-done", Filled, 300, 3);
    let visible = order("command.v5-visible", "v5-visible", Resting, 0, 3);
    let mut truncated = follow_up(&context, None, 1, vec![visible.clone()], vec![]);
    truncated.orders_complete = false;
    truncated.trigger = TriggerV6::Owner(OwnerTriggerV6::Recovery);
    let first = decide(&truncated, nothing);
    let runner = &first.result.kernel_checkpoint.as_ref().unwrap().runner;
    assert!(!runner.seeded && runner.entries.is_empty());
    assert!(
        first
            .result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "runner_not_seeded")
    );
    assert!(first.result.acknowledged_command_ids.is_empty());
    // The first complete view seeds: the hidden open order is recorded as seen and the hidden
    // terminal one acknowledged.
    let second = decide(
        &follow_up(
            &context,
            Some(&first.result),
            2,
            vec![visible.clone(), hidden_done, hidden_open.clone()],
            vec![],
        ),
        nothing,
    );
    let runner = &second.result.kernel_checkpoint.as_ref().unwrap().runner;
    assert!(runner.seeded);
    assert_eq!(runner.entries.len(), 2);
    assert_eq!(
        second.result.acknowledged_command_ids,
        ["command.v5-hidden-done"]
    );
    // Its fill is then reported.
    let mut filled = hidden_open;
    filled.status = Filled;
    filled.filled_quantity_hundredths = 300;
    filled.remaining_quantity_hundredths = 0;
    filled.reserved_principal_micros = 0;
    filled.average_fill_price_micros = Some(400_000);
    filled.revision = 9;
    let third = decide(
        &follow_up(
            &context,
            Some(&second.result),
            3,
            vec![visible, filled],
            vec![],
        ),
        nothing,
    );
    assert_eq!(
        third.updates.iter().map(summary).collect::<Vec<_>>(),
        [(OrderUpdateStatus::Filled, 300, 0, true)]
    );
}

#[test]
fn a_converted_v5_checkpoint_records_open_orders_as_seen_without_updates() {
    let context = priced_context();
    // A V5 checkpoint as the host stored it, converted at cutover.
    let mut v5 = KernelCheckpointV5Layout {
        codec_profile: "script.checkpoint.v1".to_owned(),
        codec_version: 1,
        strategy_id: "fixture".to_owned(),
        strategy_profile: "daily-high".to_owned(),
        profile_and_calculator_digest: DIGEST.to_owned(),
        sequence: 7,
        state: b"v5-state".to_vec(),
        state_sha256: [0; 32],
    };
    v5.state_sha256 = v5_digest(&v5);
    let converted = convert_v5_kernel_checkpoint(v5).unwrap();

    let mut cutover = follow_up(
        &context,
        None,
        2,
        vec![
            order(
                "command.v5-resting",
                "v5-resting",
                BrokerOrderStatusV6::Resting,
                0,
                4,
            ),
            order(
                "command.v5-filled",
                "v5-filled",
                BrokerOrderStatusV6::Filled,
                300,
                9,
            ),
        ],
        vec![refused(
            "command.v5-refused",
            BrokerCommandKindV6::PlaceOrder,
            "price_moved",
        )],
    );
    cutover.owner_state.trigger = strategy_core_v3::decision_v4::TriggerV4::Recovery;
    cutover.trigger = TriggerV6::Owner(OwnerTriggerV6::Recovery);
    cutover.kernel_checkpoint = Some(converted);
    let first = decide(&cutover, nothing);
    assert!(
        first.updates.is_empty(),
        "no burst of updates for old orders"
    );
    assert_eq!(first.seen, Vec::<String>::new(), "Recovery runs on_start");
    let checkpoint = first.result.kernel_checkpoint.as_ref().unwrap();
    assert_eq!(checkpoint.sequence, 8);
    assert_eq!(checkpoint.state, b"script");
    assert_eq!(
        checkpoint
            .runner
            .entries
            .iter()
            .map(|entry| entry.command_id.as_str())
            .collect::<Vec<_>>(),
        ["command.v5-resting"]
    );
    assert_eq!(
        first.result.acknowledged_command_ids,
        ["command.v5-refused", "command.v5-filled"]
    );

    // From then on the old order's changes are news.
    let filled = follow_up(
        &context,
        Some(&first.result),
        3,
        vec![order(
            "command.v5-resting",
            "v5-resting",
            BrokerOrderStatusV6::Filled,
            300,
            5,
        )],
        vec![],
    );
    let decision = decide(&filled, nothing);
    assert_eq!(
        decision.updates.iter().map(summary).collect::<Vec<_>>(),
        [(OrderUpdateStatus::Filled, 300, 0, true)]
    );
}

fn v5_digest(checkpoint: &KernelCheckpointV5Layout) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let component = |hasher: &mut Sha256, value: &[u8]| {
        hasher.update((value.len() as u64).to_be_bytes());
        hasher.update(value);
    };
    let mut hasher = Sha256::new();
    hasher.update(b"strategy-core/decision-v5/checkpoint/v1\0");
    component(&mut hasher, checkpoint.codec_profile.as_bytes());
    hasher.update(checkpoint.codec_version.to_be_bytes());
    component(&mut hasher, checkpoint.strategy_id.as_bytes());
    component(&mut hasher, checkpoint.strategy_profile.as_bytes());
    component(
        &mut hasher,
        checkpoint.profile_and_calculator_digest.as_bytes(),
    );
    hasher.update(checkpoint.sequence.to_be_bytes());
    component(&mut hasher, &checkpoint.state);
    hasher.finalize().into()
}

#[test]
fn events_come_from_the_contributor_station_that_triggered_them() {
    const SECOND: &str = "KBFI";
    let mut context = observation_context(Some("22.8"), Some("73"));
    let primary = context.owner_state.stations[0].clone();
    let mut second = primary.clone();
    second.identity.station_id = SECOND.to_owned();
    second.identity.logical_location = SECOND.to_owned();
    second.observation.station_id = SECOND.to_owned();
    second.observation.temperature_milli_c = Some(19_000);
    second.observation_meta.revision = 3;
    second.weather_events.clear();
    context.owner_state.stations.insert(0, second);
    context.owner_state.opportunity.contributor_stations =
        vec![SECOND.to_owned(), STATION.to_owned()];
    context.owner_state.trigger = strategy_core_v3::decision_v4::TriggerV4::Weather {
        station_id: SECOND.to_owned(),
        source_generation: 3,
        source_sequence: 61,
    };
    context.trigger = TriggerV6::Owner(OwnerTriggerV6::Observation {
        station_id: SECOND.to_owned(),
        observed_at_unix_ms: OBSERVED_NS / 1_000_000,
        component_revision: 3,
        source_generation: 3,
        source_sequence: 61,
    });
    let mut observation = observation(Some("19"), Some("66.2"));
    observation.station_id = SECOND.to_owned();
    observation.is_from_report = false;
    observation.report_type = None;
    observation.source_report_id = None;
    let mut station = supplied_station(observation.clone());
    station.station_id = SECOND.to_owned();
    station.reports.clear();
    station.weather_events.clear();
    station.oracle_tables.clear();
    context.supplied.stations.insert(0, station);
    context.supplied.originating_event = Some(SuppliedEventV6::Observation(observation));
    context.validate().unwrap();

    let decision = decide(&context, |context, seen| {
        seen.push(format!("{:?}", context.contributor_stations()));
        Ok(())
    });
    assert!(
        decision.seen[0].starts_with("event=Observation("),
        "{}",
        decision.seen[0]
    );
    assert!(
        decision.seen[0].contains(r#"station_id: "KBFI""#),
        "the event is the triggering station's: {}",
        decision.seen[0]
    );
    assert!(decision.seen[0].contains("temperature_f: Some(66.2)"));
    assert_eq!(decision.seen[1], r#"["KBFI", "KSEA"]"#);
}

/// A context of the fixture Sleeve at Broker revision `revision`: the previous decision's
/// checkpoint and the orders the host reports, complete or truncated.
fn view(
    context: &DecisionContextV6,
    previous: &DecisionResultV6,
    delivery: u32,
    revision: u64,
    orders: Vec<BrokerOrderV6>,
    complete: bool,
) -> DecisionContextV6 {
    let mut next = follow_up(context, Some(previous), delivery, orders, vec![]);
    next.broker.revision = revision;
    next.owner_state.broker.revision = revision;
    next.owner_state.fence.broker_revision = revision;
    next.trigger = TriggerV6::BrokerState {
        broker_revision: revision,
    };
    next.orders_complete = complete;
    next.validate().unwrap();
    next
}

#[test]
fn an_order_missing_from_a_view_is_no_news_until_a_complete_newer_view() {
    use BrokerOrderStatusV6::*;
    let issuing = view(
        &priced_context(),
        &decide(&priced_context(), nothing).result,
        1,
        100,
        vec![],
        true,
    );
    let placed = decide(&issuing, place_yes);
    let command_id = cid(1, 0);
    let resting = vec![order(&command_id, "yes-1", Resting, 0, 1)];
    let first = decide(
        &view(&issuing, &placed.result, 2, 101, resting.clone(), true),
        nothing,
    );
    assert_eq!(first.updates.len(), 1);
    for (name, revision, complete) in [
        ("a view that may predate the command's admission", 100, true),
        ("an older view", 90, true),
        ("a truncated view", 150, false),
    ] {
        let missing = decide(
            &view(&issuing, &first.result, 3, revision, vec![], complete),
            nothing,
        );
        assert!(missing.updates.is_empty(), "{name}");
        assert_eq!(
            missing
                .result
                .kernel_checkpoint
                .as_ref()
                .unwrap()
                .runner
                .entries,
            first
                .result
                .kernel_checkpoint
                .as_ref()
                .unwrap()
                .runner
                .entries,
            "{name}: the entry is kept as it was"
        );
    }
    // An older record of the order than the one last reported is stale.
    let partial = vec![order(&command_id, "yes-1", PartiallyFilled, 100, 5)];
    let filled_some = decide(
        &view(&issuing, &first.result, 3, 102, partial, true),
        nothing,
    );
    let older = decide(
        &view(&issuing, &filled_some.result, 4, 103, resting, true),
        nothing,
    );
    assert!(older.updates.is_empty(), "an older order record is ignored");
}

#[test]
fn a_vanished_order_that_reappears_reports_what_it_missed() {
    use BrokerOrderStatusV6::*;
    let context = priced_context();
    let placed = decide(&context, place_yes);
    let command_id = cid(1, 0);
    let command = command_id.as_str();
    // D2: resting.
    let d2 = follow_up(
        &context,
        Some(&placed.result),
        2,
        vec![order(command, "yes-1", Resting, 0, 1)],
        vec![],
    );
    let r2 = decide(&d2, nothing);
    // D3: a complete, newer view no longer shows the order: reported final once.
    let r3 = decide(
        &follow_up(&context, Some(&r2.result), 3, vec![], vec![]),
        nothing,
    );
    assert_eq!(
        r3.updates.iter().map(summary).collect::<Vec<_>>(),
        [(OrderUpdateStatus::Resting, 0, 0, true)]
    );
    let r3b = decide(
        &follow_up(&context, Some(&r3.result), 4, vec![], vec![]),
        nothing,
    );
    assert!(r3b.updates.is_empty(), "reported once");
    // D5: it is back, filled: the fill is reported against the tombstone, never swallowed.
    let d5 = follow_up(
        &context,
        Some(&r3b.result),
        5,
        vec![order(command, "yes-1", Filled, 300, 4)],
        vec![],
    );
    let r5 = decide(&d5, nothing);
    assert_eq!(
        r5.updates.iter().map(summary).collect::<Vec<_>>(),
        [(OrderUpdateStatus::Filled, 300, 0, true)]
    );
    assert_eq!(r5.result.acknowledged_command_ids, [command]);
}

#[test]
fn a_refused_place_is_reported_even_when_its_client_id_matches_an_old_order() {
    use BrokerOrderStatusV6::*;
    let context = priced_context();
    let placed = decide(&context, place_yes);
    let old = cid(1, 0);
    let d2 = follow_up(
        &context,
        Some(&placed.result),
        2,
        vec![order(&old, "yes-1", Filled, 300, 2)],
        vec![],
    );
    let r2 = decide(&d2, nothing);
    // The acknowledged order left the view; the kernel reuses its client id.
    let r3 = decide(
        &follow_up(&context, Some(&r2.result), 3, vec![], vec![]),
        place_yes,
    );
    let new = cid(3, 0);
    assert_eq!(r3.result.commands[0].command_id(), new);
    // The Broker refused the duplicate; the old order is back in the view.
    let d4 = follow_up(
        &context,
        Some(&r3.result),
        4,
        vec![order(&old, "yes-1", Filled, 300, 2)],
        vec![refused(
            &new,
            BrokerCommandKindV6::PlaceOrder,
            "duplicate_client_order_id",
        )],
    );
    let r4 = decide(&d4, nothing);
    assert_eq!(r4.updates.len(), 1);
    assert_eq!(r4.updates[0].command_id, new);
    assert!(matches!(
        r4.updates[0].status,
        OrderUpdateStatus::Refused { .. }
    ));
}

#[test]
fn kernel_client_ids_may_not_use_the_derived_prefix() {
    let decision = decide(&priced_context(), |context, seen| {
        seen.push(
            context
                .broker()
                .place_order(limit_buy("tv3paper_mine", ContractSide::Yes, 100, 0.4))
                .unwrap_err()
                .to_string(),
        );
        Ok(())
    });
    assert!(decision.seen[0].contains("reserved"), "{:?}", decision.seen);
    assert!(decision.result.commands.is_empty());
}

#[test]
fn a_truncated_view_allows_no_cancel_all() {
    let mut context = priced_context();
    context.orders_complete = false;
    let decision = decide(&context, |context, seen| {
        seen.push(
            context
                .broker()
                .cancel_all_orders()
                .unwrap_err()
                .to_string(),
        );
        Ok(())
    });
    assert!(
        decision.seen[0].contains("truncated"),
        "{:?}",
        decision.seen
    );
}

/// A small deterministic generator for the property test.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, bound: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % bound
    }
}

#[test]
fn every_fill_is_reported_exactly_once_whatever_the_views() {
    use BrokerOrderStatusV6::*;
    for seed in 0..150 {
        let mut rng = Lcg(seed);
        let base = priced_context();
        let seeded = decide(&base, nothing).result;
        let issuing = view(&base, &seeded, 1, 100, vec![], true);
        let placed = decide(&issuing, place_yes);
        let command_id = cid(1, 0);

        // The order's true history: filled grows to 300, each state a newer order revision.
        let mut history = vec![order(&command_id, "yes-1", DurablyAccepted, 0, 1)];
        let mut filled = 0;
        while filled < 300 {
            filled = (filled + 25 * (1 + rng.next(4))).min(300);
            let status = if filled == 300 {
                Filled
            } else if rng.next(2) == 0 {
                PartiallyFilled
            } else {
                CancellationRequested
            };
            let revision = history.len() as u64 + 1;
            history.push(order(&command_id, "yes-1", status, filled, revision));
        }

        let mut previous = placed.result;
        let mut truth = 0;
        let mut revision = 100;
        let mut reported_fill = 0;
        let mut filled_updates = 0;
        let mut delivery = 2;
        let mut step = |orders: Vec<BrokerOrderV6>,
                        revision: u64,
                        complete: bool,
                        previous: &mut DecisionResultV6| {
            let context = view(&issuing, previous, delivery, revision, orders, complete);
            delivery += 1;
            let decision = decide(&context, nothing);
            *previous = decision.result;
            decision.updates
        };
        for _ in 0..(history.len() * 3) {
            truth = (truth + rng.next(2) as usize).min(history.len() - 1);
            revision += 1 + rng.next(3);
            let (orders, at, complete) = match rng.next(10) {
                // A view that may predate the admission, or is older still.
                0 | 1 => (vec![], 100 - rng.next(20), true),
                // A truncated view without the order.
                2 | 3 => (vec![], revision, false),
                // A complete view without it (the order vanished for now).
                4 => (vec![], revision, true),
                // A delayed or reordered view of an earlier state.
                _ => {
                    let earliest = truth.saturating_sub(2);
                    let shown = earliest + rng.next((truth - earliest + 1) as u64) as usize;
                    (vec![history[shown].clone()], revision, true)
                }
            };
            for update in step(orders, at, complete, &mut previous) {
                reported_fill += update.newly_filled.hundredths();
                filled_updates += usize::from(update.status == OrderUpdateStatus::Filled);
            }
        }
        // Eventually the host shows the final state.
        revision += 1;
        for update in step(
            vec![history.last().unwrap().clone()],
            revision,
            true,
            &mut previous,
        ) {
            reported_fill += update.newly_filled.hundredths();
            filled_updates += usize::from(update.status == OrderUpdateStatus::Filled);
        }
        assert_eq!(reported_fill, 300, "seed {seed}: every fill exactly once");
        assert_eq!(filled_updates, 1, "seed {seed}: one Filled update");
    }
}

#[test]
fn large_logs_fill_only_the_room_the_result_has_left() {
    let context = priced_context();
    let decision = decide(&context, |context, _| {
        for index in 0..20 {
            context.emit(KernelAction::Log(LogAction {
                level: "info".to_owned(),
                message: format!("{index}:{}", "x".repeat(60 * 1024)),
            }))?;
        }
        Ok(())
    });
    let result = decision.result;
    assert_eq!(result.disposition, DecisionDispositionV6::Completed);
    let encoded = strategy_core_v3::decision_v6::encode_decision_result_v6(&result).unwrap();
    assert!(encoded.len() <= strategy_core_v3::decision_v6::RESULT_ENCODED_BUDGET_BYTES);
    let overflow = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "kernel_telemetry_overflow")
        .expect("the logs past the room are counted");
    let lost =
        serde_json::from_str::<serde_json::Value>(&overflow.message).unwrap()["lost_entries"]
            .as_u64()
            .unwrap();
    let kept = result
        .evidence
        .iter()
        .filter(|evidence| evidence.code == "kernel_log")
        .count() as u64;
    assert_eq!(kept + lost, 20);
    assert!(lost > 0 && kept > 0);
}

#[test]
fn commands_past_the_result_size_bound_are_a_local_error() {
    let decision = decide(&priced_context(), |context, seen| {
        for index in 0..MAX_STRATEGY_COMMANDS {
            let mut request = limit_buy(&format!("big-{index}"), ContractSide::Yes, 100, 0.4);
            request.signal_metadata = Some("m".repeat(60 * 1024));
            if let Err(error) = context.broker().place_order(request) {
                seen.push(error.to_string());
                break;
            }
        }
        Ok(())
    });
    assert_eq!(
        decision.seen,
        ["the decision's commands would exceed the result size bound"]
    );
    assert!(!decision.result.commands.is_empty());
    let encoded =
        strategy_core_v3::decision_v6::encode_decision_result_v6(&decision.result).unwrap();
    assert!(encoded.len() <= strategy_core_v3::decision_v6::MAX_DECISION_RESULT_V6_BYTES);
}

#[test]
fn a_provider_rejection_carries_the_provider_text() {
    let context = priced_context();
    let placed = decide(&context, place_yes);
    let command_id = cid(1, 0);
    for (reason, expected) in [
        (Some("market paused"), "market paused"),
        (None, strategy_core_v3::kernel_v6::PROVIDER_REJECTED_REASON),
    ] {
        let mut rejected = order(&command_id, "yes-1", BrokerOrderStatusV6::Rejected, 0, 2);
        rejected.rejection_reason = reason.map(str::to_owned);
        let next = follow_up(&context, Some(&placed.result), 2, vec![rejected], vec![]);
        let decision = decide(&next, nothing);
        assert_eq!(
            decision.updates.iter().map(summary).collect::<Vec<_>>(),
            [(
                OrderUpdateStatus::Refused {
                    code: strategy_core_v3::kernel_v6::PROVIDER_REJECTED_CODE.to_owned(),
                    reason: expected.to_owned(),
                },
                0,
                300,
                true
            )]
        );
    }
}

#[test]
fn a_live_market_sell_is_a_local_error() {
    let mut live = priced_context();
    live.deployment_mode = DeploymentModeV6::Live;
    for (context, admitted) in [(priced_context(), true), (live, false)] {
        let decision = decide(&context, |context, seen| {
            let mut sell = limit_buy("sell-1", ContractSide::Yes, 100, 0.4);
            sell.action = OrderAction::Sell;
            sell.order_type = OrderType::Market;
            sell.limit_price = None;
            sell.reduce_only = true;
            seen.push(match context.broker().place_order(sell) {
                Ok(_) => "admitted".to_owned(),
                Err(error) => error.to_string(),
            });
            Ok(())
        });
        if admitted {
            assert_eq!(decision.seen, ["admitted"]);
        } else {
            assert!(
                decision.seen[0].contains("not admitted in live"),
                "{:?}",
                decision.seen
            );
            assert!(decision.result.commands.is_empty());
        }
    }
}

/// Keeps a book of its orders' fills; a vanished order stays in the book in case it returns.
#[derive(Clone)]
struct BookKernel {
    book: std::collections::BTreeMap<String, i64>,
    position: i64,
    place: bool,
}

impl NativeKernel for BookKernel {
    fn name(&self) -> &str {
        "book"
    }
    fn on_start(&mut self, context: &mut dyn StrategyKernelContext) -> KernelResult<()> {
        if self.place {
            let ticket =
                context
                    .broker()
                    .place_order(limit_buy("yes-1", ContractSide::Yes, 300, 0.4))?;
            self.book.insert(ticket.client_order_id, 0);
        }
        Ok(())
    }
    fn on_event(
        &mut self,
        event: StrategyEventView<'_>,
        _context: &mut dyn StrategyKernelContext,
    ) -> KernelResult<()> {
        if let StrategyEventView::OrderUpdate(update) = event {
            let Some(filled) = self.book.get_mut(&update.client_order_id) else {
                return Err(strategy_core_kernel::KernelError::new("unknown order"));
            };
            *filled += update.newly_filled.hundredths();
            self.position += update.newly_filled.hundredths();
            if update.is_final && !update.vanished {
                self.book.remove(&update.client_order_id);
            }
        }
        Ok(())
    }
}

impl TransactionKernel for BookKernel {
    fn encode_checkpoint_state(&self) -> Result<Vec<u8>, KernelTransactionError> {
        let mut text = self.position.to_string();
        for (client, filled) in &self.book {
            text.push_str(&format!(";{client}={filled}"));
        }
        Ok(text.into_bytes())
    }
}

struct BookFactory {
    place: bool,
}

impl TransactionKernelFactory for BookFactory {
    type Kernel = BookKernel;
    fn checkpoint_codec(&self, _: &str) -> Result<KernelCheckpointCodec, KernelTransactionError> {
        Ok(KernelCheckpointCodec {
            profile: "script.checkpoint.v1".to_owned(),
            version: 1,
        })
    }
    fn create(&self, _: &DecisionContextV6) -> Result<BookKernel, KernelTransactionError> {
        Ok(BookKernel {
            book: Default::default(),
            position: 0,
            place: self.place,
        })
    }
    fn restore(
        &self,
        _: &DecisionContextV6,
        checkpoint: &strategy_core_v3::decision_v6::KernelCheckpointV6,
    ) -> Result<BookKernel, KernelTransactionError> {
        let text = String::from_utf8(checkpoint.state.clone()).unwrap();
        let mut parts = text.split(';');
        let position = parts.next().unwrap().parse().unwrap_or(0);
        let book = parts
            .map(|part| {
                let (client, filled) = part.split_once('=').unwrap();
                (client.to_owned(), filled.parse().unwrap())
            })
            .collect();
        Ok(BookKernel {
            book,
            position,
            place: self.place,
        })
    }
}

#[test]
fn a_kernel_that_keeps_a_vanished_order_counts_its_fill_when_it_returns() {
    use BrokerOrderStatusV6::*;
    let context = priced_context();
    let placed = run_transaction(&BookFactory { place: true }, &context).unwrap();
    let command_id = cid(1, 0);
    let run = |context: &DecisionContextV6| {
        let result = run_transaction(&BookFactory { place: false }, context).unwrap();
        validate_decision_result_v6(context, &result).unwrap();
        result
    };
    let r2 = run(&follow_up(
        &context,
        Some(&placed),
        2,
        vec![order(&command_id, "yes-1", Resting, 0, 1)],
        vec![],
    ));
    // A stale view at a newer account-wide revision without the order: a false vanish.
    let r3 = run(&follow_up(&context, Some(&r2), 3, vec![], vec![]));
    // It is back, filled.
    let r4 = run(&follow_up(
        &context,
        Some(&r3),
        4,
        vec![order(&command_id, "yes-1", Filled, 300, 4)],
        vec![],
    ));
    let state = String::from_utf8(r4.kernel_checkpoint.as_ref().unwrap().state.clone()).unwrap();
    assert_eq!(
        state, "300",
        "the fill is counted once and the order leaves the book"
    );
    assert!(r4.diagnostics.is_empty(), "{:?}", r4.diagnostics);
}

#[test]
fn a_failed_update_is_undone_through_the_kernels_own_codec() {
    /// Counts updates in its checkpoint and fails on each after changing its count, placing
    /// an order, logging and counting.
    struct Counting {
        updates: u32,
    }
    impl NativeKernel for Counting {
        fn name(&self) -> &str {
            "counting"
        }
        fn on_event(
            &mut self,
            event: StrategyEventView<'_>,
            context: &mut dyn StrategyKernelContext,
        ) -> KernelResult<()> {
            if let StrategyEventView::OrderUpdate(_) = event {
                self.updates += 1;
                context.broker().place_order(limit_buy(
                    "replacement",
                    ContractSide::Yes,
                    100,
                    0.4,
                ))?;
                context.telemetry().counter("orders_placed", 1.0, &[])?;
                context.emit(KernelAction::Log(LogAction {
                    level: "warn".to_owned(),
                    message: "about to fail".to_owned(),
                }))?;
                return Err(strategy_core_kernel::KernelError::new("boom"));
            }
            Ok(())
        }
    }
    impl TransactionKernel for Counting {
        fn encode_checkpoint_state(&self) -> Result<Vec<u8>, KernelTransactionError> {
            Ok(self.updates.to_string().into_bytes())
        }
    }
    struct CountingFactory;
    impl TransactionKernelFactory for CountingFactory {
        type Kernel = Counting;
        fn checkpoint_codec(
            &self,
            _: &str,
        ) -> Result<KernelCheckpointCodec, KernelTransactionError> {
            Ok(KernelCheckpointCodec {
                profile: "script.checkpoint.v1".to_owned(),
                version: 1,
            })
        }
        fn create(&self, _: &DecisionContextV6) -> Result<Counting, KernelTransactionError> {
            Ok(Counting { updates: 0 })
        }
        fn restore(
            &self,
            _: &DecisionContextV6,
            checkpoint: &strategy_core_v3::decision_v6::KernelCheckpointV6,
        ) -> Result<Counting, KernelTransactionError> {
            let text = String::from_utf8(checkpoint.state.clone()).unwrap();
            Ok(Counting {
                updates: text.parse().unwrap_or(0),
            })
        }
    }
    let context = priced_context();
    let placed = decide(&context, place_yes);
    let next = follow_up(
        &context,
        Some(&placed.result),
        2,
        vec![order(
            &cid(1, 0),
            "yes-1",
            BrokerOrderStatusV6::Resting,
            0,
            1,
        )],
        vec![],
    );
    let result = run_transaction(&CountingFactory, &next).unwrap();
    assert_eq!(
        result.kernel_checkpoint.as_ref().unwrap().state,
        b"0",
        "state undone"
    );
    assert!(result.commands.is_empty(), "the order was never sent");
    assert!(result.telemetry.is_empty(), "nor counted");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "kernel_log"
                && diagnostic.message == "about to fail"),
        "its logs stay"
    );
}
