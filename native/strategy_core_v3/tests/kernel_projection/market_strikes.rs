use super::*;
use strategy_core_v3::decision_v6::{DecisionV6Error, MarketStrikesV6, decision_fence_v6_sha256};

#[test]
fn native_fahrenheit_caps_reach_kernel_without_celsius_roundtrip() {
    for (floor, cap) in [
        (None, Some(71_000)),
        (Some(71_000), Some(72_000)),
        (Some(78_000), None),
        (Some(-5_000), Some(-4_375)),
    ] {
        let mut context = base_context();
        context.owner_state.markets[0].identity.floor_strike_milli_f = floor;
        context.market_strikes = Some(vec![MarketStrikesV6 {
            market_id: MARKET.to_owned(),
            cap_strike_milli_f: cap,
        }]);
        let bytes = encode_decision_context_v6(&context).unwrap();
        assert!(bytes.starts_with(b"SDCTXV6A"));
        let decoded = decode_decision_context_v6(&bytes).unwrap();
        assert_eq!(decoded, context);
        let snapshot = KernelSnapshot::from_context(&decoded).unwrap();
        let market = &snapshot.market_states()[0];
        assert_eq!(market.floor_strike, floor.map(|v| v as f64 / 1_000.0));
        assert_eq!(market.cap_strike, cap.map(|v| v as f64 / 1_000.0));
    }
}

#[test]
fn native_caps_are_scoped_unambiguous_and_fenced() {
    let mut context = base_context();
    context.market_strikes = Some(vec![MarketStrikesV6 {
        market_id: MARKET.to_owned(),
        cap_strike_milli_f: Some(80_000),
    }]);
    let fence = decision_fence_v6_sha256(&context).unwrap();
    let mut changed = context.clone();
    changed.market_strikes.as_mut().unwrap()[0].cap_strike_milli_f = Some(81_000);
    assert_ne!(decision_fence_v6_sha256(&changed).unwrap(), fence);
    for mutation in 0..4 {
        let mut invalid = context.clone();
        match mutation {
            0 => invalid.market_strikes.as_mut().unwrap().clear(),
            1 => invalid.market_strikes.as_mut().unwrap()[0].market_id = "unrelated".to_owned(),
            2 => invalid.owner_state.markets[0].identity.cap_strike_milli_c = Some(80_000),
            _ => invalid.market_strikes.as_mut().unwrap()[0].cap_strike_milli_f = Some(79_000),
        }
        assert_eq!(invalid.validate(), Err(DecisionV6Error::InvalidContract));
    }
}
