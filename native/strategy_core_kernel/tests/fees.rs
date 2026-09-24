//! Exact fee helpers: what the Broker charges and reserves, the legacy schedule they replace,
//! and the direct-member rule the Broker binds to.

use strategy_core_kernel::{
    ContractQuantity, MarketState, OrderAction,
    fees::{
        FeeError, FeeTerms, FeeType, LiquidityRole, apply_fee_rounding_micros,
        buy_commitment_micros, buy_fee_reservation_micros, calculate_direct_member_fill_fee_micros,
        calculate_fill_fee_micros, calculate_trade_fee_micros, price_micros,
        reserve_direct_member_buy_fee_micros,
    },
};

use FeeType::{Flat, Quadratic, QuadraticWithMakerFees};
use LiquidityRole::{Maker, Taker};
use OrderAction::{Buy, Sell};

/// `(trade, rounding, rebate, net, posted cash change, next accumulator)` in microdollars.
type Charge = [i128; 6];

struct Case {
    name: &'static str,
    action: OrderAction,
    price_micros: u64,
    quantity_hundredths: i64,
    role: LiquidityRole,
    accumulator_micros: u64,
    terms: FeeTerms,
    broker: Charge,
    /// The legacy `strategy_core::calculate_fill_fee` result (whole quantities) or
    /// `calculate_fill_fee_hundredths` (fractional) for the same inputs, captured before the
    /// legacy crate is deleted. Legacy defaults (`None` fee type and multiplier) are written
    /// out as the terms they resolved to.
    legacy: Charge,
}

const fn terms(fee_type: FeeType, multiplier_millionths: u64) -> FeeTerms {
    FeeTerms::new(fee_type, multiplier_millionths)
}

