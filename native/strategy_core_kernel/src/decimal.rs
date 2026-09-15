//! Exact decimal digits of a provider number.

use bincode::{Decode, Encode};

/// Largest scale a supplied decimal may carry.
pub const MAX_DECIMAL_SCALE: u8 = 18;

/// A provider number that could not be captured exactly.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecimalError;

impl core::fmt::Display for DecimalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("decimal is not exactly representable")
    }
}
impl std::error::Error for DecimalError {}

/// Exact decimal digits of a provider JSON number.
///
/// `coefficient * 10^-scale`. The canonical form has no trailing fractional zeros, a scale of
/// at most [`MAX_DECIMAL_SCALE`], and zero is always `{0, 0}`. A negative zero token
/// normalizes to zero: the sign carries no provider meaning for a temperature or quantity.
#[derive(Clone, Copy, Debug, Default, Encode, Decode, Eq, PartialEq, Ord, PartialOrd)]
pub struct Decimal {
    pub coefficient: i64,
    pub scale: u8,
}

impl Decimal {
    /// Parses a JSON number's textual representation exactly.
    ///
    /// Accepts an optional sign, digits, an optional fraction and an optional exponent. The
    /// result is normalized. Values needing more than 64 signed bits of coefficient or more
    /// than the scale bound are rejected rather than rounded.
    pub fn parse(text: &str) -> Result<Self, DecimalError> {
        let (mantissa, exponent) = match text.split_once(['e', 'E']) {
            Some((mantissa, exponent)) => {
                (mantissa, exponent.parse::<i32>().map_err(|_| DecimalError)?)
            }
            None => (text, 0),
        };
        let negative = mantissa.starts_with('-');
        let unsigned = mantissa.strip_prefix(['-', '+']).unwrap_or(mantissa);
        let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
        if whole.is_empty()
            || (unsigned.contains('.') && fraction.is_empty())
            || !whole.bytes().all(|byte| byte.is_ascii_digit())
            || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(DecimalError);
        }
        let digits = format!("{whole}{fraction}");
        let mut coefficient = digits.parse::<i128>().map_err(|_| DecimalError)?;
        if negative {
            coefficient = -coefficient;
        }
        let mut scale = i32::try_from(fraction.len())
            .map_err(|_| DecimalError)?
            .checked_sub(exponent)
            .ok_or(DecimalError)?;
        if scale < -38 {
            return Err(DecimalError);
        }
        while scale < 0 {
            coefficient = coefficient.checked_mul(10).ok_or(DecimalError)?;
            scale += 1;
        }
        while scale > 0 && coefficient % 10 == 0 {
            coefficient /= 10;
            scale -= 1;
        }
        if coefficient == 0 {
            scale = 0;
        }
        let value = Self {
            coefficient: i64::try_from(coefficient).map_err(|_| DecimalError)?,
            scale: u8::try_from(scale).map_err(|_| DecimalError)?,
        };
        if !value.is_canonical() {
            return Err(DecimalError);
        }
        Ok(value)
    }

    pub fn from_i64(value: i64) -> Self {
        Self {
            coefficient: value,
            scale: 0,
        }
        .normalized()
    }

    fn normalized(self) -> Self {
        let mut coefficient = self.coefficient;
        let mut scale = self.scale;
        while scale > 0 && coefficient % 10 == 0 {
            coefficient /= 10;
            scale -= 1;
        }
        if coefficient == 0 {
            scale = 0;
        }
        Self { coefficient, scale }
    }

    pub fn is_canonical(self) -> bool {
        self.scale <= MAX_DECIMAL_SCALE
            && (self.scale == 0 || self.coefficient % 10 != 0)
            && (self.coefficient != 0 || self.scale == 0)
    }

    /// The nearest IEEE-754 double, which is the provider's original double for values captured
    /// from a shortest round-trip representation.
    pub fn to_f64(self) -> f64 {
        format!("{}e-{}", self.coefficient, self.scale)
            .parse::<f64>()
            .unwrap_or(f64::NAN)
    }
}

impl core::fmt::Display for Decimal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.scale == 0 {
            return write!(f, "{}", self.coefficient);
        }
        let negative = self.coefficient < 0;
        let digits = self.coefficient.unsigned_abs().to_string();
        let scale = usize::from(self.scale);
        let (whole, fraction) = if digits.len() > scale {
            let (whole, fraction) = digits.split_at(digits.len() - scale);
            (whole.to_owned(), fraction.to_owned())
        } else {
            ("0".to_owned(), format!("{:0>scale$}", digits))
        };
        write!(f, "{}{whole}.{fraction}", if negative { "-" } else { "" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_preserves_exact_digits_and_normalizes() {
        for (text, coefficient, scale, rendered) in [
            ("72.5", 725, 1, "72.5"),
            (
                "22.77777777777778",
                2_277_777_777_777_778,
                14,
                "22.77777777777778",
            ),
            ("80", 80, 0, "80"),
            ("80.0", 80, 0, "80"),
            ("-0", 0, 0, "0"),
            ("0.000001", 1, 6, "0.000001"),
            ("1e2", 100, 0, "100"),
            ("1.25e-3", 125, 5, "0.00125"),
            ("-3.5", -35, 1, "-3.5"),
            (
                "0.123456789012345678",
                123_456_789_012_345_678,
                18,
                "0.123456789012345678",
            ),
        ] {
            let value = Decimal::parse(text).unwrap();
            assert_eq!(
                (value.coefficient, value.scale),
                (coefficient, scale),
                "{text}"
            );
            assert_eq!(value.to_string(), rendered, "{text}");
            assert_eq!(value.to_f64(), text.parse::<f64>().unwrap(), "{text}");
        }
        for text in [
            "",
            "abc",
            "1.",
            ".5",
            "1e400",
            "99999999999999999999",
            "1.2.3",
            "0.1234567890123456789",
        ] {
            assert!(Decimal::parse(text).is_err(), "{text}");
        }
        for (coefficient, scale) in [(10, 1), (0, 1), (1, 19)] {
            assert!(!Decimal { coefficient, scale }.is_canonical());
        }
    }
}
