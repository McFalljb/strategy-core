//! Frozen positional `SDCTXV5E`: D's fields followed by bounded Broker replay.

use bincode::{Decode, Encode};

use crate::decision_v5::DecisionContextV5;
use crate::replay_v5::BrokerReplayV5;
use crate::wire_d::FrozenDDecisionContextV5;

#[derive(Clone, Debug, Encode, Decode)]
pub(crate) struct FrozenEDecisionContextV5 {
    owner: FrozenDDecisionContextV5,
    broker_replay: Option<BrokerReplayV5>,
}

impl FrozenEDecisionContextV5 {
    pub(crate) fn from_current(context: &DecisionContextV5) -> Self {
        Self {
            owner: FrozenDDecisionContextV5::from_current(context),
            broker_replay: context.broker_replay.clone(),
        }
    }

    pub(crate) fn into_current(self) -> DecisionContextV5 {
        let mut context = self.owner.into_current();
        context.retained_supplied_encoding.canonical_d = false;
        context.retained_supplied_encoding.canonical_e = true;
        context.broker_replay = self.broker_replay;
        context
    }
}
