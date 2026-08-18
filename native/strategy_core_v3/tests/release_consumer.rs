use strategy_core_v3::{CanonicalValue, canonical_sha256};

#[test]
fn released_crate_exposes_the_pinned_canonical_profile() {
    let digest = canonical_sha256(
        "strategy.identifier",
        &CanonicalValue::string("café".to_owned()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        digest,
        "2f0f6c5372687a487cfb5a430e921eaf879cc82e5f08b6c2abef207aae7830a1"
    );
}
