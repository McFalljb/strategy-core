//! The Decision V6 conformance corpus, `conformance/v6/decision-transactions.json`.
//!
//! Every vector is built here and recorded with its exact bytes, length and SHA-256. Valid
//! contexts and results decode and validate (results against their named context); invalid
//! vectors fail with their recorded category, either when decoded or when validated against
//! their context. The overlay section records the provisional budget arithmetic a Broker
//! reservation must equal. Regenerate with
//! `cargo test -p strategy-core-v3 -- --ignored write_v6_corpus`.

use std::path::PathBuf;

use serde_json::{Value, json};
use strategy_core_kernel::{BrokerFinancialState, ContractQuantity, fees};

use super::tests::*;
use super::*;
use crate::decision_v4::{StationV4, TriggerV4};

const SCHEMA: &str = "strategy-core-decision-v6-corpus/1";

fn corpus_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/v6/decision-transactions.json")
}

fn category(error: &DecisionV6Error) -> &'static str {
    match error {
        DecisionV6Error::Encode => "encode",
        DecisionV6Error::Decode => "decode",
        DecisionV6Error::BoundExceeded => "bound_exceeded",
        DecisionV6Error::TrailingBytes => "trailing_bytes",
        DecisionV6Error::InvalidContract => "invalid_contract",
        DecisionV6Error::DuplicateIdentity => "duplicate_identity",
        DecisionV6Error::NonCanonicalOrder => "noncanonical_order",
        DecisionV6Error::V4(_) => "v4",
    }
}

/// The wire bytes of a value without validating it, for invalid vectors.
fn unchecked<T: Encode>(magic: &[u8; 8], value: &T) -> Vec<u8> {
    let mut bytes = magic.to_vec();
    bytes.extend(bincode::encode_to_vec(value, wire_config()).unwrap());
    bytes
}

fn fence(context: &DecisionContextV6) -> String {
    hex_digest(&decision_fence_v6_sha256(context).unwrap())
}

fn receipts_context() -> DecisionContextV6 {
    let mut context = context();
    let previous = context.kernel_checkpoint.clone().unwrap();
    let refused_place = place(&context, 0, ContractSideV6::No, "fixture-refused");
    let mut refused_entry = place_entry(&refused_place);
    refused_entry.command_id = "command.delivery.daily.0.1".to_owned();
    let mut cancel_entry = command_entry(&StrategyCommandV6::CancelOrder {
        command_id: "command.delivery.daily.0.2".to_owned(),
        target: CancelTargetV6::Order {
            order_id: "order.daily.1".to_owned(),
            expected_order_revision: 2,
        },
    });
    cancel_entry.order_id = Some("order.daily.1".to_owned());
    cancel_entry.client_order_id = Some("dsm-v10-ksea-20260830".to_owned());
    let mut runner = previous.runner.clone();
    runner.entries.extend([refused_entry, cancel_entry]);
    context.kernel_checkpoint = Some(checkpoint(previous.sequence, &previous.state, runner));
    context.command_receipts = vec![
        receipt(
            "command.delivery.daily.0.1",
            BrokerCommandKindV6::PlaceOrder,
            true,
        ),
        receipt(
            "command.delivery.daily.0.2",
            BrokerCommandKindV6::CancelOrder,
            false,
        ),
    ];
    context.trigger = TriggerV6::BrokerState { broker_revision: 9 };
    context
}

/// KBFI and KSEA settle the event; the observation arrives from KBFI, not the primary KSEA.
fn multi_station_context() -> DecisionContextV6 {
    let mut context = context();
    context.owner_state.stations.insert(0, station("KBFI"));
    context.owner_state.opportunity.contributor_stations =
        vec!["KBFI".to_owned(), "KSEA".to_owned()];
    context.owner_state.trigger = TriggerV4::Weather {
        station_id: "KBFI".to_owned(),
        source_generation: 3,
        source_sequence: 61,
    };
    context.trigger = TriggerV6::Owner(OwnerTriggerV6::Observation {
        station_id: "KBFI".to_owned(),
        observed_at_unix_ms: 0,
        component_revision: 0,
        source_generation: 3,
        source_sequence: 61,
    });
    context
}

