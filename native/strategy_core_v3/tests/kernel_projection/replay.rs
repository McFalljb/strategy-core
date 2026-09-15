use super::*;
use strategy_core_kernel::{BrokerFinancialState, CancelOrderRequest};
use strategy_core_v3::decision_v5::{
    BrokerOrderStatusV5, BrokerOrderV5, CancelOrderReturnV5, ContractSideV5, KernelBrokerErrorV5,
    KernelCheckpointV5, OrderActionV5, OrderTypeV5,
};
use strategy_core_v3::replay_v5::{BrokerExecutionStateV5, broker_outcome_context_v5};

// A real NativeKernel invocation that places, cancels, then handles a refusal. Its private
// checkpoint records only completed returns, never the host's replay history.
#[derive(Clone, Default)]
struct SequenceKernel {
    balances: Vec<u64>,
    market_orders: bool,
}

fn request(side: ContractSide) -> PlaceOrderRequest {
    PlaceOrderRequest {
        ticker: MARKET.to_owned(),
        action: OrderAction::Buy,
        contract_side: side.clone(),
        order_type: OrderType::Limit,
        quantity: ContractQuantity::from_hundredths(if side == ContractSide::Yes {
            29
        } else {
            73
        }),
        limit_price: Some(0.4),
        expires_after_ms: None,
        reduce_only: false,
        signal_type: None,
        signal_metadata: None,
        client_order_id: Some(format!("client.replay.{side:?}")),
    }
}

impl SequenceKernel {
    fn request(&self, side: ContractSide) -> PlaceOrderRequest {
        let mut request = request(side);
        if self.market_orders {
            request.order_type = OrderType::Market;
            request.limit_price = None;
        }
        request
    }

    fn record_return(&mut self, context: &mut dyn StrategyKernelContext) {
        self.balances
            .push(context.broker().financial_state().buying_power_micros());
        assert_eq!(
            context.runtime().now().unwrap().timestamp_millis(),
            DECISION_MS
        );
        assert_eq!(
            context
                .state()
                .station(STATION)
                .unwrap()
                .observation
                .as_ref()
                .unwrap()
                .supplied
                .as_ref()
                .unwrap()
                .temperature_f,
            decimal("73.000000000000001"),
            "Broker returns must not replace the original Source inputs"
        );
    }
}

impl NativeKernel for SequenceKernel {
    fn name(&self) -> &str {
        "broker-sequence"
    }

    fn on_event(
        &mut self,
        _: StrategyEventView<'_>,
        context: &mut dyn StrategyKernelContext,
    ) -> KernelResult<()> {
        assert!(
            self.balances.is_empty(),
            "every replay starts at the pre-event checkpoint"
        );
        let placed = context
            .broker()
            .place_order(self.request(ContractSide::Yes))?;
        self.record_return(context);
        assert_eq!(
            context.broker().pending_orders()[0].requested_quantity,
            ContractQuantity::from_hundredths(29)
        );
        assert!(context.broker().cancel_order(CancelOrderRequest {
            order_id: placed.order_id
        })?);
        self.record_return(context);
        assert!(context.broker().pending_orders().is_empty());
        let refused = context.broker().place_order(self.request(ContractSide::No));
        if let Err(error) = &refused {
            if error.to_string() == "fixture refusal" {
                self.record_return(context);
                return Ok(());
            }
        }
        refused?;
        panic!("the third call must return the typed refusal");
    }
}

impl TransactionKernel for SequenceKernel {
    fn encode_checkpoint_state(&self) -> Result<Vec<u8>, KernelTransactionError> {
        Ok(serde_json::to_vec(&self.balances).unwrap())
    }

    fn market_buy_price_cap_micros(
        &self,
        _: &PlaceOrderRequest,
    ) -> Result<Option<u64>, KernelTransactionError> {
        // The native API asks for the cap of the request just suspended, not a request from
        // an earlier stage after later returns have advanced this private planning state.
        Ok(Some(if self.balances.is_empty() {
            400_000
        } else {
            600_000
        }))
    }
}