const CASES: &[Case] = &[
    Case {
        name: "buy 0.30 x1 taker, legacy defaults",
        action: Buy,
        price_micros: 300_000,
        quantity_hundredths: 100,
        role: Taker,
        accumulator_micros: 0,
        terms: terms(QuadraticWithMakerFees, 1_000_000),
        broker: [14_700, 0, 0, 14_700, -314_700, 0],
        legacy: [14_700, 5_300, 0, 20_000, -320_000, 5_300],
    },
    Case {
        name: "buy 0.50 x100 taker, legacy defaults (both schedules agree)",
        action: Buy,
        price_micros: 500_000,
        quantity_hundredths: 10_000,
        role: Taker,
        accumulator_micros: 0,
        terms: terms(QuadraticWithMakerFees, 1_000_000),
        broker: [1_750_000, 0, 0, 1_750_000, -51_750_000, 0],
        legacy: [1_750_000, 0, 0, 1_750_000, -51_750_000, 0],
    },
    Case {
        name: "buy 0.90 x1 taker, legacy defaults",
        action: Buy,
        price_micros: 900_000,
        quantity_hundredths: 100,
        role: Taker,
        accumulator_micros: 0,
        terms: terms(QuadraticWithMakerFees, 1_000_000),
        broker: [6_300, 0, 0, 6_300, -906_300, 0],
        legacy: [6_300, 3_700, 0, 10_000, -910_000, 3_700],
    },
    Case {
        name: "buy 0.25 x10 maker, maker fees",
        action: Buy,
        price_micros: 250_000,
        quantity_hundredths: 1_000,
        role: Maker,
        accumulator_micros: 0,
        terms: terms(QuadraticWithMakerFees, 1_000_000),
        broker: [32_813, 87, 0, 32_900, -2_532_900, 87],
        legacy: [32_900, 7_100, 0, 40_000, -2_540_000, 7_100],
    },
    Case {
        name: "buy 0.25 x10 maker, quadratic (maker exempt)",
        action: Buy,
        price_micros: 250_000,
        quantity_hundredths: 1_000,
        role: Maker,
        accumulator_micros: 0,
        terms: terms(Quadratic, 1_000_000),
        broker: [0, 0, 0, 0, -2_500_000, 0],
        legacy: [0, 0, 0, 0, -2_500_000, 0],
    },
    Case {
        name: "buy 0.30 x100 taker, flat",
        action: Buy,
        price_micros: 300_000,
        quantity_hundredths: 10_000,
        role: Taker,
        accumulator_micros: 0,
        terms: terms(Flat, 1_000_000),
        broker: [735_000, 0, 0, 735_000, -30_735_000, 0],
        legacy: [735_000, 5_000, 0, 740_000, -30_740_000, 5_000],
    },
    Case {
        name: "buy 0.50 x100 taker, flat",
        action: Buy,
        price_micros: 500_000,
        quantity_hundredths: 10_000,
        role: Taker,
        accumulator_micros: 0,
        terms: terms(Flat, 1_000_000),
        broker: [875_000, 0, 0, 875_000, -50_875_000, 0],
        legacy: [875_000, 5_000, 0, 880_000, -50_880_000, 5_000],
    },
    Case {
        name: "sell 0.55 x7 taker, quadratic",
        action: Sell,
        price_micros: 550_000,
        quantity_hundredths: 700,
        role: Taker,
        accumulator_micros: 0,
        terms: terms(Quadratic, 1_000_000),
        broker: [121_275, 25, 0, 121_300, 3_728_700, 25],
        legacy: [121_300, 8_700, 0, 130_000, 3_720_000, 8_700],
    },
    Case {
        name: "buy 0.30 x10 taker, quadratic, multiplier 2",
        action: Buy,
        price_micros: 300_000,
        quantity_hundredths: 1_000,
        role: Taker,
        accumulator_micros: 0,
        terms: terms(Quadratic, 2_000_000),
        broker: [294_000, 0, 0, 294_000, -3_294_000, 0],
        legacy: [294_000, 6_000, 0, 300_000, -3_300_000, 6_000],
    },
    Case {
        name: "buy 0.42 x3 taker with a carried accumulator",
        action: Buy,
        price_micros: 420_000,
        quantity_hundredths: 300,
        role: Taker,
        accumulator_micros: 7_100,
        terms: terms(QuadraticWithMakerFees, 1_000_000),
        broker: [51_156, 44, 7_100, 44_100, -1_304_100, 44],
        legacy: [51_200, 8_800, 10_000, 50_000, -1_310_000, 5_900],
    },
    Case {
        name: "buy 0.60 x5 taker, quadratic",
        action: Buy,
        price_micros: 600_000,
        quantity_hundredths: 500,
        role: Taker,
        accumulator_micros: 0,
        terms: terms(Quadratic, 1_000_000),
        broker: [84_000, 0, 0, 84_000, -3_084_000, 0],
        legacy: [84_000, 6_000, 0, 90_000, -3_090_000, 6_000],
    },
    Case {
        name: "buy 0.42 x1.25 taker, quadratic (fractional quantity)",
        action: Buy,
        price_micros: 420_000,
        quantity_hundredths: 125,
        role: Taker,
        accumulator_micros: 0,
        terms: terms(Quadratic, 1_000_000),
        broker: [21_315, 85, 0, 21_400, -546_400, 85],
        legacy: [21_400, 3_600, 0, 25_000, -550_000, 3_600],
    },
    Case {
        name: "sell 0.73 x33 taker, quadratic",
        action: Sell,
        price_micros: 730_000,
        quantity_hundredths: 3_300,
        role: Taker,
        accumulator_micros: 0,
        terms: terms(Quadratic, 1_000_000),
        broker: [455_301, 99, 0, 455_400, 23_634_600, 99],
        legacy: [455_400, 4_600, 0, 460_000, 23_630_000, 4_600],
    },
    Case {
        name: "buy 0.73 x33 taker, quadratic",
        action: Buy,
        price_micros: 730_000,
        quantity_hundredths: 3_300,
        role: Taker,
        accumulator_micros: 0,
        terms: terms(Quadratic, 1_000_000),
        broker: [455_301, 99, 0, 455_400, -24_545_400, 99],
        legacy: [455_400, 4_600, 0, 460_000, -24_550_000, 4_600],
    },
    Case {
        name: "buy 0.99 x100 taker, quadratic, multiplier 1.25",
        action: Buy,
        price_micros: 990_000,
        quantity_hundredths: 10_000,
        role: Taker,
        accumulator_micros: 0,
        terms: terms(Quadratic, 1_250_000),
        broker: [86_625, 75, 0, 86_700, -99_086_700, 75],
        legacy: [86_700, 3_300, 0, 90_000, -99_090_000, 3_300],
    },
    Case {
        name: "buy 0.505 x1 maker exempt: legacy rebate makes the net fee negative",
        action: Buy,
        price_micros: 505_000,
        quantity_hundredths: 100,
        role: Maker,
        accumulator_micros: 9_900,
        terms: terms(Quadratic, 1_000_000),
        broker: [0, 0, 0, 0, -505_000, 9_900],
        legacy: [0, 5_000, 10_000, -5_000, -500_000, 4_900],
    },
    Case {
        name: "buy 0.01 x1 maker with a carried accumulator: rebate capped at the fill's fee",
        action: Buy,
        price_micros: 10_000,
        quantity_hundredths: 100,
        role: Maker,
        accumulator_micros: 9_900,
        terms: terms(QuadraticWithMakerFees, 1_000_000),
        broker: [174, 26, 200, 0, -10_000, 9_726],
        legacy: [200, 9_800, 10_000, 0, -10_000, 9_700],
    },
];

