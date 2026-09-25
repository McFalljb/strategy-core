//! A randomized Sleeve against a simulated Broker whose views are stale, truncated, reordered
//! or missing orders, with a kernel that fails on some updates: every fill of every order is
//! counted exactly once and the runner section stays within its bounds.

use std::collections::BTreeMap;

use super::decisions::{Lcg, follow_up, limit_buy, priced_context};
use super::*;
use strategy_core_kernel::{BrokerCommandKind, CancelOrderRequest, CancelTarget, KernelError};
use strategy_core_v3::decision_v6::{
    BrokerCommandKindV6, BrokerOrderStatusV6, BrokerOrderV6, CancelTargetV6, CommandOutcomeV6,
    CommandReceiptV6, ContractSideV6, DecisionResultV6, KernelCheckpointV6, MAX_RUNNER_ENTRIES,
    MAX_TOMBSTONES, OrderActionV6, OrderTypeV6, validate_decision_result_v6,
};

/// What the test drives from outside the kernel: its randomness and the failures it injects.
struct Plan {
    rng: Lcg,
    /// Consecutive deliveries the kernel failed, per command.
    failures: BTreeMap<String, u8>,
    /// No new commands and no failures: the Sleeve winds down.
    quiet: bool,
}

/// Counts every fill it is told of, per order, in its checkpoint.
struct Ledger {
    next: u32,
    /// Client id → (fill counted, final update seen).
    book: BTreeMap<String, (i64, bool)>,
    plan: Rc<RefCell<Plan>>,
}

impl Ledger {
    fn act(&mut self, context: &mut dyn StrategyKernelContext) -> KernelResult<()> {
        if self.plan.borrow().quiet {
            return Ok(());
        }
        let (place, cancel, cancel_all, pick) = {
            let mut plan = self.plan.borrow_mut();
            (
                plan.rng.next(2) == 0,
                plan.rng.next(3) == 0,
                plan.rng.next(20) == 0,
                plan.rng.next(64),
            )
        };
        if place {
            let client = format!("o{}", self.next);
            if context
                .broker()
                .place_order(limit_buy(&client, ContractSide::Yes, 300, 0.4))
                .is_ok()
            {
                self.next += 1;
                self.book.insert(client, (0, false));
            }
        }
        let open = self
            .book
            .iter()
            .filter(|(_, (_, closed))| !closed)
            .map(|(client, _)| client.clone())
            .collect::<Vec<_>>();
        if cancel && !open.is_empty() {
            let client = open[pick as usize % open.len()].clone();
            // The runner refuses a cancel of an order it cannot see; that is fine here.
            let _ = context.broker().cancel_order(CancelOrderRequest {
                target: CancelTarget::ClientOrderId(client),
            });
        }
        if cancel_all {
            let _ = context.broker().cancel_all_orders();
        }
        Ok(())
    }
}

impl NativeKernel for Ledger {
    fn name(&self) -> &str {
        "ledger"
    }

    fn on_start(&mut self, context: &mut dyn StrategyKernelContext) -> KernelResult<()> {
        self.act(context)
    }

    fn on_event(
        &mut self,
        event: StrategyEventView<'_>,
        context: &mut dyn StrategyKernelContext,
    ) -> KernelResult<()> {
        let StrategyEventView::OrderUpdate(update) = event else {
            return self.act(context);
        };
        {
            let mut plan = self.plan.borrow_mut();
            let fail = plan.rng.next(4) == 0 && !plan.quiet;
            let failures = plan.failures.entry(update.command_id.clone()).or_default();
            // Fail at most twice in a row per command (the runner delivers three times), so
            // no update is abandoned.
            if fail && *failures < 2 {
                *failures += 1;
                return Err(KernelError::new("transient"));
            }
            *failures = 0;
        }
        if update.command_kind != BrokerCommandKind::PlaceOrder {
            return Ok(());
        }
        let entry = self
            .book
            .get_mut(&update.client_order_id)
            .ok_or_else(|| KernelError::new("an update for an order never placed"))?;
        entry.0 += update.newly_filled.hundredths();
        if update.is_final && !update.vanished {
            entry.1 = true;
        }
        Ok(())
    }
}

