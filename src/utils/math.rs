use {
    super::BPS_POWER,
    anyhow::{anyhow, Result},
    std::fmt::Display,
};

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