fn charge_of(case: &Case) -> Charge {
    let charge = calculate_fill_fee_micros(
        case.action.clone(),
        case.price_micros,
        ContractQuantity::from_hundredths(case.quantity_hundredths),
        case.role,
        case.accumulator_micros,
        case.terms,
    )
    .unwrap_or_else(|error| panic!("{}: {error}", case.name));
    [
        i128::from(charge.trade_fee_micros),
        i128::from(charge.rounding_fee_micros),
        i128::from(charge.rebate_micros),
        i128::from(charge.net_fee_micros),
        charge.posted_balance_change_micros,
        i128::from(charge.fee_accumulator_micros),
    ]
}

/// The legacy schedule restated in microdollars from the Broker's exact trade fee: the trade
/// fee rounds up to $0.0001 instead of $0.000001, posted cash rounds down to $0.01 instead of
/// $0.0001, and the rebate is not capped at the fill's fee. Nothing else differs.
fn legacy_schedule(case: &Case) -> Charge {
    let exact_trade = calculate_trade_fee_micros(
        case.price_micros,
        ContractQuantity::from_hundredths(case.quantity_hundredths),
        case.role,
        case.terms,
    )
    .unwrap();
    let trade = i128::from(exact_trade.div_ceil(100) * 100);
    let principal = i128::from(case.price_micros) * i128::from(case.quantity_hundredths) / 100;
    let revenue = match case.action {
        Buy => -principal,
        Sell => principal,
    };
    let unposted = revenue - trade;
    let rounding = unposted - unposted.div_euclid(10_000) * 10_000;
    let accumulated = i128::from(case.accumulator_micros) + rounding;
    let rebate = accumulated.div_euclid(10_000) * 10_000;
    let net = trade + rounding - rebate;
    [
        trade,
        rounding,
        rebate,
        net,
        revenue - net,
        accumulated - rebate,
    ]
}

#[test]
fn fill_fees_are_the_broker_charge_and_differ_from_legacy_only_by_the_documented_rounding() {
    for case in CASES {
        assert_eq!(charge_of(case), case.broker, "{}: Broker charge", case.name);
        assert_eq!(
            legacy_schedule(case),
            case.legacy,
            "{}: legacy result",
            case.name
        );
        let [trade, _, _, net, posted, _] = case.broker;
        let principal = i128::from(case.price_micros) * i128::from(case.quantity_hundredths) / 100;
        let revenue = if case.action == Buy {
            -principal
        } else {
            principal
        };
        assert_eq!(
            posted,
            revenue - net,
            "{}: cash is revenue less fee",
            case.name
        );
        assert!(
            net >= 0 && net <= trade + 99,
            "{}: net fee bounds",
            case.name
        );
    }
}

