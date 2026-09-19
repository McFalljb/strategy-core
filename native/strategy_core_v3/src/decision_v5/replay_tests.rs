fn replay_corpus_context(
    mut context: DecisionContextV5,
    calls: u64,
) -> Result<DecisionContextV5, DecisionV5Error> {
    use crate::replay_v5::{
        BrokerExecutionStateV5, BrokerFinancialState, broker_outcome_context_v5,
    };

    for generation in 1..=calls {
        let mut result = awaiting_result_for(&context);
        let continuation_id = format!("continuation.replay.{generation}");
        let command_id = format!("command.replay.{generation}");
        result.expected_broker_revision = context.admission_broker().revision;
        result.disposition = DecisionDispositionV5::AwaitingBrokerOutcome {
            continuation_id: continuation_id.clone(),
            continuation_generation: generation,
            awaited_command_id: command_id.clone(),
        };
        let StrategyCommandV5::PlaceOrder(order) = &mut result.commands[0] else {
            unreachable!()
        };
        order.command_id = command_id.clone();
        order.provider_client_id = format!("client.replay.{generation}");
        order.fence = CommandFenceV5 {
            continuation_id: continuation_id.clone(),
            continuation_generation: generation,
            expected_broker_revision: result.expected_broker_revision,
        };
        let commitment = continuation_commitment_v5(&context, &result)?.unwrap();
        // Refusals have no financial effects. Every publication explicitly retains the same
        // exact balances and holdings; no financial value is inferred from an execution return.
        let mut returned_state = context
            .broker_replay
            .as_ref()
            .map(|history| history.returned_state.clone())
            .unwrap_or_else(|| BrokerExecutionStateV5 {
                broker: context.broker.clone(),
                finances: BrokerFinancialState {
                    allowance_limit_micros: context.owner_state.broker.allowance_limit,
                    current_commitment_micros: context.owner_state.broker.current_commitment,
                    provider_available_balance_micros: context
                        .owner_state
                        .broker
                        .provider_available_balance,
                    locally_reserved_cash_micros: context.owner_state.broker.locally_reserved_cash,
                },
            });
        returned_state.broker.revision += 1;
        let outcome = BrokerOutcomeV5 {
            outcome_id: format!("outcome.replay.{generation}"),
            continuation_id,
            continuation_generation: generation,
            command_id,
            command_kind: BrokerCommandKindV5::PlaceOrder,
            transition_sequence: 1,
            target_order_id: None,
            order_id: None,
            intent_id: None,
            provider_order_id: None,
            provider_client_id: None,
            status: BrokerOutcomeStatusV5::Rejected,
            return_value: BrokerCommandReturnV5::PlaceOrder(PlaceOrderReturnV5::Err(
                KernelBrokerErrorV5 {
                    code: "fixture_refusal".to_owned(),
                    message: "fixture refusal".to_owned(),
                    retryable: false,
                },
            )),
            requested_quantity_hundredths: 0,
            filled_quantity_hundredths: 0,
            remaining_quantity_hundredths: 0,
            average_fill_price_micros: None,
            reason: Some("fixture refusal".to_owned()),
            updated_at_unix_ms: context.decision_time_unix_ms,
            broker_revision: returned_state.broker.revision,
        };
        context = broker_outcome_context_v5(&context, &commitment, outcome, returned_state)?;
    }
    Ok(context)
}