fn converted_context() -> DecisionContextV6 {
    let mut context = context();
    let mut v5 = KernelCheckpointV5Layout {
        codec_profile: "dsm-reaction-v10-checkpoint".to_owned(),
        codec_version: 1,
        strategy_id: "dsm_reaction_v10".to_owned(),
        strategy_profile: "daily-high".to_owned(),
        profile_and_calculator_digest: DIGEST.to_owned(),
        sequence: 17,
        state: b"v5-kernel-state".to_vec(),
        state_sha256: [0; 32],
    };
    let mut hasher = Sha256::new();
    hasher.update(V5_CHECKPOINT_DIGEST_DOMAIN);
    hash_component(&mut hasher, v5.codec_profile.as_bytes());
    hasher.update(v5.codec_version.to_be_bytes());
    hash_component(&mut hasher, v5.strategy_id.as_bytes());
    hash_component(&mut hasher, v5.strategy_profile.as_bytes());
    hash_component(&mut hasher, v5.profile_and_calculator_digest.as_bytes());
    hasher.update(v5.sequence.to_be_bytes());
    hash_component(&mut hasher, &v5.state);
    v5.state_sha256 = hasher.finalize().into();
    context.kernel_checkpoint = Some(convert_v5_kernel_checkpoint(v5).unwrap());
    context
}

fn live_context() -> DecisionContextV6 {
    let mut context = context();
    context.deployment_mode = DeploymentModeV6::Live;
    context.capabilities = CapabilityGrantV6 {
        timers: false,
        external_requests: vec!["forecast.lookup".to_owned(), "weather.lookup".to_owned()],
    };
    context
}

/// A second order the provider rejected, with its rejection text.
fn provider_rejection_context() -> DecisionContextV6 {
    let mut context = context();
    context.broker.orders.push(BrokerOrderV6 {
        command_id: "command.delivery.daily.0.1".to_owned(),
        intent_id: "intent.daily.2".to_owned(),
        order_id: "order.daily.2".to_owned(),
        provider_order_id: None,
        provider_client_id: "dsm-v10-ksea-20260830-2".to_owned(),
        filled_quantity_hundredths: 0,
        remaining_quantity_hundredths: 300,
        average_fill_price_micros: None,
        reserved_principal_micros: 0,
        fees_micros: 0,
        status: BrokerOrderStatusV6::Rejected,
        rejection_reason: Some("post only cross: the order would take liquidity".to_owned()),
        ..resting_order()
    });
    context
}

fn truncated_context() -> DecisionContextV6 {
    let mut context = context();
    context.orders_complete = false;
    context
}

fn valid_contexts() -> Vec<(&'static str, DecisionContextV6)> {
    vec![
        ("daily-high-recovery", context()),
        ("receipts-broker-state", receipts_context()),
        ("multi-station-observation", multi_station_context()),
        ("converted-v5-checkpoint", converted_context()),
        ("live-request-grants", live_context()),
        ("row-limit-view", row_limit_case().0),
        ("truncated-order-view", truncated_context()),
        ("provider-rejection-reason", provider_rejection_context()),
    ]
}

fn order_updates_result(context: &DecisionContextV6) -> DecisionResultV6 {
    let previous = context.kernel_checkpoint.as_ref().unwrap();
    let refused = &previous.runner.entries[1];
    let record = OrderUpdateRecordV6 {
        command_id: refused.command_id.clone(),
        kind: BrokerCommandKindV6::PlaceOrder,
        client_order_id: refused.client_order_id.clone(),
        order_id: None,
        market_id: refused.market_id.clone(),
        action: refused.action,
        side: refused.side,
        status: OrderUpdateStatusV6::Refused {
            code: "allowance_exceeded".to_owned(),
            reason: "the Sleeve allowance is spent".to_owned(),
        },
        requested_quantity_hundredths: refused.requested_quantity_hundredths,
        filled_quantity_hundredths: 0,
        remaining_quantity_hundredths: 0,
        newly_filled_quantity_hundredths: 0,
        average_fill_price_micros: None,
        fees_micros: 0,
        is_final: true,
        vanished: false,
    };
    DecisionResultV6 {
        delivery_id: context.owner_state.delivery_id.clone(),
        sleeve_identity: context.owner_state.sleeve.sleeve_id.clone(),
        state_fence: fence(context),
        expected_broker_revision: context.broker.revision,
        disposition: DecisionDispositionV6::Completed,
        kernel_checkpoint: Some(checkpoint(
            previous.sequence + 1,
            b"after-updates",
            RunnerSectionV6 {
                seeded: true,
                newest_view_revision: 0,
                entries: previous.runner.entries[..1].to_vec(),
            },
        )),
        commands: vec![],
        acknowledged_command_ids: vec![
            "command.delivery.daily.0.1".to_owned(),
            "command.delivery.daily.0.2".to_owned(),
        ],
        evidence: order_update_evidence(&[record]).unwrap(),
        diagnostics: vec![],
        telemetry: vec![TelemetryEntryV6::Annotation {
            name: "refusal".to_owned(),
            value: AnnotationValueV6::Text("allowance_exceeded".to_owned()),
            fields: vec![],
        }],
    }
}

