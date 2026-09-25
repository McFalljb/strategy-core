//! Order updates, derived by comparing the context's Broker state and command receipts with
//! what the checkpoint's runner section last saw.
//!
//! Nothing is queued: a checkpoint that goes back produces the same updates again, and a
//! dropped or coalesced trigger loses nothing because the next comparison sees the change.
//! A context that is stale, truncated or reordered never costs a fill: an order is matched by
//! its command id only, an older record of it than the one last reported is ignored, its
//! absence from a view that may predate its admission or that the host truncated is no news,
//! and an order (or a cancel's receipt) missing from a complete newer view stays tracked as a
//! tombstone, so a reappearance reports what it missed. Tombstones are bounded and expire.
//! An open order of the Sleeve the section does not track (the section is not yet seeded, or
//! the order's tombstone expired) is adopted from its current status and tracked from then on.

use std::collections::{BTreeMap, BTreeSet};

use super::KernelTransactionError;
use crate::decision_v6::{
    BrokerCommandKindV6, BrokerOrderStatusV6, BrokerOrderV6, CommandOutcomeV6, DecisionContextV6,
    MAX_DELIVERY_ATTEMPTS, MAX_DELIVERY_DEFERRALS, MAX_RUNNER_SECTION_ENTRIES, MAX_TOMBSTONES,
    OrderUpdateRecordV6, OrderUpdateStatusV6, RunnerEntryV6, TOMBSTONE_EXPIRY_VIEWS,
};

/// Refusal code of an order the provider rejected after admission.
pub const PROVIDER_REJECTED_CODE: &str = "provider_rejected";
/// The refusal reason of a rejected order whose provider gave no text.
pub const PROVIDER_REJECTED_REASON: &str = "the provider rejected the order";

/// Something the comparison did that the result reports as a diagnostic.
pub(super) struct Note {
    pub code: &'static str,
    pub command_id: String,
    /// Fill quantity (hundredths) the kernel was never told of, for an abandoned update.
    pub lost_hundredths: u64,
}

impl Note {
    fn new(code: &'static str, command_id: String) -> Self {
        Self {
            code,
            command_id,
            lost_hundredths: 0,
        }
    }
}

/// How the kernel failed on one order update.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Failure {
    /// The attempt counts toward `MAX_DELIVERY_ATTEMPTS`.
    Counted,
    /// The kernel returned the host's refusal for room in the decision that earlier updates
    /// of the same decision took: counts toward `MAX_DELIVERY_DEFERRALS` only.
    Deferred,
    /// Undone with every other update of the decision (the kernel could not be restored
    /// after another one): it stays pending as it was.
    RolledBack,
}

/// One runner entry's comparison: the entry before, the entry after (none once its outcome
/// is seen), and the update to deliver.
pub(super) struct Step {
    previous: Option<RunnerEntryV6>,
    next: Option<RunnerEntryV6>,
    pub update: Option<OrderUpdateRecordV6>,
}

/// The comparison's outcome, before the updates are delivered.
pub(super) struct Derived {
    /// In issue order; adopted orders come last.
    pub steps: Vec<Step>,
    /// The section's `newest_view_revision` after this decision.
    pub newest_view_revision: u64,
    pub notes: Vec<Note>,
}

impl Step {
    /// The step's update was deferred by the previous decision: it is delivered first.
    pub fn deferred(&self) -> bool {
        self.update.is_some()
            && self
                .previous
                .as_ref()
                .is_some_and(|entry| entry.delivery_deferrals > 0)
    }

    /// Consecutive decisions that deferred this step's update.
    pub fn deferrals(&self) -> u8 {
        self.previous
            .as_ref()
            .map_or(0, |entry| entry.delivery_deferrals)
    }

    /// The consecutive deferral this step's update would be.
    pub fn deferral(&self) -> u8 {
        self.previous
            .as_ref()
            .map_or(1, |entry| entry.delivery_deferrals + 1)
    }