#[test]
fn trade_fee_rounds_up_to_the_microdollar() {
    // Legacy `calculate_trade_fee` rounded up to $0.0001: 0.0147, 1.75, 0.0329, 0.084, 0.0004.
    for (price, hundredths, role, terms, broker, legacy) in [
        (
            300_000,
            100,
            Taker,
            terms(QuadraticWithMakerFees, 1_000_000),
            14_700,
            14_700,
        ),
        (
            500_000,
            10_000,
            Taker,
            terms(QuadraticWithMakerFees, 1_000_000),
            1_750_000,
            1_750_000,
        ),
        (
            250_000,
            1_000,
            Maker,
            terms(QuadraticWithMakerFees, 1_000_000),
            32_813,
            32_900,
        ),
        (
            600_000,
            500,
            Taker,
            terms(Quadratic, 1_000_000),
            84_000,
            84_000,
        ),
        (5_500, 100, Taker, terms(Quadratic, 1_000_000), 383, 400),
    ] {
        let fee = calculate_trade_fee_micros(
            price,
            ContractQuantity::from_hundredths(hundredths),
            role,
            terms,
        )
        .unwrap();
        assert_eq!(fee, broker);
        assert_eq!(fee.div_ceil(100) * 100, legacy);
    }
    assert!(
        calculate_trade_fee_micros(
            1_000_001,
            ContractQuantity::from_hundredths(100),
            Taker,
            terms(Quadratic, 1_000_000)
        )
        .is_err()
    );
    assert!(
        calculate_trade_fee_micros(
            500_000,
            ContractQuantity::from_hundredths(-100),
            Taker,
            terms(Quadratic, 1_000_000)
        )
        .is_err(),
        "legacy computed a negative fee for a negative quantity; the Broker has none"
    );
}

#[test]
fn fee_rounding_carries_the_accumulator_and_caps_the_rebate() {
    // Legacy `apply_fee_rounding(-0.055, 0.0085, acc)` posted -0.07 then -0.06 (a whole-cent
    // grid). On the $0.0001 grid the same inputs round nothing.
    let first = apply_fee_rounding_micros(-55_000, 8_500, 0).unwrap();
    assert_eq!(
        (
            first.rounding_fee_micros,
            first.rebate_micros,
            first.net_fee_micros
        ),
        (0, 0, 8_500)
    );
    assert_eq!(first.posted_balance_change_micros, -63_500);

    let mut accumulator = 0;
    let mut posted = Vec::new();
    for _ in 0..3 {
        let fill = apply_fee_rounding_micros(-55_000, 8_535, accumulator).unwrap();
        accumulator = fill.fee_accumulator_micros;
        posted.push((
            fill.rounding_fee_micros,
            fill.rebate_micros,
            fill.net_fee_micros,
            fill.posted_balance_change_micros,
            accumulator,
        ));
    }
    assert_eq!(
        posted,
        [
            (65, 0, 8_600, -63_600, 65),
            (65, 100, 8_500, -63_500, 30),
            (65, 0, 8_600, -63_600, 95),
        ]
    );
    // A fill with no fee cannot draw a rebate from the accumulator.
    let exempt = apply_fee_rounding_micros(-505_050, 0, 9_900).unwrap();
    assert_eq!(exempt.rounding_fee_micros, 50);
    assert_eq!(exempt.rebate_micros, 0);
    assert_eq!(exempt.net_fee_micros, 50);
    assert_eq!(exempt.fee_accumulator_micros, 9_950);
    assert!(apply_fee_rounding_micros(i128::MIN, 1, 0).is_err());
}

