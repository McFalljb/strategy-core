//! Order updates, derived by comparing the context's Broker state and command receipts with
//! what the checkpoint's runner section last saw.
//!
//! Nothing is queued: a checkpoint that goes back produces the same updates again, and a
//! dropped or coalesced trigger loses nothing because the next comparison sees the change.

use std::collections::{BTreeMap, BTreeSet};

use super::KernelTransactionError;
use crate::decision_v6::{
    BrokerCommandKindV6, BrokerOrderStatusV6, BrokerOrderV6, CommandOutcomeV6, DecisionContextV6,
    MAX_RUNNER_ENTRIES, OrderUpdateRecordV6, OrderUpdateStatusV6, RunnerEntryV6,
};

/// Refusal code of an order the provider rejected after admission.
pub const PROVIDER_REJECTED_CODE: &str = "provider_rejected";

/// The comparison's outcome: the updates to deliver in issue order, the runner entries still
/// tracked afterwards, and the receipts and terminal orders whose outcome the Strategy has now
/// seen.
pub(super) struct Derived {
    pub updates: Vec<OrderUpdateRecordV6>,
    pub entries: Vec<RunnerEntryV6>,
    pub acknowledged: Vec<String>,
}

pub(super) fn derive(context: &DecisionContextV6) -> Result<Derived, KernelTransactionError> {
    let previous = context
        .kernel_checkpoint
        .as_ref()
        .map(|checkpoint| checkpoint.runner.entries.as_slice())
        .unwrap_or_default();
    let orders = &context.broker.orders;
    let by_command = orders
        .iter()
        .map(|order| (order.command_id.as_str(), order))
        .collect::<BTreeMap<_, _>>();
    let by_client = orders
        .iter()
        .map(|order| (order.provider_client_id.as_str(), order))
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
    let mut matched_orders = BTreeSet::new();
    for entry in previous {
        match entry.kind {
            BrokerCommandKindV6::PlaceOrder => {
                let order = by_command.get(entry.command_id.as_str()).or_else(|| {
                    entry
                        .client_order_id
                        .as_deref()
                        .and_then(|client| by_client.get(client))
                });
                if let Some(order) = order {
                    matched_orders.insert(order.order_id.as_str());
                    let status = seen_status(order, entry.last_status.as_ref());
                    let changed = entry.last_status.as_ref() != Some(&status)
                        || entry.filled_quantity_hundredths != order.filled_quantity_hundredths;
                    let is_final = status.is_terminal();
                    if changed {
                        updates.push(order_record(entry, order, status.clone(), is_final));
                    }
                    if !is_final {
                        entries.push(RunnerEntryV6 {
                            order_id: Some(order.order_id.clone()),
                            last_status: Some(status),
                            filled_quantity_hundredths: order.filled_quantity_hundredths,
                            order_revision: order.revision,
                            ..entry.clone()
                        });
                    }
                } else if let Some(receipt) = receipts.get(entry.command_id.as_str()) {
                    // Validation admits only refusals for places.
                    if let CommandOutcomeV6::Refused { code, reason } = &receipt.outcome {
                        updates.push(command_record(
                            entry,
                            OrderUpdateStatusV6::Refused {
                                code: code.clone(),
                                reason: reason.clone(),
                            },
                            None,
                        ));
                    }
                } else {
                    // The Broker no longer reports the order: report the last status once as
                    // final, with nothing remaining.
                    updates.push(command_record(
                        entry,
                        entry
                            .last_status
                            .clone()
                            .unwrap_or(OrderUpdateStatusV6::Accepted),
                        None,
                    ));
                }
            }
            BrokerCommandKindV6::CancelOrder | BrokerCommandKindV6::CancelAllOrders => {
                // An admitted cancel shows on its target's own updates; only a refusal is news.
                if let Some(CommandOutcomeV6::Refused { code, reason }) = receipts
                    .get(entry.command_id.as_str())
                    .map(|receipt| &receipt.outcome)
                {
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
                    updates.push(command_record(
                        entry,
                        OrderUpdateStatusV6::Refused {
                            code: code.clone(),
                            reason: reason.clone(),
                        },
                        target,
                    ));
                }
            }
        }
    }

    // An open order the section does not track (a checkpoint converted from V5, or the first
    // decision) is recorded as seen without an update.
    let tracked_clients = entries
        .iter()
        .filter_map(|entry| entry.client_order_id.clone())
        .collect::<BTreeSet<_>>();
    for order in orders {
        if order.status.is_terminal()
            || matched_orders.contains(order.order_id.as_str())
            || tracked_clients.contains(&order.provider_client_id)
        {
            continue;
        }
        entries.push(RunnerEntryV6 {
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
        });
    }
    if entries.len() > MAX_RUNNER_ENTRIES {
        return Err(KernelTransactionError::RunnerSectionFull);
    }

    let tracked = entries
        .iter()
        .map(|entry| entry.command_id.as_str())
        .collect::<BTreeSet<_>>();
    let acknowledged = context
        .command_receipts
        .iter()
        .map(|receipt| receipt.command_id.as_str())
        .chain(
            orders
                .iter()
                .filter(|order| order.status.is_terminal())
                .map(|order| order.command_id.as_str()),
        )
        .filter(|command_id| !tracked.contains(command_id))
        .map(str::to_owned)
        .collect();
    Ok(Derived {
        updates,
        entries,
        acknowledged,
    })
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
    let requested = target.map_or(entry.requested_quantity_hundredths, |order| {
        order.quantity_hundredths
    });
    let filled = target.map_or(entry.filled_quantity_hundredths, |order| {
        order.filled_quantity_hundredths
    });
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
        requested_quantity_hundredths: requested,
        filled_quantity_hundredths: filled,
        remaining_quantity_hundredths: target
            .map_or(0, |order| order.remaining_quantity_hundredths),
        newly_filled_quantity_hundredths: 0,
        average_fill_price_micros: target.and_then(|order| order.average_fill_price_micros),
        fees_micros: target.map_or(0, |order| order.fees_micros),
        is_final: true,
    }
}