    /// The delivery attempt this step's update is on.
    pub fn attempt(&self) -> u8 {
        self.previous
            .as_ref()
            .map_or(1, |entry| entry.delivery_failures + 1)
    }
}

impl Derived {
    /// The indexes of the steps with an update, in delivery order. Updates the previous
    /// decision deferred come first, the most deferred first (then in issue order), so the
    /// one closest to `MAX_DELIVERY_DEFERRALS` runs without earlier commands; the others
    /// follow in issue order. A cancel's update never precedes an update of its target: the
    /// target's is pulled forward to just before it (for a cancel-all, every order placed
    /// before it).
    pub fn delivery_order(&self) -> Vec<usize> {
        let with_update = |index: &usize| self.steps[*index].update.is_some();
        let mut deferred = (0..self.steps.len())
            .filter(|index| self.steps[*index].deferred())
            .collect::<Vec<_>>();
        deferred.sort_by_key(|index| (std::cmp::Reverse(self.steps[*index].deferrals()), *index));
        let rest = (0..self.steps.len())
            .filter(with_update)
            .filter(|index| !self.steps[*index].deferred());
        let mut order = Vec::new();
        let mut placed = BTreeSet::new();
        for index in deferred.into_iter().chain(rest) {
            if placed.contains(&index) {
                continue;
            }
            for target in self.targets(index) {
                if placed.insert(target) {
                    order.push(target);
                }
            }
            placed.insert(index);
            order.push(index);
        }
        order
    }

    /// The steps with an update of the orders the cancel at `index` targets (none for a
    /// place).
    fn targets(&self, index: usize) -> Vec<usize> {
        let Some(cancel) = self.steps[index].previous.as_ref() else {
            return Vec::new();
        };
        let is_target = |other: usize| {
            let step = &self.steps[other];
            let Some(order) = step.previous.as_ref() else {
                return false;
            };
            step.update.is_some()
                && order.kind == BrokerCommandKindV6::PlaceOrder
                && match cancel.kind {
                    BrokerCommandKindV6::PlaceOrder => false,
                    BrokerCommandKindV6::CancelOrder => {
                        cancel.client_order_id.is_some()
                            && order.client_order_id == cancel.client_order_id
                    }
                    BrokerCommandKindV6::CancelAllOrders => other < index,
                }
        };
        (0..self.steps.len())
            .filter(|other| is_target(*other))
            .collect()
    }

    /// The entries as they stand if every update is handled.
    pub fn entries(&self) -> Vec<RunnerEntryV6> {
        self.steps
            .iter()
            .filter_map(|step| step.next.clone())
            .collect()
    }

