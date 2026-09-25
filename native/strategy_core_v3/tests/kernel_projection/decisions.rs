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
    MAX_BROKER_ORDERS, MAX_DECISION_PLAN_ROWS, MAX_STRATEGY_COMMANDS, OrderActionV6, OrderTypeV6,
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
        [r#"OrderTicket { command_id: "command.delivery.daily.1.0", client_order_id: "yes-1" }"#]
    );
    let command = "command.delivery.daily.1.0";

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
    let command = "command.delivery.daily.1.0";
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
                refused(
                    "command.delivery.daily.2.0",
                    CancelOrder,
                    "stale_order_revision",
                ),
                refused(
                    "command.delivery.daily.2.1",
                    CancelAllOrders,
                    "live_dispatch_unarmed",
                ),
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
                    command_id: "command.delivery.daily.2.0".to_owned(),
                    kind: CancelOrder,
                    outcome: CommandOutcomeV6::Accepted,
                },
                CommandReceiptV6 {
                    command_id: "command.delivery.daily.2.1".to_owned(),
                    kind: CancelAllOrders,
                    outcome: CommandOutcomeV6::Accepted,
                },
            ],
            vec![(
                BrokerCommandKind::PlaceOrder,
                OrderUpdateStatus::Cancelled,
                true,
            )],
        ),
        (
            "a provider rejection is a refusal",
            vec![order(command, "yes-1", BrokerOrderStatusV6::Rejected, 0, 2)],
            vec![],
            vec![(BrokerCommandKind::PlaceOrder, refusal_rejected(), true)],
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
        ),
    ];
    for (name, orders, receipts, expected) in cases {
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
        let order_open = next
            .broker
            .orders
            .iter()
            .any(|order| !order.status.is_terminal());
        assert_eq!(tracked, usize::from(order_open), "{name}");
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
fn a_kernel_error_in_an_update_rejects_the_decision_and_the_update_comes_again() {
    #[derive(Clone)]
    struct Failing;
    impl NativeKernel for Failing {
        fn name(&self) -> &str {
            "failing"
        }
        fn on_event(
            &mut self,
            event: StrategyEventView<'_>,
            _context: &mut dyn StrategyKernelContext,
        ) -> KernelResult<()> {
            match event {
                StrategyEventView::OrderUpdate(_) => {
                    Err(strategy_core_kernel::KernelError::new("cannot handle it"))
                }
                _ => Ok(()),
            }
        }
    }
    impl TransactionKernel for Failing {
        fn encode_checkpoint_state(&self) -> Result<Vec<u8>, KernelTransactionError> {
            Ok(b"failing".to_vec())
        }
    }
    struct FailingFactory;
    impl TransactionKernelFactory for FailingFactory {
        type Kernel = Failing;
        fn checkpoint_codec(
            &self,
            _strategy_id: &str,
        ) -> Result<KernelCheckpointCodec, KernelTransactionError> {
            Ok(KernelCheckpointCodec {
                profile: "script.checkpoint.v1".to_owned(),
                version: 1,
            })
        }
        fn create(&self, _: &DecisionContextV6) -> Result<Failing, KernelTransactionError> {
            Ok(Failing)
        }
        fn restore(
            &self,
            _: &DecisionContextV6,
            _: &strategy_core_v3::decision_v6::KernelCheckpointV6,
        ) -> Result<Failing, KernelTransactionError> {
            Ok(Failing)
        }
    }
    let context = priced_context();
    let placed = decide(&context, place_yes);
    let next = follow_up(
        &context,
        Some(&placed.result),
        2,
        vec![order(
            "command.delivery.daily.1.0",
            "yes-1",
            BrokerOrderStatusV6::Resting,
            0,
            1,
        )],
        vec![],
    );
    let rejected = run_transaction(&FailingFactory, &next).unwrap();
    assert_eq!(rejected.disposition, DecisionDispositionV6::Rejected);
    assert_eq!(
        rejected.kernel_checkpoint, next.kernel_checkpoint,
        "nothing is marked seen"
    );
    assert!(rejected.acknowledged_command_ids.is_empty());
    assert_eq!(
        decide(&next, nothing).updates.len(),
        1,
        "the next run sees it again"
    );
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
    assert_eq!(decision.seen, ["command.delivery.daily.1.1"]);
    assert_eq!(
        decision.result.commands[1],
        StrategyCommandV6::CancelOrder {
            command_id: "command.delivery.daily.1.1".to_owned(),
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
            "command.delivery.daily.1.0",
            "yes-1",
            BrokerOrderStatusV6::Cancelled,
            0,
            1,
        )],
        vec![CommandReceiptV6 {
            command_id: "command.delivery.daily.1.1".to_owned(),
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
    assert_eq!(
        decision.result.commands[63].command_id(),
        "command.delivery.daily.1.63"
    );
}

#[test]
fn the_runner_tracks_at_most_256_orders_and_commands() {
    let context = crowded_context(MAX_BROKER_ORDERS);
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
    assert_eq!(
        decision
            .result
            .kernel_checkpoint
            .unwrap()
            .runner
            .entries
            .len(),
        MAX_BROKER_ORDERS,
        "every open order is tracked"
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