/// The Broker's own vectors: traderv3 `compose.rs`
/// `v5_market_buy_reserves_typed_cap_and_rounded_fee_authority` and the `decision_contract`
/// fill test (a $0.99 buy fits a $1 budget; half a contract filled at $0.50 leaves $0.7412).
#[test]
fn reservation_and_commitment_match_the_broker_admission() {
    for (cap, hundredths, fee_type, reservation, commitment) in [
        (990_000, 100, Quadratic, 800, 990_800),
        (400_000, 2, Flat, 300, 8_300),
        (990_000, 10_000, Quadratic, 70_100, 99_070_100),
        (400_000, 200, Flat, 16_900, 816_900),
        (420_000, 125, Quadratic, 21_500, 546_500),
        (600_000, 500, QuadraticWithMakerFees, 84_600, 3_084_600),
    ] {
        let quantity = ContractQuantity::from_hundredths(hundredths);
        let terms = terms(fee_type, 1_000_000);
        assert_eq!(
            buy_fee_reservation_micros(cap, quantity, terms).unwrap(),
            reservation
        );
        assert_eq!(
            reserve_direct_member_buy_fee_micros(cap, hundredths as u64, fee_type, 1_000_000)
                .unwrap(),
            reservation
        );
        assert_eq!(
            buy_commitment_micros(cap, quantity, terms).unwrap(),
            commitment
        );
    }
    let quadratic = terms(Quadratic, 1_000_000);
    let high = buy_fee_reservation_micros(
        990_000,
        ContractQuantity::from_hundredths(1),
        terms(Quadratic, 100_000_000),
    )
    .unwrap();
    assert!(9_900 + high >= 5_000 + 17_500);

    let fill = calculate_fill_fee_micros(
        Buy,
        500_000,
        ContractQuantity::from_hundredths(50),
        Taker,
        0,
        quadratic,
    )
    .unwrap();
    assert_eq!(1_000_000 + fill.posted_balance_change_micros, 741_200);

    for (cap, hundredths) in [(990_000, 0), (0, 100), (990_000, -100), (420_050, 1)] {
        assert!(
            buy_commitment_micros(
                cap,
                ContractQuantity::from_hundredths(hundredths),
                quadratic
            )
            .is_err(),
            "the Broker refuses cap={cap} quantity={hundredths}"
        );
    }
    assert!(
        buy_commitment_micros(1_000_001, ContractQuantity::from_hundredths(100), quadratic)
            .is_err()
    );
}

#[test]
fn fee_terms_come_from_the_market_authority_without_defaults() {
    assert_eq!(
        FeeTerms::from_market("quadratic", Some(1_250_000)).unwrap(),
        terms(Quadratic, 1_250_000)
    );
    assert_eq!(
        FeeTerms::from_market("quadratic_with_maker_fees", Some(0)).unwrap(),
        terms(QuadraticWithMakerFees, 0)
    );
    assert_eq!(
        FeeTerms::from_market("flat", Some(1_000_000)).unwrap(),
        terms(Flat, 1_000_000)
    );
    assert_eq!(
        FeeTerms::from_market("unknown", Some(1_000_000)),
        Err(FeeError::UnknownFeeType("unknown".to_owned()))
    );
    assert!(FeeTerms::from_market(" quadratic", Some(1_000_000)).is_err());
    assert!(FeeTerms::from_market("quadratic", None).is_err());
    assert!(FeeTerms::from_market("quadratic", Some(-1)).is_err());

    let market = MarketState {
        fee_type: "flat".to_owned(),
        fee_multiplier_millionths: Some(1_000_000),
        ..MarketState::default()
    };
    assert_eq!(market.fee_terms().unwrap(), terms(Flat, 1_000_000));
    assert!(MarketState::default().fee_terms().is_err());
}

#[test]
fn price_micros_matches_the_host_conversion() {
    assert_eq!(price_micros(0.0).unwrap(), 0);
    assert_eq!(price_micros(0.42).unwrap(), 420_000);
    assert_eq!(price_micros(0.5555).unwrap(), 555_500);
    assert_eq!(price_micros(0.1234567).unwrap(), 123_457);
    assert_eq!(price_micros(1.0).unwrap(), 1_000_000);
    for invalid in [-0.01, 1.01, f64::NAN, f64::INFINITY] {
        assert!(price_micros(invalid).is_err());
    }
}

// The direct-member vectors below were first pinned through the legacy crate's
// `calculate_direct_member_fill_fee_micros` wrapper; they now live with the kernel.

