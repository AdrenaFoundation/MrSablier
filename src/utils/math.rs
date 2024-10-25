use {
    super::BPS_POWER,
    anyhow::{anyhow, Result},
    std::fmt::Display,
};
pub fn checked_float_div<T>(arg1: T, arg2: T) -> Result<T>
where
    T: num_traits::Float + Display,
{
    if arg2 == T::zero() {
        log::info!("Error: Overflow in {} / {}", arg1, arg2);
        return Err(anyhow!("Math overflow"));
    }

    let res = arg1 / arg2;

    if !res.is_finite() {
        log::info!("Error: Overflow in {} / {}", arg1, arg2);
        Err(anyhow!("Math overflow"))
    } else {
        Ok(res)
    }
}

pub fn checked_ceil_div<T>(arg1: T, arg2: T) -> Result<T>
where
    T: num_traits::PrimInt + Display + num_traits::Unsigned,
{
    if arg1 > T::zero() {
        if arg1 == arg2 && arg2 != T::zero() {
            return Ok(T::one());
        }

        if let Some(res) = (arg1 - T::one()).checked_div(&arg2) {
            Ok(res + T::one())
        } else {
            log::info!("Error: Overflow in {} / {}", arg1, arg2);
            Err(anyhow!("Math overflow"))
        }
    } else if let Some(res) = arg1.checked_div(&arg2) {
        Ok(res)
    } else {
        log::info!("Error: Overflow in {} / {}", arg1, arg2);
        Err(anyhow!("Math overflow"))
    }
}
pub fn checked_decimal_div(
    coefficient1: u64,
    exponent1: i32,
    coefficient2: u64,
    exponent2: i32,
    target_exponent: i32,
) -> Result<u64> {
    if coefficient2 == 0 {
        log::info!("Error: Overflow in {} / {}", coefficient1, coefficient2);
        return Err(anyhow!("Math overflow"));
    }

    if coefficient1 == 0 {
        return Ok(0);
    }

    // compute scale factor for the dividend
    let mut scale_factor = 0;
    let mut target_power = exponent1 - exponent2 - target_exponent;

    if exponent1 > 0 {
        scale_factor += exponent1;
        target_power -= exponent1;
    }

    if exponent2 < 0 {
        scale_factor -= exponent2;
        target_power += exponent2;
    }

    if target_exponent < 0 {
        scale_factor -= target_exponent;
        target_power += target_exponent;
    }

    let scaled_coeff1 = if scale_factor > 0 {
        coefficient1 as u128 * checked_pow(10u128, scale_factor as usize)?
    } else {
        coefficient1 as u128
    };

    if target_power >= 0 {
        checked_as_u64(
            (scaled_coeff1 / coefficient2 as u128) * checked_pow(10u128, target_power as usize)?,
        )
    } else {
        checked_as_u64(
            (scaled_coeff1 / coefficient2 as u128) / checked_pow(10u128, (-target_power) as usize)?,
        )
    }
}

pub fn checked_decimal_ceil_div(
    coefficient1: u64,
    exponent1: i32,
    coefficient2: u64,
    exponent2: i32,
    target_exponent: i32,
) -> Result<u64> {
    if coefficient2 == 0 {
        log::info!("Error: Overflow in {} / {}", coefficient1, coefficient2);
        return Err(anyhow!("Math overflow"));
    }

    if coefficient1 == 0 {
        return Ok(0);
    }

    // Compute scale factor for the dividend
    let mut scale_factor = 0;
    let mut target_power = exponent1 - exponent2 - target_exponent;

    if exponent1 > 0 {
        scale_factor += exponent1;
        target_power += exponent1;
    }

    if exponent2 < 0 {
        scale_factor -= exponent2;
        target_power += exponent2;
    }

    if target_exponent < 0 {
        scale_factor -= target_exponent;
        target_power += target_exponent;
    }

    let scaled_coeff1 = if scale_factor > 0 {
        coefficient1 as u128 * checked_pow(10u128, scale_factor as usize)?
    } else {
        coefficient1 as u128
    };

    if target_power >= 0 {
        checked_as_u64(
            checked_ceil_div(scaled_coeff1, coefficient2 as u128)?
                * checked_pow(10u128, target_power as usize)?,
        )
    } else {
        checked_as_u64(
            checked_ceil_div(scaled_coeff1, coefficient2 as u128)?
                / checked_pow(10u128, (-target_power) as usize)?,
        )
    }
}

pub fn checked_token_div(
    amount1: u64,
    decimals1: u8,
    amount2: u64,
    decimals2: u8,
) -> Result<(u64, u8)> {
    let target_decimals = std::cmp::max(decimals1, decimals2);
    Ok((
        checked_decimal_div(
            amount1,
            -(decimals1 as i32),
            amount2,
            -(decimals2 as i32),
            -(target_decimals as i32),
        )?,
        target_decimals,
    ))
}