struct SequenceFactory {
    market_orders: bool,
}
impl TransactionKernelFactory for SequenceFactory {
    type Kernel = SequenceKernel;
    fn checkpoint_codec(&self, _: &str) -> Result<KernelCheckpointCodec, KernelTransactionError> {
        Ok(KernelCheckpointCodec {
            profile: "fixture.sequence.v1".to_owned(),
            version: 1,
        })
    }
    fn create(&self, _: &DecisionContextV5) -> Result<SequenceKernel, KernelTransactionError> {
        Ok(SequenceKernel {
            market_orders: self.market_orders,
            ..Default::default()
        })
    }
    fn restore(
        &self,
        _: &DecisionContextV5,
        checkpoint: &KernelCheckpointV5,
    ) -> Result<SequenceKernel, KernelTransactionError> {
        Ok(SequenceKernel {
            balances: serde_json::from_slice(&checkpoint.state).unwrap(),
            market_orders: self.market_orders,
        })
    }
}

fn deliver(
    context: &DecisionContextV5,
    result: &strategy_core_v3::decision_v5::DecisionResultV5,
) -> DecisionContextV5 {
    let commitment = continuation_commitment_v5(context, result)
        .unwrap()
        .unwrap();
    let mut state = context
        .broker_replay
        .as_ref()
        .map(|history| history.returned_state.clone())
        .unwrap_or_else(|| {
            let owner = &context.owner_state.broker;
            BrokerExecutionStateV5 {
                broker: context.broker.clone(),
                finances: BrokerFinancialState {
                    allowance_limit_micros: owner.allowance_limit,
                    current_commitment_micros: owner.current_commitment,
                    provider_available_balance_micros: owner.provider_available_balance,
                    locally_reserved_cash_micros: owner.locally_reserved_cash,
                },
            }
        });
    state.broker.revision += 1;
    let mut outcome = BrokerOutcomeV5 {
        outcome_id: format!("outcome.replay.{}", commitment.continuation_generation),
        continuation_id: commitment.continuation_id.clone(),
        continuation_generation: commitment.continuation_generation,
        command_id: commitment.command_id.clone(),
        command_kind: BrokerCommandKindV5::PlaceOrder,
        transition_sequence: 1,
        target_order_id: None,
        order_id: None,
        updated_at_unix_ms: context.decision_time_unix_ms,
        broker_revision: state.broker.revision,
        status: BrokerOutcomeStatusV5::Rejected,
        intent_id: None,
        provider_order_id: None,
        provider_client_id: None,
        requested_quantity_hundredths: 0,
        filled_quantity_hundredths: 0,
        remaining_quantity_hundredths: 0,
        average_fill_price_micros: None,
        reason: None,
        return_value: BrokerCommandReturnV5::PlaceOrder(PlaceOrderReturnV5::Err(
            KernelBrokerErrorV5 {
                code: "fixture_refusal".to_owned(),
                message: "fixture refusal".to_owned(),
                retryable: false,
            },
        )),
    };
    match &result.commands[0] {
        StrategyCommandV5::PlaceOrder(order) if order.side == ContractSideV5::Yes => {
            // Explicit zero-fee paper fixture: reserve $0.116 for 0.29 contracts at $0.40.
            state.broker.reserved_cash_micros = 116_000;
            state.finances.current_commitment_micros = 116_000;
            state.finances.locally_reserved_cash_micros = 116_000;
            state.broker.orders.push(BrokerOrderV5 {
                command_id: order.command_id.clone(),
                intent_id: "intent.replay".to_owned(),
                order_id: "order.replay".to_owned(),
                provider_order_id: Some("paper.replay".to_owned()),
                provider_client_id: order.provider_client_id.clone(),
                market_id: MARKET.to_owned(),
                action: OrderActionV5::Buy,
                side: ContractSideV5::Yes,
                order_type: OrderTypeV5::Limit,
                quantity_hundredths: 29,
                filled_quantity_hundredths: 0,
                remaining_quantity_hundredths: 29,
                limit_price_micros: Some(400_000),
                average_fill_price_micros: None,
                reserved_principal_micros: 116_000,
                reserved_fee_micros: 0,
                created_at_unix_ms: None,
                updated_at_unix_ms: None,
                signal_type: None,
                signal_metadata: None,
                status: BrokerOrderStatusV5::Resting,
                revision: state.broker.revision,
            });
            outcome.status = BrokerOutcomeStatusV5::Resting;
            outcome.requested_quantity_hundredths = 29;
            outcome.remaining_quantity_hundredths = 29;
            outcome.order_id = Some("order.replay".to_owned());
            outcome.intent_id = Some("intent.replay".to_owned());
            outcome.provider_order_id = Some("paper.replay".to_owned());
            outcome.provider_client_id = Some(order.provider_client_id.clone());
            outcome.return_value =
                BrokerCommandReturnV5::PlaceOrder(PlaceOrderReturnV5::Ok(KernelOrderResultV5 {
                    order_id: "order.replay".to_owned(),
                    status: KernelOrderStatusV5::Pending,
                    filled_quantity_hundredths: 0,
                    fill_price_micros: 0,
                    fee_cost_micros: 0,
                    reason: "resting".to_owned(),
                }));
        }
        StrategyCommandV5::CancelOrder { order_id, .. } => {
            let order = &mut state.broker.orders[0];
            assert_eq!(*order_id, order.order_id);
            order.status = BrokerOrderStatusV5::Cancelled;
            order.revision = state.broker.revision;
            order.remaining_quantity_hundredths = 0;
            order.reserved_principal_micros = 0;
            state.broker.reserved_cash_micros = 0;
            state.finances.current_commitment_micros = 0;
            state.finances.locally_reserved_cash_micros = 0;
            outcome.command_kind = BrokerCommandKindV5::CancelOrder;
            outcome.status = BrokerOutcomeStatusV5::Cancelled;
            outcome.requested_quantity_hundredths = 29;
            outcome.remaining_quantity_hundredths = 0;
            outcome.intent_id = Some(order.intent_id.clone());
            outcome.provider_order_id = order.provider_order_id.clone();
            outcome.provider_client_id = Some(order.provider_client_id.clone());
            outcome.target_order_id = Some(order.order_id.clone());
            outcome.order_id = Some(order.order_id.clone());
            outcome.return_value =
                BrokerCommandReturnV5::CancelOrder(CancelOrderReturnV5::Ok(true));
        }
        StrategyCommandV5::PlaceOrder(order) => {
            assert_eq!(order.side, ContractSideV5::No);
            // A separate account update at the refusal publication, not a balance inferred from a fill.
            state.finances.provider_available_balance_micros = 99_999_999;
        }
        _ => panic!("unexpected sequence command"),
    }
    let returned =
        broker_outcome_context_v5(context, &commitment, outcome, state).unwrap_or_else(|error| {
            panic!(
                "return generation {}: {error:?}",
                commitment.continuation_generation
            )
        });
    decode_decision_context_v5(&encode_decision_context_v5(&returned).unwrap()).unwrap()
}

