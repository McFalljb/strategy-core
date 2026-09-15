//! Exact direct-member execution fees and order-wide rounding credits.

use std::{error::Error, fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::actions::OrderAction;

pub type FeeResult<T> = Result<T, FeeError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FeeError {
    UnknownFeeType(String),
    InvalidDecimal(String),
    InvalidInput(&'static str),
}

impl fmt::Display for FeeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFeeType(value) => write!(formatter, "unknown Kalshi fee type: {value}"),
            Self::InvalidDecimal(value) => write!(formatter, "invalid decimal value: {value}"),
            Self::InvalidInput(value) => write!(formatter, "invalid fee input: {value}"),
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

impl FeeType {
    pub const fn base_rate_millionths(self, role: LiquidityRole) -> u64 {
        match (self, role) {
            (Self::Quadratic | Self::Flat, LiquidityRole::Maker) => 0,
            (Self::Quadratic | Self::QuadraticWithMakerFees, LiquidityRole::Taker) => 70_000,
            (Self::QuadraticWithMakerFees, LiquidityRole::Maker) => 17_500,
            (Self::Flat, LiquidityRole::Taker) => 35_000,
        }
    }
}

/// Exact execution amounts. Principal is not a fee; the posted cash change includes both.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FeeCalculationMicros {
    pub trade_fee_micros: u64,
    pub rounding_fee_micros: u64,
    pub rebate_micros: u64,
    pub net_fee_micros: u64,
    pub posted_balance_change_micros: i128,
    pub fee_accumulator_micros: u64,
}

/// Selected direct-member rule: ceil each trade fee to $0.000001, floor signed
/// cash changes to $0.0001, and cap grid-aligned rebates at this fill's fee.
/// Keep the returned accumulator across this order's fills and liquidity roles.
/// Cancellation does not create an extra rebate. No monetary value passes through f64.
pub fn calculate_direct_member_fill_fee_micros(
    action: OrderAction,
    price_micros: u64,
    quantity_hundredths: u64,
    liquidity_role: LiquidityRole,
    fee_accumulator_micros: u64,
    fee_type: FeeType,
    fee_multiplier_millionths: u64,
) -> FeeResult<FeeCalculationMicros> {
    const MICROS: u64 = 1_000_000;
    if price_micros > MICROS {
        return Err(FeeError::InvalidInput("fill price exceeds one dollar"));
    }
    let product = u128::from(price_micros) * u128::from(quantity_hundredths);
    if product % 100 != 0 {
        return Err(FeeError::InvalidInput(
            "fill principal is not exact in microdollars",
        ));
    }
    let principal = u64::try_from(product / 100)
        .map_err(|_| FeeError::InvalidInput("fill principal exceeds microdollar range"))?;
    let trade_fee = trade_fee_micros(
        price_micros,
        quantity_hundredths,
        fee_type.base_rate_millionths(liquidity_role),
        fee_multiplier_millionths,
    )?;
    let revenue = match action {
        OrderAction::Buy => -i128::from(principal),
        OrderAction::Sell => i128::from(principal),
    };
    let unposted = revenue - i128::from(trade_fee);
    let posted = unposted.div_euclid(100) * 100;
    let rounding_fee = (unposted - posted) as u64;
    let accumulated = u128::from(fee_accumulator_micros) + u128::from(rounding_fee);
    let before_rebate = u128::from(trade_fee) + u128::from(rounding_fee);
    let rebate = (accumulated / 100 * 100).min(before_rebate / 100 * 100);
    let net_fee = u64::try_from(before_rebate - rebate)
        .map_err(|_| FeeError::InvalidInput("fee amount exceeds microdollar range"))?;
    let accumulator = u64::try_from(accumulated - rebate)
        .map_err(|_| FeeError::InvalidInput("fee accumulator exceeds microdollar range"))?;
    Ok(FeeCalculationMicros {
        trade_fee_micros: trade_fee,
        rounding_fee_micros: rounding_fee,
        rebate_micros: u64::try_from(rebate)
            .map_err(|_| FeeError::InvalidInput("fee rebate exceeds microdollar range"))?,
        net_fee_micros: net_fee,
        posted_balance_change_micros: revenue - i128::from(net_fee),
        fee_accumulator_micros: accumulator,
    })
}

/// Additional cash above cap principal for a new buy order's total spending bound.
/// Covers nonzero hundredth-sized fills at/below the cap, mixed roles and capped
/// rebates. Lower fill prices can fund higher fees: this is not a standalone fee
/// ceiling, an execution charge, or a terminal rebate. Keep the unused total
/// budget reserved until completion/cancellation. Principal is floored only for
/// computing this conservative increment, never for posting actual fills.
pub fn reserve_direct_member_buy_fee_micros(
    price_cap_micros: u64,
    quantity_hundredths: u64,
    fee_type: FeeType,
    fee_multiplier_millionths: u64,
) -> FeeResult<u64> {
    if price_cap_micros > 1_000_000 {
        return Err(FeeError::InvalidInput("order price cap exceeds one dollar"));
    }
    if quantity_hundredths == 0 || price_cap_micros == 0 {
        return Ok(0);
    }
    let rate = fee_type
        .base_rate_millionths(LiquidityRole::Maker)
        .max(fee_type.base_rate_millionths(LiquidityRole::Taker));
    let coefficient = u128::from(rate) * u128::from(fee_multiplier_millionths);
    // Maximize p/100 + fee(p, 1). The unrounded quadratic's vertex is
    // (1_000_000 + 10^18/coefficient)/2. Rate <= 70_000 bounds these products.
    let peak = if coefficient == 0 {
        price_cap_micros
    } else {
        ((1_000_000 * coefficient + 1_000_000_000_000_000_000) / (2 * coefficient))
            .min(u128::from(price_cap_micros)) as u64
    };
    // Units here are hundredths of a microdollar per quantity hundredth.
    let mut per_hundredth = u128::from(price_cap_micros.div_ceil(10_000) * 10_000);
    // For a fixed p % 100, rounding the trade fee is monotone in the unrounded
    // total. Its maximum is at one of the two lattice points around the vertex
    // (or the cap). Thus at most 200 evaluations cover every integer price.
    for residue in 0..100.min(price_cap_micros + 1) {
        let last = price_cap_micros - (price_cap_micros - residue) % 100;
        let lower = if peak < residue {
            residue
        } else {
            residue + (peak - residue) / 100 * 100
        };
        for price in [lower, (lower + 100).min(last)] {
            let fee = trade_fee_micros(price, 1, rate, fee_multiplier_millionths)?;
            per_hundredth = per_hundredth.max(u128::from(price) + 100 * u128::from(fee));
        }
    }
    // If trade+rounding < 100, posted cash cannot exceed q times the cap's
    // per-hundredth principal rounded UP to the 100-micro cash grid (the initial
    // bound above). Otherwise cost <= principal+trade+delta min(accumulator,99).
    // Also ceil(q*raw_fee) <= q*ceil(raw_fee). Summing either case from A=0
    // bounds total cost by q*per_hundredth + 99, without assuming a final rebate.
    let bound = u128::from(quantity_hundredths) * per_hundredth;
    let cash = (bound.div_ceil(100) + 99).div_ceil(100) * 100;
    let principal = u128::from(quantity_hundredths) * u128::from(price_cap_micros) / 100;
    u64::try_from(cash - principal)
        .map_err(|_| FeeError::InvalidInput("fee reservation exceeds microdollar range"))
}

fn trade_fee_micros(
    price_micros: u64,
    quantity_hundredths: u64,
    base_rate_millionths: u64,
    fee_multiplier_millionths: u64,
) -> FeeResult<u64> {
    const FEE_DENOMINATOR: u128 = 100_000_000_000_000_000_000;
    let rate = u128::from(base_rate_millionths);
    // Reduce the coefficient first so every representable u64 fee fits the wide
    // intermediate, including large quantities with non-unit multipliers.
    let mut divisor = FEE_DENOMINATOR;
    let mut remainder = rate;
    while remainder != 0 {
        (divisor, remainder) = (remainder, divisor % remainder);
    }
    let denominator = FEE_DENOMINATOR / divisor;
    let numerator = [
        rate / divisor,
        u128::from(price_micros),
        u128::from(1_000_000 - price_micros),
        u128::from(quantity_hundredths),
        u128::from(fee_multiplier_millionths),
    ]
    .into_iter()
    .try_fold(1_u128, |value, factor| value.checked_mul(factor))
    .ok_or(FeeError::InvalidInput("trade fee overflow"))?;
    u64::try_from(numerator.div_ceil(denominator))
        .map_err(|_| FeeError::InvalidInput("fee amount exceeds microdollar range"))
}