    /// Live entries a failed update would bring back beyond [`Self::entries`].
    pub fn reinstatable_live(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| {
                step.update.is_some()
                    && step.previous.as_ref().is_some_and(RunnerEntryV6::is_live)
                    && !step.next.as_ref().is_some_and(RunnerEntryV6::is_live)
            })
            .count()
    }

    /// The runner section's entries after delivery. An entry whose update the kernel failed
    /// on (the indexes of `failed`) stays as it was, to be delivered again; a counted failure
    /// adds to its `delivery_failures` (and ends a run of deferrals), a deferral to its
    /// `delivery_deferrals`, and at `MAX_DELIVERY_ATTEMPTS` or `MAX_DELIVERY_DEFERRALS` its
    /// update counts as seen (abandoned, reported with the fill it carried). `issued` are the entries of the
    /// decision's own commands. Tombstones past `MAX_TOMBSTONES` are evicted, oldest first,
    /// and a tombstone whose order is back with an update still to deliver after all others.
    pub fn finalize(
        self,
        failed: &BTreeMap<usize, Failure>,
        issued: Vec<RunnerEntryV6>,
    ) -> (Vec<RunnerEntryV6>, Vec<Note>) {
        let mut notes = self.notes;
        let mut entries = Vec::new();
        // Command ids of tombstones whose order is back but whose update failed: evicted last.
        let mut pending = BTreeSet::new();
        for (index, step) in self.steps.into_iter().enumerate() {
            if let Some(failure) = failed.get(&index) {
                let previous = step.previous.expect("an update comes from an entry");
                let (attempts, deferrals) = match failure {
                    Failure::Counted => (previous.delivery_failures + 1, 0),
                    Failure::Deferred => {
                        (previous.delivery_failures, previous.delivery_deferrals + 1)
                    }
                    Failure::RolledBack => {
                        (previous.delivery_failures, previous.delivery_deferrals)
                    }
                };
                if attempts < MAX_DELIVERY_ATTEMPTS && deferrals < MAX_DELIVERY_DEFERRALS {
                    if previous.vanished {
                        pending.insert(previous.command_id.clone());
                    }
                    entries.push(RunnerEntryV6 {
                        delivery_failures: attempts,
                        delivery_deferrals: deferrals,
                        ..previous
                    });
                    continue;
                }
                notes.push(Note {
                    code: "order_update_abandoned",
                    command_id: previous.command_id.clone(),
                    lost_hundredths: step
                        .update
                        .as_ref()
                        .map_or(0, |update| update.newly_filled_quantity_hundredths),
                });
            }
            if let Some(next) = step.next {
                let (delivery_failures, delivery_deferrals) = if step.update.is_some() {
                    (0, 0)
                } else {
                    (next.delivery_failures, next.delivery_deferrals)
                };
                entries.push(RunnerEntryV6 {
                    delivery_failures,
                    delivery_deferrals,
                    ..next
                });
            }
        }
        entries.extend(issued);
        while entries.iter().filter(|entry| entry.vanished).count() > MAX_TOMBSTONES {
            let oldest = entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.vanished)
                .min_by_key(|(index, entry)| {
                    (
                        pending.contains(&entry.command_id),
                        entry.vanished_revision,
                        *index,
                    )
                })
                .map(|(index, _)| index)
                .expect("a tombstone exists");
            notes.push(Note::new(
                "runner_tombstone_evicted",
                entries.remove(oldest).command_id,
            ));
        }
        (entries, notes)
    }
}