pub fn checked_float_mul<T>(arg1: T, arg2: T) -> Result<T>
where
    T: num_traits::Float + Display,
{
    let res = arg1 * arg2;

    if !res.is_finite() {
        log::info!("Error: Overflow in {} * {}", arg1, arg2);
        Err(anyhow!("Math overflow"))
    } else {
        Ok(res)
    }
}

pub fn checked_decimal_mul(
    coefficient1: u64,
    exponent1: i32,
    coefficient2: u64,
    exponent2: i32,
    target_exponent: i32,
) -> Result<u64> {
    if coefficient1 == 0 || coefficient2 == 0 {
        return Ok(0);
    }

    let target_power = (exponent1 + exponent2) - target_exponent;

    if target_power >= 0 {
        checked_as_u64(
            (coefficient1 as u128 * coefficient2 as u128)
                * checked_pow(10u128, target_power as usize)?,
        )
    } else {
        checked_as_u64(
            (coefficient1 as u128 * coefficient2 as u128)
                / checked_pow(10u128, (-target_power) as usize)?,
        )
    }
}

pub fn checked_decimal_ceil_mul(
    coefficient1: u64,
    exponent1: i32,
    coefficient2: u64,
    exponent2: i32,
    target_exponent: i32,
) -> Result<u64> {
    if coefficient1 == 0 || coefficient2 == 0 {
        return Ok(0);
    }

    let target_power = (exponent1 + exponent2) - target_exponent;

    if target_power >= 0 {
        checked_as_u64(
            (coefficient1 as u128 * coefficient2 as u128)
                * checked_pow(10u128, target_power as usize)?,
        )
    } else {
        checked_as_u64(checked_ceil_div(
            coefficient1 as u128 * coefficient2 as u128,
            checked_pow(10u128, (-target_power) as usize)?,
        )?)
    }
}

pub fn checked_token_mul(
    amount1: u64,
    decimals1: u8,
    amount2: u64,
    decimals2: u8,
) -> Result<(u64, u8)> {
    let target_decimals = std::cmp::max(decimals1, decimals2);

    Ok((
        checked_decimal_mul(
            amount1,
            -(decimals1 as i32),
            amount2,
            -(decimals2 as i32),
            -(target_decimals as i32),
        )?,
        target_decimals,
    ))
}

pub fn checked_pow<T>(arg: T, exp: usize) -> Result<T>
where
    T: num_traits::PrimInt + Display,
{
    if let Some(res) = num_traits::checked_pow(arg, exp) {
        Ok(res)
    } else {
        log::info!("Error: Overflow in {} ^ {}", arg, exp);
        Err(anyhow!("Math overflow"))
    }
}

pub fn checked_powf(arg: f64, exp: f64) -> Result<f64> {
    let res = f64::powf(arg, exp);

    if res.is_finite() {
        Ok(res)
    } else {
        log::info!("Error: Overflow in {} ^ {}", arg, exp);
        Err(anyhow!("Math overflow"))
    }
}

pub fn checked_powi(arg: f64, exp: i32) -> Result<f64> {
    let res = if exp > 0 {
        f64::powi(arg, exp)
    } else {
        // Workaround due to f64::powi() not working properly on-chain with negative exponent
        checked_float_div(1.0, f64::powi(arg, -exp))?
    };

    if res.is_finite() {
        Ok(res)
    } else {
        log::info!("Error: Overflow in {} ^ {}", arg, exp);
        Err(anyhow!("Math overflow"))
    }
}

pub fn checked_as_usize<T>(arg: T) -> Result<usize>
where
    T: Display + num_traits::ToPrimitive + Clone,
{
    let option: Option<usize> = num_traits::NumCast::from(arg.clone());

    if let Some(res) = option {
        Ok(res)
    } else {
        log::info!("Error: Overflow in {} as usize", arg);
        Err(anyhow!("Math overflow"))
    }
}

pub fn checked_as_u16<T>(arg: T) -> Result<u16>
where
    T: Display + num_traits::ToPrimitive + Clone,
{
    let option: Option<u16> = num_traits::NumCast::from(arg.clone());

    if let Some(res) = option {
        Ok(res)
    } else {
        log::info!("Error: Overflow in {} as u16", arg);
        Err(anyhow!("Math overflow"))
    }
}

pub fn checked_as_i32<T>(arg: T) -> Result<i32>
where
    T: Display + num_traits::ToPrimitive + Clone,
{
    let option: Option<i32> = num_traits::NumCast::from(arg.clone());

    if let Some(res) = option {
        Ok(res)
    } else {
        log::info!("Error: Overflow in {} as i32", arg);
        Err(anyhow!("Math overflow"))
    }
}

pub fn checked_as_i64<T>(arg: T) -> Result<i64>
where
    T: Display + num_traits::ToPrimitive + Clone,
{
    let option: Option<i64> = num_traits::NumCast::from(arg.clone());

    if let Some(res) = option {
        Ok(res)
    } else {
        log::info!("Error: Overflow in {} as i64", arg);
        Err(anyhow!("Math overflow"))
    }
}