fn cancelled_place_corpus_context(filled: bool) -> DecisionContextV5 {
    let mut context = replay_corpus_context(context(), 1).unwrap();
    let TriggerV5::BrokerOutcome { outcome, .. } = &mut context.trigger else {
        unreachable!()
    };
    let returned = &mut context.broker_replay.as_mut().unwrap().returned_state;
    let mut order = returned.broker.orders[0].clone();
    order.command_id = outcome.command_id.clone();
    order.order_id = "order.replay.cancelled.place".to_owned();
    order.intent_id = "intent.cancelled.place".to_owned();
    order.provider_order_id = Some("paper-cancelled-place".to_owned());
    order.provider_client_id = "client.replay.1".to_owned();
    order.side = ContractSideV5::No;
    order.limit_price_micros = Some(400_000);
    order.filled_quantity_hundredths = if filled { 200 } else { 0 };
    order.remaining_quantity_hundredths = 0;
    order.average_fill_price_micros = filled.then_some(400_000);
    order.reserved_principal_micros = 0;
    order.status = BrokerOrderStatusV5::Cancelled;
    let fees_micros = if filled { 33_600 } else { 0 };
    if filled {
        returned.broker.positions.push(BrokerPositionV5 {
            market_id: order.market_id.clone(),
            side: order.side,
            quantity_hundredths: 200,
            cost_basis_micros: 800_000,
            fees_micros,
        });
        returned.finances.current_commitment_micros += 833_600;
    }
    outcome.order_id = Some(order.order_id.clone());
    outcome.intent_id = Some(order.intent_id.clone());
    outcome.provider_order_id = order.provider_order_id.clone();
    outcome.provider_client_id = Some(order.provider_client_id.clone());
    outcome.status = BrokerOutcomeStatusV5::Cancelled;
    outcome.requested_quantity_hundredths = order.quantity_hundredths;
    outcome.filled_quantity_hundredths = order.filled_quantity_hundredths;
    outcome.remaining_quantity_hundredths = 0;
    outcome.average_fill_price_micros = order.average_fill_price_micros;
    outcome.reason = Some("confirmed cancellation before place return".to_owned());
    outcome.return_value = BrokerCommandReturnV5::PlaceOrder(PlaceOrderReturnV5::Ok(
        KernelOrderResultV5 {
            order_id: order.order_id.clone(),
            status: KernelOrderStatusV5::Cancelled,
            filled_quantity_hundredths: order.filled_quantity_hundredths,
            fill_price_micros: order.average_fill_price_micros.unwrap_or(0),
            fee_cost_micros: fees_micros,
            reason: "confirmed cancellation before place return".to_owned(),
        },
    ));
    returned.broker.orders.push(order);
    context
}

fn invalid_replay_cases() -> Vec<(&'static str, DecisionContextV5, DecisionV5Error)> {
    let context = replay_corpus_context(current_packet_corpus_context(), 2).unwrap();
    let mut missing = context.clone();
    missing.broker_replay.as_mut().unwrap().preceding.clear();
    let mut order = context.clone();
    order.broker_replay.as_mut().unwrap().preceding[0]
        .outcome
        .continuation_generation = 2;
    let mut fence = context.clone();
    fence.broker_replay.as_mut().unwrap().preceding[0].expected_broker_revision += 1;
    let mut publication = context.clone();
    publication
        .broker_replay
        .as_mut()
        .unwrap()
        .returned_state
        .broker
        .revision += 1;
    let mut finances = context.clone();
    finances.broker_replay.as_mut().unwrap().preceding[0]
        .returned_state
        .finances
        .current_commitment_micros += 1;
    let mut downgrade = context.clone();
    downgrade.retained_supplied_encoding.canonical_d = true;
    let mut overflow = context;
    let history = overflow.broker_replay.as_mut().unwrap();
    history
        .preceding
        .resize(MAX_STRATEGY_COMMANDS, history.preceding[0].clone());
    let mut cases = vec![
        (
            "replay-missing-predecessor",
            missing,
            DecisionV5Error::InvalidContract,
        ),
        (
            "replay-out-of-order-generation",
            order,
            DecisionV5Error::InvalidContract,
        ),
        (
            "replay-stale-call-fence",
            fence,
            DecisionV5Error::InvalidContract,
        ),
        (
            "replay-incoherent-publication-revision",
            publication,
            DecisionV5Error::InvalidContract,
        ),
        (
            "replay-incoherent-returned-finances",
            finances,
            DecisionV5Error::InvalidContract,
        ),
        (
            "replay-cannot-downgrade-to-d",
            downgrade,
            DecisionV5Error::InvalidContract,
        ),
        (
            "replay-call-bound-exceeded",
            overflow,
            DecisionV5Error::BoundExceeded,
        ),
    ];
    for id in [
        "cancelled-place-changed-request",
        "cancelled-place-fabricated-open-quantity",
        "cancelled-place-changed-fill",
        "cancelled-place-changed-price",
        "cancelled-place-missing-order",
        "cancelled-place-unconfirmed-cancellation",
    ] {
        let mut invalid = cancelled_place_corpus_context(true);
        let TriggerV5::BrokerOutcome { outcome, .. } = &mut invalid.trigger else {
            unreachable!()
        };
        let BrokerCommandReturnV5::PlaceOrder(PlaceOrderReturnV5::Ok(result)) =
            &mut outcome.return_value
        else {
            unreachable!()
        };
        let returned = &mut invalid.broker_replay.as_mut().unwrap().returned_state;
        let order = returned.broker.orders.last_mut().unwrap();
        match id {
            "cancelled-place-changed-request" => {
                outcome.requested_quantity_hundredths = 200;
            }
            "cancelled-place-fabricated-open-quantity" => {
                outcome.remaining_quantity_hundredths = 100;
            }
            "cancelled-place-changed-fill" => {
                outcome.filled_quantity_hundredths = 199;
                result.filled_quantity_hundredths = 199;
            }
            "cancelled-place-changed-price" => {
                outcome.average_fill_price_micros = Some(400_001);
                result.fill_price_micros = 400_001;
            }
            "cancelled-place-missing-order" => {
                returned.broker.orders.pop();
            }
            "cancelled-place-unconfirmed-cancellation" => {
                order.status = BrokerOrderStatusV5::CancellationRequested;
                order.remaining_quantity_hundredths = 100;
                order.reserved_principal_micros = 400_000;
                returned.broker.reserved_cash_micros += 400_000;
                returned.finances.locally_reserved_cash_micros += 400_000;
                returned.finances.current_commitment_micros += 400_000;
            }
            _ => unreachable!(),
        }
        cases.push((id, invalid, DecisionV5Error::InvalidContract));
    }
    cases
}

