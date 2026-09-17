use super::*;
use crate::replay_v5::ReplayOriginEncodingV5;

pub(crate) fn replay_origin_digest(
    context: &DecisionContextV5,
    encoding: ReplayOriginEncodingV5,
) -> Result<[u8; 32], DecisionV5Error> {
    let mut original = context.clone();
    original.broker_replay = None;
    original.retained_supplied_encoding.canonical_c = false;
    original.retained_supplied_encoding.canonical_d = false;
    original.retained_supplied_encoding.canonical_e = false;
    if original.market_strikes.is_some() && encoding != ReplayOriginEncodingV5::NativeStrikesF {
        return Err(DecisionV5Error::InvalidContract);
    }
    let bytes = match encoding {
        ReplayOriginEncodingV5::NativeStrikesF => encode_bounded(
            DECISION_CONTEXT_V5_MAGIC,
            &original,
            MAX_DECISION_CONTEXT_V5_BYTES,
        )?,
        ReplayOriginEncodingV5::CurrentE => encode_bounded(
            REPLAY_E_DECISION_CONTEXT_V5_MAGIC,
            &crate::wire_e::FrozenEDecisionContextV5::from_current(&original),
            MAX_DECISION_CONTEXT_V5_BYTES,
        )?,
        ReplayOriginEncodingV5::HostSelectedD => encode_bounded(
            HOST_D_DECISION_CONTEXT_V5_MAGIC,
            &crate::wire_d::FrozenDDecisionContextV5::from_current(&original),
            MAX_DECISION_CONTEXT_V5_BYTES,
        )?,
        ReplayOriginEncodingV5::SuppliedC => {
            if original.has_current_only_fields() {
                return Err(DecisionV5Error::InvalidContract);
            }
            encode_bounded(
                CANONICAL_C_DECISION_CONTEXT_V5_MAGIC,
                &original.frozen_c(),
                MAX_DECISION_CONTEXT_V5_BYTES,
            )?
        }
        ReplayOriginEncodingV5::SuppliedS => {
            return supplied_s_decision_context_v5_sha256(&original);
        }
        ReplayOriginEncodingV5::Hundredths => {
            return hundredths_decision_context_v5_sha256(&original);
        }
        ReplayOriginEncodingV5::Whole => return legacy_decision_context_v5_sha256(&original),
    };
    Ok(Sha256::digest(bytes).into())
}

pub(crate) fn replay_origin_encoding(
    context: &DecisionContextV5,
    expected: [u8; 32],
) -> Result<ReplayOriginEncodingV5, DecisionV5Error> {
    for encoding in [
        ReplayOriginEncodingV5::NativeStrikesF,
        ReplayOriginEncodingV5::CurrentE,
        ReplayOriginEncodingV5::HostSelectedD,
        ReplayOriginEncodingV5::SuppliedC,
        ReplayOriginEncodingV5::SuppliedS,
        ReplayOriginEncodingV5::Hundredths,
        ReplayOriginEncodingV5::Whole,
    ] {
        match replay_origin_digest(context, encoding) {
            Ok(digest) if digest == expected => return Ok(encoding),
            Ok(_) | Err(DecisionV5Error::InvalidContract) => {}
            Err(error) => return Err(error),
        }
    }
    Err(DecisionV5Error::InvalidContract)
}