fn rejected_result(context: &DecisionContextV6) -> DecisionResultV6 {
    DecisionResultV6 {
        disposition: DecisionDispositionV6::Rejected,
        kernel_checkpoint: context.kernel_checkpoint.clone(),
        commands: vec![],
        acknowledged_command_ids: vec![],
        evidence: vec![],
        diagnostics: vec![ResultDiagnosticV6 {
            severity: "error".to_owned(),
            code: "kernel_error".to_owned(),
            message: "fixture failure".to_owned(),
        }],
        telemetry: vec![],
        ..multi_order_result(context)
    }
}

fn converted_result(context: &DecisionContextV6) -> DecisionResultV6 {
    let previous = context.kernel_checkpoint.as_ref().unwrap();
    let order = &context.broker.orders[0];
    // The open V5 order is recorded as seen without an update.
    let adopted = RunnerEntryV6 {
        command_id: order.command_id.clone(),
        kind: BrokerCommandKindV6::PlaceOrder,
        client_order_id: Some(order.provider_client_id.clone()),
        order_id: Some(order.order_id.clone()),
        market_id: Some(order.market_id.clone()),
        action: Some(order.action),
        side: Some(order.side),
        requested_quantity_hundredths: order.quantity_hundredths,
        last_status: Some(OrderUpdateStatusV6::PartiallyFilled),
        filled_quantity_hundredths: order.filled_quantity_hundredths,
        order_revision: order.revision,
        issued_broker_revision: 0,
        vanished: false,
        vanished_revision: 0,
        absent_views: 0,
        delivery_failures: 0,
        delivery_deferrals: 0,
    };
    DecisionResultV6 {
        delivery_id: context.owner_state.delivery_id.clone(),
        sleeve_identity: context.owner_state.sleeve.sleeve_id.clone(),
        state_fence: fence(context),
        expected_broker_revision: context.broker.revision,
        disposition: DecisionDispositionV6::Completed,
        kernel_checkpoint: Some(
            KernelCheckpointV6 {
                sequence: previous.sequence + 1,
                runner: RunnerSectionV6 {
                    seeded: true,
                    newest_view_revision: 0,
                    entries: vec![adopted],
                },
                ..previous.clone()
            }
            .seal(),
        ),
        commands: vec![],
        acknowledged_command_ids: vec![],
        evidence: vec![],
        diagnostics: vec![],
        telemetry: vec![],
    }
}

/// `(id, context id, result)`.
fn valid_results() -> Vec<(&'static str, &'static str, DecisionResultV6)> {
    vec![
        (
            "multi-order-same-decision-cancel",
            "daily-high-recovery",
            multi_order_result(&context()),
        ),
        (
            "order-updates-acknowledged",
            "receipts-broker-state",
            order_updates_result(&receipts_context()),
        ),
        (
            "rejected-keeps-checkpoint",
            "daily-high-recovery",
            rejected_result(&context()),
        ),
        (
            "converted-checkpoint-adopts-open-order",
            "converted-v5-checkpoint",
            converted_result(&converted_context()),
        ),
        (
            "live-cancel-all-expands-per-order",
            "live-request-grants",
            live_cancel_all_result(&live_context()),
        ),
        (
            "live-market-sell-goes-to-the-broker",
            "live-request-grants",
            {
                let context = live_context();
                let mut result = multi_order_result(&context);
                live_market_sell(&context, &mut result);
                result
            },
        ),
    ]
}

/// In live a YES place then a cancel-all: 4 + 5 + (3 for the resting order + 1 for the
/// collapsed place) plan rows.
fn live_cancel_all_result(context: &DecisionContextV6) -> DecisionResultV6 {
    let mut result = multi_order_result(context);
    let cancel_all = StrategyCommandV6::CancelAllOrders {
        command_id: context.command_id(1),
    };
    result.commands = vec![result.commands[0].clone(), cancel_all.clone()];
    let previous = context.kernel_checkpoint.as_ref().unwrap();
    let mut entries = previous.runner.entries.clone();
    entries.push(place_entry(&result.commands[0]));
    entries.push(command_entry(&cancel_all));
    result.kernel_checkpoint = Some(checkpoint(
        previous.sequence + 1,
        b"post-event-kernel-state",
        seeded(entries),
    ));
    result
}

