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

use std::collections::{BTreeMap, BTreeSet};

use super::KernelTransactionError;
use crate::decision_v6::{
    BrokerCommandKindV6, BrokerOrderStatusV6, BrokerOrderV6, CommandOutcomeV6, DecisionContextV6,
    MAX_DELIVERY_ATTEMPTS, MAX_REJECTION_REASON_BYTES, MAX_RUNNER_ENTRIES, MAX_TOMBSTONES,
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
    /// In issue order; orders recorded by seeding come last.
    pub steps: Vec<Step>,
    /// The section is seeded after this decision.
    pub seeded: bool,
    pub notes: Vec<Note>,
}

impl Step {
    /// The delivery attempt this step's update is on.
    pub fn attempt(&self) -> u8 {
        self.previous
            .as_ref()
            .map_or(1, |entry| entry.delivery_failures + 1)
    }
}

impl Derived {
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
    /// on (the indexes of `failed`) stays as it was, to be delivered again, until
    /// `MAX_DELIVERY_ATTEMPTS`; then its update counts as seen. `issued` are the entries of
    /// the decision's own commands. Tombstones past `MAX_TOMBSTONES` are evicted, oldest
    /// first.
    pub fn finalize(
        self,
        failed: &BTreeSet<usize>,
        issued: Vec<RunnerEntryV6>,
    ) -> (Vec<RunnerEntryV6>, Vec<Note>) {
        let mut notes = self.notes;
        let mut entries = Vec::new();
        for (index, step) in self.steps.into_iter().enumerate() {
            if failed.contains(&index) {
                let previous = step.previous.expect("an update comes from an entry");
                let attempts = previous.delivery_failures + 1;
                if attempts < MAX_DELIVERY_ATTEMPTS {
                    entries.push(RunnerEntryV6 {
                        delivery_failures: attempts,
                        ..previous
                    });
                    continue;
                }
                notes.push(Note {
                    code: "order_update_abandoned",
                    command_id: previous.command_id.clone(),
                });
            }
            if let Some(next) = step.next {
                let delivery_failures = if step.update.is_some() {
                    0
                } else {
                    next.delivery_failures
                };
                entries.push(RunnerEntryV6 {
                    delivery_failures,
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
                .min_by_key(|(index, entry)| (entry.vanished_revision, *index))
                .map(|(index, _)| index)
                .expect("a tombstone exists");
            notes.push(Note {
                code: "runner_tombstone_evicted",
                command_id: entries.remove(oldest).command_id,
            });
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
                notes.push(Note {
                    code: "runner_tombstone_expired",
                    command_id: entry.command_id,
                });
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

    let mut seeded = section.seeded;
    if !seeded {
        if complete {
            // The first decision over a complete view (or the first after converting a V5
            // checkpoint) records open orders as seen, from their current status, without
            // updates. The result acknowledges terminal ones.
            let tracked = steps
                .iter()
                .filter_map(|step| step.next.as_ref())
                .map(|entry| entry.command_id.clone())
                .collect::<BTreeSet<_>>();
            for order in orders {
                if !order.status.is_terminal() && !tracked.contains(&order.command_id) {
                    steps.push(Step {
                        previous: None,
                        next: Some(adopted(order, revision)),
                        update: None,
                    });
                }
            }
            seeded = true;
        } else {
            notes.push(Note {
                code: "runner_not_seeded",
                command_id: String::new(),
            });
        }
    }
    let live = steps
        .iter()
        .filter(|step| step.next.as_ref().is_some_and(RunnerEntryV6::is_live))
        .count();
    if live > MAX_RUNNER_ENTRIES {
        return Err(KernelTransactionError::RunnerSectionFull);
    }
    Ok(Derived {
        steps,
        seeded,
        notes,
    })
}

/// The receipts and terminal orders a result acknowledges: every receipt the runner section
/// no longer tracks (an entry is pruned only once its outcome was delivered), and, once the
/// section is seeded, every terminal order it no longer tracks (it is no Strategy news: the
/// section tracks each of the Strategy's orders until its final update was delivered).
pub(super) fn acknowledgements(
    context: &DecisionContextV6,
    entries: &[RunnerEntryV6],
    seeded: bool,
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
                .filter(|order| seeded && order.status.is_terminal())
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

/// The provider's text of a rejected order, cut to `MAX_REJECTION_REASON_BYTES` on a
/// character boundary; the fixed text when the provider gave none.
fn rejection_reason(order: &BrokerOrderV6) -> String {
    match order.rejection_reason.as_deref() {
        Some(reason) if !reason.is_empty() => {
            let mut end = reason.len().min(MAX_REJECTION_REASON_BYTES);
            while !reason.is_char_boundary(end) {
                end -= 1;
            }
            reason[..end].to_owned()
        }
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