#[test]
fn direct_member_fee_precision_preserves_signed_fractional_revenue() {
    for (action, price, quantity, trade, rounding, net, change) in [
        (Buy, 55_000, 100, 3_639, 61, 3_700, -58_700),
        (Sell, 55_000, 100, 3_639, 61, 3_700, 51_300),
        (Buy, 555_500, 1, 173, 72, 245, -5_800),
        (Sell, 555_500, 1, 173, 82, 255, 5_300),
        (Buy, 600_000, 500, 84_000, 0, 84_000, -3_084_000),
    ] {
        let exact = calculate_direct_member_fill_fee_micros(
            action, price, quantity, Taker, 0, Quadratic, 1_000_000,
        )
        .unwrap();
        assert_eq!(exact.trade_fee_micros, trade);
        assert_eq!(exact.rounding_fee_micros, rounding);
        assert_eq!(exact.net_fee_micros, net);
        assert_eq!(exact.posted_balance_change_micros, change);
        assert_eq!(exact.fee_accumulator_micros, rounding);
    }
    for (fee_type, role, multiplier, expected) in [
        (Flat, Taker, 1_000_000, 42_000),
        (QuadraticWithMakerFees, Maker, 1_000_000, 21_000),
        (Quadratic, Taker, 1_250_000, 105_000),
    ] {
        let exact = calculate_direct_member_fill_fee_micros(
            Buy, 600_000, 500, role, 0, fee_type, multiplier,
        )
        .unwrap();
        assert_eq!(exact.trade_fee_micros, expected);
        assert_eq!(exact.net_fee_micros, expected);
    }
    // Above f64's consecutive-integer range and odd: nothing may pass through a float.
    let exact = calculate_direct_member_fill_fee_micros(
        Buy,
        500_000,
        90_071_992_547_409,
        Taker,
        0,
        Quadratic,
        1_000_000,
    )
    .unwrap();
    assert_eq!(exact.trade_fee_micros, 15_762_598_695_796_575);
    assert_eq!(exact.net_fee_micros, 15_762_598_695_796_600);
    assert_eq!(exact.posted_balance_change_micros, -466_122_561_432_841_600);
    assert_eq!(exact.fee_accumulator_micros, 25);
    // The unreduced numerator exceeds u128, but all monetary outputs are representable.
    let large = calculate_direct_member_fill_fee_micros(
        Sell,
        500_000,
        3_000_000_000_000_000,
        Taker,
        0,
        Quadratic,
        10_000_000,
    )
    .unwrap();
    assert_eq!(large.trade_fee_micros, 5_250_000_000_000_000_000);
    assert_eq!(
        large.posted_balance_change_micros,
        9_750_000_000_000_000_000
    );
    for (price, quantity, multiplier) in [
        (1_000_001, 1, 1_000_000),
        (1, 1, 1_000_000),
        (500_000, u64::MAX, u64::MAX),
        (500_000, 1_000_000_000_000_000, u64::MAX),
    ] {
        assert!(
            calculate_direct_member_fill_fee_micros(
                Buy, price, quantity, Taker, 0, Quadratic, multiplier,
            )
            .is_err(),
            "invalid price, sub-micro principal and overflow must reject"
        );
    }
}