/// A live YES place replaced by a Market sell, without the (ungranted) timer: the Broker
/// refuses it with a receipt, so it counts 5 + 1 plan rows.
fn live_market_sell(context: &DecisionContextV6, result: &mut DecisionResultV6) {
    result.commands.truncate(4);
    let StrategyCommandV6::PlaceOrder(order) = &mut result.commands[0] else {
        unreachable!()
    };
    order.action = OrderActionV6::Sell;
    order.order_type = OrderTypeV6::Market;
    order.limit_price_micros = None;
    order.reduce_only = true;
    result.state_fence = fence(context);
}

type ContextMutation = fn(&mut DecisionContextV6);

fn invalid_contexts() -> Vec<(&'static str, DecisionV6Error, ContextMutation)> {
    vec![
        (
            "receipts-noncanonical-order",
            DecisionV6Error::NonCanonicalOrder,
            |context| {
                *context = receipts_context();
                context.command_receipts.reverse();
            },
        ),
        (
            "receipt-for-an-admitted-place",
            DecisionV6Error::InvalidContract,
            |context| {
                *context = receipts_context();
                context.command_receipts[0].outcome = CommandOutcomeV6::Accepted;
            },
        ),
        (
            "receipt-and-order-for-one-command",
            DecisionV6Error::InvalidContract,
            |context| {
                *context = receipts_context();
                context.command_receipts[0].command_id = "command.delivery.daily.0.0".to_owned();
            },
        ),
        (
            "contributor-station-without-owner-state",
            DecisionV6Error::InvalidContract,
            |context| {
                context
                    .owner_state
                    .opportunity
                    .contributor_stations
                    .push("KBFI".to_owned());
            },
        ),
        (
            "owner-station-not-a-contributor",
            DecisionV6Error::InvalidContract,
            |context| {
                *context = multi_station_context();
                context.owner_state.opportunity.contributor_stations = vec!["KSEA".to_owned()];
            },
        ),
        (
            "runner-entry-already-terminal",
            DecisionV6Error::InvalidContract,
            |context| {
                let mut checkpoint = context.kernel_checkpoint.take().unwrap();
                checkpoint.runner.entries[0].last_status = Some(OrderUpdateStatusV6::Filled);
                context.kernel_checkpoint = Some(checkpoint.seal());
            },
        ),
        (
            "rejection-reason-over-4-kib",
            DecisionV6Error::InvalidContract,
            |context| {
                *context = provider_rejection_context();
                context.broker.orders[1].rejection_reason = Some("r".repeat(MAX_REASON_BYTES + 1));
            },
        ),
        (
            "runner-section-over-bound",
            DecisionV6Error::BoundExceeded,
            |context| {
                let mut checkpoint = context.kernel_checkpoint.take().unwrap();
                let entry = checkpoint.runner.entries[0].clone();
                checkpoint.runner.entries = (0..=MAX_RUNNER_SECTION_ENTRIES)
                    .map(|index| RunnerEntryV6 {
                        command_id: format!("command.old.{index}"),
                        client_order_id: Some(format!("client.old.{index}")),
                        ..entry.clone()
                    })
                    .collect();
                context.kernel_checkpoint = Some(checkpoint.seal());
            },
        ),
        (
            "checkpoint-digest-mismatch",
            DecisionV6Error::InvalidContract,
            |context| {
                context.kernel_checkpoint.as_mut().unwrap().runner.entries[0].order_revision = 3;
            },
        ),
        (
            "stale-broker-state-trigger",
            DecisionV6Error::InvalidContract,
            |context| context.trigger = TriggerV6::BrokerState { broker_revision: 8 },
        ),
        (
            "noncanonical-request-grants",
            DecisionV6Error::NonCanonicalOrder,
            |context| {
                *context = live_context();
                context.capabilities.external_requests.reverse();
            },
        ),
    ]
}

type ResultMutation = fn(&DecisionContextV6, &mut DecisionResultV6);

