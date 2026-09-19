//! Bounded, host-owned state for replaying a synchronous native Broker invocation.
//!
//! Strategy checkpoints remain pre-event checkpoints while a call is outstanding. The host
//! retains completed calls here so replay presents each exact return to its matching request,
//! advancing only Broker state between returns; Source inputs remain the original invocation.

use bincode::{Decode, Encode};
pub use strategy_core_kernel::BrokerFinancialState;

use crate::decision_v5::{
    self as wire, BrokerCommandReturnV5, BrokerDetailV5, BrokerOutcomeV5, ContinuationCommitmentV5,
    DecisionContextV5, DecisionV5Error, OriginatingTriggerV5, PlaceOrderReturnV5, TriggerV5,
};

#[derive(Clone, Copy, Debug, Encode, Decode, Eq, PartialEq)]
pub enum ReplayOriginEncodingV5 {
    CurrentE,
    HostSelectedD,
    SuppliedC,
    SuppliedS,
    Hundredths,
    Whole,
    /// Appended to preserve every historical variant index.
    NativeStrikesF,
}

/// One coherent host Broker publication, not a financial state inferred from fill averages.
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct BrokerExecutionStateV5 {
    pub broker: BrokerDetailV5,
    pub finances: BrokerFinancialState,
}

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct CompletedBrokerCallV5 {
    pub command_sha256: [u8; 32],
    pub expected_broker_revision: u64,
    pub outcome: BrokerOutcomeV5,
    pub returned_state: BrokerExecutionStateV5,
}

/// The current outcome remains in `TriggerV5`; only its predecessors are repeated here.
/// Populated by the host from retained continuations and coherent Broker publications.
#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq)]
pub struct BrokerReplayV5 {
    pub origin_encoding: ReplayOriginEncodingV5,
    pub preceding: Vec<CompletedBrokerCallV5>,
    pub returned_state: BrokerExecutionStateV5,
}

impl BrokerExecutionStateV5 {
    fn validate(
        &self,
        context: &DecisionContextV5,
        outcome: &BrokerOutcomeV5,
    ) -> Result<(), DecisionV5Error> {
        wire::validate_broker_parts(
            &context.strategy.market_ids,
            &self.broker,
            self.finances.locally_reserved_cash_micros,
            self.finances.current_commitment_micros,
        )?;
        if self.broker.revision != outcome.broker_revision {
            return Err(DecisionV5Error::InvalidContract);
        }
        let confirms_cancellation = matches!(
            &outcome.return_value,
            BrokerCommandReturnV5::CancelOrder(wire::CancelOrderReturnV5::Ok(true))
        ) || matches!(
            &outcome.return_value,
            BrokerCommandReturnV5::PlaceOrder(PlaceOrderReturnV5::Ok(result))
                if result.status == wire::KernelOrderStatusV5::Cancelled
        );
        if confirms_cancellation && !wire::cancelled_order_matches(&self.broker, outcome) {
            return Err(DecisionV5Error::InvalidContract);
        }
        if let BrokerCommandReturnV5::PlaceOrder(PlaceOrderReturnV5::Ok(result)) =
            &outcome.return_value
        {
            if !self
                .broker
                .orders
                .iter()
                .any(|order| order.order_id == result.order_id)
            {
                return Err(DecisionV5Error::InvalidContract);
            }
        }
        Ok(())
    }
}

pub(crate) fn validate(context: &DecisionContextV5) -> Result<(), DecisionV5Error> {
    let Some(replay) = &context.broker_replay else {
        return Ok(());
    };
    let TriggerV5::BrokerOutcome { outcome, .. } = &context.trigger else {
        return Err(DecisionV5Error::InvalidContract);
    };
    let commitment = context
        .continuation
        .as_ref()
        .ok_or(DecisionV5Error::InvalidContract)?;
    if replay.preceding.len() >= wire::MAX_STRATEGY_COMMANDS {
        return Err(DecisionV5Error::BoundExceeded);
    }
    if outcome.continuation_generation != replay.preceding.len() as u64 + 1 {
        return Err(DecisionV5Error::InvalidContract);
    }
    let mut revision = context.broker.revision;
    for (index, call) in replay.preceding.iter().enumerate() {
        if call.outcome.continuation_generation != index as u64 + 1
            || call.expected_broker_revision != revision
            || call.outcome.broker_revision < revision
        {
            return Err(DecisionV5Error::InvalidContract);
        }
        call.returned_state.validate(context, &call.outcome)?;
        wire::validate_broker_outcome_fields(&call.returned_state.broker, &call.outcome)?;
        revision = call.outcome.broker_revision;
    }
    if commitment.expected_broker_revision != revision || outcome.broker_revision < revision {
        return Err(DecisionV5Error::InvalidContract);
    }
    replay.returned_state.validate(context, outcome)
}

/// Build an outcome delivery from the exact retained invocation and a coherent host Broker
/// publication. An old follow-up without its earlier returned state cannot be reconstructed
/// from current balances or fill averages: it fails closed without changing the retained row.
pub fn broker_outcome_context_v5(
    originating: &DecisionContextV5,
    commitment: &ContinuationCommitmentV5,
    outcome: BrokerOutcomeV5,
    returned_state: BrokerExecutionStateV5,
) -> Result<DecisionContextV5, DecisionV5Error> {
    originating.validate()?;
    let (trigger, origin_encoding, preceding) = match &originating.trigger {
        TriggerV5::Owner(trigger) => (
            OriginatingTriggerV5::Owner(trigger.clone()),
            wire::replay_origin_encoding(originating, commitment.originating_context_sha256)?,
            Vec::new(),
        ),
        TriggerV5::BrokerState { broker_revision } => (
            OriginatingTriggerV5::BrokerState {
                broker_revision: *broker_revision,
            },
            wire::replay_origin_encoding(originating, commitment.originating_context_sha256)?,
            Vec::new(),
        ),
        TriggerV5::BrokerOutcome {
            outcome: previous,
            originating_trigger,
        } => {
            let history = originating
                .broker_replay
                .as_ref()
                .ok_or(DecisionV5Error::InvalidContract)?;
            let previous_commitment = originating
                .continuation
                .as_ref()
                .ok_or(DecisionV5Error::InvalidContract)?;
            let mut preceding = history.preceding.clone();
            if preceding.len() + 1 >= wire::MAX_STRATEGY_COMMANDS {
                return Err(DecisionV5Error::BoundExceeded);
            }
            preceding.push(CompletedBrokerCallV5 {
                command_sha256: previous_commitment.command_sha256,
                expected_broker_revision: previous_commitment.expected_broker_revision,
                outcome: (**previous).clone(),
                returned_state: history.returned_state.clone(),
            });
            (
                (**originating_trigger).clone(),
                history.origin_encoding,
                preceding,
            )
        }
    };
    let mut replay = originating.clone();
    replay.retained_supplied_encoding.canonical_c = false;
    replay.retained_supplied_encoding.canonical_d = false;
    replay.retained_supplied_encoding.canonical_e = false;
    replay.kernel_checkpoint = Some(commitment.pre_event_checkpoint.clone());
    replay.continuation = Some(commitment.clone());
    replay.trigger = TriggerV5::BrokerOutcome {
        outcome: Box::new(outcome),
        originating_trigger: Box::new(trigger),
    };
    replay.broker_replay = Some(BrokerReplayV5 {
        origin_encoding,
        preceding,
        returned_state,
    });
    replay.validate()?;
    Ok(replay)
}