impl TransactionKernel for Ledger {
    fn encode_checkpoint_state(&self) -> Result<Vec<u8>, KernelTransactionError> {
        let mut text = self.next.to_string();
        for (client, (filled, closed)) in &self.book {
            text.push_str(&format!(";{client}={filled}={closed}"));
        }
        Ok(text.into_bytes())
    }
}

struct LedgerFactory(Rc<RefCell<Plan>>);

impl TransactionKernelFactory for LedgerFactory {
    type Kernel = Ledger;
    fn checkpoint_codec(&self, _: &str) -> Result<KernelCheckpointCodec, KernelTransactionError> {
        Ok(KernelCheckpointCodec {
            profile: "ledger.checkpoint.v1".to_owned(),
            version: 1,
        })
    }
    fn create(&self, _: &DecisionContextV6) -> Result<Ledger, KernelTransactionError> {
        Ok(Ledger {
            next: 0,
            book: BTreeMap::new(),
            plan: Rc::clone(&self.0),
        })
    }
    fn restore(
        &self,
        _: &DecisionContextV6,
        checkpoint: &KernelCheckpointV6,
    ) -> Result<Ledger, KernelTransactionError> {
        let text = String::from_utf8(checkpoint.state.clone()).unwrap();
        let mut parts = text.split(';');
        let next = parts.next().unwrap().parse().unwrap();
        let book = parts
            .map(|part| {
                let mut fields = part.split('=');
                let client = fields.next().unwrap().to_owned();
                let filled = fields.next().unwrap().parse().unwrap();
                let closed = fields.next().unwrap() == "true";
                (client, (filled, closed))
            })
            .collect();
        Ok(Ledger {
            next,
            book,
            plan: Rc::clone(&self.0),
        })
    }
}

/// One order as the simulated Broker holds it.
#[derive(Clone)]
struct WorldOrder {
    command_id: String,
    client: String,
    status: BrokerOrderStatusV6,
    filled: u64,
    revision: u64,
    cancel_requested: bool,
    /// Left the Broker's view for good without a final status.
    gone: bool,
    /// Final and acknowledged: the Broker dropped it from the view.
    acknowledged: bool,
}

impl WorldOrder {
    fn record(&self) -> BrokerOrderV6 {
        let terminal = self.status.is_terminal();
        let remaining = if terminal { 0 } else { 300 - self.filled };
        BrokerOrderV6 {
            command_id: self.command_id.clone(),
            intent_id: format!("intent.{}", self.client),
            order_id: format!("order.{}", self.client),
            provider_order_id: None,
            provider_client_id: self.client.clone(),
            market_id: MARKET.to_owned(),
            action: OrderActionV6::Buy,
            side: ContractSideV6::Yes,
            order_type: OrderTypeV6::Limit,
            quantity_hundredths: 300,
            filled_quantity_hundredths: self.filled,
            remaining_quantity_hundredths: remaining,
            limit_price_micros: Some(400_000),
            average_fill_price_micros: (self.filled > 0).then_some(400_000),
            reserved_principal_micros: remaining * 400_000 / 100,
            reserved_fee_micros: 0,
            fees_micros: self.filled * 70,
            rejection_reason: None,
            created_at_unix_ms: Some(DECISION_MS),
            updated_at_unix_ms: Some(DECISION_MS),
            signal_type: None,
            signal_metadata: None,
            status: self.status,
            revision: self.revision,
        }
    }
}

#[derive(Clone, Default)]
struct World {
    revision: u64,
    orders: Vec<WorldOrder>,
    receipts: Vec<CommandReceiptV6>,
}