#[test]
fn ordered_place_cancel_refusal_replays_exact_finances_and_only_commits_final_private_state() {
    let original = observation_context(Some("22.8"), Some("73.000000000000001"));
    for market_orders in [false, true] {
        let factory = SequenceFactory { market_orders };
        let mut context = original.clone();
        for revision in 0..3 {
            let result = run_transaction(&factory, &context).unwrap_or_else(|error| {
                panic!("market={market_orders} revision={revision}: {error:?}")
            });
            if market_orders {
                if let StrategyCommandV5::PlaceOrder(order) = &result.commands[0] {
                    assert_eq!(
                        order.market_price_cap_micros,
                        Some(if revision == 0 { 400_000 } else { 600_000 })
                    );
                }
            }
            assert!(
                matches!(result.disposition, DecisionDispositionV5::AwaitingBrokerOutcome { continuation_generation, .. } if continuation_generation == revision + 1)
            );
            assert_eq!(result.expected_broker_revision, revision);
            assert_eq!(result.kernel_checkpoint.as_ref().unwrap().state, b"[]");
            context = deliver(&context, &result);
            if revision == 1 {
                for requested in [0, 28] {
                    let mut invalid = context.clone();
                    let TriggerV5::BrokerOutcome { outcome, .. } = &mut invalid.trigger else {
                        unreachable!()
                    };
                    outcome.requested_quantity_hundredths = requested;
                    assert!(
                        invalid.validate().is_err(),
                        "cancelled quantities must match the returned order"
                    );
                }
            }
            assert_eq!(context.owner_state, original.owner_state);
            assert_eq!(context.broker, original.broker);
        }
        let completed = run_transaction(&factory, &context).unwrap();
        assert_eq!(completed.disposition, DecisionDispositionV5::Completed);
        assert_eq!(completed.expected_broker_revision, 3);
        assert!(completed.commands.is_empty());
        assert_eq!(
            serde_json::from_slice::<Vec<u64>>(&completed.kernel_checkpoint.unwrap().state)
                .unwrap(),
            [99_884_000, 100_000_000, 99_999_999]
        );
        assert_eq!(context.broker_replay.as_ref().unwrap().preceding.len(), 2);

        // No balance reconstruction or feeding the last return into the first call for old deliveries.
        context.broker_replay = None;
        assert!(
            run_transaction(&factory, &context).is_err(),
            "missing earlier Broker states must fail closed"
        );
    }
}
