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
    vec![
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
    ]
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
