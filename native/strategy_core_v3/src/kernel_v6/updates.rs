//! Order updates, derived by comparing the context's Broker state and command receipts with
//! what the checkpoint's runner section last saw.
//!
//! Nothing is queued: a checkpoint that goes back produces the same updates again, and a
//! dropped or coalesced trigger loses nothing because the next comparison sees the change.
//! A context that is stale, truncated or reordered never costs a fill: an order is matched by
//! its command id only, an older record of it than the one last reported is ignored, its
//! absence from a view that may predate its admission or that the host truncated is no news,
//! and an order reported as vanished stays tracked so a reappearance reports what it missed.

use std::collections::{BTreeMap, BTreeSet};

use super::KernelTransactionError;
use crate::decision_v6::{
    BrokerCommandKindV6, BrokerOrderStatusV6, BrokerOrderV6, CommandOutcomeV6, DecisionContextV6,
    MAX_BROKER_ORDERS, MAX_RUNNER_ENTRIES, OrderUpdateRecordV6, OrderUpdateStatusV6, RunnerEntryV6,
};

/// Refusal code of an order the provider rejected after admission.
pub const PROVIDER_REJECTED_CODE: &str = "provider_rejected";

/// The comparison's outcome: the updates to deliver in issue order, the runner section's
/// entries and reported terminal orders afterwards, and the receipts and terminal orders the
/// result acknowledges.
pub(super) struct Derived {
    pub updates: Vec<OrderUpdateRecordV6>,
    pub entries: Vec<RunnerEntryV6>,
    pub reported: Vec<String>,
    pub acknowledged: Vec<String>,
}

