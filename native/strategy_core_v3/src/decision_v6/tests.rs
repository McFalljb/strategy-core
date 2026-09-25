use super::*;
use crate::decision_v4::{
    BrokerV4, ConfigV4, FenceV4, MarketComparisonV4, MarketIdentityV4, MarketV4, OpportunityV4,
    StationIdentityV4, StationV4, SupervisorV4,
};

pub(super) const MARKET: &str = "KXHIGHTSEA-26AUG30-T80";
pub(super) const DIGEST: &str = "profile-calculator-digest";

pub(super) fn seeded(entries: Vec<RunnerEntryV6>) -> RunnerSectionV6 {
    RunnerSectionV6 {
        seeded: true,
        entries,
        reported: vec![],
    }
}

pub(super) fn checkpoint(
    sequence: u64,
    state: &[u8],
    runner: RunnerSectionV6,
) -> KernelCheckpointV6 {
    KernelCheckpointV6 {
        codec_profile: "dsm-reaction-v10-checkpoint".to_owned(),
        codec_version: 1,
        strategy_id: "dsm_reaction_v10".to_owned(),
        strategy_profile: "daily-high".to_owned(),
        profile_and_calculator_digest: DIGEST.to_owned(),
        sequence,
        state: state.to_vec(),
        runner,
        state_sha256: [0; 32],
    }
    .seal()
}