/// `(id, context id, category, mutation of the context's valid multi-order result)`.
fn invalid_results() -> Vec<(&'static str, &'static str, DecisionV6Error, ResultMutation)> {
    vec![
        (
            "command-ids-out-of-issue-order",
            "daily-high-recovery",
            DecisionV6Error::InvalidContract,
            |_, result| result.commands.swap(0, 1),
        ),
        (
            "same-decision-cancel-before-its-place",
            "daily-high-recovery",
            DecisionV6Error::InvalidContract,
            |context, result| {
                result.commands.swap(0, 2);
                for (ordinal, command) in result.commands.iter_mut().enumerate() {
                    let id = context.command_id(ordinal);
                    match command {
                        StrategyCommandV6::PlaceOrder(order) => order.command_id = id,
                        StrategyCommandV6::CancelOrder { command_id, .. }
                        | StrategyCommandV6::ScheduleTimer { command_id, .. } => *command_id = id,
                        _ => {}
                    }
                }
            },
        ),
        (
            "duplicate-client-order-id",
            "daily-high-recovery",
            DecisionV6Error::DuplicateIdentity,
            |context, result| {
                result.commands[1] = place(context, 1, ContractSideV6::No, "fixture-yes")
            },
        ),
        (
            "cancel-names-a-stale-order-revision",
            "daily-high-recovery",
            DecisionV6Error::InvalidContract,
            |_, result| {
                result.commands[3] = StrategyCommandV6::CancelOrder {
                    command_id: result.commands[3].command_id().to_owned(),
                    target: CancelTargetV6::Order {
                        order_id: "order.daily.1".to_owned(),
                        expected_order_revision: 1,
                    },
                }
            },
        ),
        (
            "timer-with-a-foreign-generation",
            "daily-high-recovery",
            DecisionV6Error::InvalidContract,
            |_, result| {
                if let StrategyCommandV6::ScheduleTimer { generation, .. } = &mut result.commands[4]
                {
                    *generation = "timer.delivery.other".to_owned();
                }
            },
        ),
        (
            "broker-command-untracked-by-the-runner",
            "daily-high-recovery",
            DecisionV6Error::InvalidContract,
            |_, result| {
                let checkpoint = result.kernel_checkpoint.take().unwrap();
                let mut runner = checkpoint.runner.clone();
                runner.entries.pop();
                result.kernel_checkpoint = Some(
                    KernelCheckpointV6 {
                        runner,
                        ..checkpoint
                    }
                    .seal(),
                );
            },
        ),
        (
            "checkpoint-skips-a-sequence",
            "daily-high-recovery",
            DecisionV6Error::InvalidContract,
            |_, result| {
                let checkpoint = result.kernel_checkpoint.take().unwrap();
                result.kernel_checkpoint = Some(
                    KernelCheckpointV6 {
                        sequence: checkpoint.sequence + 1,
                        ..checkpoint
                    }
                    .seal(),
                );
            },
        ),
        (
            "rejected-with-commands",
            "daily-high-recovery",
            DecisionV6Error::InvalidContract,
            |context, result| {
                result.disposition = DecisionDispositionV6::Rejected;
                result.kernel_checkpoint = context.kernel_checkpoint.clone();
            },
        ),
        (
            "acknowledges-an-open-order",
            "daily-high-recovery",
            DecisionV6Error::InvalidContract,
            |_, result| {
                result.acknowledged_command_ids = vec!["command.delivery.daily.0.0".to_owned()]
            },
        ),
        (
            "more-than-64-commands",
            "daily-high-recovery",
            DecisionV6Error::BoundExceeded,
            |context, result| {
                result.commands = (0..=MAX_STRATEGY_COMMANDS)
                    .map(|ordinal| StrategyCommandV6::Stop {
                        command_id: context.command_id(ordinal),
                        reason: "full".to_owned(),
                    })
                    .collect()
            },
        ),
        (
            "plan-rows-over-the-decision-limit",
            "row-limit-view",
            DecisionV6Error::BoundExceeded,
            |_, result| *result = row_limit_case().1,
        ),
        (
            "acknowledges-a-tracked-command",
            "receipts-broker-state",
            DecisionV6Error::InvalidContract,
            |context, result| {
                *result = order_updates_result(context);
                let checkpoint = result.kernel_checkpoint.take().unwrap();
                let previous = context.kernel_checkpoint.as_ref().unwrap();
                let runner = RunnerSectionV6 {
                    seeded: true,
                    newest_view_revision: 0,
                    entries: previous.runner.entries[..2].to_vec(),
                };
                result.kernel_checkpoint = Some(
                    KernelCheckpointV6 {
                        runner,
                        ..checkpoint
                    }
                    .seal(),
                );
            },
        ),
        (
            "cancel-all-over-a-truncated-view",
            "truncated-order-view",
            DecisionV6Error::InvalidContract,
            |context, result| {
                let command = StrategyCommandV6::CancelAllOrders {
                    command_id: context.command_id(4),
                };
                result.commands[4] = command.clone();
                let checkpoint = result.kernel_checkpoint.take().unwrap();
                let mut runner = checkpoint.runner.clone();
                runner.entries.push(command_entry(&command));
                result.kernel_checkpoint = Some(
                    KernelCheckpointV6 {
                        runner,
                        ..checkpoint
                    }
                    .seal(),
                );
            },
        ),
        (
            "timer-without-the-timer-grant",
            "live-request-grants",
            DecisionV6Error::InvalidContract,
            |context, result| result.state_fence = fence(context),
        ),
    ]
}