pub(super) fn derive(context: &DecisionContextV6) -> Result<Derived, KernelTransactionError> {
    let section = context
        .kernel_checkpoint
        .as_ref()
        .map(|checkpoint| checkpoint.runner.clone())
        .unwrap_or_default();
    let seeding = !section.seeded;
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
    let receipts = context
        .command_receipts
        .iter()
        .map(|receipt| (receipt.command_id.as_str(), receipt))
        .collect::<BTreeMap<_, _>>();

    let mut updates = Vec::new();
    let mut entries = Vec::new();
    let mut reported = section.reported.clone();
    for entry in section.entries {
        // The view may predate the command's admission: its absence is no news.
        let may_predate = revision <= entry.issued_broker_revision;
        match entry.kind {
            BrokerCommandKindV6::PlaceOrder => {
                if let Some(order) = by_command.get(entry.command_id.as_str()) {
                    if order.revision < entry.order_revision {
                        // An older record than the one last reported.
                        entries.push(entry);
                        continue;
                    }
                    let status = seen_status(order, entry.last_status.as_ref());
                    let changed = entry.vanished
                        || entry.last_status.as_ref() != Some(&status)
                        || entry.filled_quantity_hundredths != order.filled_quantity_hundredths;
                    let is_final = status.is_terminal();
                    if changed {
                        updates.push(order_record(&entry, order, status.clone(), is_final));
                    }
                    if is_final {
                        reported.push(entry.command_id.clone());
                    } else {
                        entries.push(RunnerEntryV6 {
                            order_id: Some(order.order_id.clone()),
                            last_status: Some(status),
                            filled_quantity_hundredths: order
                                .filled_quantity_hundredths
                                .max(entry.filled_quantity_hundredths),
                            order_revision: order.revision,
                            vanished: false,
                            ..entry
                        });
                    }
                } else if let Some(CommandOutcomeV6::Refused { code, reason }) = receipts
                    .get(entry.command_id.as_str())
                    .map(|receipt| &receipt.outcome)
                {
                    updates.push(command_record(
                        &entry,
                        OrderUpdateStatusV6::Refused {
                            code: code.clone(),
                            reason: reason.clone(),
                        },
                        None,
                    ));
                } else if may_predate || !complete || entry.vanished {
                    entries.push(entry);
                } else {
                    // A complete, newer view no longer shows the order: report the last status
                    // once as final, with nothing remaining, and keep the entry as a tombstone.
                    updates.push(command_record(
                        &entry,
                        entry
                            .last_status
                            .clone()
                            .unwrap_or(OrderUpdateStatusV6::Accepted),
                        None,
                    ));
                    entries.push(RunnerEntryV6 {
                        vanished: true,
                        ..entry
                    });
                }
            }
            BrokerCommandKindV6::CancelOrder | BrokerCommandKindV6::CancelAllOrders => {
                match receipts
                    .get(entry.command_id.as_str())
                    .map(|receipt| &receipt.outcome)
                {
                    // An admitted cancel shows on its target's own updates.
                    Some(CommandOutcomeV6::Accepted) => {}
                    Some(CommandOutcomeV6::Refused { code, reason }) => {
                        let target = entry
                            .order_id
                            .as_deref()
                            .and_then(|order_id| by_order_id.get(order_id))
                            .copied();
                        updates.push(command_record(
                            &entry,
                            OrderUpdateStatusV6::Refused {
                                code: code.clone(),
                                reason: reason.clone(),
                            },
                            target,
                        ));
                    }
                    // Not yet visible, or already acknowledged in a durable write.
                    None if may_predate => entries.push(entry),
                    None => {}
                }
            }
        }
    }

    let mut acknowledged_terminal = Vec::new();
    if seeding {
        // The first decision (or the first after converting a V5 checkpoint) records the
        // Broker state as seen: open orders are tracked from their current status and
        // terminal orders are acknowledged, without updates.
        let tracked = entries
            .iter()
            .map(|entry| entry.command_id.clone())
            .collect::<BTreeSet<_>>();
        for order in orders {
            if tracked.contains(&order.command_id) || reported.contains(&order.command_id) {
                continue;
            }
            if order.status.is_terminal() {
                acknowledged_terminal.push(order.command_id.clone());
            } else {
                entries.push(adopted(order, revision));
            }
        }
    }
    if entries.len() > MAX_RUNNER_ENTRIES {
        return Err(KernelTransactionError::RunnerSectionFull);
    }

    reported.extend(acknowledged_terminal);
    // A reported order stays until a complete view no longer shows it (its acknowledgement
    // reached a durable write).
    if complete {
        reported.retain(|command_id| by_command.contains_key(command_id.as_str()));
    }
    let mut seen = BTreeSet::new();
    reported.retain(|command_id| seen.insert(command_id.clone()));
    if reported.len() > MAX_BROKER_ORDERS {
        reported.drain(..reported.len() - MAX_BROKER_ORDERS);
    }

    let tracked = entries
        .iter()
        .map(|entry| entry.command_id.as_str())
        .collect::<BTreeSet<_>>();
    let reported_ids = reported.iter().map(String::as_str).collect::<BTreeSet<_>>();
    // Receipts are only ever written for this Sleeve's commands, and an entry is pruned only
    // once its outcome was reported, so an untracked receipt has been seen.
    let acknowledged = context
        .command_receipts
        .iter()
        .map(|receipt| receipt.command_id.as_str())
        .filter(|command_id| !tracked.contains(command_id))
        .chain(
            orders
                .iter()
                .filter(|order| order.status.is_terminal())
                .map(|order| order.command_id.as_str())
                .filter(|command_id| reported_ids.contains(command_id)),
        )
        .map(str::to_owned)
        .collect();
    Ok(Derived {
        updates,
        entries,
        reported,
        acknowledged,
    })
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
            reason: "the provider rejected the order".to_owned(),
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
    }
}

/// A final update from what the entry recorded, with the target order's current quantities
/// when the Broker still reports it (else nothing remains).
fn command_record(
    entry: &RunnerEntryV6,
    status: OrderUpdateStatusV6,
    target: Option<&BrokerOrderV6>,
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
    }
}
