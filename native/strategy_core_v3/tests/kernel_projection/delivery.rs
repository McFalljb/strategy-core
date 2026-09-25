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
fn a_refusal_at_a_sleeve_wide_bound_counts_against_the_update() {
    use BrokerOrderStatusV6::*;
    // 192 open orders: the Sleeve is at its open-order cap, whatever the decision holds.
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
    for delivery in 2..5 {
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
        let pending = entry(&result, "command.old.000").unwrap();
        if delivery < 4 {
            assert_eq!(pending.delivery_failures, delivery as u8 - 1, "counted");
            assert_eq!(codes(&result), [("error", "kernel_error")]);
        } else {
            assert_eq!(pending.filled_quantity_hundredths, 100, "abandoned");
            assert_eq!(
                codes(&result),
                [
                    ("error", "order_update_abandoned"),
                    ("error", "kernel_error")
                ]
            );
        }
        previous = result;
    }
    // It fills completely, which frees a slot: the hedge goes out.
    let next = follow_up(&crowded, Some(&previous), 5, orders(300, Filled, 3), vec![]);
    let result = run_transaction(&HedgeFactory, &next).unwrap();
    validate_decision_result_v6(&next, &result).unwrap();
    assert_eq!(state(&result), "200");
    assert_eq!(result.commands.len(), 1);
}

/// Issues the `index`th command of a run; `tag` names the update that issues it.
type Issue = fn(&mut dyn StrategyKernelContext, &str, usize) -> KernelResult<()>;

/// What the scripted kernel does with the refusal that ends a run of commands.
#[derive(Clone, Copy)]
enum OnRefusal {
    /// Stop the run and handle the update.
    Swallow,
    /// Return the refusal (`?`).
    Return,
    /// Catch it and return an error of its own.
    ReturnOther,
}

/// On an update of `client`, issues commands with `issue` (at most `count`) and handles the
/// refusal that ends the run as `on_refusal`; counts every fill it handles.
#[derive(Clone, Copy)]
struct Act {
    client: &'static str,
    issue: Issue,
    count: usize,
    on_refusal: OnRefusal,
}

#[derive(Clone)]
struct Scripted {
    counted: i64,
    acts: &'static [Act],
}