/// Wire bytes whose length prefix at some level claims 2^62 elements. Each must fail to decode
/// with `Decode` within the decoder's allocation limit, never abort allocating.
fn oversized_lengths() -> Vec<(&'static str, &'static str, Vec<u8>)> {
    const HUGE: u64 = 1 << 62;
    fn put<T: Encode>(bytes: &mut Vec<u8>, value: &T) {
        bytes.extend(bincode::encode_to_vec(value, wire_config()).unwrap());
    }
    let context = context();
    let result = multi_order_result(&context);
    let result_head = |bytes: &mut Vec<u8>| {
        put(bytes, &result.delivery_id);
        put(bytes, &result.sleeve_identity);
        put(bytes, &result.state_fence);
        put(bytes, &result.expected_broker_revision);
        put(bytes, &result.disposition);
    };
    let result_vector = |fill: &dyn Fn(&mut Vec<u8>)| {
        let mut bytes = DECISION_RESULT_V6_MAGIC.to_vec();
        fill(&mut bytes);
        put(&mut bytes, &HUGE);
        bytes
    };
    let context_vector = |fill: &dyn Fn(&mut Vec<u8>)| {
        let mut bytes = DECISION_CONTEXT_V6_MAGIC.to_vec();
        fill(&mut bytes);
        put(&mut bytes, &HUGE);
        bytes
    };
    let owner = &context.owner_state;
    vec![
        (
            "result-delivery-id-length-2-62",
            "decision_result_v6",
            result_vector(&|_| {}),
        ),
        (
            "result-checkpoint-state-length-2-62",
            "decision_result_v6",
            result_vector(&|bytes| {
                result_head(bytes);
                put(bytes, &1_u8);
                let checkpoint = result.kernel_checkpoint.as_ref().unwrap();
                put(bytes, &checkpoint.codec_profile);
                put(bytes, &checkpoint.codec_version);
                put(bytes, &checkpoint.strategy_id);
                put(bytes, &checkpoint.strategy_profile);
                put(bytes, &checkpoint.profile_and_calculator_digest);
                put(bytes, &checkpoint.sequence);
            }),
        ),
        (
            "result-command-list-length-2-62",
            "decision_result_v6",
            result_vector(&|bytes| {
                result_head(bytes);
                put(bytes, &0_u8);
            }),
        ),
        (
            "result-place-market-id-length-2-62",
            "decision_result_v6",
            result_vector(&|bytes| {
                result_head(bytes);
                put(bytes, &0_u8);
                put(bytes, &1_u64);
                put(bytes, &0_u32);
                put(bytes, &result.commands[0].command_id().to_owned());
            }),
        ),
        (
            "result-evidence-payload-length-2-62",
            "decision_result_v6",
            result_vector(&|bytes| {
                result_head(bytes);
                put(bytes, &0_u8);
                put(bytes, &0_u64);
                put(bytes, &0_u64);
                put(bytes, &1_u64);
                put(bytes, &ORDER_UPDATES_EVIDENCE_CODE.to_owned());
            }),
        ),
        (
            "context-delivery-id-length-2-62",
            "decision_context_v6",
            context_vector(&|_| {}),
        ),
        (
            "context-station-list-length-2-62",
            "decision_context_v6",
            context_vector(&|bytes| {
                put(bytes, &owner.delivery_id);
                put(bytes, &owner.sleeve);
                put(bytes, &owner.trigger);
                put(bytes, &owner.fence);
                put(bytes, &owner.config);
            }),
        ),
        (
            "context-receipt-list-length-2-62",
            "decision_context_v6",
            context_vector(&|bytes| {
                put(bytes, &context.owner_state);
                put(bytes, &context.strategy);
                put(bytes, &context.deployment_mode);
                put(bytes, &context.capabilities);
                put(bytes, &context.broker);
            }),
        ),
    ]
}

