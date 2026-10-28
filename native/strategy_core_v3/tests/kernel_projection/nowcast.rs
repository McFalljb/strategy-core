use super::*;
use strategy_core_kernel::{ComponentAuthority, StrategyKernelState};
use strategy_core_v3::decision_v4::{AuthorityV4, ComponentMetaV4};
use strategy_core_v3::decision_v6::{DecisionV6Error, decision_fence_v6_sha256};
use strategy_core_v3::nowcast_v6::{
    NowcastComponentsV6, NowcastInputV6, SuppliedNowcastArtifactV6, SuppliedNowcastEnvelopeV6,
    SuppliedNowcastLeadV6, SuppliedNowcastStationV6, SuppliedNowcastV6, SuppliedStationForecastV6,
    sha256_hex,
};

const BODY: &str = r#"{"coef":[1.1383442675728548],"intercept":-0.3241092365517112}"#;

fn input(decision_time_unix_ms: i64) -> NowcastInputV6 {
    let issue = (decision_time_unix_ms - 300_000) * 1_000_000;
    let envelope = SuppliedNowcastEnvelopeV6 {
        event_id: "nowcast-7".to_owned(),
        seq: 7,
        emitted_at_unix_ns: Some(issue + 35_000_000_000),
        received_at_unix_ns: issue + 36_000_000_000,
    };
    NowcastInputV6 {
        supplied: SuppliedNowcastV6 {
            city: "miami".to_owned(),
            seq: 7,
            stations: vec!["KMIA".to_owned()],
            forecasts: vec![SuppliedNowcastStationV6 {
                station_id: "KMIA".to_owned(),
                issues: vec![SuppliedStationForecastV6 {
                    station_id: "KMIA".to_owned(),
                    issue_time_unix_ns: issue,
                    envelope: envelope.clone(),
                    levels: vec![decimal("0.25").unwrap(), decimal("0.75").unwrap()],
                    leads: vec![SuppliedNowcastLeadV6 {
                        lead_min: 5,
                        target_time_unix_ns: issue + 300_000_000_000,
                        median: decimal("84.3").unwrap(),
                        q: vec![decimal("83.9").unwrap(), decimal("84.7").unwrap()],
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
            }],
            artifacts: vec![SuppliedNowcastArtifactV6 {
                kind: "blend".to_owned(),
                model_id: "lr_full".to_owned(),
                version: "frozen".to_owned(),
                sha256: sha256_hex(BODY.as_bytes()),
                valid_from_unix_ns: issue - 86_400_000_000_000,
                envelope,
                body_json: BODY.to_owned(),
                ..Default::default()
            }],
            ..Default::default()
        },
        components: NowcastComponentsV6 {
            feed: ComponentMetaV4 {
                authority: AuthorityV4::Uncertain,
                revision: 3,
                generation: 1,
                ..Default::default()
            },
        },
    }
}

/// The nowcast reaches the kernel exactly, under its city only, with the feed's authority; a
/// context without it reads none; it is not part of the state fence; and an artefact whose
/// body does not hash to its `sha256` makes the context invalid.
#[test]
fn the_nowcast_reaches_the_kernel_by_city_with_its_feed_authority() {
    let context = base_context();
    let without = KernelSnapshot::from_context(&context).unwrap();
    assert!(
        (&without as &dyn StrategyKernelState)
            .nowcast("miami")
            .is_none()
    );

    let mut context = base_context();
    context.nowcast = Some(input(context.decision_time_unix_ms));
    let decoded =
        decode_decision_context_v6(&encode_decision_context_v6(&context).unwrap()).unwrap();
    assert_eq!(decoded, context);
    assert_eq!(
        decision_fence_v6_sha256(&decoded).unwrap(),
        decision_fence_v6_sha256(&base_context()).unwrap()
    );
    let snapshot = KernelSnapshot::from_context(&decoded).unwrap();
    let state: &dyn StrategyKernelState = &snapshot;
    assert!(state.nowcast("nyc").is_none());
    let nowcast = state.nowcast("miami").unwrap();
    assert_eq!(nowcast.feed.authority, ComponentAuthority::Uncertain);
    assert_eq!(nowcast.supplied, context.nowcast.as_ref().unwrap().supplied);
    assert_eq!(
        nowcast.supplied.issues("KMIA")[0].leads[0].q[1].to_f64(),
        84.7
    );
    assert_eq!(
        nowcast
            .supplied
            .artifacts_of("blend", "lr_full")
            .next()
            .unwrap()
            .body_json,
        BODY
    );

    let mut context = base_context();
    let mut tampered = input(context.decision_time_unix_ms);
    tampered.supplied.artifacts[0].body_json = BODY.replace("1.13", "1.14");
    context.nowcast = Some(tampered);
    assert_eq!(
        encode_decision_context_v6(&context).unwrap_err(),
        DecisionV6Error::InvalidContract
    );
}
