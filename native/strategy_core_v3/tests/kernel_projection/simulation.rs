//! A randomized Sleeve against a simulated Broker whose views are stale, truncated, reordered
//! or missing orders (some for good, some long enough for their tombstones to expire before
//! they return), starting with orders of its own over truncated views, with a kernel that
//! fails on some updates, never handles others (abandoned), and places orders inside update
//! handlers past the decision's capacity limits: every fill of every order is counted exactly
//! once, or reported as lost or as filled before the runner adopted the order, and the runner
//! section stays within its bounds.

use std::collections::{BTreeMap, BTreeSet};

use super::decisions::{Lcg, follow_up, limit_buy, priced_context};
use super::*;
use strategy_core_kernel::{BrokerCommandKind, CancelOrderRequest, CancelTarget, KernelError};
use strategy_core_v3::decision_v6::{
    BrokerCommandKindV6, BrokerOrderStatusV6, BrokerOrderV6, CancelTargetV6, CommandOutcomeV6,
    CommandReceiptV6, ContractSideV6, DecisionResultV6, KernelCheckpointV6, MAX_BROKER_ORDERS,
    MAX_RUNNER_SECTION_ENTRIES, MAX_TOMBSTONES, OrderActionV6, OrderTypeV6,
    validate_decision_result_v6,
};

/// What the test drives from outside the kernel: its randomness and the failures it injects.
struct Plan {
    rng: Lcg,
    /// Consecutive deliveries the kernel failed, per command.
    failures: BTreeMap<String, u8>,
    /// Orders whose every update the kernel fails on: their updates are abandoned.
    poisoned: BTreeSet<String>,
    /// Bursts of places left: each takes one decision to its 64 commands or its open-order
    /// cap.
    bursts: u8,
    /// No new commands and no transient failures: the Sleeve winds down.
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
    /// Places a 3-contract order under a new client id starting with `prefix`.
    fn place(
        &mut self,
        context: &mut dyn StrategyKernelContext,
        prefix: &str,
    ) -> KernelResult<String> {
        let client = format!("{prefix}{}", self.next);
        context
            .broker()
            .place_order(limit_buy(&client, ContractSide::Yes, 300, 0.4))?;
        self.next += 1;
        self.book.insert(client.clone(), (0, false));
        Ok(client)
    }

