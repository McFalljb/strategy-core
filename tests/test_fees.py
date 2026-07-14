"""Tests for Kalshi fee schedule and fee-rounding helpers."""

import pytest

from strategy_core.fees import apply_fee_rounding, calculate_fill_fee


def test_general_taker_fee_matches_fee_schedule_examples() -> None:
    assert (
        calculate_fill_fee(
            action="buy",
            price=0.30,
            quantity=1,
            liquidity_role="taker",
        ).net_fee
        == 0.02
    )
    assert (
        calculate_fill_fee(
            action="buy",
            price=0.50,
            quantity=100,
            liquidity_role="taker",
        ).net_fee
        == 1.75
    )
    assert (
        calculate_fill_fee(
            action="buy",
            price=0.90,
            quantity=1,
            liquidity_role="taker",
        ).net_fee
        == 0.01
    )


def test_fee_rounding_accumulator_applies_rebate_once_whole_cent_is_reached() -> None:
    first = apply_fee_rounding(revenue=-0.055, trade_fee=0.0085)
    second = apply_fee_rounding(revenue=-0.055, trade_fee=0.0085, fee_accumulator=first.fee_accumulator)
    third = apply_fee_rounding(revenue=-0.055, trade_fee=0.0085, fee_accumulator=second.fee_accumulator)

    assert first.rounding_fee == 0.0065
    assert first.rebate == 0.0
    assert first.net_fee == 0.015
    assert first.posted_balance_change == -0.07

    assert second.rounding_fee == 0.0065
    assert second.rebate == 0.01
    assert second.net_fee == 0.005
    assert second.posted_balance_change == -0.06

    assert third.rounding_fee == 0.0065
    assert third.rebate == 0.0
    assert third.net_fee == 0.015
    assert third.posted_balance_change == -0.07


def test_fee_multiplier_scales_quadratic_fees() -> None:
    default_fee = calculate_fill_fee(
        action="buy",
        price=0.30,
        quantity=10,
        liquidity_role="taker",
        fee_type="quadratic",
        fee_multiplier=1.0,
    )
    doubled_fee = calculate_fill_fee(
        action="buy",
        price=0.30,
        quantity=10,
        liquidity_role="taker",
        fee_type="quadratic",
        fee_multiplier=2.0,
    )

    assert doubled_fee.trade_fee == pytest.approx(default_fee.trade_fee * 2)
    assert doubled_fee.net_fee > default_fee.net_fee


def test_flat_taker_fee_matches_specific_fee_schedule_examples() -> None:
    assert (
        calculate_fill_fee(
            action="buy",
            price=0.30,
            quantity=100,
            liquidity_role="taker",
            fee_type="flat",
            fee_multiplier=1.0,
        ).net_fee
        == 0.74
    )
    assert (
        calculate_fill_fee(
            action="buy",
            price=0.50,
            quantity=100,
            liquidity_role="taker",
            fee_type="flat",
            fee_multiplier=1.0,
        ).net_fee
        == 0.88
    )


def test_maker_fee_exemptions_and_unknown_fee_type() -> None:
    default_maker = calculate_fill_fee(
        action="buy",
        price=0.25,
        quantity=10,
        liquidity_role="maker",
    )
    explicit_maker = calculate_fill_fee(
        action="buy",
        price=0.25,
        quantity=10,
        liquidity_role="maker",
        fee_type="quadratic_with_maker_fees",
    )

    assert default_maker.trade_fee == pytest.approx(0.0329)
    assert default_maker.rounding_fee == pytest.approx(0.0071)
    assert default_maker.net_fee == pytest.approx(0.04)
    assert default_maker.posted_balance_change == pytest.approx(-2.54)
    assert default_maker.fee_accumulator == pytest.approx(0.0071)
    assert explicit_maker == default_maker

    assert (
        calculate_fill_fee(
            action="buy",
            price=0.25,
            quantity=10,
            liquidity_role="maker",
            fee_type="quadratic",
        ).net_fee
        == 0.0
    )
    assert (
        calculate_fill_fee(
            action="buy",
            price=0.25,
            quantity=10,
            liquidity_role="maker",
            fee_type="flat",
            fee_multiplier=1.0,
        ).net_fee
        == 0.0
    )
    with pytest.raises(ValueError, match="unknown Kalshi fee type"):
        calculate_fill_fee(
            action="buy",
            price=0.25,
            quantity=10,
            liquidity_role="taker",
            fee_type="unknown",
        )


@pytest.mark.parametrize("price", [float("nan"), float("inf"), float("-inf")])
def test_non_finite_fee_inputs_are_rejected(price: float) -> None:
    with pytest.raises(ValueError, match="invalid decimal value"):
        calculate_fill_fee(
            action="buy",
            price=price,
            quantity=1,
            liquidity_role="taker",
        )