pub(super) fn derive(context: &DecisionContextV6) -> Result<Derived, KernelTransactionError> {
    let section = context
        .kernel_checkpoint
        .as_ref()
        .map(|checkpoint| checkpoint.runner.clone())
        .unwrap_or_default();
    let revision = context.broker.revision;
    let complete = context.orders_complete;
    let orders = &context.broker.orders;
    let by_command = orders
        .iter()
        .map(|order| (order.command_id.as_str(), order))
        .collect::<BTreeMap<_, _>>();
    let by_order_id = orders
        .iter()
        .map(|order| (order.order_id.as_str(), order))
        .collect::<BTreeMap<_, _>>();
    let by_client = orders
        .iter()
        .map(|order| (order.provider_client_id.as_str(), order))
        .collect::<BTreeMap<_, _>>();
    let receipts = context
        .command_receipts
        .iter()
        .map(|receipt| (receipt.command_id.as_str(), receipt))
        .collect::<BTreeMap<_, _>>();

    let mut steps = Vec::new();
    let mut notes = Vec::new();
    for entry in section.entries {
        // The view may predate the command's admission: its absence is no news.
        let may_predate = revision <= entry.issued_broker_revision;
        // What becomes of an entry a complete, newer view does not show.
        let absent = |entry: RunnerEntryV6, notes: &mut Vec<Note>| {
            if may_predate || !complete {
                Some(entry)
            } else if !entry.vanished {
                Some(RunnerEntryV6 {
                    vanished: true,
                    vanished_revision: revision,
                    absent_views: 0,
                    ..entry
                })
            } else if revision <= entry.vanished_revision {
                Some(entry)
            } else if entry.absent_views + 1 >= TOMBSTONE_EXPIRY_VIEWS {
                notes.push(Note::new("runner_tombstone_expired", entry.command_id));
                None
            } else {
                Some(RunnerEntryV6 {
                    absent_views: entry.absent_views + 1,
                    ..entry
                })
            }
        };
        let (next, update) = match entry.kind {
            BrokerCommandKindV6::PlaceOrder => {
                if let Some(order) = by_command.get(entry.command_id.as_str()) {
                    if order.revision < entry.order_revision {
                        // An older record than the one last reported.
                        (Some(entry.clone()), None)
                    } else {
                        let status = seen_status(order, entry.last_status.as_ref());
                        let changed = entry.vanished
                            || entry.last_status.as_ref() != Some(&status)
                            || entry.filled_quantity_hundredths != order.filled_quantity_hundredths;
                        let is_final = status.is_terminal();
                        let update =
                            changed.then(|| order_record(&entry, order, status.clone(), is_final));
                        let next = (!is_final).then(|| RunnerEntryV6 {
                            order_id: Some(order.order_id.clone()),
                            last_status: Some(status),
                            filled_quantity_hundredths: order
                                .filled_quantity_hundredths
                                .max(entry.filled_quantity_hundredths),
                            order_revision: order.revision,
                            vanished: false,
                            vanished_revision: 0,
                            absent_views: 0,
                            ..entry.clone()
                        });
                        (next, update)
                    }
                } else if let Some(CommandOutcomeV6::Refused { code, reason }) = receipts
                    .get(entry.command_id.as_str())
                    .map(|receipt| &receipt.outcome)
                {
                    let status = OrderUpdateStatusV6::Refused {
                        code: code.clone(),
                        reason: reason.clone(),
                    };
                    (None, Some(command_record(&entry, status, None, false)))
                } else {
                    let first_vanish = !entry.vanished && !may_predate && complete;
                    // Report the last status once as final, with nothing remaining.
                    let update = first_vanish.then(|| {
                        let status = entry
                            .last_status
                            .clone()
                            .unwrap_or(OrderUpdateStatusV6::Accepted);
                        command_record(&entry, status, None, true)
                    });
                    (absent(entry.clone(), &mut notes), update)
                }
            }
            BrokerCommandKindV6::CancelOrder | BrokerCommandKindV6::CancelAllOrders => {
                let target = entry
                    .order_id
                    .as_deref()
                    .and_then(|order_id| by_order_id.get(order_id))
                    .or_else(|| {
                        entry
                            .client_order_id
                            .as_deref()
                            .and_then(|client| by_client.get(client))
                    })
                    .copied();
                match receipts
                    .get(entry.command_id.as_str())
                    .map(|receipt| &receipt.outcome)
                {
                    // An admitted cancel shows on its target's own updates.
                    Some(CommandOutcomeV6::Accepted) => (None, None),
                    Some(CommandOutcomeV6::Refused { code, reason }) => {
                        let status = OrderUpdateStatusV6::Refused {
                            code: code.clone(),
                            reason: reason.clone(),
                        };
                        (None, Some(command_record(&entry, status, target, false)))
                    }
                    // A final target tells the story whatever the cancel's outcome.
                    None if entry.kind == BrokerCommandKindV6::CancelOrder
                        && target.is_some_and(|order| order.status.is_terminal()) =>
                    {
                        (None, None)
                    }
                    // Not yet visible: keep waiting for the receipt.
                    None => (absent(entry.clone(), &mut notes), None),
                }
            }
        };
        steps.push(Step {
            previous: Some(entry),
            next,
            update,
        });
    }

    // Every open order of the Sleeve is tracked. The host's view, even truncated, holds every
    // open order (it drops terminal ones only), so the first decision (or the first after
    // converting a V5 checkpoint) records them as seen, from their current status, without
    // updates, and the result acknowledges the terminal ones. Later, an open order the section
    // does not track (its tombstone expired or was evicted) is adopted the same way, and
    // reported, from a view newer than any the section has seen: an older one may show an
    // order the Strategy already saw further along, even final.
    let fresh = !section.seeded || revision > section.newest_view_revision;
    let tracked = steps
        .iter()
        .flat_map(|step| step.previous.iter().chain(step.next.iter()))
        .map(|entry| entry.command_id.clone())
        .collect::<BTreeSet<_>>();
    // Entries the section may hold after delivery: a pruned entry comes back when its update
    // fails.
    let mut held = steps
        .iter()
        .filter(|step| step.next.is_some() || step.update.is_some())
        .count();
    if held > MAX_RUNNER_SECTION_ENTRIES {
        return Err(KernelTransactionError::RunnerSectionFull);
    }
    for order in orders.iter().filter(|_| fresh) {
        if order.status.is_terminal() || tracked.contains(&order.command_id) {
            continue;
        }
        if held >= MAX_RUNNER_SECTION_ENTRIES {
            notes.push(Note::new(
                "runner_order_not_adopted",
                order.command_id.clone(),
            ));
            continue;
        }
        if section.seeded {
            notes.push(Note::new("runner_order_adopted", order.command_id.clone()));
        }
        held += 1;
        steps.push(Step {
            previous: None,
            next: Some(adopted(order, revision)),
            update: None,
        });
    }
    Ok(Derived {
        steps,
        newest_view_revision: section.newest_view_revision.max(revision),
        notes,
    })
}