impl NativeKernel for Scripted {
    fn name(&self) -> &str {
        "scripted"
    }
    fn on_event(
        &mut self,
        event: StrategyEventView<'_>,
        context: &mut dyn StrategyKernelContext,
    ) -> KernelResult<()> {
        let StrategyEventView::OrderUpdate(update) = event else {
            return Ok(());
        };
        self.counted += update.newly_filled.hundredths();
        let tag = format!("{}-{}", update.client_order_id, update.filled.hundredths());
        for act in self
            .acts
            .iter()
            .filter(|act| act.client == update.client_order_id)
        {
            for index in 0..act.count {
                if let Err(error) = (act.issue)(context, &tag, index) {
                    match act.on_refusal {
                        OnRefusal::Swallow => break,
                        OnRefusal::Return => return Err(error),
                        OnRefusal::ReturnOther => {
                            return Err(strategy_core_kernel::KernelError::new("gave up"));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

impl TransactionKernel for Scripted {
    fn encode_checkpoint_state(&self) -> Result<Vec<u8>, KernelTransactionError> {
        Ok(self.counted.to_string().into_bytes())
    }
}

struct ScriptedFactory(&'static [Act]);

impl TransactionKernelFactory for ScriptedFactory {
    type Kernel = Scripted;
    fn checkpoint_codec(&self, _: &str) -> Result<KernelCheckpointCodec, KernelTransactionError> {
        Ok(KernelCheckpointCodec {
            profile: "script.checkpoint.v1".to_owned(),
            version: 1,
        })
    }
    fn create(&self, _: &DecisionContextV6) -> Result<Scripted, KernelTransactionError> {
        Ok(Scripted {
            counted: 0,
            acts: self.0,
        })
    }
    fn restore(
        &self,
        _: &DecisionContextV6,
        checkpoint: &KernelCheckpointV6,
    ) -> Result<Scripted, KernelTransactionError> {
        Ok(Scripted {
            counted: String::from_utf8(checkpoint.state.clone())
                .unwrap()
                .parse()
                .unwrap_or(0),
            acts: self.0,
        })
    }
}

fn place_small(
    context: &mut dyn StrategyKernelContext,
    tag: &str,
    index: usize,
) -> KernelResult<()> {
    context
        .broker()
        .place_order(limit_buy(
            &format!("s-{tag}-{index}"),
            ContractSide::No,
            100,
            0.01,
        ))
        .map(drop)
}

/// A place whose metadata takes 60 KiB of the result.
fn place_big(context: &mut dyn StrategyKernelContext, tag: &str, index: usize) -> KernelResult<()> {
    let mut request = limit_buy(&format!("b-{tag}-{index}"), ContractSide::No, 100, 0.01);
    request.signal_metadata = Some("m".repeat(60 * 1024));
    context.broker().place_order(request).map(drop)
}

/// A cancel-all, then small places.
fn cancel_all_then_place(
    context: &mut dyn StrategyKernelContext,
    tag: &str,
    index: usize,
) -> KernelResult<()> {
    if index == 0 {
        context.broker().cancel_all_orders().map(drop)
    } else {
        place_small(context, tag, index)
    }
}

/// `a-first` and `z-target` (issued in that order), and `fillers` more open orders; each
/// view moves both actors along, so both have an update.
fn actors_view(delivery: u32, fillers: usize) -> Vec<BrokerOrderV6> {
    use BrokerOrderStatusV6::*;
    let filled = u64::from(delivery);
    let mut orders = vec![
        order(
            "command.a-first",
            "a-first",
            PartiallyFilled,
            filled,
            filled + 1,
        ),
        order(
            "command.z-target",
            "z-target",
            PartiallyFilled,
            filled,
            filled + 1,
        ),
    ];
    orders.extend((0..fillers).map(|index| {
        order(
            &format!("command.filler.{index:03}"),
            &format!("filler-{index:03}"),
            Resting,
            0,
            1,
        )
    }));
    orders
}

/// Runs `factory` over decisions 1..=`last` of the actors' views; returns each result.
fn run_actors(
    factory: &ScriptedFactory,
    mode: DeploymentModeV6,
    fillers: usize,
    last: u32,
) -> Vec<DecisionResultV6> {
    let mut context = priced_context();
    context.deployment_mode = mode;
    let mut first = follow_up(&context, None, 1, actors_view(0, fillers), vec![]);
    first.trigger = TriggerV6::Owner(OwnerTriggerV6::Recovery);
    let mut results = vec![run_transaction(factory, &first).unwrap()];
    for delivery in 2..=last {
        let next = follow_up(
            &context,
            results.last(),
            delivery,
            actors_view(delivery, fillers),
            vec![],
        );
        let result = run_transaction(factory, &next).unwrap();
        validate_decision_result_v6(&next, &result).unwrap();
        results.push(result);
    }
    results
}

/// The update diagnostics of a result (tombstones of places the views never show aside).
fn update_codes(result: &DecisionResultV6) -> Vec<(&str, &str)> {
    codes(result)
        .into_iter()
        .filter(|(_, code)| !code.starts_with("runner_tombstone"))
        .collect()
}

const COMMANDS: &[Act] = &[
    Act {
        client: "a-first",
        issue: place_small,
        count: 64,
        on_refusal: OnRefusal::Swallow,
    },
    Act {
        client: "z-target",
        issue: place_small,
        count: 1,
        on_refusal: OnRefusal::Return,
    },
];

#[test]
fn a_deferred_update_is_delivered_first_in_the_next_decision() {
    // Decision 2: the first order's update takes all 64 commands, so the target's place is
    // refused for room an earlier update took: deferred. Decision 3: the target goes first.
    let results = run_actors(&ScriptedFactory(COMMANDS), DeploymentModeV6::Paper, 0, 3);
    let deferred = &results[1];
    assert_eq!(deferred.commands.len(), 64);
    assert_eq!(update_codes(deferred), [("warn", "order_update_deferred")]);
    let target = entry(deferred, "command.z-target").unwrap();
    assert_eq!(
        (target.delivery_deferrals, target.delivery_failures),
        (1, 0)
    );
    assert_eq!(
        state(deferred),
        "2",
        "only the first order's fill is handled"
    );

    let delivered = &results[2];
    assert!(
        update_codes(delivered).is_empty(),
        "{:?}",
        delivered.diagnostics
    );
    assert_eq!(
        state(delivered),
        "6",
        "both fills of both orders counted once"
    );
    let target = entry(delivered, "command.z-target").unwrap();
    assert_eq!(
        (target.delivery_deferrals, target.delivery_failures),
        (0, 0)
    );
    // The target's place is the decision's first command; the first order's burst fills the
    // rest.
    let StrategyCommandV6::PlaceOrder(first) = &delivered.commands[0] else {
        panic!("a place");
    };
    assert!(first.provider_client_id.starts_with("s-z-target"));
    assert_eq!(delivered.commands.len(), 64);
    let recorded = delivered
        .evidence
        .iter()
        .flat_map(|evidence| {
            strategy_core_v3::decision_v6::decode_order_update_evidence(evidence).unwrap()
        })
        .map(|record| record.command_id)
        .collect::<Vec<_>>();
    assert_eq!(
        recorded[..2],
        ["command.z-target", "command.a-first"],
        "evidence follows the delivery order"
    );
}

#[test]
fn deferrals_are_abandoned_after_8_in_a_row() {
    // Two deferred updates that each need 40 commands: delivered first, the second is
    // refused for the room the first took, and on its eighth deferral it is abandoned.
    const FORTY: &[Act] = &[
        Act {
            client: "a-first",
            issue: place_small,
            count: 40,
            on_refusal: OnRefusal::Return,
        },
        Act {
            client: "z-target",
            issue: place_small,
            count: 40,
            on_refusal: OnRefusal::Return,
        },
    ];
    let factory = ScriptedFactory(FORTY);
    let results = run_actors(&factory, DeploymentModeV6::Paper, 0, 2);
    let mut checkpoint = results[1].kernel_checkpoint.clone().unwrap();
    let target = entry(&results[1], "command.z-target").unwrap();
    assert_eq!(
        target.delivery_deferrals, 1,
        "deferred behind the first order"
    );
    // Both are pending at their seventh deferral.
    for entry in &mut checkpoint.runner.entries {
        if entry.command_id == "command.a-first" || entry.command_id == "command.z-target" {
            entry.delivery_deferrals = 7;
            entry.last_status = Some(OrderUpdateStatusV6::Resting);
            entry.filled_quantity_hundredths = 0;
        }
    }
    let context = priced_context();
    let mut next = follow_up(&context, None, 3, actors_view(3, 0), vec![]);
    next.kernel_checkpoint = Some(checkpoint.seal());
    next.validate().unwrap();
    let result = run_transaction(&factory, &next).unwrap();
    validate_decision_result_v6(&next, &result).unwrap();
    assert_eq!(result.commands.len(), 40, "the first delivered in full");
    assert_eq!(
        update_codes(&result),
        [
            ("error", "order_update_abandoned"),
            ("warn", "order_update_deferred")
        ]
    );
    let abandoned = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "order_update_abandoned")
        .unwrap();
    assert!(abandoned.message.contains("command.z-target"));
    assert_eq!(
        entry(&result, "command.z-target")
            .unwrap()
            .filled_quantity_hundredths,
        3
    );
}

#[test]
fn a_refusal_for_plan_rows_earlier_updates_took_defers_one_that_needs_them_alone_counts() {
    // Live, 100 more open orders: a cancel-all costs 3 rows per open context order, so the
    // first order's cancel-all and places fill the 512 rows.
    const ROWS: &[Act] = &[
        Act {
            client: "a-first",
            issue: cancel_all_then_place,
            count: 64,
            on_refusal: OnRefusal::Swallow,
        },
        Act {
            client: "z-target",
            issue: place_small,
            count: 1,
            on_refusal: OnRefusal::Return,
        },
    ];
    let results = run_actors(&ScriptedFactory(ROWS), DeploymentModeV6::Live, 100, 2);
    let deferred = &results[1];
    assert!(deferred.commands.len() < 64, "the rows ran out first");
    assert_eq!(update_codes(deferred), [("warn", "order_update_deferred")]);
    assert!(
        deferred
            .diagnostics
            .iter()
            .any(|d| d.message.contains("plan rows"))
    );
    let target = entry(deferred, "command.z-target").unwrap();
    assert_eq!(
        (target.delivery_deferrals, target.delivery_failures),
        (1, 0)
    );

    // An update whose own cancel-all and places pass the rows fails on its own: counted.
    const ALONE: &[Act] = &[Act {
        client: "z-target",
        issue: cancel_all_then_place,
        count: 64,
        on_refusal: OnRefusal::Return,
    }];
    let results = run_actors(&ScriptedFactory(ALONE), DeploymentModeV6::Live, 100, 2);
    let counted = &results[1];
    assert_eq!(update_codes(counted), [("error", "kernel_error")]);
    assert!(counted.diagnostics[0].message.contains("plan rows"));
    let target = entry(counted, "command.z-target").unwrap();
    assert_eq!(
        (target.delivery_deferrals, target.delivery_failures),
        (0, 1)
    );
}

#[test]
fn a_refusal_for_result_bytes_earlier_updates_took_defers_one_that_needs_them_alone_counts() {
    const BYTES: &[Act] = &[
        Act {
            client: "a-first",
            issue: place_big,
            count: 64,
            on_refusal: OnRefusal::Swallow,
        },
        Act {
            client: "z-target",
            issue: place_big,
            count: 1,
            on_refusal: OnRefusal::Return,
        },
    ];
    let results = run_actors(&ScriptedFactory(BYTES), DeploymentModeV6::Paper, 0, 2);
    let deferred = &results[1];
    assert!(deferred.commands.len() < 64, "the bytes ran out first");
    assert_eq!(update_codes(deferred), [("warn", "order_update_deferred")]);
    assert!(
        deferred
            .diagnostics
            .iter()
            .any(|d| d.message.contains("result size"))
    );
    let target = entry(deferred, "command.z-target").unwrap();
    assert_eq!(
        (target.delivery_deferrals, target.delivery_failures),
        (1, 0)
    );

    const ALONE: &[Act] = &[Act {
        client: "z-target",
        issue: place_big,
        count: 64,
        on_refusal: OnRefusal::Return,
    }];
    let results = run_actors(&ScriptedFactory(ALONE), DeploymentModeV6::Paper, 0, 2);
    let counted = &results[1];
    assert_eq!(update_codes(counted), [("error", "kernel_error")]);
    assert!(counted.diagnostics[0].message.contains("result size"));
    let target = entry(counted, "command.z-target").unwrap();
    assert_eq!(
        (target.delivery_deferrals, target.delivery_failures),
        (0, 1)
    );
}

#[test]
fn a_kernel_that_catches_the_refusal_and_returns_its_own_error_counts() {
    const OTHER: &[Act] = &[
        Act {
            client: "a-first",
            issue: place_small,
            count: 64,
            on_refusal: OnRefusal::Swallow,
        },
        Act {
            client: "z-target",
            issue: place_small,
            count: 1,
            on_refusal: OnRefusal::ReturnOther,
        },
    ];
    let results = run_actors(&ScriptedFactory(OTHER), DeploymentModeV6::Paper, 0, 2);
    let counted = &results[1];
    assert_eq!(update_codes(counted), [("error", "kernel_error")]);
    assert!(counted.diagnostics[0].message.contains("gave up"));
    let target = entry(counted, "command.z-target").unwrap();
    assert_eq!(
        (target.delivery_deferrals, target.delivery_failures),
        (0, 1)
    );
}

#[test]
fn an_update_alone_over_a_decision_limit_is_counted_not_deferred() {
    use BrokerOrderStatusV6::*;
    let context = priced_context();
    let placed = decide(&context, place_yes);
    let id = cid(1, 0);
    let factory = BurstFactory {
        burst_on: None,
        fail_snapshot_restore: false,
    };
    let mut previous = placed.result;
    for delivery in 2..5 {
        let next = follow_up(
            &context,
            Some(&previous),
            delivery,
            vec![order(&id, "yes-1", Filled, 300, 2)],
            vec![],
        );
        let result = run_transaction(&factory, &next).unwrap();
        validate_decision_result_v6(&next, &result).unwrap();
        assert_eq!(state(&result), "0", "the fill never reaches the kernel");
        match entry(&result, &id) {
            Some(pending) => {
                assert_eq!(pending.delivery_failures, delivery as u8 - 1);
                assert_eq!(pending.delivery_deferrals, 0);
                assert!(!result.acknowledged_command_ids.contains(&id));
                assert_eq!(codes(&result), [("error", "kernel_error")]);
            }
            None => {
                assert_eq!(delivery, 4, "abandoned after three counted failures");
                assert!(result.acknowledged_command_ids.contains(&id));
                assert_eq!(
                    codes(&result),
                    [
                        ("error", "order_update_abandoned"),
                        ("error", "kernel_error")
                    ]
                );
            }
        }
        previous = result;
    }
}

#[test]
fn a_failed_restore_counts_the_update_and_the_others_are_delivered_once_it_is_abandoned() {
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
    let factory = BurstFactory {
        burst_on: Some("b"),
        fail_snapshot_restore: true,
    };
    let mut previous = placed.result;
    for delivery in 2..6 {
        let orders = ids
            .iter()
            .zip(["a", "b", "c"])
            .map(|(command_id, client)| order(command_id, client, Filled, 300, 2))
            .collect::<Vec<_>>();
        let next = follow_up(&context, Some(&previous), delivery, orders, vec![]);
        let result = run_transaction(&factory, &next).unwrap();
        validate_decision_result_v6(&next, &result).unwrap();
        let failures = ids
            .iter()
            .map(|id| entry(&result, id).map(|entry| entry.delivery_failures))
            .collect::<Vec<_>>();
        match delivery {
            2 | 3 => {
                assert_eq!(failures, [Some(0), Some(delivery as u8 - 1), Some(0)]);
                assert_eq!(state(&result), "0");
                assert_eq!(codes(&result), [("error", "kernel_error")]);
                assert!(
                    result.diagnostics[0]
                        .message
                        .contains("could not be restored")
                );
            }
            // b's third counted failure abandons it; a and c were rolled back again.
            4 => {
                assert_eq!(failures, [Some(0), None, Some(0)]);
                assert_eq!(state(&result), "0");
            }
            // a and c are delivered.
            _ => {
                assert_eq!(failures, [None, None, None]);
                assert_eq!(state(&result), "600");
                assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
            }
        }
        previous = result;
    }
}

/// On the update of `burst_on` (any client when None) places 65 orders with `?`.
struct Burst {
    counted: i64,
    burst_on: Option<&'static str>,
}

impl NativeKernel for Burst {
    fn name(&self) -> &str {
        "burst"
    }
    fn on_event(
        &mut self,
        event: StrategyEventView<'_>,
        context: &mut dyn StrategyKernelContext,
    ) -> KernelResult<()> {
        if let StrategyEventView::OrderUpdate(update) = event {
            self.counted += update.newly_filled.hundredths();
            if self
                .burst_on
                .is_none_or(|client| client == update.client_order_id.as_str())
            {
                for index in 0..65 {
                    context.broker().place_order(limit_buy(
                        &format!("burst-{}-{index}", update.client_order_id),
                        ContractSide::No,
                        100,
                        0.4,
                    ))?;
                }
            }
        }
        Ok(())
    }
}

impl TransactionKernel for Burst {
    fn encode_checkpoint_state(&self) -> Result<Vec<u8>, KernelTransactionError> {
        Ok(self.counted.to_string().into_bytes())
    }
}

struct BurstFactory {
    burst_on: Option<&'static str>,
    /// Mid-decision snapshots (their runner section is empty and unseeded) cannot be restored.
    fail_snapshot_restore: bool,
}

impl TransactionKernelFactory for BurstFactory {
    type Kernel = Burst;
    fn checkpoint_codec(&self, _: &str) -> Result<KernelCheckpointCodec, KernelTransactionError> {
        Ok(KernelCheckpointCodec {
            profile: "script.checkpoint.v1".to_owned(),
            version: 1,
        })
    }
    fn create(&self, _: &DecisionContextV6) -> Result<Burst, KernelTransactionError> {
        Ok(Burst {
            counted: 0,
            burst_on: self.burst_on,
        })
    }
    fn restore(
        &self,
        _: &DecisionContextV6,
        checkpoint: &KernelCheckpointV6,
    ) -> Result<Burst, KernelTransactionError> {
        if self.fail_snapshot_restore && !checkpoint.runner.seeded {
            return Err(KernelTransactionError::Checkpoint(
                "restore failed".to_owned(),
            ));
        }
        Ok(Burst {
            counted: String::from_utf8(checkpoint.state.clone())
                .unwrap()
                .parse()
                .unwrap_or(0),
            burst_on: self.burst_on,
        })
    }
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

#[test]
fn contenders_for_the_same_room_take_turns_and_none_is_abandoned() {
    use BrokerOrderStatusV6::*;
    // Three orders each move every decision, and each update needs 40 of the 64 commands:
    // one fits a decision after another. The most deferred goes first, so the turns rotate.
    const FORTY3: &[Act] = &[
        Act {
            client: "a-first",
            issue: place_small,
            count: 40,
            on_refusal: OnRefusal::Return,
        },
        Act {
            client: "m-mid",
            issue: place_small,
            count: 40,
            on_refusal: OnRefusal::Return,
        },
        Act {
            client: "z-target",
            issue: place_small,
            count: 40,
            on_refusal: OnRefusal::Return,
        },
    ];
    let factory = ScriptedFactory(FORTY3);
    let view = |delivery: u64| {
        ["a-first", "m-mid", "z-target"]
            .into_iter()
            .map(|client| {
                order(
                    &format!("command.{client}"),
                    client,
                    PartiallyFilled,
                    delivery,
                    delivery + 1,
                )
            })
            .collect::<Vec<_>>()
    };
    let context = priced_context();
    let mut first = follow_up(&context, None, 1, view(0), vec![]);
    first.trigger = TriggerV6::Owner(OwnerTriggerV6::Recovery);
    let mut previous = run_transaction(&factory, &first).unwrap();
    for delivery in 2..=20_u32 {
        let next = follow_up(
            &context,
            Some(&previous),
            delivery,
            view(u64::from(delivery)),
            vec![],
        );
        let result = run_transaction(&factory, &next).unwrap();
        validate_decision_result_v6(&next, &result).unwrap();
        assert!(
            update_codes(&result)
                .iter()
                .all(|(_, code)| *code == "order_update_deferred"),
            "delivery {delivery}: {:?}",
            update_codes(&result)
        );
        let deferrals = ["a-first", "m-mid", "z-target"].map(|client| {
            entry(&result, &format!("command.{client}"))
                .unwrap()
                .delivery_deferrals
        });
        assert!(
            deferrals.iter().all(|deferrals| *deferrals <= 2),
            "delivery {delivery}: {deferrals:?}"
        );
        assert_eq!(
            result.commands.len(),
            40,
            "one update's commands per decision"
        );
        previous = result;
    }
}

#[test]
fn a_deferred_cancel_is_never_delivered_before_its_target() {
    use BrokerOrderStatusV6::*;
    const NONE: &[Act] = &[];
    let factory = ScriptedFactory(NONE);
    let context = priced_context();
    let mut first = follow_up(
        &context,
        None,
        1,
        vec![order("command.t", "t", Resting, 0, 1)],
        vec![],
    );
    first.trigger = TriggerV6::Owner(OwnerTriggerV6::Recovery);
    let seeded = run_transaction(&factory, &first).unwrap();
    // A cancel of the order, whose refusal the previous decision deferred.
    let mut checkpoint = seeded.kernel_checkpoint.clone().unwrap();
    let mut cancel = checkpoint.runner.entries[0].clone();
    cancel.command_id = "command.c".to_owned();
    cancel.kind = BrokerCommandKindV6::CancelOrder;
    cancel.delivery_deferrals = 1;
    cancel.last_status = None;
    cancel.issued_broker_revision = 15;
    checkpoint.runner.entries.push(cancel);
    let mut next = follow_up(
        &context,
        None,
        2,
        vec![order("command.t", "t", Filled, 300, 5)],
        vec![CommandReceiptV6 {
            command_id: "command.c".to_owned(),
            kind: BrokerCommandKindV6::CancelOrder,
            outcome: CommandOutcomeV6::Refused {
                code: "order_not_open".to_owned(),
                reason: "filled".to_owned(),
            },
        }],
    );
    next.kernel_checkpoint = Some(checkpoint.seal());
    next.validate().unwrap();
    let result = run_transaction(&factory, &next).unwrap();
    validate_decision_result_v6(&next, &result).unwrap();
    let recorded = result
        .evidence
        .iter()
        .flat_map(|evidence| {
            strategy_core_v3::decision_v6::decode_order_update_evidence(evidence).unwrap()
        })
        .map(|record| record.command_id)
        .collect::<Vec<_>>();
    assert_eq!(
        recorded,
        ["command.t", "command.c"],
        "the fill, then the refusal"
    );
    assert_eq!(state(&result), "300");
}

static UNIQUE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A small place under a client id no other place in the process uses.
fn place_unique(context: &mut dyn StrategyKernelContext, _: &str, _: usize) -> KernelResult<()> {
    let unique = UNIQUE.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    context
        .broker()
        .place_order(limit_buy(
            &format!("u-{unique}"),
            ContractSide::No,
            100,
            0.01,
        ))
        .map(drop)
}

/// A checkpoint of the seeded `orders` plus a cancel of `target` (a cancel-all when none)
/// whose refused update the previous decision deferred.
fn with_deferred_cancel(
    factory: &ScriptedFactory,
    orders: Vec<BrokerOrderV6>,
    target: Option<&str>,
) -> KernelCheckpointV6 {
    let context = priced_context();
    let mut first = follow_up(&context, None, 1, orders, vec![]);
    first.trigger = TriggerV6::Owner(OwnerTriggerV6::Recovery);
    let seeded = run_transaction(factory, &first).unwrap();
    let mut checkpoint = seeded.kernel_checkpoint.unwrap();
    let template = checkpoint.runner.entries[0].clone();
    let cancel = match target {
        Some(client) => strategy_core_v3::decision_v6::RunnerEntryV6 {
            command_id: "command.cancel".to_owned(),
            kind: BrokerCommandKindV6::CancelOrder,
            client_order_id: Some(client.to_owned()),
            order_id: Some(format!("order.{client}")),
            last_status: None,
            issued_broker_revision: 15,
            delivery_deferrals: 1,
            ..template
        },
        None => strategy_core_v3::decision_v6::RunnerEntryV6 {
            command_id: "command.cancel".to_owned(),
            kind: BrokerCommandKindV6::CancelAllOrders,
            client_order_id: None,
            order_id: None,
            market_id: None,
            action: None,
            side: None,
            requested_quantity_hundredths: 0,
            last_status: None,
            filled_quantity_hundredths: 0,
            order_revision: 0,
            issued_broker_revision: 15,
            delivery_deferrals: 1,
            ..template
        },
    };
    checkpoint.runner.entries.push(cancel);
    checkpoint.seal()
}

/// How a cancel's refusal came to count as seen.
struct Seen {
    /// The decision its entry went in.
    delivery: u32,
    /// Its (failures, deferrals) after each earlier decision.
    pending: Vec<(u8, u8)>,
    /// It was abandoned rather than delivered.
    abandoned: bool,
}

/// Runs decisions 2.. over `view(delivery)` with the cancel's refusal receipt until the
/// cancel's entry is gone.
fn run_until_cancel_is_seen(
    factory: &ScriptedFactory,
    checkpoint: KernelCheckpointV6,
    kind: BrokerCommandKindV6,
    view: impl Fn(u32) -> Vec<BrokerOrderV6>,
) -> Seen {
    let context = priced_context();
    let receipt = || {
        vec![CommandReceiptV6 {
            command_id: "command.cancel".to_owned(),
            kind,
            outcome: CommandOutcomeV6::Refused {
                code: "rate_limited".to_owned(),
                reason: "busy".to_owned(),
            },
        }]
    };
    let mut next = follow_up(&context, None, 2, view(2), receipt());
    next.kernel_checkpoint = Some(checkpoint);
    next.validate().unwrap();
    let mut history = Vec::new();
    for delivery in 2..=20_u32 {
        if delivery > 2 {
            let previous = history_result(&history);
            next = follow_up(
                &context,
                Some(previous),
                delivery,
                view(delivery),
                receipt(),
            );
        }
        let result = run_transaction(factory, &next).unwrap();
        validate_decision_result_v6(&next, &result).unwrap();
        let cancel = entry(&result, "command.cancel")
            .map(|cancel| (cancel.delivery_failures, cancel.delivery_deferrals));
        let seen = result
            .acknowledged_command_ids
            .contains(&"command.cancel".to_owned());
        history.push((result, cancel));
        if cancel.is_none() {
            assert!(seen, "the refusal's receipt is acknowledged once seen");
            let (last, _) = history.last().unwrap();
            let abandoned = last.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "order_update_abandoned"
                    && diagnostic.message.contains("command.cancel")
            });
            let pending = history.iter().filter_map(|(_, cancel)| *cancel).collect();
            return Seen {
                delivery,
                pending,
                abandoned,
            };
        }
    }
    panic!("the cancel's refusal never counted as seen");
}

fn history_result(history: &[(DecisionResultV6, Option<(u8, u8)>)]) -> &DecisionResultV6 {
    &history.last().unwrap().0
}

#[test]
fn a_deferred_cancel_whose_unit_never_fits_is_abandoned_after_three_counted_failures() {
    use BrokerOrderStatusV6::*;
    // Every update of `t` (the cancel's refusal names `t` too) places 40 orders: its fill
    // pulled before the deferred cancel takes 40 of the 64 commands. They are one delivery
    // unit that needs 80 commands and never fits, so the cancel's refusal for the room
    // counts, and it is abandoned (by design) after three counted failures instead of being
    // deferred until the deferral bound.
    const FORTY_T: &[Act] = &[Act {
        client: "t",
        issue: place_unique,
        count: 40,
        on_refusal: OnRefusal::Return,
    }];
    let factory = ScriptedFactory(FORTY_T);
    let view = |delivery: u32| {
        let filled = u64::from(delivery);
        vec![order("command.t", "t", PartiallyFilled, filled, filled + 1)]
    };
    let checkpoint = with_deferred_cancel(&factory, view(1), Some("t"));
    let seen =
        run_until_cancel_is_seen(&factory, checkpoint, BrokerCommandKindV6::CancelOrder, view);
    assert!(seen.abandoned, "an 80-command unit never fits: abandoned");
    assert!(
        seen.delivery <= 7,
        "seen in decision {}: {:?}",
        seen.delivery,
        seen.pending
    );
    assert!(
        seen.pending.iter().all(|(_, deferrals)| *deferrals <= 1),
        "never deferred twice in a row: {:?}",
        seen.pending
    );
}

#[test]
fn a_deferred_cancel_is_delivered_once_its_target_stops_moving() {
    use BrokerOrderStatusV6::*;
    const FORTY_T: &[Act] = &[Act {
        client: "t",
        issue: place_unique,
        count: 40,
        on_refusal: OnRefusal::Return,
    }];
    let factory = ScriptedFactory(FORTY_T);
    // `t` moves in decisions 2 and 3, then stays.
    let view = |delivery: u32| {
        let filled = u64::from(delivery.min(3));
        vec![order("command.t", "t", PartiallyFilled, filled, filled + 1)]
    };
    let checkpoint = with_deferred_cancel(&factory, view(1), Some("t"));
    let seen =
        run_until_cancel_is_seen(&factory, checkpoint, BrokerCommandKindV6::CancelOrder, view);
    assert!(!seen.abandoned, "delivered: {:?}", seen.pending);
    assert_eq!(
        seen.delivery, 4,
        "the first decision without a move of its target"
    );
}

#[test]
fn a_deferred_cancel_all_whose_unit_never_fits_is_abandoned_after_three_counted_failures() {
    use BrokerOrderStatusV6::*;
    // Three orders move every decision and each update places 20 orders; a deferred
    // cancel-all pulls all three before it (60 commands), then its own refusal update tries
    // 20 more: an 80-command unit that never fits, so it is abandoned (by design) after
    // three counted failures.
    const TWENTY: &[Act] = &[
        Act {
            client: "a",
            issue: place_unique,
            count: 20,
            on_refusal: OnRefusal::Return,
        },
        Act {
            client: "b",
            issue: place_unique,
            count: 20,
            on_refusal: OnRefusal::Return,
        },
        Act {
            client: "c",
            issue: place_unique,
            count: 20,
            on_refusal: OnRefusal::Return,
        },
        Act {
            client: "",
            issue: place_unique,
            count: 20,
            on_refusal: OnRefusal::Return,
        },
    ];
    let factory = ScriptedFactory(TWENTY);
    let view = |delivery: u32| {
        let filled = u64::from(delivery);
        ["a", "b", "c"]
            .into_iter()
            .map(|client| {
                order(
                    &format!("command.{client}"),
                    client,
                    PartiallyFilled,
                    filled,
                    filled + 1,
                )
            })
            .collect::<Vec<_>>()
    };
    let checkpoint = with_deferred_cancel(&factory, view(1), None);
    let seen = run_until_cancel_is_seen(
        &factory,
        checkpoint,
        BrokerCommandKindV6::CancelAllOrders,
        view,
    );
    assert!(seen.abandoned, "an 80-command unit never fits: abandoned");
    assert!(
        seen.delivery <= 7,
        "seen in decision {}: {:?}",
        seen.delivery,
        seen.pending
    );
    assert!(
        seen.pending.iter().all(|(_, deferrals)| *deferrals <= 1),
        "never deferred twice in a row: {:?}",
        seen.pending
    );
}

#[test]
fn a_deferred_cancel_all_waits_for_a_target_whose_update_failed() {
    use BrokerOrderStatusV6::*;
    // a and b place 30 orders each, c 30 more: in the cancel-all's unit, c's update is
    // refused for room (counted). The cancel-all is not delivered in that decision; it
    // follows c's update in the next.
    const THIRTIES: &[Act] = &[
        Act {
            client: "a",
            issue: place_unique,
            count: 30,
            on_refusal: OnRefusal::Return,
        },
        Act {
            client: "b",
            issue: place_unique,
            count: 30,
            on_refusal: OnRefusal::Return,
        },
        Act {
            client: "c",
            issue: place_unique,
            count: 30,
            on_refusal: OnRefusal::Return,
        },
        Act {
            client: "",
            issue: place_unique,
            count: 4,
            on_refusal: OnRefusal::Return,
        },
    ];
    let factory = ScriptedFactory(THIRTIES);
    // Every order moves in decision 2, then stays.
    let view = |delivery: u32| {
        let filled = u64::from(delivery.min(2));
        ["a", "b", "c"]
            .into_iter()
            .map(|client| {
                order(
                    &format!("command.{client}"),
                    client,
                    PartiallyFilled,
                    filled,
                    filled + 1,
                )
            })
            .collect::<Vec<_>>()
    };
    let checkpoint = with_deferred_cancel(&factory, view(1), None);
    let seen = run_until_cancel_is_seen(
        &factory,
        checkpoint,
        BrokerCommandKindV6::CancelAllOrders,
        view,
    );
    // Decision 2: held, as it was (no failure, no further deferral).
    assert_eq!(seen.pending, [(0, 1)]);
    // Decision 3: c's update, then the cancel-all's.
    assert_eq!(seen.delivery, 3);
    assert!(!seen.abandoned);
}