/// The provisional budget of successive buys: each commitment is the Broker's reservation.
fn overlay_vectors() -> Vec<Value> {
    let start = BrokerFinancialState {
        allowance_limit_micros: 20_000_000,
        current_commitment_micros: 1_800_000,
        provider_available_balance_micros: 50_000_000,
        locally_reserved_cash_micros: 600_000,
    };
    // (id, fee type, multiplier, [(price micros, quantity hundredths)])
    type Case = (&'static str, fees::FeeType, u64, &'static [(u64, i64)]);
    let cases: [Case; 4] = [
        (
            "yes-then-no-quadratic",
            fees::FeeType::Quadratic,
            1_000_000,
            &[(400_000, 300), (550_000, 500)],
        ),
        (
            "maker-fees-half-multiplier",
            fees::FeeType::QuadraticWithMakerFees,
            500_000,
            &[(410_000, 125), (590_000, 200)],
        ),
        (
            "flat-fee",
            fees::FeeType::Flat,
            1_000_000,
            &[(990_000, 1_000)],
        ),
        (
            "market-buy-at-the-payout-cap",
            fees::FeeType::Quadratic,
            1_000_000,
            &[(1_000_000, 100)],
        ),
    ];
    cases
        .into_iter()
        .map(|(id, fee_type, multiplier, orders)| {
            let terms = fees::FeeTerms::new(fee_type, multiplier);
            let mut finances = start;
            let orders = orders
                .iter()
                .map(|(price, quantity)| {
                    let commitment = fees::buy_commitment_micros(
                        *price,
                        ContractQuantity::from_hundredths(*quantity),
                        terms,
                    )
                    .unwrap();
                    finances.current_commitment_micros += commitment;
                    finances.locally_reserved_cash_micros += commitment;
                    json!({
                        "price_micros": price,
                        "quantity_hundredths": quantity,
                        "commitment_micros": commitment,
                        "buying_power_after_micros": finances.buying_power_micros(),
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "id": id,
                "fee_terms": serde_json::to_value(terms).unwrap(),
                "start": {
                    "allowance_limit_micros": start.allowance_limit_micros,
                    "current_commitment_micros": start.current_commitment_micros,
                    "provider_available_balance_micros": start.provider_available_balance_micros,
                    "locally_reserved_cash_micros": start.locally_reserved_cash_micros,
                    "buying_power_micros": start.buying_power_micros(),
                },
                "orders": orders,
            })
        })
        .collect()
}

fn measured(id: &str, bytes: &[u8]) -> serde_json::Map<String, Value> {
    let mut entry = serde_json::Map::new();
    entry.insert("id".to_owned(), json!(id));
    entry.insert("byte_count".to_owned(), json!(bytes.len()));
    entry.insert(
        "sha256".to_owned(),
        json!(hex_digest(&Sha256::digest(bytes))),
    );
    entry.insert("hex".to_owned(), json!(hex_digest(bytes)));
    entry
}

fn corpus() -> Value {
    let contexts = valid_contexts();
    let context_by_id = |id: &str| {
        contexts
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, context)| context.clone())
            .unwrap()
    };
    let mut valid = Vec::new();
    for (id, context) in &contexts {
        let mut entry = measured(id, &encode_decision_context_v6(context).unwrap());
        entry.insert("kind".to_owned(), json!("decision_context_v6"));
        valid.push(Value::Object(entry));
    }
    for (id, context_id, result) in valid_results() {
        let context = context_by_id(context_id);
        let mut entry = measured(id, &encode_decision_result_v6(&result).unwrap());
        entry.insert("kind".to_owned(), json!("decision_result_v6"));
        entry.insert("context".to_owned(), json!(context_id));
        entry.insert(
            "plan_rows".to_owned(),
            json!(decision_plan_rows_v6(&context, &result)),
        );
        valid.push(Value::Object(entry));
    }
    let mut invalid = Vec::new();
    for (id, error, mutate) in invalid_contexts() {
        let mut context = context();
        mutate(&mut context);
        let mut entry = measured(id, &unchecked(DECISION_CONTEXT_V6_MAGIC, &context));
        entry.insert("kind".to_owned(), json!("decision_context_v6"));
        entry.insert("category".to_owned(), json!(category(&error)));
        invalid.push(Value::Object(entry));
    }
    for (id, context_id, error, mutate) in invalid_results() {
        let context = context_by_id(context_id);
        let mut result = multi_order_result(&context);
        mutate(&context, &mut result);
        let mut entry = measured(id, &unchecked(DECISION_RESULT_V6_MAGIC, &result));
        entry.insert("kind".to_owned(), json!("decision_result_v6"));
        entry.insert("context".to_owned(), json!(context_id));
        entry.insert("category".to_owned(), json!(category(&error)));
        invalid.push(Value::Object(entry));
    }
    for (id, kind, bytes) in oversized_lengths() {
        let mut entry = measured(id, &bytes);
        entry.insert("kind".to_owned(), json!(kind));
        entry.insert("category".to_owned(), json!("decode"));
        invalid.push(Value::Object(entry));
    }
    json!({
        "schema": SCHEMA,
        "encoding": "8-byte magic, then bincode 2 (standard, big-endian, variable integers)",
        "valid": valid,
        "invalid": invalid,
        "overlay": overlay_vectors(),
    })
}