#[test]
fn direct_member_reservation_covers_fragmented_and_mixed_role_fills() {
    use calculate_direct_member_fill_fee_micros as fill;
    use reserve_direct_member_buy_fee_micros as reserve;
    for (cap, quantity, expected) in [
        (10_100, 100, 10_000),
        (600_000, 1_729, 292_300),
        (600_000, 500, 84_600),
        (990_000, 100, 800),
    ] {
        assert_eq!(
            reserve(cap, quantity, Quadratic, 1_000_000).unwrap(),
            expected
        );
    }
    let mut carried = 0;
    let mut paid = 0;
    for _ in 0..100 {
        let charge = fill(Buy, 10_100, 1, Taker, carried, Quadratic, 1_000_000).unwrap();
        carried = charge.fee_accumulator_micros;
        paid += charge.net_fee_micros;
    }
    assert_eq!(paid, 9_900);
    assert!(paid <= reserve(10_100, 100, Quadratic, 1_000_000).unwrap());
    let aggregate = fill(Buy, 10_100, 100, Taker, 0, Quadratic, 1_000_000).unwrap();
    assert!(
        paid > aggregate.net_fee_micros,
        "an aggregate charge is not a safe fill-split reserve"
    );

    for curve in [Quadratic, QuadraticWithMakerFees, Flat] {
        for multiplier in [0, 1_000_000, 1_250_000, 10_000_000, 100_000_000] {
            for cap in [
                10_100, 10_150, 400_000, 555_500, 600_000, 990_000, 1_000_000,
            ] {
                let prices: Vec<_> = (100..=cap)
                    .step_by(100)
                    .chain([cap, cap - 1, cap / 2 + 37])
                    .map(|price| {
                        let quantum = (1..=100).find(|count| price * count % 100 == 0).unwrap();
                        (price, quantum)
                    })
                    .collect();
                let mut budgets = std::collections::BTreeMap::new();
                // Each price gets its own order; cheaper earlier fills must not conceal an
                // insufficient reserve at a later, expensive price.
                for (price, quantum) in prices {
                    let total_quantity = 12 * quantum;
                    let budget = *budgets.entry(quantum).or_insert_with(|| {
                        cap * total_quantity / 100
                            + reserve(cap, total_quantity, curve, multiplier).unwrap()
                    });
                    for roles in [[Maker; 4], [Taker; 4], [Maker, Taker, Taker, Maker]] {
                        let mut carried = 0;
                        let mut spent = 0;
                        let mut quantity = 0;
                        for (role, count) in roles.into_iter().zip([1, 1, 7, 3]) {
                            let count = count * quantum;
                            let charge =
                                fill(Buy, price, count, role, carried, curve, multiplier).unwrap();
                            carried = charge.fee_accumulator_micros;
                            spent += u64::try_from(-charge.posted_balance_change_micros).unwrap();
                            quantity += count;
                            assert!(
                                spent + cap * (total_quantity - quantity) / 100 <= budget,
                                "curve={curve:?}, multiplier={multiplier}, cap={cap}, price={price}, quantity={quantity}"
                            );
                        }
                    }
                }
            }
        }
    }
    assert_eq!(reserve(500_000, 0, Quadratic, 1_000_000).unwrap(), 0);
    assert!(reserve(1_000_001, 1, Quadratic, 1_000_000).is_err());
    assert!(reserve(500_000, u64::MAX, Quadratic, 1_000_000).is_err());
    assert!(reserve(500_000, 10_000, Quadratic, u64::MAX).is_err());
}

#[test]
fn direct_member_rebates_remain_aligned_and_cannot_make_a_fill_fee_negative() {
    let mut accumulator = 0;
    for (role, price, trade, rounding, rebate, net, change, carried) in [
        (Taker, 5_500, 4, 41, 0, 45, -100, 41),
        (Maker, 5_500, 0, 45, 0, 45, -100, 86),
        // A $0.0001 refund here would make this fill's fee negative. Retain it instead.
        (Maker, 5_500, 0, 45, 0, 45, -100, 131),
        (Taker, 500_000, 175, 25, 100, 100, -5_100, 56),
    ] {
        let exact = calculate_direct_member_fill_fee_micros(
            Buy,
            price,
            1,
            role,
            accumulator,
            Quadratic,
            1_000_000,
        )
        .unwrap();
        assert_eq!(exact.trade_fee_micros, trade);
        assert_eq!(exact.rounding_fee_micros, rounding);
        assert_eq!(exact.rebate_micros, rebate);
        assert_eq!(exact.net_fee_micros, net);
        assert_eq!(exact.posted_balance_change_micros, change);
        assert_eq!(exact.fee_accumulator_micros, carried);
        accumulator = exact.fee_accumulator_micros;
    }
    let carried = calculate_direct_member_fill_fee_micros(
        Buy,
        5_500,
        1,
        Maker,
        9_007_199_254_740_902,
        Quadratic,
        1_000_000,
    )
    .unwrap();
    assert_eq!(carried.rebate_micros, 0);
    assert_eq!(carried.fee_accumulator_micros, 9_007_199_254_740_947);
    assert!(
        calculate_direct_member_fill_fee_micros(
            Buy,
            5_500,
            1,
            Maker,
            u64::MAX - 44,
            Quadratic,
            1_000_000,
        )
        .is_err(),
        "accumulator overflow must reject before committing a charge"
    );
}
