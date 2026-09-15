//! Frozen positional container for the retained `SDCTXV5D` encoding.

use bincode::{Decode, Encode};

use crate::decision_v4::DecisionContextV4;
use crate::decision_v5::{
    BrokerDetailV5, ContinuationCommitmentV5, DecisionContextV5, KernelCheckpointV5,
    RetainedSuppliedEncodingV5, StationForecastIssuanceV5, StationWeatherV5, StrategyScopeV5,
    TriggerV5,
};
use crate::supplied_v5::SuppliedInputsV5;

#[derive(Clone, Debug, Encode, Decode)]
pub(crate) struct FrozenDDecisionContextV5 {
    owner_state: DecisionContextV4,
    strategy: StrategyScopeV5,
    broker: BrokerDetailV5,
    trigger: TriggerV5,
    kernel_checkpoint: Option<KernelCheckpointV5>,
    continuation: Option<ContinuationCommitmentV5>,
    decision_time_unix_ms: i64,
    supplied: SuppliedInputsV5,
    current_weather: Option<Vec<StationWeatherV5>>,
    forecast_issuance: Option<Vec<StationForecastIssuanceV5>>,
    current_inputs: Option<crate::current_v5::CurrentInputsV5>,
    retained_supplied_encoding: RetainedSuppliedEncodingV5,
}

impl FrozenDDecisionContextV5 {
    pub(crate) fn from_current(context: &DecisionContextV5) -> Self {
        Self {
            owner_state: context.owner_state.clone(),
            strategy: context.strategy.clone(),
            broker: context.broker.clone(),
            trigger: context.trigger.clone(),
            kernel_checkpoint: context.kernel_checkpoint.clone(),
            continuation: context.continuation.clone(),
            decision_time_unix_ms: context.decision_time_unix_ms,
            supplied: context.supplied.clone(),
            current_weather: context.current_weather.clone(),
            forecast_issuance: context.forecast_issuance.clone(),
            current_inputs: context.current_inputs.clone(),
            retained_supplied_encoding: context.retained_supplied_encoding.clone(),
        }
    }

    pub(crate) fn into_current(mut self) -> DecisionContextV5 {
        self.retained_supplied_encoding.canonical_d = true;
        DecisionContextV5 {
            owner_state: self.owner_state,
            strategy: self.strategy,
            broker: self.broker,
            trigger: self.trigger,
            kernel_checkpoint: self.kernel_checkpoint,
            continuation: self.continuation,
            decision_time_unix_ms: self.decision_time_unix_ms,
            supplied: self.supplied,
            current_weather: self.current_weather,
            forecast_issuance: self.forecast_issuance,
            current_inputs: self.current_inputs,
            retained_supplied_encoding: self.retained_supplied_encoding,
            broker_replay: None,
        }
    }
}