pub fn checked_as_u64<T>(arg: T) -> Result<u64>
where
    T: Display + num_traits::ToPrimitive + Clone,
{
    let option: Option<u64> = num_traits::NumCast::from(arg.clone());

    if let Some(res) = option {
        Ok(res)
    } else {
        log::info!("Error: Overflow in {} as u64", arg);
        Err(anyhow!("Math overflow"))
    }
}

pub fn checked_as_u128<T>(arg: T) -> Result<u128>
where
    T: Display + num_traits::ToPrimitive + Clone,
{
    let option: Option<u128> = num_traits::NumCast::from(arg.clone());

    if let Some(res) = option {
        Ok(res)
    } else {
        log::info!("Error: Overflow in {} as u128", arg);
        Err(anyhow!("Math overflow"))
    }
}

pub fn checked_as_f64<T>(arg: T) -> Result<f64>
where
    T: Display + num_traits::ToPrimitive + Clone,
{
    let option: Option<f64> = num_traits::NumCast::from(arg.clone());

    if let Some(res) = option {
        Ok(res)
    } else {
        log::info!("Error: Overflow in {} as f64", arg);
        Err(anyhow!("Math overflow"))
    }
}

pub fn scale_to_exponent(arg: u64, exponent: i32, target_exponent: i32) -> Result<u64> {
    if target_exponent == exponent {
        return Ok(arg);
    }

    let delta = target_exponent - exponent;

    if delta > 0 {
        // Arg's number of decimals is shrinking
        let result = arg / checked_pow(10, delta as usize)?;

        // Accept 1bps loss of precision
        if result * checked_pow(10, delta as usize)? < (arg - (arg / BPS_POWER as u64)) {
            return Err(anyhow!(
                "Pyth price exponent too large incurring precision loss"
            ));
        }

        return Ok(result);
    }

    // arg's number of decimals is increasing, there is no loss of precision
    Ok(arg * checked_pow(10, (-delta) as usize)?)
}

pub fn to_ui_amount(amount: u64, decimals: u8) -> Result<f64> {
    checked_float_div(
        checked_as_f64(amount)?,
        checked_powi(10.0, decimals as i32)?,
    )
}

pub fn to_token_amount(ui_amount: f64, decimals: u8) -> Result<u64> {
    checked_as_u64(checked_float_mul(
        ui_amount,
        checked_powi(10.0, decimals as i32)?,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_div() {
        // checked_decimal_div(1_000, 1, 5_000_000, -6, 0).unwrap()
        let test_case = (5, 1, 50_000_000, -6, 0);
        let (c1, e1, c2, e2, et) = test_case;

        let res = checked_decimal_div(c1, e1, c2, e2, et).unwrap();
        println!(
            "({}*10**{}) / ({}*10**{}) = {}*10**{}",
            c1, e1, c2, e2, res, et
        );
    }

    #[test]
    fn test_checked_decimal_mul_ok() {
        // Nominal test
        assert_eq!(
            585,
            checked_decimal_mul(14999835, -6, 39012, -9, -6).unwrap()
        );
    }

    #[test]
    fn test_checked_decimal_div_ok() {
        // Nominal test
        assert_eq!(
            2_000_000,
            checked_decimal_div(1_000, -6, 500, -6, -6).unwrap()
        );

        // Different exponents
        assert_eq!(
            2_000_000_000,
            checked_decimal_div(1_000, -6, 500, -9, -6).unwrap()
        );

        // Different exponents
        assert_eq!(2_000, checked_decimal_div(1_000, -9, 500, -6, -6).unwrap());

        // MAX coefficient values
        assert_eq!(
            1_000_000,
            checked_decimal_div(u64::MAX, -6, u64::MAX, -6, -6).unwrap()
        );

        assert_eq!(0, checked_decimal_div(0, -6, u64::MAX, -6, -6).unwrap());

        // Maximum coefficients delta depends on target exponent
        assert_eq!(
            18_446_744_073_709_000_000,
            checked_decimal_div(u64::MAX / checked_pow(10u64, 6).unwrap(), -6, 1, -6, -6,).unwrap()
        );

        // Maximum coefficients delta depends on target exponent
        assert_eq!(
            18_446_744_073_000_000_000,
            checked_decimal_div(u64::MAX / checked_pow(10u64, 9).unwrap(), -6, 1, -6, -9,).unwrap()
        );
    }

    #[test]
    fn test_checked_decimal_div_ko() {
        // Division by zero
        assert!(checked_decimal_div(1_000_000, -6, 0, -6, -6).is_err());

        // Overflowing result
        assert!(checked_decimal_div(u64::MAX, -6, 1, -6, -6).is_err());
    }

    #[test]
    fn test_scale_to_exponent() {
        assert_eq!(28567, scale_to_exponent(285675, -7, -6).unwrap());

        // When target is lower than original exponent, will never fail
        assert_eq!(
            31618650779400,
            scale_to_exponent(316186507794, -8, -10).unwrap()
        );

        assert_eq!(31618650, scale_to_exponent(316186507794, -10, -6).unwrap());

        // Losing too much precision on the price
        assert!(scale_to_exponent(285675, -10, -6).is_err());
    }
}