/// The receipts and terminal orders a result acknowledges: every one the runner section no
/// longer tracks. An entry is pruned only once its outcome was delivered (or abandoned), and
/// the section, seeded by every decision, tracks each open order of the Sleeve until its final
/// update: a terminal order it does not track is no Strategy news.
pub(super) fn acknowledgements(
    context: &DecisionContextV6,
    entries: &[RunnerEntryV6],
) -> Vec<String> {
    let tracked = entries
        .iter()
        .map(|entry| entry.command_id.as_str())
        .collect::<BTreeSet<_>>();
    context
        .command_receipts
        .iter()
        .map(|receipt| receipt.command_id.as_str())
        .filter(|command_id| !tracked.contains(command_id))
        .chain(
            context
                .broker
                .orders
                .iter()
                .filter(|order| order.status.is_terminal())
                .map(|order| order.command_id.as_str())
                .filter(|command_id| !tracked.contains(command_id)),
        )
        .map(str::to_owned)
        .collect()
}

/// Every receipt and terminal order a result could acknowledge, to size the result.
pub(super) fn acknowledgeable(context: &DecisionContextV6) -> Vec<String> {
    context
        .command_receipts
        .iter()
        .map(|receipt| receipt.command_id.clone())
        .chain(
            context
                .broker
                .orders
                .iter()
                .filter(|order| order.status.is_terminal())
                .map(|order| order.command_id.clone()),
        )
        .collect()
}

fn adopted(order: &BrokerOrderV6, revision: u64) -> RunnerEntryV6 {
    RunnerEntryV6 {
        command_id: order.command_id.clone(),
        kind: BrokerCommandKindV6::PlaceOrder,
        client_order_id: Some(order.provider_client_id.clone()),
        order_id: Some(order.order_id.clone()),
        market_id: Some(order.market_id.clone()),
        action: Some(order.action),
        side: Some(order.side),
        requested_quantity_hundredths: order.quantity_hundredths,
        last_status: Some(seen_status(order, None)),
        filled_quantity_hundredths: order.filled_quantity_hundredths,
        order_revision: order.revision,
        issued_broker_revision: revision,
        vanished: false,
        vanished_revision: 0,
        absent_views: 0,
        delivery_failures: 0,
        delivery_deferrals: 0,
    }
}