impl World {
    /// Admits a result's commands and applies its acknowledgements.
    fn admit(&mut self, result: &DecisionResultV6, rng: &mut Lcg) {
        for command in &result.commands {
            let receipt = |kind, refused: bool| CommandReceiptV6 {
                command_id: command.command_id().to_owned(),
                kind,
                outcome: if refused {
                    CommandOutcomeV6::Refused {
                        code: "stale_order_revision".to_owned(),
                        reason: "refused".to_owned(),
                    }
                } else {
                    CommandOutcomeV6::Accepted
                },
            };
            match command {
                StrategyCommandV6::PlaceOrder(order) => {
                    if rng.next(10) == 0 {
                        self.receipts
                            .push(receipt(BrokerCommandKindV6::PlaceOrder, true));
                    } else {
                        self.orders.push(WorldOrder {
                            command_id: order.command_id.clone(),
                            client: order.provider_client_id.clone(),
                            status: BrokerOrderStatusV6::DurablyAccepted,
                            filled: 0,
                            revision: 1,
                            cancel_requested: false,
                            gone: false,
                            acknowledged: false,
                        });
                    }
                }
                StrategyCommandV6::CancelOrder { target, .. } => {
                    let client = match target {
                        CancelTargetV6::Order { order_id, .. } => {
                            order_id.trim_start_matches("order.").to_owned()
                        }
                        CancelTargetV6::SameDecision { provider_client_id } => {
                            provider_client_id.clone()
                        }
                    };
                    let target = self
                        .orders
                        .iter_mut()
                        .find(|order| order.client == client && !order.status.is_terminal());
                    let accepted = target.is_some() && rng.next(5) != 0;
                    if let (true, Some(order)) = (accepted, target) {
                        order.cancel_requested = true;
                    }
                    self.receipts
                        .push(receipt(BrokerCommandKindV6::CancelOrder, !accepted));
                }
                StrategyCommandV6::CancelAllOrders { .. } => {
                    for order in &mut self.orders {
                        order.cancel_requested |= !order.status.is_terminal();
                    }
                    self.receipts
                        .push(receipt(BrokerCommandKindV6::CancelAllOrders, false));
                }
                _ => {}
            }
        }
        let acknowledged = &result.acknowledged_command_ids;
        self.receipts
            .retain(|receipt| !acknowledged.contains(&receipt.command_id));
        for order in &mut self.orders {
            order.acknowledged |=
                order.status.is_terminal() && acknowledged.contains(&order.command_id);
        }
        self.revision += 1;
    }

    /// Moves every open order along; `settle` drives them all to a final status.
    fn advance(&mut self, rng: &mut Lcg, settle: bool) {
        for order in &mut self.orders {
            if order.gone || order.status.is_terminal() {
                continue;
            }
            let roll = rng.next(100);
            let before = (order.status, order.filled);
            if order.cancel_requested && roll < 50 {
                order.status = BrokerOrderStatusV6::Cancelled;
            } else if settle || roll < 35 {
                order.filled = (order.filled + 25 * (1 + rng.next(4))).min(300);
                order.status = if order.filled == 300 {
                    BrokerOrderStatusV6::Filled
                } else {
                    BrokerOrderStatusV6::PartiallyFilled
                };
            } else if roll < 38 {
                order.status = BrokerOrderStatusV6::Expired;
            } else if roll < 40 {
                order.status = BrokerOrderStatusV6::Rejected;
            } else if roll < 41 {
                order.gone = true;
            } else if roll < 60 && order.status == BrokerOrderStatusV6::DurablyAccepted {
                order.status = BrokerOrderStatusV6::Resting;
            }
            if (order.status, order.filled) != before {
                order.revision += 1;
            }
        }
        self.revision += 1;
    }

    fn visible(&self) -> Vec<BrokerOrderV6> {
        self.orders
            .iter()
            .filter(|order| !order.gone && !order.acknowledged)
            .map(WorldOrder::record)
            .collect()
    }
}

/// Chains decisions: each context carries the previous result's checkpoint.
struct Driver {
    previous: Option<DecisionResultV6>,
    delivery: u32,
}

impl Driver {
    fn decide(
        &mut self,
        factory: &LedgerFactory,
        base: &DecisionContextV6,
        orders: Vec<BrokerOrderV6>,
        receipts: Vec<CommandReceiptV6>,
        revision: u64,
        complete: bool,
    ) -> DecisionResultV6 {
        let mut context = follow_up(
            base,
            self.previous.as_ref(),
            self.delivery,
            orders,
            receipts,
        );
        context.broker.revision = revision;
        context.owner_state.broker.revision = revision;
        context.owner_state.fence.broker_revision = revision;
        context.orders_complete = complete;
        context.trigger = if self.previous.is_none() {
            TriggerV6::Owner(OwnerTriggerV6::Recovery)
        } else {
            TriggerV6::BrokerState {
                broker_revision: revision,
            }
        };
        context.validate().unwrap();
        let result = run_transaction(factory, &context).unwrap();
        validate_decision_result_v6(&context, &result).unwrap();
        let entries = &result.kernel_checkpoint.as_ref().unwrap().runner.entries;
        let tombstones = entries.iter().filter(|entry| entry.vanished).count();
        assert!(entries.len() - tombstones <= MAX_RUNNER_ENTRIES);
        assert!(tombstones <= MAX_TOMBSTONES);
        self.delivery += 1;
        self.previous = Some(result.clone());
        result
    }
}