#[test]
fn v5_replay_encoding_preserves_d_origins_and_enforces_order_finances_and_bounds() {
    use crate::replay_v5::ReplayOriginEncodingV5;
    for (historical_d, historical_e, encoding) in [
        (false, false, ReplayOriginEncodingV5::NativeStrikesF),
        (false, true, ReplayOriginEncodingV5::CurrentE),
        (true, false, ReplayOriginEncodingV5::HostSelectedD),
    ] {
        let mut origin = current_packet_corpus_context();
        origin.retained_supplied_encoding.canonical_d = historical_d;
        origin.retained_supplied_encoding.canonical_e = historical_e;
        if !historical_d && !historical_e {
            origin.market_strikes = Some(origin.owner_state.markets.iter().map(|market| MarketStrikesV5 {
                market_id: market.identity.market_id.clone(), cap_strike_milli_f: Some(84_000),
            }).collect());
        }
        let origin_bytes = encode_decision_context_v5(&origin).unwrap();
        let restored = decode_decision_context_v5(&origin_bytes).unwrap();
        let replay = replay_corpus_context(restored.clone(), 2).unwrap();
        assert_eq!(replay.owner_state, restored.owner_state);
        assert_eq!(replay.broker, restored.broker);
        assert_eq!(replay.kernel_checkpoint, restored.kernel_checkpoint);
        assert_eq!(
            replay.broker_replay.as_ref().unwrap().origin_encoding,
            encoding
        );
        let encoded = encode_decision_context_v5(&replay).unwrap();
        assert!(encoded.starts_with(DECISION_CONTEXT_V5_MAGIC));
        assert_eq!(decode_decision_context_v5(&encoded).unwrap(), replay);
        if encoding == ReplayOriginEncodingV5::NativeStrikesF {
            let mut tampered = replay.clone();
            tampered.market_strikes.as_mut().unwrap()[0].cap_strike_milli_f = Some(85_000);
            assert_eq!(tampered.validate(), Err(DecisionV5Error::InvalidContract));
            for historical in [ReplayOriginEncodingV5::CurrentE, ReplayOriginEncodingV5::HostSelectedD] {
                assert_eq!(replay_origin_digest(&origin, historical), Err(DecisionV5Error::InvalidContract));
            }
            let mut downgrade = origin.clone();
            downgrade.retained_supplied_encoding.canonical_e = true;
            assert_eq!(encode_decision_context_v5(&downgrade), Err(DecisionV5Error::InvalidContract));
        }
    }
    for filled in [false, true] {
        let cancelled = cancelled_place_corpus_context(filled);
        let bytes = encode_decision_context_v5(&cancelled).unwrap();
        assert_eq!(decode_decision_context_v5(&bytes).unwrap(), cancelled);
    }
    for (id, invalid, error) in invalid_replay_cases() {
        assert_eq!(encode_decision_context_v5(&invalid), Err(error), "{id}");
    }
    let original = current_packet_corpus_context();
    let at_limit = replay_corpus_context(original.clone(), MAX_STRATEGY_COMMANDS as u64).unwrap();
    assert_eq!(
        at_limit.broker_replay.as_ref().unwrap().preceding.len(),
        MAX_STRATEGY_COMMANDS - 1
    );
    assert_eq!(at_limit.kernel_checkpoint, original.kernel_checkpoint);
    let encoded = encode_decision_context_v5(&at_limit).unwrap();
    assert_eq!(decode_decision_context_v5(&encoded).unwrap(), at_limit);
    assert_eq!(
        replay_corpus_context(original, MAX_STRATEGY_COMMANDS as u64 + 1),
        Err(DecisionV5Error::BoundExceeded)
    );
}