/// The status a Strategy sees for an order. A cancellation request or a recovery hold is not
/// news of its own: the order keeps its last status (partially filled once anything filled).
pub(super) fn seen_status(
    order: &BrokerOrderV6,
    last: Option<&OrderUpdateStatusV6>,
) -> OrderUpdateStatusV6 {
    match order.status {
        BrokerOrderStatusV6::DurablyAccepted | BrokerOrderStatusV6::Dispatched => {
            OrderUpdateStatusV6::Accepted
        }
        BrokerOrderStatusV6::Resting => OrderUpdateStatusV6::Resting,
        BrokerOrderStatusV6::PartiallyFilled => OrderUpdateStatusV6::PartiallyFilled,
        BrokerOrderStatusV6::Filled => OrderUpdateStatusV6::Filled,
        BrokerOrderStatusV6::Cancelled => OrderUpdateStatusV6::Cancelled,
        BrokerOrderStatusV6::Expired => OrderUpdateStatusV6::Expired,
        BrokerOrderStatusV6::Rejected => OrderUpdateStatusV6::Refused {
            code: PROVIDER_REJECTED_CODE.to_owned(),
            reason: rejection_reason(order),
        },
        BrokerOrderStatusV6::CancellationRequested | BrokerOrderStatusV6::RecoveryRequired => {
            if order.filled_quantity_hundredths > 0 {
                OrderUpdateStatusV6::PartiallyFilled
            } else {
                last.cloned().unwrap_or(OrderUpdateStatusV6::Accepted)
            }
        }
    }
}

/// The provider's text of a rejected order (at most `MAX_REASON_BYTES`, validated); the
/// fixed text when the provider gave none.
fn rejection_reason(order: &BrokerOrderV6) -> String {
    match order.rejection_reason.as_deref() {
        Some(reason) if !reason.is_empty() => reason.to_owned(),
        _ => PROVIDER_REJECTED_REASON.to_owned(),
    }
}

fn order_record(
    entry: &RunnerEntryV6,
    order: &BrokerOrderV6,
    status: OrderUpdateStatusV6,
    is_final: bool,
) -> OrderUpdateRecordV6 {
    OrderUpdateRecordV6 {
        command_id: entry.command_id.clone(),
        kind: entry.kind,
        client_order_id: entry.client_order_id.clone(),
        order_id: Some(order.order_id.clone()),
        market_id: entry.market_id.clone(),
        action: entry.action,
        side: entry.side,
        status,
        requested_quantity_hundredths: order.quantity_hundredths,
        filled_quantity_hundredths: order.filled_quantity_hundredths,
        remaining_quantity_hundredths: order.remaining_quantity_hundredths,
        newly_filled_quantity_hundredths: order
            .filled_quantity_hundredths
            .saturating_sub(entry.filled_quantity_hundredths),
        average_fill_price_micros: order.average_fill_price_micros,
        fees_micros: order.fees_micros,
        is_final,
        vanished: false,
    }
}

/// A final update from what the entry recorded, with the target order's current quantities
/// when the Broker still reports it (else nothing remains).
fn command_record(
    entry: &RunnerEntryV6,
    status: OrderUpdateStatusV6,
    target: Option<&BrokerOrderV6>,
    vanished: bool,
) -> OrderUpdateRecordV6 {
    OrderUpdateRecordV6 {
        command_id: entry.command_id.clone(),
        kind: entry.kind,
        client_order_id: entry.client_order_id.clone(),
        order_id: target
            .map(|order| order.order_id.clone())
            .or_else(|| entry.order_id.clone()),
        market_id: entry.market_id.clone(),
        action: entry.action,
        side: entry.side,
        status,
        requested_quantity_hundredths: target
            .map_or(entry.requested_quantity_hundredths, |order| {
                order.quantity_hundredths
            }),
        filled_quantity_hundredths: target.map_or(entry.filled_quantity_hundredths, |order| {
            order.filled_quantity_hundredths
        }),
        remaining_quantity_hundredths: target
            .map_or(0, |order| order.remaining_quantity_hundredths),
        newly_filled_quantity_hundredths: 0,
        average_fill_price_micros: target.and_then(|order| order.average_fill_price_micros),
        fees_micros: target.map_or(0, |order| order.fees_micros),
        is_final: true,
        vanished,
    }
}