    fn act(&mut self, context: &mut dyn StrategyKernelContext) -> KernelResult<()> {
        if self.plan.borrow().quiet {
            return Ok(());
        }
        let (place, poison, cancel, cancel_all, pick) = {
            let mut plan = self.plan.borrow_mut();
            (
                plan.rng.next(2) == 0,
                plan.rng.next(10) == 0,
                plan.rng.next(3) == 0,
                plan.rng.next(20) == 0,
                plan.rng.next(64),
            )
        };
        if place {
            if let Ok(client) = self.place(context, "o") {
                if poison {
                    self.plan.borrow_mut().poisoned.insert(client);
                }
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
        let (burst, hedge) = {
            let mut plan = self.plan.borrow_mut();
            if plan.poisoned.contains(&update.client_order_id) {
                return Err(KernelError::new("poisoned"));
            }
            let fail = plan.rng.next(4) == 0 && !plan.quiet;
            let failures = plan.failures.entry(update.command_id.clone()).or_default();
            // Fail at most twice in a row per command (the runner delivers three times), so
            // no update of an order that is not poisoned is abandoned.
            if fail && *failures < 2 {
                *failures += 1;
                return Err(KernelError::new("transient"));
            }
            let quiet = plan.quiet;
            let burst = !quiet && plan.bursts > 0 && plan.rng.next(40) == 0;
            plan.bursts -= u8::from(burst);
            (burst, !quiet && plan.rng.next(3) == 0)
        };
        // A burst of places takes the decision to one of its capacity limits (64 commands,
        // the open-order cap); calls past it are refused.
        if burst {
            for _ in 0..64 {
                let _ = self.place(context, "b");
            }
        }
        if update.command_kind == BrokerCommandKind::PlaceOrder {
            // An order the Ledger never placed was adopted (seeded, or tracked again).
            let entry = self.book.entry(update.client_order_id.clone()).or_default();
            entry.0 += update.newly_filled.hundredths();
            if update.is_final && !update.vanished {
                entry.1 = true;
            }
            // A fill of an order the Sleeve chose is hedged at once; a refusal fails the
            // update, which the runner delivers again (deferred, not counted, when an earlier
            // update of the decision took the room; counted, and possibly abandoned, at the
            // open-order cap).
            if hedge && update.newly_filled.is_positive() && update.client_order_id.starts_with('o')
            {
                self.place(context, "h")?;
            }
        }
        // Only an update handled in full ends the run of failures.
        self.plan
            .borrow_mut()
            .failures
            .insert(update.command_id.clone(), 0);
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

/// The fill a Ledger checkpoint counted per client.
fn counted(result: Option<&DecisionResultV6>) -> BTreeMap<String, i64> {
    let Some(checkpoint) = result.and_then(|result| result.kernel_checkpoint.as_ref()) else {
        return BTreeMap::new();
    };
    String::from_utf8(checkpoint.state.clone())
        .unwrap()
        .split(';')
        .skip(1)
        .map(|part| {
            let mut fields = part.split('=');
            let client = fields.next().unwrap().to_owned();
            (client, fields.next().unwrap().parse().unwrap())
        })
        .collect()
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
    /// Steps it stays out of the Broker's view before it returns (the Broker keeps working
    /// it meanwhile); long enough, its tombstone expires first.
    away: u8,
    /// Final and acknowledged: the Broker dropped it from the view.
    acknowledged: bool,
}

impl WorldOrder {
    fn new(command_id: String, client: String) -> Self {
        Self {
            command_id,
            client,
            status: BrokerOrderStatusV6::DurablyAccepted,
            filled: 0,
            revision: 1,
            cancel_requested: false,
            gone: false,
            away: 0,
            acknowledged: false,
        }
    }

    fn visible(&self) -> bool {
        !self.gone && self.away == 0 && !self.acknowledged
    }

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
    /// A Sleeve that already holds orders of its own when the runner first sees it (a V5
    /// Strategy's orders): some open, some final.
    fn with_existing_orders() -> Self {
        let mut world = Self {
            revision: 1,
            ..Self::default()
        };
        for (index, (status, filled)) in [
            (BrokerOrderStatusV6::Resting, 0),
            (BrokerOrderStatusV6::PartiallyFilled, 100),
            (BrokerOrderStatusV6::CancellationRequested, 50),
            (BrokerOrderStatusV6::Filled, 300),
            (BrokerOrderStatusV6::Cancelled, 75),
        ]
        .into_iter()
        .enumerate()
        {
            let mut order = WorldOrder::new(format!("command.pre.{index}"), format!("pre-{index}"));
            order.status = status;
            order.filled = filled;
            order.revision = 3;
            world.orders.push(order);
        }
        world
    }

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
                        self.orders.push(WorldOrder::new(
                            order.command_id.clone(),
                            order.provider_client_id.clone(),
                        ));
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

    /// Moves every open order along; `settle` brings back every order that is away and
    /// drives them all to a final status.
    fn advance(&mut self, rng: &mut Lcg, settle: bool) {
        for order in &mut self.orders {
            order.away = if settle {
                0
            } else {
                order.away.saturating_sub(1)
            };
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
            } else if roll < 43 && order.away == 0 {
                order.away = 12 + rng.next(30) as u8;
            } else if roll < 60 && order.status == BrokerOrderStatusV6::DurablyAccepted {
                order.status = BrokerOrderStatusV6::Resting;
            }
            if (order.status, order.filled) != before {
                order.revision += 1;
            }
        }
        self.revision += 1;
    }

    /// The Sleeve's order view and whether it is complete. The host truncates a view past
    /// `MAX_BROKER_ORDERS` by dropping terminal orders, never an open one.
    fn visible(&self) -> (Vec<BrokerOrderV6>, bool) {
        let mut orders = self
            .orders
            .iter()
            .filter(|order| order.visible())
            .map(WorldOrder::record)
            .collect::<Vec<_>>();
        let complete = orders.len() <= MAX_BROKER_ORDERS;
        while orders.len() > MAX_BROKER_ORDERS {
            let terminal = orders
                .iter()
                .position(|order| order.status.is_terminal())
                .expect("open orders fit the view");
            orders.remove(terminal);
        }
        (orders, complete)
    }

    /// A view at a newer Broker revision that still misses the orders admitted after `old`
    /// (the revision is account-wide and moves with other Sleeves). It shows the orders it
    /// has as of now: a view never shows an order older than a view at a lower revision did.
    fn newer_but_incomplete(&self, old: &World) -> Vec<BrokerOrderV6> {
        old.visible()
            .0
            .into_iter()
            .filter_map(|record| {
                self.orders
                    .iter()
                    .find(|order| order.command_id == record.command_id && order.visible())
                    .map(WorldOrder::record)
            })
            .collect()
    }
}

/// What the runner reported it did not deliver, per command id, so the Ledger's counts can
/// be checked exactly: `counted + lost + unreported == filled`.
#[derive(Default)]
struct Accounts {
    /// Fill of updates abandoned after three failed deliveries.
    lost: BTreeMap<String, i64>,
    /// Fill an order already had when the runner adopted it, beyond what the kernel counted.
    unreported: BTreeMap<String, i64>,
    /// Orders the runner stopped tracking (tombstone expired or evicted) and has not adopted
    /// again: their later fills may be unreported.
    untracked: BTreeSet<String>,
    abandoned: usize,
    adopted: usize,
    expired: usize,
    deferred: usize,
}

fn diagnostic(result: &DecisionResultV6, code: &str) -> Option<serde_json::Value> {
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == code)?;
    Some(serde_json::from_str(&diagnostic.message).unwrap())
}

fn ids(message: &serde_json::Value) -> Vec<String> {
    message["command_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap().to_owned())
        .collect()
}

impl Accounts {
    fn record(
        &mut self,
        world: &World,
        previous: Option<&DecisionResultV6>,
        orders: &[BrokerOrderV6],
        result: &DecisionResultV6,
        seeding: bool,
    ) {
        let before = counted(previous);
        let client = |command_id: &str| {
            world
                .orders
                .iter()
                .find(|order| order.command_id == command_id)
                .map(|order| order.client.clone())
        };
        let adopt = |accounts: &mut Self, command_id: &str| {
            let order = orders
                .iter()
                .find(|order| order.command_id == command_id)
                .unwrap();
            let known = before.get(&order.provider_client_id).copied().unwrap_or(0)
                + accounts.lost.get(command_id).copied().unwrap_or(0)
                + accounts.unreported.get(command_id).copied().unwrap_or(0);
            *accounts
                .unreported
                .entry(command_id.to_owned())
                .or_default() += order.filled_quantity_hundredths as i64 - known;
            accounts.untracked.remove(command_id);
        };
        if seeding {
            for order in orders.iter().filter(|order| !order.status.is_terminal()) {
                adopt(self, &order.command_id);
            }
        }
        if let Some(message) = diagnostic(result, "order_update_abandoned") {
            assert_eq!(
                result
                    .diagnostics
                    .iter()
                    .find(|diagnostic| diagnostic.code == "order_update_abandoned")
                    .unwrap()
                    .severity,
                "error"
            );
            assert_eq!(
                message["count"].as_u64().unwrap() as usize,
                ids(&message).len(),
                "every abandoned update is listed"
            );
            let lost = message["lost_newly_filled_hundredths"].as_array().unwrap();
            for (command_id, lost) in ids(&message).iter().zip(lost) {
                self.abandoned += 1;
                if client(command_id).is_some() {
                    *self.lost.entry(command_id.clone()).or_default() += lost.as_i64().unwrap();
                }
            }
        }
        // The diagnostics name at most 8 command ids each: compare the runner sections.
        let entries = |result: Option<&DecisionResultV6>| {
            result
                .and_then(|result| result.kernel_checkpoint.as_ref())
                .map(|checkpoint| checkpoint.runner.entries.clone())
                .unwrap_or_default()
        };
        let old = entries(previous);
        let new = entries(Some(result));
        for entry in &old {
            let kept = new.iter().any(|next| next.command_id == entry.command_id);
            let final_shown = orders
                .iter()
                .any(|order| order.command_id == entry.command_id && order.status.is_terminal());
            if !kept && !final_shown && client(&entry.command_id).is_some() {
                // Expired or evicted (a final order's entry is pruned once delivered).
                self.expired += 1;
                self.untracked.insert(entry.command_id.clone());
            }
        }
        let issued = result
            .commands
            .iter()
            .map(|command| command.command_id())
            .collect::<BTreeSet<_>>();
        for entry in &new {
            let fresh = !old
                .iter()
                .any(|previous| previous.command_id == entry.command_id);
            if fresh && !issued.contains(entry.command_id.as_str()) && !seeding {
                self.adopted += 1;
                adopt(self, &entry.command_id);
            }
        }
        assert_eq!(
            diagnostic(result, "runner_order_adopted")
                .map_or(0, |message| message["count"].as_u64().unwrap() as usize),
            new.iter()
                .filter(|entry| {
                    !seeding
                        && !old
                            .iter()
                            .any(|previous| previous.command_id == entry.command_id)
                        && !issued.contains(entry.command_id.as_str())
                })
                .count(),
            "every adoption is reported"
        );
        assert!(
            diagnostic(result, "runner_order_not_adopted").is_none(),
            "the section always has room for the Sleeve's open orders"
        );
        self.deferred += result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "order_update_deferred")
            .count();
    }
}

/// Chains decisions: each context carries the previous result's checkpoint.
struct Driver<'a> {
    factory: &'a LedgerFactory,
    base: &'a DecisionContextV6,
    previous: Option<DecisionResultV6>,
    delivery: u32,
    accounts: Accounts,
}

impl Driver<'_> {
    fn decide(
        &mut self,
        world: &mut World,
        orders: Vec<BrokerOrderV6>,
        receipts: Vec<CommandReceiptV6>,
        revision: u64,
        complete: bool,
    ) -> DecisionResultV6 {
        let mut context = follow_up(
            self.base,
            self.previous.as_ref(),
            self.delivery,
            orders.clone(),
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
        let result = run_transaction(self.factory, &context).unwrap();
        validate_decision_result_v6(&context, &result).unwrap();
        let entries = &result.kernel_checkpoint.as_ref().unwrap().runner.entries;
        let tombstones = entries.iter().filter(|entry| entry.vanished).count();
        assert!(entries.len() <= MAX_RUNNER_SECTION_ENTRIES);
        assert!(tombstones <= MAX_TOMBSTONES);
        self.accounts.record(
            world,
            self.previous.as_ref(),
            &context.broker.orders,
            &result,
            self.previous.is_none(),
        );
        self.delivery += 1;
        self.previous = Some(result.clone());
        result
    }
}

#[test]
fn every_fill_is_counted_once_across_orders_cancels_and_failures() {
    let mut totals = Accounts::default();
    for seed in 0..24 {
        let plan = Rc::new(RefCell::new(Plan {
            rng: Lcg(seed),
            failures: BTreeMap::new(),
            poisoned: BTreeSet::new(),
            bursts: 3,
            quiet: false,
        }));
        let factory = LedgerFactory(Rc::clone(&plan));
        let base = priced_context();
        let mut world = World::with_existing_orders();
        let mut history: Vec<World> = Vec::new();
        let mut driver = Driver {
            factory: &factory,
            base: &base,
            previous: None,
            delivery: 1,
            accounts: Accounts::default(),
        };
        for step in 0..60_u64 {
            let (kind, pick) = {
                let mut plan = plan.borrow_mut();
                (plan.rng.next(10), plan.rng.next(8) as usize)
            };
            let past = history.len().saturating_sub(1 + pick % 4);
            let (visible, complete) = world.visible();
            let (orders, receipts, revision, complete) = match kind {
                // A truncated view: the host dropped terminal orders (all of them in the
                // Sleeve's first views), never an open one.
                _ if step < 3 || kind == 0 => {
                    let mut orders = visible;
                    if step < 3 {
                        orders.retain(|order| !order.status.is_terminal());
                    } else if let Some(terminal) =
                        orders.iter().position(|order| order.status.is_terminal())
                    {
                        orders.remove(terminal);
                    }
                    (orders, world.receipts.clone(), world.revision, false)
                }
                // A stale view at its own, older revision.
                1 if !history.is_empty() => (
                    history[past].visible().0,
                    history[past].receipts.clone(),
                    history[past].revision,
                    true,
                ),
                // A view at a newer (account-wide) revision without the newer orders and
                // receipts.
                2 if !history.is_empty() => (
                    world.newer_but_incomplete(&history[past]),
                    history[past].receipts.clone(),
                    world.revision,
                    true,
                ),
                // A complete view missing one order for a while.
                3 => {
                    let mut orders = visible;
                    if !orders.is_empty() {
                        orders.remove(pick % orders.len());
                    }
                    (orders, world.receipts.clone(), world.revision, complete)
                }
                _ => (visible, world.receipts.clone(), world.revision, complete),
            };
            let result = driver.decide(&mut world, orders, receipts, revision, complete);
            history.push(world.clone());
            let mut rng = Lcg(seed * 1_000 + step);
            world.admit(&result, &mut rng);
            world.advance(&mut rng, false);
        }
        // Settle: complete, fresh views until every order is final and reported; then wind
        // down with no new commands or transient failures while the last states are
        // delivered.
        for round in 0..60 {
            plan.borrow_mut().quiet = round >= 40;
            let (orders, complete) = world.visible();
            let receipts = world.receipts.clone();
            let revision = world.revision;
            let result = driver.decide(&mut world, orders, receipts, revision, complete);
            let mut rng = Lcg(seed * 7_919 + round);
            world.admit(&result, &mut rng);
            world.advance(&mut rng, true);
        }
        let counted = counted(driver.previous.as_ref());
        let accounts = &driver.accounts;
        for order in &world.orders {
            let counted = counted.get(&order.client).copied().unwrap_or(0);
            let lost = accounts.lost.get(&order.command_id).copied().unwrap_or(0);
            let unreported = accounts
                .unreported
                .get(&order.command_id)
                .copied()
                .unwrap_or(0);
            let told = counted + lost + unreported;
            assert!(
                counted <= 300,
                "seed {seed}: {} never counted twice",
                order.client
            );
            if order.command_id.starts_with("command.pre.")
                && order.status.is_terminal()
                && told == 0
            {
                // Final before the runner first saw it: acknowledged, never news.
                continue;
            }
            if order.gone || accounts.untracked.contains(&order.command_id) {
                assert!(
                    told <= order.filled as i64,
                    "seed {seed}: {} told {told} of {}",
                    order.client,
                    order.filled
                );
            } else if order.status.is_terminal() {
                assert_eq!(
                    told, order.filled as i64,
                    "seed {seed}: {} counts every fill once (counted {counted}, lost {lost}, \
                     unreported {unreported})",
                    order.client
                );
            }
        }
        totals.abandoned += accounts.abandoned;
        totals.adopted += accounts.adopted;
        totals.expired += accounts.expired;
        totals.deferred += accounts.deferred;
    }
    // The simulation exercised every path it is meant to.
    assert!(totals.abandoned > 0, "no update was abandoned");
    assert!(totals.expired > 0, "no tombstone expired");
    assert!(totals.adopted > 0, "no order was adopted again");
    assert!(totals.deferred > 0, "no update waited for capacity");
}
