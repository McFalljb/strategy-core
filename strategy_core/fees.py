"""Kalshi fee calculation helpers shared by portable strategies and runtimes."""

from __future__ import annotations

from dataclasses import dataclass
from decimal import ROUND_CEILING, ROUND_FLOOR, Decimal
from typing import Literal

LiquidityRole = Literal["maker", "taker"]
FeeType = Literal["quadratic", "quadratic_with_maker_fees", "flat"]

_ZERO = Decimal("0")
_ONE = Decimal("1")
_CENT = Decimal("0.01")
_CENTICENT = Decimal("0.0001")
_GENERAL_TAKER_MULTIPLIER = Decimal("0.07")
_GENERAL_MAKER_MULTIPLIER = Decimal("0.0175")
_FLAT_TAKER_MULTIPLIER = Decimal("0.035")


@dataclass(frozen=True)
class FeeCalculation:
    """Fee breakdown for a single fill."""

    trade_fee: float
    rounding_fee: float
    rebate: float
    net_fee: float
    posted_balance_change: float
    fee_accumulator: float


def calculate_trade_fee(
    *,
    price: float,
    quantity: int,
    liquidity_role: LiquidityRole,
    fee_type: FeeType | str | None = None,
    fee_multiplier: float | None = None,
) -> float:
    """Return the base Kalshi trade fee before cent-alignment rounding."""
    multiplier = _resolve_fee_multiplier(liquidity_role, fee_type, fee_multiplier)
    price_decimal = Decimal(str(price))
    quantity_decimal = Decimal(quantity)
    raw_fee = _raw_trade_fee(
        price_decimal,
        quantity_decimal,
        multiplier=multiplier,
        fee_type=fee_type,
    )
    return float(_ceil_to_increment(raw_fee, _CENTICENT))


def apply_fee_rounding(
    *,
    revenue: float,
    trade_fee: float,
    fee_accumulator: float = 0.0,
) -> FeeCalculation:
    """Apply Kalshi's cent-alignment rules to a fill."""
    revenue_decimal = Decimal(str(revenue))
    rounded_trade_fee = _ceil_to_increment(Decimal(str(trade_fee)), _CENTICENT)
    accumulator_decimal = Decimal(str(fee_accumulator))

    balance_change = revenue_decimal - rounded_trade_fee
    floored_balance_change = _floor_to_increment(balance_change, _CENT)
    rounding_fee = balance_change - floored_balance_change

    next_accumulator = accumulator_decimal + rounding_fee
    rebate = _floor_to_increment(next_accumulator, _CENT)
    next_accumulator -= rebate

    net_fee = rounded_trade_fee + rounding_fee - rebate
    posted_balance_change = revenue_decimal - net_fee

    return FeeCalculation(
        trade_fee=float(rounded_trade_fee),
        rounding_fee=float(rounding_fee),
        rebate=float(rebate),
        net_fee=float(net_fee),
        posted_balance_change=float(posted_balance_change),
        fee_accumulator=float(next_accumulator),
    )


def calculate_fill_fee(
    *,
    action: Literal["buy", "sell"],
    price: float,
    quantity: int,
    liquidity_role: LiquidityRole,
    fee_accumulator: float = 0.0,
    fee_type: FeeType | str | None = None,
    fee_multiplier: float | None = None,
) -> FeeCalculation:
    """Return the fully rounded fee/cash impact for one Kalshi fill."""
    price_decimal = Decimal(str(price))
    quantity_decimal = Decimal(quantity)
    revenue = price_decimal * quantity_decimal
    if action == "buy":
        revenue = -revenue

    trade_fee = calculate_trade_fee(
        price=price,
        quantity=quantity,
        liquidity_role=liquidity_role,
        fee_type=fee_type,
        fee_multiplier=fee_multiplier,
    )
    return apply_fee_rounding(
        revenue=float(revenue),
        trade_fee=trade_fee,
        fee_accumulator=fee_accumulator,
    )


def _resolve_fee_multiplier(
    liquidity_role: LiquidityRole,
    fee_type: FeeType | str | None,
    fee_multiplier: float | None,
) -> Decimal:
    normalized_fee_type = fee_type or "quadratic_with_maker_fees"
    multiplier = Decimal(str(fee_multiplier)) if fee_multiplier is not None else Decimal("1")

    if normalized_fee_type == "quadratic":
        if liquidity_role == "maker":
            return _ZERO
        return _GENERAL_TAKER_MULTIPLIER * multiplier

    if normalized_fee_type == "quadratic_with_maker_fees":
        if liquidity_role == "taker":
            return _GENERAL_TAKER_MULTIPLIER * multiplier
        return _GENERAL_MAKER_MULTIPLIER * multiplier

    if normalized_fee_type == "flat":
        if liquidity_role == "maker":
            return _ZERO
        return _FLAT_TAKER_MULTIPLIER * multiplier

    msg = f"unknown Kalshi fee type: {normalized_fee_type}"
    raise ValueError(msg)


def _raw_trade_fee(
    price: Decimal,
    quantity: Decimal,
    *,
    multiplier: Decimal,
    fee_type: FeeType | str | None,
) -> Decimal:
    normalized_fee_type = fee_type or "quadratic_with_maker_fees"
    if normalized_fee_type in {"quadratic", "quadratic_with_maker_fees", "flat"}:
        return multiplier * quantity * price * (_ONE - price)
    msg = f"unknown Kalshi fee type: {normalized_fee_type}"
    raise ValueError(msg)


def _ceil_to_increment(value: Decimal, increment: Decimal) -> Decimal:
    units = (value / increment).to_integral_value(rounding=ROUND_CEILING)
    return units * increment


def _floor_to_increment(value: Decimal, increment: Decimal) -> Decimal:
    units = (value / increment).to_integral_value(rounding=ROUND_FLOOR)
    return units * increment