fn decode_hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap())
        .collect()
}

#[test]
fn v6_corpus_is_current_and_every_vector_decodes_to_its_verdict() {
    let recorded: Value =
        serde_json::from_str(&std::fs::read_to_string(corpus_path()).unwrap()).unwrap();
    assert_eq!(recorded["schema"], SCHEMA);
    assert_eq!(
        recorded,
        corpus(),
        "regenerate with `cargo test -p strategy-core-v3 -- --ignored write_v6_corpus`"
    );

    let bytes = |entry: &Value| {
        let bytes = decode_hex(entry["hex"].as_str().unwrap());
        assert_eq!(bytes.len() as u64, entry["byte_count"].as_u64().unwrap());
        assert_eq!(
            hex_digest(&Sha256::digest(&bytes)),
            entry["sha256"].as_str().unwrap()
        );
        bytes
    };
    let mut contexts = BTreeMap::new();
    for entry in recorded["valid"].as_array().unwrap() {
        if entry["kind"] == "decision_context_v6" {
            let context = decode_decision_context_v6(&bytes(entry)).unwrap();
            contexts.insert(entry["id"].as_str().unwrap().to_owned(), context);
        }
    }
    for entry in recorded["valid"].as_array().unwrap() {
        if entry["kind"] == "decision_result_v6" {
            let result = decode_decision_result_v6(&bytes(entry)).unwrap();
            let context = &contexts[entry["context"].as_str().unwrap()];
            validate_decision_result_v6(context, &result).unwrap();
            assert_eq!(
                decision_plan_rows_v6(context, &result) as u64,
                entry["plan_rows"].as_u64().unwrap()
            );
        }
    }
    let invalid = recorded["invalid"].as_array().unwrap();
    assert_eq!(invalid.len(), 33);
    for entry in invalid {
        let id = entry["id"].as_str().unwrap();
        let bytes = bytes(entry);
        let error = if entry["kind"] == "decision_context_v6" {
            decode_decision_context_v6(&bytes).unwrap_err()
        } else {
            match decode_decision_result_v6(&bytes) {
                Err(error) => error,
                Ok(result) => validate_decision_result_v6(
                    &contexts[entry["context"].as_str().unwrap()],
                    &result,
                )
                .unwrap_err(),
            }
        };
        assert_eq!(category(&error), entry["category"], "{id}");
    }

    for vector in recorded["overlay"].as_array().unwrap() {
        let terms: fees::FeeTerms = serde_json::from_value(vector["fee_terms"].clone()).unwrap();
        let start = &vector["start"];
        let mut spent = 0;
        for order in vector["orders"].as_array().unwrap() {
            let commitment = fees::buy_commitment_micros(
                order["price_micros"].as_u64().unwrap(),
                ContractQuantity::from_hundredths(order["quantity_hundredths"].as_i64().unwrap()),
                terms,
            )
            .unwrap();
            assert_eq!(commitment, order["commitment_micros"].as_u64().unwrap());
            spent += commitment;
            let finances = BrokerFinancialState {
                allowance_limit_micros: start["allowance_limit_micros"].as_u64().unwrap(),
                current_commitment_micros: start["current_commitment_micros"].as_u64().unwrap()
                    + spent,
                provider_available_balance_micros: start["provider_available_balance_micros"]
                    .as_u64()
                    .unwrap(),
                locally_reserved_cash_micros: start["locally_reserved_cash_micros"]
                    .as_u64()
                    .unwrap()
                    + spent,
            };
            assert_eq!(
                finances.buying_power_micros(),
                order["buying_power_after_micros"].as_u64().unwrap()
            );
        }
    }
}

/// Rewrites `conformance/v6/decision-transactions.json` from the builders above.
#[test]
#[ignore]
fn write_v6_corpus() {
    let mut text = serde_json::to_string_pretty(&corpus()).unwrap();
    text.push('\n');
    std::fs::write(corpus_path(), text).unwrap();
}

#[test]
fn corpus_builders_are_what_the_ids_say() {
    let multi = multi_station_context();
    assert_eq!(multi.contributor_stations(), ["KBFI", "KSEA"]);
    assert_eq!(multi.owner_trigger().unwrap().station_id(), Some("KBFI"));
    assert_ne!(
        multi.strategy.station_id, "KBFI",
        "the trigger is not the primary station"
    );
    assert!(
        converted_context()
            .kernel_checkpoint
            .unwrap()
            .runner
            .entries
            .is_empty()
    );
    let _: StationV4 = station("KSEA");
}
