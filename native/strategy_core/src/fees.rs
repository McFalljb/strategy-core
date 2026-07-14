use std::{error::Error, fmt, str::FromStr};

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::broker::Action;

pub type FeeResult<T> = Result<T, FeeError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FeeError {
    UnknownFeeType(String),
    InvalidDecimal(String),
}

impl fmt::Display for FeeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFeeType(value) => write!(formatter, "unknown Kalshi fee type: {value}"),
            Self::InvalidDecimal(value) => write!(formatter, "invalid decimal value: {value}"),
        }
    }
}

impl Error for FeeError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LiquidityRole {
    Maker,
    Taker,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeeType {
    Quadratic,
    QuadraticWithMakerFees,
    Flat,
}

impl FromStr for FeeType {
    type Err = FeeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "quadratic" => Ok(Self::Quadratic),
            "quadratic_with_maker_fees" => Ok(Self::QuadraticWithMakerFees),
            "flat" => Ok(Self::Flat),
            _ => Err(FeeError::UnknownFeeType(value.to_string())),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FeeCalculation {
    pub trade_fee: f64,
    pub rounding_fee: f64,
    pub rebate: f64,
    pub net_fee: f64,
    pub posted_balance_change: f64,
    pub fee_accumulator: f64,
}

pub fn calculate_trade_fee(
    price: f64,
    quantity: i64,
    liquidity_role: LiquidityRole,
    fee_type: Option<FeeType>,
    fee_multiplier: Option<f64>,
) -> FeeResult<f64> {
    let price = decimal_from_f64(price)?;
    let quantity = Decimal::from(quantity);
    let fee =
        calculate_trade_fee_decimal(price, quantity, liquidity_role, fee_type, fee_multiplier)?;
    Ok(decimal_to_f64(fee))
}

fn calculate_trade_fee_decimal(
    price: Decimal,
    quantity: Decimal,
    liquidity_role: LiquidityRole,
    fee_type: Option<FeeType>,
    fee_multiplier: Option<f64>,
) -> FeeResult<Decimal> {
    let fee_type = fee_type.unwrap_or(FeeType::QuadraticWithMakerFees);
    let multiplier = resolve_fee_multiplier(liquidity_role, fee_type, fee_multiplier)?;
    let raw_fee = raw_trade_fee(price, quantity, multiplier, fee_type);
    Ok(ceil_to_increment(raw_fee, centicent()))
}

pub fn apply_fee_rounding(
    revenue: f64,
    trade_fee: f64,
    fee_accumulator: f64,
) -> FeeResult<FeeCalculation> {
    let revenue = decimal_from_f64(revenue)?;
    let rounded_trade_fee = ceil_to_increment(decimal_from_f64(trade_fee)?, centicent());
    let mut accumulator = decimal_from_f64(fee_accumulator)?;
    Ok(apply_fee_rounding_decimal(
        revenue,
        rounded_trade_fee,
        &mut accumulator,
    ))
}

fn apply_fee_rounding_decimal(
    revenue: Decimal,
    rounded_trade_fee: Decimal,
    accumulator: &mut Decimal,
) -> FeeCalculation {
    let balance_change = revenue - rounded_trade_fee;
    let floored_balance_change = floor_to_increment(balance_change, cent());
    let rounding_fee = balance_change - floored_balance_change;

    *accumulator += rounding_fee;
    let rebate = floor_to_increment(*accumulator, cent());
    *accumulator -= rebate;

    let net_fee = rounded_trade_fee + rounding_fee - rebate;
    let posted_balance_change = revenue - net_fee;

    FeeCalculation {
        trade_fee: decimal_to_f64(rounded_trade_fee),
        rounding_fee: decimal_to_f64(rounding_fee),
        rebate: decimal_to_f64(rebate),
        net_fee: decimal_to_f64(net_fee),
        posted_balance_change: decimal_to_f64(posted_balance_change),
        fee_accumulator: decimal_to_f64(*accumulator),
    }
}

pub fn calculate_fill_fee(
    action: Action,
    price: f64,
    quantity: i64,
    liquidity_role: LiquidityRole,
    fee_accumulator: f64,
    fee_type: Option<FeeType>,
    fee_multiplier: Option<f64>,
) -> FeeResult<FeeCalculation> {
    let price_decimal = decimal_from_f64(price)?;
    let quantity_decimal = Decimal::from(quantity);
    let mut revenue = price_decimal * quantity_decimal;
    if action == Action::Buy {
        revenue = -revenue;
    }

    let trade_fee = calculate_trade_fee_decimal(
        price_decimal,
        quantity_decimal,
        liquidity_role,
        fee_type,
        fee_multiplier,
    )?;
    let mut fee_accumulator = decimal_from_f64(fee_accumulator)?;
    Ok(apply_fee_rounding_decimal(
        revenue,
        trade_fee,
        &mut fee_accumulator,
    ))
}

fn resolve_fee_multiplier(
    liquidity_role: LiquidityRole,
    fee_type: FeeType,
    fee_multiplier: Option<f64>,
) -> FeeResult<Decimal> {
    let multiplier = fee_multiplier
        .map(decimal_from_f64)
        .transpose()?
        .unwrap_or_else(one);

    let base = match (fee_type, liquidity_role) {
        (FeeType::Quadratic, LiquidityRole::Maker) => zero(),
        (FeeType::Quadratic, LiquidityRole::Taker) => general_taker_multiplier(),
        (FeeType::QuadraticWithMakerFees, LiquidityRole::Taker) => general_taker_multiplier(),
        (FeeType::QuadraticWithMakerFees, LiquidityRole::Maker) => general_maker_multiplier(),
        (FeeType::Flat, LiquidityRole::Maker) => zero(),
        (FeeType::Flat, LiquidityRole::Taker) => flat_taker_multiplier(),
    };
    Ok(base * multiplier)
}

fn raw_trade_fee(
    price: Decimal,
    quantity: Decimal,
    multiplier: Decimal,
    fee_type: FeeType,
) -> Decimal {
    match fee_type {
        FeeType::Quadratic | FeeType::QuadraticWithMakerFees | FeeType::Flat => {
            multiplier * quantity * price * (one() - price)
        }
    }
}

fn decimal_from_f64(value: f64) -> FeeResult<Decimal> {
    Decimal::from_str(&value.to_string()).map_err(|_| FeeError::InvalidDecimal(value.to_string()))
}

fn decimal_to_f64(value: Decimal) -> f64 {
    value
        .to_string()
        .parse::<f64>()
        .expect("Decimal string should parse as f64")
}

fn ceil_to_increment(value: Decimal, increment: Decimal) -> Decimal {
    let units = (value / increment).ceil();
    units * increment
}

fn floor_to_increment(value: Decimal, increment: Decimal) -> Decimal {
    let units = (value / increment).floor();
    units * increment
}

const fn zero() -> Decimal {
    Decimal::ZERO
}

const fn one() -> Decimal {
    Decimal::ONE
}

fn cent() -> Decimal {
    Decimal::new(1, 2)
}

fn centicent() -> Decimal {
    Decimal::new(1, 4)
}

fn general_taker_multiplier() -> Decimal {
    Decimal::new(7, 2)
}

fn general_maker_multiplier() -> Decimal {
    Decimal::new(175, 4)
}

fn flat_taker_multiplier() -> Decimal {
    Decimal::new(35, 3)
}