#[test]
fn every_fill_is_counted_once_across_orders_cancels_and_failures() {
    for seed in 0..40 {
        let plan = Rc::new(RefCell::new(Plan {
            rng: Lcg(seed),
            failures: BTreeMap::new(),
            quiet: false,
        }));
        let factory = LedgerFactory(Rc::clone(&plan));
        let base = priced_context();
        let mut world = World {
            revision: 1,
            ..World::default()
        };
        let mut history: Vec<World> = Vec::new();
        let mut driver = Driver {
            previous: None,
            delivery: 1,
        };
        for step in 0..60_u64 {
            let (kind, pick) = {
                let mut plan = plan.borrow_mut();
                (plan.rng.next(10), plan.rng.next(8) as usize)
            };
            let past = history.len().saturating_sub(1 + pick % 4);
            let (orders, receipts, revision, complete) = match kind {
                // A truncated view.
                0 => {
                    let mut orders = world.visible();
                    if !orders.is_empty() {
                        orders.remove(pick % orders.len());
                    }
                    (orders, world.receipts.clone(), world.revision, false)
                }
                // A stale view at its own, older revision.
                1 if !history.is_empty() => {
                    let old: &World = &history[past];
                    (old.visible(), old.receipts.clone(), old.revision, true)
                }
                // A stale view at a newer (account-wide) revision, without newer receipts.
                2 if !history.is_empty() => {
                    let old: &World = &history[past];
                    (old.visible(), old.receipts.clone(), world.revision, true)
                }
                // A complete view missing one order for a while.
                3 => {
                    let mut orders = world.visible();
                    if !orders.is_empty() {
                        orders.remove(pick % orders.len());
                    }
                    (orders, world.receipts.clone(), world.revision, true)
                }
                _ => (
                    world.visible(),
                    world.receipts.clone(),
                    world.revision,
                    true,
                ),
            };
            let result = driver.decide(&factory, &base, orders, receipts, revision, complete);
            history.push(world.clone());
            let mut rng = Lcg(seed * 1_000 + step);
            world.admit(&result, &mut rng);
            world.advance(&mut rng, false);
        }
        // Settle: complete, fresh views until every order is final and reported.
        for round in 0..40 {
            let result = driver.decide(
                &factory,
                &base,
                world.visible(),
                world.receipts.clone(),
                world.revision,
                true,
            );
            let mut rng = Lcg(seed * 7_919 + round);
            world.admit(&result, &mut rng);
            world.advance(&mut rng, true);
        }
        // Wind down: no new commands or failures while the last states are delivered.
        plan.borrow_mut().quiet = true;
        for round in 0..20 {
            let result = driver.decide(
                &factory,
                &base,
                world.visible(),
                world.receipts.clone(),
                world.revision,
                true,
            );
            let mut rng = Lcg(seed * 104_729 + round);
            world.admit(&result, &mut rng);
            world.advance(&mut rng, true);
        }
        let last = driver.previous.unwrap();
        let state = String::from_utf8(last.kernel_checkpoint.unwrap().state).unwrap();
        let counted = state
            .split(';')
            .skip(1)
            .map(|part| {
                let mut fields = part.split('=');
                let client = fields.next().unwrap().to_owned();
                let filled: i64 = fields.next().unwrap().parse().unwrap();
                (client, filled)
            })
            .collect::<BTreeMap<_, _>>();
        for order in &world.orders {
            let counted = counted.get(&order.client).copied().unwrap_or(0);
            if order.gone {
                assert!(
                    counted <= order.filled as i64,
                    "seed {seed}: {} counted {counted} of {}",
                    order.client,
                    order.filled
                );
            } else if order.status.is_terminal() {
                assert_eq!(
                    counted, order.filled as i64,
                    "seed {seed}: {} counts every fill once",
                    order.client
                );
            }
        }
        // Orders the world acknowledged and dropped were final and fully counted before.
        for (client, filled) in &counted {
            assert!(*filled <= 300, "seed {seed}: {client} never counted twice");
        }
    }
}