pub(super) fn station(station_id: &str) -> StationV4 {
    StationV4 {
        climate_event_date: "2026-08-30".to_owned(),
        climate_day_start_utc_unix_ms: 1,
        climate_day_end_utc_unix_ms: 2,
        identity: StationIdentityV4 {
            station_id: station_id.to_owned(),
            logical_location: station_id.to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub(super) fn resting_order() -> BrokerOrderV6 {
    BrokerOrderV6 {
        command_id: "command.delivery.daily.0.0".to_owned(),
        intent_id: "intent.daily.1".to_owned(),
        order_id: "order.daily.1".to_owned(),
        provider_order_id: Some("paper-order-1".to_owned()),
        provider_client_id: "dsm-v10-ksea-20260830".to_owned(),
        market_id: MARKET.to_owned(),
        action: OrderActionV6::Buy,
        side: ContractSideV6::Yes,
        order_type: OrderTypeV6::Limit,
        quantity_hundredths: 300,
        filled_quantity_hundredths: 200,
        remaining_quantity_hundredths: 100,
        limit_price_micros: Some(600_000),
        average_fill_price_micros: Some(590_000),
        reserved_principal_micros: 600_000,
        reserved_fee_micros: 0,
        fees_micros: 20_000,
        rejection_reason: None,
        created_at_unix_ms: Some(1),
        updated_at_unix_ms: Some(2),
        signal_type: Some("dsm_reaction_v10".to_owned()),
        signal_metadata: Some("{}".to_owned()),
        status: BrokerOrderStatusV6::PartiallyFilled,
        revision: 2,
    }
}

/// A daily-high Sleeve on KSEA with one partially filled order and one position.
pub(super) fn context() -> DecisionContextV6 {
    let owner_state = DecisionContextV4 {
        delivery_id: "delivery.daily.1".to_owned(),
        sleeve: SupervisorV4 {
            sleeve_id: derive_sleeve_identity_v6(
                "dsm_reaction_v10",
                "binding.daily.v10",
                "kalshi",
                "KXHIGHTSEA-26AUG30",
            ),
            incarnation: 1,
            process_attempt: 1,
            route_epoch: 1,
        },
        trigger: TriggerV4::Recovery,
        fence: FenceV4 {
            profile_and_calculator_digest: DIGEST.to_owned(),
            route_plan_sha256: [7; 32],
            broker_revision: 9,
            ..Default::default()
        },
        config: ConfigV4 {
            profile_and_calculator_digest: DIGEST.to_owned(),
            ..Default::default()
        },
        stations: vec![station("KSEA")],
        opportunity: OpportunityV4 {
            opportunity_id: "KXHIGHTSEA-26AUG30".to_owned(),
            venue_id: "kalshi".to_owned(),
            match_profile: "daily-high".to_owned(),
            market_ids: vec![MARKET.to_owned()],
            contributor_stations: vec!["KSEA".to_owned()],
            ..Default::default()
        },
        markets: vec![MarketV4 {
            identity: MarketIdentityV4 {
                market_id: MARKET.to_owned(),
                opportunity_id: "KXHIGHTSEA-26AUG30".to_owned(),
                event_ticker: "KXHIGHTSEA-26AUG30".to_owned(),
                fee_type: "quadratic".to_owned(),
                fee_multiplier_millionths: Some(1_000_000),
                ..Default::default()
            },
            revision: 3,
            minutetemp_comparison: Some(MarketComparisonV4 {
                event_date: "2026-08-30".to_owned(),
                ..Default::default()
            }),
            ..Default::default()
        }],
        broker: BrokerV4 {
            revision: 9,
            provider_available_balance: 50_000_000,
            allowance_limit: 20_000_000,
            locally_reserved_cash: 600_000,
            current_commitment: 1_800_000,
            ..Default::default()
        },
        delivered_at_monotonic_ns: 1,
        hard_expires_at_monotonic_ns: 2,
        ..Default::default()
    };
    DecisionContextV6 {
        owner_state,
        strategy: StrategyScopeV6 {
            strategy_id: "dsm_reaction_v10".to_owned(),
            binding_id: "binding.daily.v10".to_owned(),
            profile: "daily-high".to_owned(),
            station_id: "KSEA".to_owned(),
            event_ticker: "KXHIGHTSEA-26AUG30".to_owned(),
            event_date: "2026-08-30".to_owned(),
            market_ids: vec![MARKET.to_owned()],
            parameters: vec![],
            profile_and_calculator_digest: DIGEST.to_owned(),
        },
        deployment_mode: DeploymentModeV6::Paper,
        capabilities: CapabilityGrantV6 {
            timers: true,
            external_requests: Vec::new(),
        },
        broker: BrokerDetailV6 {
            revision: 9,
            reserved_cash_micros: 600_000,
            positions: vec![BrokerPositionV6 {
                market_id: MARKET.to_owned(),
                side: ContractSideV6::Yes,
                quantity_hundredths: 200,
                cost_basis_micros: 1_180_000,
                fees_micros: 20_000,
            }],
            orders: vec![resting_order()],
        },
        command_receipts: Vec::new(),
        orders_complete: true,
        trigger: TriggerV6::Owner(OwnerTriggerV6::Recovery),
        kernel_checkpoint: Some(checkpoint(
            1,
            b"durable-kernel-state",
            RunnerSectionV6 {
                seeded: true,
                reported: vec![],
                entries: vec![RunnerEntryV6 {
                    command_id: "command.delivery.daily.0.0".to_owned(),
                    kind: BrokerCommandKindV6::PlaceOrder,
                    client_order_id: Some("dsm-v10-ksea-20260830".to_owned()),
                    order_id: Some("order.daily.1".to_owned()),
                    market_id: Some(MARKET.to_owned()),
                    action: Some(OrderActionV6::Buy),
                    side: Some(ContractSideV6::Yes),
                    requested_quantity_hundredths: 300,
                    last_status: Some(OrderUpdateStatusV6::PartiallyFilled),
                    filled_quantity_hundredths: 200,
                    order_revision: 2,
                    issued_broker_revision: 0,
                    vanished: false,
                }],
            },
        )),
        decision_time_unix_ms: 1_788_062_400_000,
        supplied: SuppliedInputsV6::default(),
        current_weather: None,
        forecast_issuance: None,
        current_inputs: None,
        market_strikes: None,
    }
}

pub(super) fn place(
    context: &DecisionContextV6,
    ordinal: usize,
    side: ContractSideV6,
    client: &str,
) -> StrategyCommandV6 {
    StrategyCommandV6::PlaceOrder(PlaceOrderV6 {
        command_id: context.command_id(ordinal),
        market_id: MARKET.to_owned(),
        action: OrderActionV6::Buy,
        side,
        order_type: OrderTypeV6::Limit,
        quantity_hundredths: 300,
        limit_price_micros: Some(400_000),
        market_price_cap_micros: None,
        expires_after_ms: Some(30_000),
        reduce_only: false,
        provider_client_id: client.to_owned(),
        signal_type: Some("dsm_reaction_v10".to_owned()),
        signal_metadata: Some("{}".to_owned()),
        metadata: vec![],
    })
}

pub(super) fn place_entry(command: &StrategyCommandV6) -> RunnerEntryV6 {
    let StrategyCommandV6::PlaceOrder(order) = command else {
        panic!("expected a place");
    };
    RunnerEntryV6 {
        command_id: order.command_id.clone(),
        kind: BrokerCommandKindV6::PlaceOrder,
        client_order_id: Some(order.provider_client_id.clone()),
        order_id: None,
        market_id: Some(order.market_id.clone()),
        action: Some(order.action),
        side: Some(order.side),
        requested_quantity_hundredths: order.quantity_hundredths,
        last_status: None,
        filled_quantity_hundredths: 0,
        order_revision: 0,
        issued_broker_revision: 0,
        vanished: false,
    }
}

pub(super) fn command_entry(command: &StrategyCommandV6) -> RunnerEntryV6 {
    RunnerEntryV6 {
        command_id: command.command_id().to_owned(),
        kind: command.broker_kind().unwrap(),
        client_order_id: None,
        order_id: None,
        market_id: None,
        action: None,
        side: None,
        requested_quantity_hundredths: 0,
        last_status: None,
        filled_quantity_hundredths: 0,
        order_revision: 0,
        issued_broker_revision: 0,
        vanished: false,
    }
}

/// YES, then NO, then a cancel of the YES from the same decision, a cancel of the resting
/// order, a timer and a stop: every command kind in one result.
pub(super) fn multi_order_result(context: &DecisionContextV6) -> DecisionResultV6 {
    let yes = place(context, 0, ContractSideV6::Yes, "fixture-yes");
    let no = place(context, 1, ContractSideV6::No, "fixture-no");
    let cancel_same = StrategyCommandV6::CancelOrder {
        command_id: context.command_id(2),
        target: CancelTargetV6::SameDecision {
            provider_client_id: "fixture-yes".to_owned(),
        },
    };
    let cancel_resting = StrategyCommandV6::CancelOrder {
        command_id: context.command_id(3),
        target: CancelTargetV6::Order {
            order_id: "order.daily.1".to_owned(),
            expected_order_revision: 2,
        },
    };
    let timer = StrategyCommandV6::ScheduleTimer {
        command_id: context.command_id(4),
        key: "exit".to_owned(),
        scheduled_at_epoch_ns: 1_788_062_460_000_000_000,
        generation: context.timer_generation(),
        semantics: vec![],
    };
    let previous = context.kernel_checkpoint.as_ref().unwrap();
    let mut entries = previous.runner.entries.clone();
    entries.push(place_entry(&yes));
    entries.push(place_entry(&no));
    entries.push(command_entry(&cancel_same));
    entries.push(command_entry(&cancel_resting));
    DecisionResultV6 {
        delivery_id: context.owner_state.delivery_id.clone(),
        sleeve_identity: context.owner_state.sleeve.sleeve_id.clone(),
        state_fence: hex_digest(&decision_fence_v6_sha256(context).unwrap()),
        expected_broker_revision: context.broker.revision,
        disposition: DecisionDispositionV6::Completed,
        kernel_checkpoint: Some(checkpoint(
            previous.sequence + 1,
            b"post-event-kernel-state",
            seeded(entries),
        )),
        commands: vec![yes, no, cancel_same, cancel_resting, timer],
        acknowledged_command_ids: vec![],
        evidence: vec![],
        diagnostics: vec![],
        telemetry: vec![TelemetryEntryV6::Gauge {
            name: "depth".to_owned(),
            value_bits: 2.5_f64.to_bits(),
            fields: vec![("side".to_owned(), "yes".to_owned())],
        }],
    }
}

#[test]
fn context_and_multi_order_result_round_trip_under_the_v6_magics() {
    let context = context();
    let encoded = encode_decision_context_v6(&context).unwrap();
    assert!(encoded.starts_with(DECISION_CONTEXT_V6_MAGIC));
    assert_eq!(decode_decision_context_v6(&encoded).unwrap(), context);

    let result = multi_order_result(&context);
    validate_decision_result_v6(&context, &result).unwrap();
    let encoded = encode_decision_result_v6(&result).unwrap();
    assert!(encoded.starts_with(DECISION_RESULT_V6_MAGIC));
    assert_eq!(decode_decision_result_v6(&encoded).unwrap(), result);

    let mut trailing = encoded.clone();
    trailing.push(0);
    assert_eq!(
        decode_decision_result_v6(&trailing),
        Err(DecisionV6Error::TrailingBytes)
    );
    let mut wrong_magic = encoded;
    wrong_magic[7] = b'B';
    assert_eq!(
        decode_decision_result_v6(&wrong_magic),
        Err(DecisionV6Error::Decode)
    );
}

#[test]
fn result_contract_violations_are_rejected() {
    let context = context();
    type Mutation = fn(&DecisionContextV6, &mut DecisionResultV6);
    let cases: [(&str, Mutation, DecisionV6Error); 13] = [
        (
            "command ids follow issue order",
            |_, result| result.commands.swap(0, 1),
            DecisionV6Error::InvalidContract,
        ),
        (
            "a same-decision cancel names an earlier place",
            |_, result| result.commands.swap(1, 2),
            DecisionV6Error::InvalidContract,
        ),
        (
            "client ids are unique in a decision",
            |context, result| {
                result.commands[1] = place(context, 1, ContractSideV6::No, "fixture-yes")
            },
            DecisionV6Error::DuplicateIdentity,
        ),
        (
            "a cancel names the order revision the decision saw",
            |_, result| {
                result.commands[3] = StrategyCommandV6::CancelOrder {
                    command_id: result.commands[3].command_id().to_owned(),
                    target: CancelTargetV6::Order {
                        order_id: "order.daily.1".to_owned(),
                        expected_order_revision: 1,
                    },
                }
            },
            DecisionV6Error::InvalidContract,
        ),
        (
            "timers carry the decision's generation",
            |_, result| {
                if let StrategyCommandV6::ScheduleTimer { generation, .. } = &mut result.commands[4]
                {
                    *generation = "timer.other".to_owned();
                }
            },
            DecisionV6Error::InvalidContract,
        ),
        (
            "one timer operation per key",
            |context, result| {
                result.commands.push(StrategyCommandV6::CancelTimer {
                    command_id: context.command_id(5),
                    key: "exit".to_owned(),
                    generation: "timer.delivery.daily.0".to_owned(),
                })
            },
            DecisionV6Error::DuplicateIdentity,
        ),
        (
            "every Broker command is tracked by the runner section",
            |_, result| {
                let checkpoint = result.kernel_checkpoint.as_mut().unwrap();
                checkpoint.runner.entries.pop();
                *checkpoint = checkpoint.clone().seal();
            },
            DecisionV6Error::InvalidContract,
        ),
        (
            "a completed decision advances the checkpoint by exactly one",
            |_, result| {
                let checkpoint = result.kernel_checkpoint.as_mut().unwrap();
                checkpoint.sequence += 1;
                *checkpoint = checkpoint.clone().seal();
            },
            DecisionV6Error::InvalidContract,
        ),
        (
            "a rejected decision carries no commands",
            |_, result| result.disposition = DecisionDispositionV6::Rejected,
            DecisionV6Error::InvalidContract,
        ),
        (
            "acknowledgements name receipts or terminal orders",
            |_, result| {
                result.acknowledged_command_ids = vec!["command.delivery.daily.0.0".to_owned()]
            },
            DecisionV6Error::InvalidContract,
        ),
        (
            "the fence is the decision's Broker revision",
            |_, result| result.expected_broker_revision = 8,
            DecisionV6Error::InvalidContract,
        ),
        (
            "a changed runner section breaks the checkpoint digest",
            |_, result| {
                result.kernel_checkpoint.as_mut().unwrap().runner.entries[0].order_revision = 3
            },
            DecisionV6Error::InvalidContract,
        ),
        (
            "at most 64 commands",
            |context, result| {
                result.commands = (0..=MAX_STRATEGY_COMMANDS)
                    .map(|ordinal| StrategyCommandV6::Stop {
                        command_id: context.command_id(ordinal),
                        reason: "full".to_owned(),
                    })
                    .collect()
            },
            DecisionV6Error::BoundExceeded,
        ),
    ];
    for (name, mutate, expected) in cases {
        let mut result = multi_order_result(&context);
        mutate(&context, &mut result);
        assert_eq!(
            validate_decision_result_v6(&context, &result),
            Err(expected),
            "{name}"
        );
    }

    let mut no_timers = context.clone();
    no_timers.capabilities.timers = false;
    let mut result = multi_order_result(&no_timers);
    result.state_fence = hex_digest(&decision_fence_v6_sha256(&no_timers).unwrap());
    assert_eq!(
        validate_decision_result_v6(&no_timers, &result),
        Err(DecisionV6Error::InvalidContract),
        "timers need the timer capability"
    );
}

#[test]
fn a_rejected_decision_keeps_the_checkpoint_and_acknowledges_nothing() {
    let context = context();
    let mut result = multi_order_result(&context);
    result.disposition = DecisionDispositionV6::Rejected;
    result.commands.clear();
    result.kernel_checkpoint = context.kernel_checkpoint.clone();
    validate_decision_result_v6(&context, &result).unwrap();

    result.kernel_checkpoint = Some(checkpoint(2, b"changed", seeded(vec![])));
    assert_eq!(
        validate_decision_result_v6(&context, &result),
        Err(DecisionV6Error::InvalidContract)
    );
}

pub(super) fn receipt(
    command_id: &str,
    kind: BrokerCommandKindV6,
    refused: bool,
) -> CommandReceiptV6 {
    CommandReceiptV6 {
        command_id: command_id.to_owned(),
        kind,
        outcome: if refused {
            CommandOutcomeV6::Refused {
                code: "allowance_exceeded".to_owned(),
                reason: "the Sleeve allowance is spent".to_owned(),
            }
        } else {
            CommandOutcomeV6::Accepted
        },
    }
}

#[test]
fn receipts_are_canonical_and_acknowledgeable() {
    let mut context = context();
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
    context.validate().unwrap();
    let mut result = multi_order_result(&context);
    result.state_fence = hex_digest(&decision_fence_v6_sha256(&context).unwrap());
    result.acknowledged_command_ids = vec![
        "command.delivery.daily.0.1".to_owned(),
        "command.delivery.daily.0.2".to_owned(),
    ];
    validate_decision_result_v6(&context, &result).unwrap();

    type ContextMutation = fn(&mut DecisionContextV6);
    let invalid: [(&str, ContextMutation, DecisionV6Error); 4] = [
        (
            "receipts are sorted by command id",
            |context| context.command_receipts.reverse(),
            DecisionV6Error::NonCanonicalOrder,
        ),
        (
            "an admitted place has an order record, not a receipt",
            |context| context.command_receipts[0].outcome = CommandOutcomeV6::Accepted,
            DecisionV6Error::InvalidContract,
        ),
        (
            "a command has an order record or a receipt, not both",
            |context| {
                context.command_receipts[0].command_id = "command.delivery.daily.0.0".to_owned()
            },
            DecisionV6Error::InvalidContract,
        ),
        (
            "refusal codes are identifiers",
            |context| {
                context.command_receipts[0].outcome = CommandOutcomeV6::Refused {
                    code: "price moved".to_owned(),
                    reason: "moved".to_owned(),
                }
            },
            DecisionV6Error::InvalidContract,
        ),
    ];
    for (name, mutate, expected) in invalid {
        let mut context = context.clone();
        mutate(&mut context);
        assert_eq!(context.validate(), Err(expected), "{name}");
    }

    let mut bounded = context.clone();
    bounded.command_receipts = (0..=MAX_COMMAND_RECEIPTS)
        .map(|index| {
            receipt(
                &format!("command.old.{index:04}"),
                BrokerCommandKindV6::CancelAllOrders,
                false,
            )
        })
        .collect();
    assert_eq!(bounded.validate(), Err(DecisionV6Error::BoundExceeded));
}

#[test]
fn plan_rows_count_places_cancels_and_cancel_all_over_the_view() {
    let context = context();
    let result = multi_order_result(&context);
    // 4 fixed + 5 + 5 + 3 (same-decision cancel) + 3 (resting cancel); timers cost nothing.
    assert_eq!(decision_plan_rows_v6(&context, &result), 20);

    let mut acknowledged = context.clone();
    acknowledged.command_receipts = vec![receipt(
        "command.old.1",
        BrokerCommandKindV6::CancelOrder,
        false,
    )];
    let mut with_ack = result.clone();
    with_ack.acknowledged_command_ids = vec!["command.old.1".to_owned()];
    assert_eq!(decision_plan_rows_v6(&acknowledged, &with_ack), 21);

    let mut cancel_all = result.clone();
    cancel_all.commands.truncate(2);
    cancel_all
        .commands
        .push(StrategyCommandV6::CancelAllOrders {
            command_id: context.command_id(2),
        });
    // The resting order and both of this decision's places.
    assert_eq!(decision_plan_rows_v6(&context, &cancel_all), 4 + 10 + 3 + 3);

    let mut timers_only = result;
    timers_only.commands = vec![StrategyCommandV6::Stop {
        command_id: context.command_id(0),
        reason: "done".to_owned(),
    }];
    assert_eq!(decision_plan_rows_v6(&context, &timers_only), 0);

    let mut final_target = context.clone();
    let order = &mut final_target.broker.orders[0];
    order.status = BrokerOrderStatusV6::Filled;
    order.filled_quantity_hundredths = 300;
    order.remaining_quantity_hundredths = 0;
    order.reserved_principal_micros = 0;
    final_target.broker.reserved_cash_micros = 0;
    final_target.owner_state.broker.locally_reserved_cash = 0;
    final_target.owner_state.broker.current_commitment = 1_200_000;
    final_target.validate().unwrap();
    let cancel = DecisionResultV6 {
        commands: vec![StrategyCommandV6::CancelOrder {
            command_id: context.command_id(0),
            target: CancelTargetV6::Order {
                order_id: "order.daily.1".to_owned(),
                expected_order_revision: 2,
            },
        }],
        ..multi_order_result(&context)
    };
    assert_eq!(
        decision_plan_rows_v6(&final_target, &cancel),
        4 + REFUSED_COMMAND_PLAN_ROWS,
        "a cancel of a final order is refused"
    );
}

/// 128 resting orders, then 63 places and a cancel-all over all 191 orders: 513 rows.
pub(super) fn row_limit_case() -> (DecisionContextV6, DecisionResultV6) {
    let mut context = context();
    // 64 places cost 4 + 320 rows; a cancel-all over them and 128 resting orders costs more.
    let orders = (0..128)
        .map(|index| BrokerOrderV6 {
            command_id: format!("command.old.{index:03}"),
            intent_id: format!("intent.old.{index:03}"),
            order_id: format!("order.old.{index:03}"),
            provider_order_id: None,
            provider_client_id: format!("client.old.{index:03}"),
            quantity_hundredths: 100,
            filled_quantity_hundredths: 0,
            remaining_quantity_hundredths: 100,
            limit_price_micros: Some(10_000),
            average_fill_price_micros: None,
            reserved_principal_micros: 10_000,
            fees_micros: 0,
            status: BrokerOrderStatusV6::Resting,
            ..resting_order()
        })
        .collect::<Vec<_>>();
    context.broker.orders = orders;
    context.broker.reserved_cash_micros = 1_280_000;
    context.owner_state.broker.locally_reserved_cash = 1_280_000;
    context.owner_state.broker.current_commitment = 1_200_000 + 1_280_000;
    context.kernel_checkpoint = Some(checkpoint(1, b"state", seeded(vec![])));
    context.validate().unwrap();

    let mut commands = (0..63)
        .map(|ordinal| {
            place(
                &context,
                ordinal,
                ContractSideV6::Yes,
                &format!("new.{ordinal}"),
            )
        })
        .collect::<Vec<_>>();
    commands.push(StrategyCommandV6::CancelAllOrders {
        command_id: context.command_id(63),
    });
    let entries = commands
        .iter()
        .map(|command| match command {
            StrategyCommandV6::PlaceOrder(_) => place_entry(command),
            _ => command_entry(command),
        })
        .collect();
    let result = DecisionResultV6 {
        delivery_id: context.owner_state.delivery_id.clone(),
        sleeve_identity: context.owner_state.sleeve.sleeve_id.clone(),
        state_fence: hex_digest(&decision_fence_v6_sha256(&context).unwrap()),
        expected_broker_revision: 9,
        disposition: DecisionDispositionV6::Completed,
        kernel_checkpoint: Some(checkpoint(2, b"state", seeded(entries))),
        commands,
        acknowledged_command_ids: vec![],
        evidence: vec![],
        diagnostics: vec![],
        telemetry: vec![],
    };
    (context, result)
}

#[test]
fn a_result_over_the_decision_row_limit_is_rejected() {
    let (context, result) = row_limit_case();
    assert_eq!(
        decision_plan_rows_v6(&context, &result),
        4 + 63 * 5 + 3 + 128 + 63
    );
    assert_eq!(
        validate_decision_result_v6(&context, &result),
        Err(DecisionV6Error::BoundExceeded)
    );
}

#[test]
fn broker_state_is_a_trigger_of_its_own() {
    let mut context = context();
    context.owner_state.trigger = TriggerV4::Bootstrap;
    context.trigger = TriggerV6::BrokerState { broker_revision: 9 };
    context.validate().unwrap();
    context.trigger = TriggerV6::BrokerState { broker_revision: 8 };
    assert_eq!(context.validate(), Err(DecisionV6Error::InvalidContract));
}

#[test]
fn contributor_stations_are_the_owner_stations() {
    let mut context = context();
    context.owner_state.stations.push(station("KBFI"));
    context
        .owner_state
        .stations
        .sort_by(|left, right| left.identity.station_id.cmp(&right.identity.station_id));
    assert_eq!(context.validate(), Err(DecisionV6Error::InvalidContract));
    context.owner_state.opportunity.contributor_stations =
        vec!["KBFI".to_owned(), "KSEA".to_owned()];
    context.validate().unwrap();
    assert_eq!(context.contributor_stations(), ["KBFI", "KSEA"]);

    context.owner_state.opportunity.contributor_stations =
        vec!["KSEA".to_owned(), "KSEA".to_owned()];
    assert_eq!(context.validate(), Err(DecisionV6Error::DuplicateIdentity));
}

#[test]
fn capabilities_are_canonical() {
    let mut context = context();
    context.capabilities.external_requests = vec!["b.request".to_owned(), "a.request".to_owned()];
    assert_eq!(context.validate(), Err(DecisionV6Error::NonCanonicalOrder));
    context.capabilities.external_requests.sort();
    context.validate().unwrap();
}

#[test]
fn runner_section_entries_are_bounded_and_never_terminal() {
    let context = context();
    let mut runner = context.kernel_checkpoint.clone().unwrap().runner;
    runner.entries[0].last_status = Some(OrderUpdateStatusV6::Filled);
    assert_eq!(
        checkpoint(2, b"state", runner).validate(),
        Err(DecisionV6Error::InvalidContract)
    );
    let entry = context.kernel_checkpoint.unwrap().runner.entries[0].clone();
    let full = RunnerSectionV6 {
        seeded: true,
        reported: vec![],
        entries: (0..=MAX_RUNNER_ENTRIES)
            .map(|index| RunnerEntryV6 {
                command_id: format!("command.{index}"),
                client_order_id: Some(format!("client.{index}")),
                ..entry.clone()
            })
            .collect(),
    };
    assert_eq!(
        checkpoint(2, b"state", full).validate(),
        Err(DecisionV6Error::BoundExceeded)
    );
}

#[test]
fn a_v5_checkpoint_converts_with_its_state_and_an_empty_runner_section() {
    let mut v5 = KernelCheckpointV5Layout {
        codec_profile: "trader-strategies.dsm-reaction-v12.v3".to_owned(),
        codec_version: 3,
        strategy_id: "dsm_reaction_v12".to_owned(),
        strategy_profile: "daily-high".to_owned(),
        profile_and_calculator_digest: DIGEST.to_owned(),
        sequence: 41,
        state: b"v12-private-state".to_vec(),
        state_sha256: [0; 32],
    };
    // The V5 digest: domain, then each field, variable ones length-prefixed.
    let mut hasher = Sha256::new();
    hasher.update(b"strategy-core/decision-v5/checkpoint/v1\0");
    hash_component(&mut hasher, v5.codec_profile.as_bytes());
    hasher.update(v5.codec_version.to_be_bytes());
    hash_component(&mut hasher, v5.strategy_id.as_bytes());
    hash_component(&mut hasher, v5.strategy_profile.as_bytes());
    hash_component(&mut hasher, v5.profile_and_calculator_digest.as_bytes());
    hasher.update(v5.sequence.to_be_bytes());
    hash_component(&mut hasher, &v5.state);
    v5.state_sha256 = hasher.finalize().into();

    // A host decodes its stored bytes into the V5 layout with the codec it wrote them with.
    let stored = bincode::encode_to_vec(&v5, bincode::config::standard()).unwrap();
    let (decoded, _): (KernelCheckpointV5Layout, usize) =
        bincode::decode_from_slice(&stored, bincode::config::standard()).unwrap();
    let converted = convert_v5_kernel_checkpoint(decoded).unwrap();
    assert_eq!(converted.state, b"v12-private-state");
    assert_eq!(converted.sequence, 41);
    assert_eq!(converted.codec_profile, v5.codec_profile);
    assert_eq!(converted.codec_version, 3);
    assert!(converted.runner.entries.is_empty());
    assert_eq!(
        converted.state_sha256,
        kernel_checkpoint_v6_sha256(&converted)
    );
    assert_ne!(
        converted.state_sha256, v5.state_sha256,
        "V6 digests its own domain"
    );

    let mut tampered = v5;
    tampered.state.push(0);
    assert_eq!(
        convert_v5_kernel_checkpoint(tampered),
        Err(DecisionV6Error::InvalidContract)
    );
}

#[test]
fn derived_provider_client_ids_match_the_broker_vector() {
    // traderv3 `admission_contract::canonical_payload_and_provider_client_identity_have_fixed_vectors`.
    let sleeve =
        derive_sleeve_identity_v6("strategy-one", "binding-one", "venue", "opportunity-one");
    assert_eq!(
        derive_provider_client_id_v6(DeploymentModeV6::Paper, &sleeve, 1, "fixed-vector", 0)
            .unwrap(),
        "tv3paper_b6f9a42c4af2a6ad450ccd5a"
    );
    assert!(
        derive_provider_client_id_v6(DeploymentModeV6::Paper, "not-a-digest", 1, "d", 0).is_none()
    );
}

#[test]
fn order_update_evidence_round_trips_in_bounded_chunks() {
    let record = OrderUpdateRecordV6 {
        command_id: "command.delivery.daily.0.0".to_owned(),
        kind: BrokerCommandKindV6::PlaceOrder,
        client_order_id: Some("x".repeat(MAX_IDENTIFIER_BYTES)),
        order_id: Some("y".repeat(MAX_IDENTIFIER_BYTES)),
        market_id: Some(MARKET.to_owned()),
        action: Some(OrderActionV6::Buy),
        side: Some(ContractSideV6::Yes),
        status: OrderUpdateStatusV6::Refused {
            code: "price_moved".to_owned(),
            reason: "r".repeat(MAX_REASON_BYTES),
        },
        requested_quantity_hundredths: 300,
        filled_quantity_hundredths: 0,
        remaining_quantity_hundredths: 0,
        newly_filled_quantity_hundredths: 0,
        average_fill_price_micros: None,
        fees_micros: 0,
        is_final: true,
    };
    let records = vec![record; 40];
    let evidence = order_update_evidence(&records).unwrap();
    assert!(evidence.len() > 1);
    assert!(
        evidence
            .iter()
            .all(|entry| entry.payload.len() <= MAX_EVIDENCE_PAYLOAD_BYTES)
    );
    let decoded = evidence
        .iter()
        .flat_map(|entry| decode_order_update_evidence(entry).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(decoded, records);
}

#[test]
fn a_huge_length_prefix_fails_to_decode_without_allocating() {
    for length in [1_u64 << 62, 1_u64 << 63, u64::MAX] {
        for magic in [DECISION_RESULT_V6_MAGIC, DECISION_CONTEXT_V6_MAGIC] {
            let mut bytes = magic.to_vec();
            bytes.push(0xFD);
            bytes.extend_from_slice(&length.to_be_bytes());
            let error = if magic == DECISION_RESULT_V6_MAGIC {
                decode_decision_result_v6(&bytes).unwrap_err()
            } else {
                decode_decision_context_v6(&bytes).unwrap_err()
            };
            assert_eq!(error, DecisionV6Error::Decode, "{length}");
        }
    }
}

#[test]
fn terminal_orders_that_stopped_early_may_report_nothing_remaining() {
    let markets = [MARKET.to_owned()];
    for status in [
        BrokerOrderStatusV6::Cancelled,
        BrokerOrderStatusV6::Expired,
        BrokerOrderStatusV6::Rejected,
        BrokerOrderStatusV6::Filled,
        BrokerOrderStatusV6::Resting,
    ] {
        let order = BrokerOrderV6 {
            status,
            filled_quantity_hundredths: 100,
            remaining_quantity_hundredths: 0,
            reserved_principal_micros: 0,
            ..resting_order()
        };
        let stopped_early = !matches!(
            status,
            BrokerOrderStatusV6::Filled | BrokerOrderStatusV6::Resting
        );
        assert_eq!(
            validate_broker_order_v6(&order, &markets).is_ok(),
            stopped_early,
            "{status:?}"
        );
    }
    let mut foreign = resting_order();
    foreign.market_id = "KXOTHER".to_owned();
    assert_eq!(
        validate_broker_order_v6(&foreign, &markets),
        Err(DecisionV6Error::InvalidContract),
        "a host can quarantine a record before building the context"
    );
}

#[test]
fn command_ids_are_the_intent_ids_and_unique_across_sleeves() {
    let context = context();
    let sleeve = &context.owner_state.sleeve;
    let intent = intent_id_v6(
        &sleeve.sleeve_id,
        sleeve.incarnation,
        &context.owner_state.delivery_id,
        3,
    )
    .unwrap();
    assert_eq!(
        context.command_id(3),
        format!("command.{}", &hex_digest(&intent)[..32])
    );
    let other_sleeve =
        derive_sleeve_identity_v6("other", "binding", "kalshi", "KXHIGHTSEA-26AUG30");
    let variants = [
        command_id_v6(&sleeve.sleeve_id, 1, "delivery.daily.1", 0),
        command_id_v6(&other_sleeve, 1, "delivery.daily.1", 0),
        command_id_v6(&sleeve.sleeve_id, 2, "delivery.daily.1", 0),
        command_id_v6(&sleeve.sleeve_id, 1, "delivery.daily.2", 0),
        command_id_v6(&sleeve.sleeve_id, 1, "delivery.daily.1", 1),
    ];
    assert_eq!(
        variants.iter().collect::<BTreeSet<_>>().len(),
        variants.len(),
        "Sleeve, incarnation, delivery and ordinal all separate command ids"
    );
    // The derived client order id is the same IntentId's prefix.
    let client = derive_provider_client_id_v6(
        DeploymentModeV6::Paper,
        &sleeve.sleeve_id,
        1,
        "delivery.daily.1",
        3,
    )
    .unwrap();
    assert_eq!(client, format!("tv3paper_{}", &hex_digest(&intent)[..24]));
}

#[test]
fn a_cancel_all_counts_the_orders_open_when_it_is_issued() {
    let context = context();
    let base = multi_order_result(&context);
    let place =
        |ordinal, client: &str| super::tests::place(&context, ordinal, ContractSideV6::Yes, client);
    let cancel_all = |ordinal| StrategyCommandV6::CancelAllOrders {
        command_id: context.command_id(ordinal),
    };
    let cases: [(&str, Vec<StrategyCommandV6>, usize); 3] = [
        (
            "places before a cancel-all are cancelled by it",
            vec![place(0, "a"), place(1, "b"), cancel_all(2)],
            4 + 5 + 5 + (3 + 1 + 2),
        ),
        (
            "a place after a cancel-all is not",
            vec![place(0, "a"), cancel_all(1), place(2, "b")],
            4 + 5 + (3 + 1 + 1) + 5,
        ),
        (
            "a second cancel-all counts only what opened since the first",
            vec![place(0, "a"), cancel_all(1), place(2, "b"), cancel_all(3)],
            4 + 5 + (3 + 1 + 1) + 5 + (3 + 1),
        ),
    ];
    for (name, commands, rows) in cases {
        let result = DecisionResultV6 {
            commands,
            ..base.clone()
        };
        assert_eq!(decision_plan_rows_v6(&context, &result), rows, "{name}");
    }
}

#[test]
fn a_rejection_reason_belongs_to_a_rejected_order_within_its_bound() {
    let markets = [MARKET.to_owned()];
    let rejected = BrokerOrderV6 {
        status: BrokerOrderStatusV6::Rejected,
        filled_quantity_hundredths: 0,
        remaining_quantity_hundredths: 300,
        reserved_principal_micros: 0,
        rejection_reason: Some("r".repeat(MAX_REJECTION_REASON_BYTES)),
        ..resting_order()
    };
    validate_broker_order_v6(&rejected, &markets).unwrap();
    for invalid in [
        BrokerOrderV6 {
            rejection_reason: Some("r".repeat(MAX_REJECTION_REASON_BYTES + 1)),
            ..rejected.clone()
        },
        BrokerOrderV6 {
            rejection_reason: Some(String::new()),
            ..rejected.clone()
        },
        BrokerOrderV6 {
            rejection_reason: Some("rejected".to_owned()),
            ..resting_order()
        },
    ] {
        assert_eq!(
            validate_broker_order_v6(&invalid, &markets),
            Err(DecisionV6Error::InvalidContract)
        );
    }
}

#[test]
fn a_live_cancel_all_counts_a_cancel_per_open_order() {
    let mut context = context();
    context.deployment_mode = DeploymentModeV6::Live;
    context.capabilities.timers = false;
    let base = multi_order_result(&context);
    let place =
        |ordinal, client: &str| super::tests::place(&context, ordinal, ContractSideV6::Yes, client);
    let cancel_all = |ordinal| StrategyCommandV6::CancelAllOrders {
        command_id: context.command_id(ordinal),
    };
    let cases: [(&str, Vec<StrategyCommandV6>, usize); 2] = [
        (
            "3 per open context order and 1 per collapsed own place",
            vec![place(0, "a"), place(1, "b"), cancel_all(2)],
            4 + 5 + 5 + (3 + 1 + 1),
        ),
        (
            "a cancel-all over nothing still writes its receipt",
            vec![cancel_all(0), cancel_all(1)],
            4 + 3 + 1,
        ),
    ];
    for (name, commands, rows) in cases {
        let result = DecisionResultV6 {
            commands,
            ..base.clone()
        };
        assert_eq!(decision_plan_rows_v6(&context, &result), rows, "{name}");
    }
    let mut paper = context.clone();
    paper.deployment_mode = DeploymentModeV6::Paper;
    let result = DecisionResultV6 {
        commands: vec![place(0, "a"), cancel_all(1)],
        ..base
    };
    assert_eq!(decision_plan_rows_v6(&paper, &result), 4 + 5 + (3 + 1 + 1));
    assert_eq!(decision_plan_rows_v6(&context, &result), 4 + 5 + (3 + 1));
}
