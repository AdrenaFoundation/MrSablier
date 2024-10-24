use {
    crate::adrena::u128_split::U128Split,
    borsh::{BorshDeserialize, BorshSerialize},
    solana_sdk::pubkey::Pubkey,
};

pub const MAX_STABLE_CUSTODY: usize = 2;

// in BPS
pub const MIN_INITIAL_LEVERAGE: u32 = 11_000;

// Fees have implied BPS_DECIMALS decimals
#[derive(Copy, Clone, PartialEq, BorshSerialize, BorshDeserialize, Default, Debug)]
#[repr(C)]
pub struct Fees {
    // Base fees
    pub swap_in: u16,
    pub swap_out: u16,
    pub stable_swap_in: u16,
    pub stable_swap_out: u16,
    pub add_liquidity: u16,
    pub remove_liquidity: u16,
    pub close_position: u16,
    pub liquidation: u16,
    pub fee_max: u16,
    pub _padding: [u8; 6],
    pub _padding2: u64, // force u64 alignment
}

#[derive(Copy, Clone, PartialEq, BorshSerialize, BorshDeserialize, Default, Debug)]
#[repr(C)]
pub struct FeesStats {
    pub swap_usd: u64,
    pub add_liquidity_usd: u64,
    pub remove_liquidity_usd: u64,
    pub close_position_usd: u64,
    pub liquidation_usd: u64,
    pub borrow_usd: u64,
}

#[derive(Copy, Clone, PartialEq, BorshSerialize, BorshDeserialize, Default, Debug)]
#[repr(C)]
pub struct VolumeStats {
    pub swap_usd: u64,
    pub add_liquidity_usd: u64,
    pub remove_liquidity_usd: u64,
    pub open_position_usd: u64,
    pub close_position_usd: u64,
    pub liquidation_usd: u64,
}

#[derive(Copy, Clone, PartialEq, BorshSerialize, BorshDeserialize, Default, Debug)]
#[repr(C)]
pub struct TradeStats {
    pub profit_usd: u64,
    pub loss_usd: u64,
    pub oi_long_usd: u64,
    pub oi_short_usd: u64,
}

#[derive(Copy, Clone, PartialEq, BorshSerialize, BorshDeserialize, Default, Debug)]
#[repr(C)]
pub struct Assets {
    // Collateral debt
    pub collateral: u64,
    // Owned = total_assets - collateral + collected_fees - protocol_fees
    pub owned: u64,
    // Locked funds for pnl payoff
    pub locked: u64,
}

#[derive(Copy, Clone, PartialEq, BorshSerialize, BorshDeserialize, Default, Debug)]
#[repr(C)]
pub struct PricingParams {
    // Pricing params have implied BPS_DECIMALS decimals (except ended with _usd)
    pub max_initial_leverage: u32,
    pub max_leverage: u32,
    // One position size can't exceed this amount
    pub max_position_locked_usd: u64,
    // Limit the total size of short positions for the custody
    pub max_cumulative_short_position_size_usd: u64,
}

#[derive(Copy, Clone, PartialEq, BorshSerialize, BorshDeserialize, Default, Debug)]
#[repr(C)]
pub struct BorrowRateParams {
    // Borrow rate params have implied RATE_DECIMALS decimals
    pub max_hourly_borrow_interest_rate: u64,
}

#[derive(Copy, Clone, PartialEq, BorshSerialize, BorshDeserialize, Default, Debug)]
#[repr(C)]
pub struct BorrowRateState {
    // Borrow rates have implied RATE_DECIMALS decimals
    pub current_rate: u64,
    pub last_update: i64,
    pub cumulative_interest: U128Split,
}

#[derive(Copy, Clone, PartialEq, BorshSerialize, BorshDeserialize, Default, Debug)]
#[repr(C)]
pub struct PositionsAccounting {
    pub open_positions: u64,
    pub size_usd: u64,
    pub borrow_size_usd: u64,
    pub locked_amount: u64,
    pub weighted_price: U128Split,
    pub total_quantity: U128Split,
    pub cumulative_interest_usd: u64,
    pub _padding: [u8; 8],
    pub cumulative_interest_snapshot: U128Split,
    // This exit fee stats is used to calculate the PnL of all opened positions, it is not reflecting the actual exit fee of the position (that can sometimes be lesser)
    // so that the AUM take into account an approximation of the PnL of all opened positions
    pub exit_fee_usd: u64,
    //
    // Store the stable custody locked amount related to this custody
    //
    // Example:
    // When Shorting 1 ETH, 1500 USDC get locked to provide for trader maximum payoff
    // USDC custody locked amount: +1500
    // eth custody stable locked amount: +1500
    //
    // Needed to be able to calculate PnL
    pub stable_locked_amount: [StableLockedAmountStat; MAX_STABLE_CUSTODY],
}

#[derive(Copy, Clone, PartialEq, BorshSerialize, BorshDeserialize, Default, Debug)]
#[repr(C)]
pub struct StableLockedAmountStat {
    pub custody: Pubkey,
    pub locked_amount: u64,
    pub _padding: [u8; 8],
}

// #[account(zero_copy)]
#[derive(Default, Debug, PartialEq, BorshSerialize, BorshDeserialize)]
#[repr(C)]
pub struct Custody {
    pub bump: u8,
    pub token_account_bump: u8,
    //
    // Permissions
    //
    // If false, make positions readonly/closeonly
    pub allow_trade: u8,
    pub allow_swap: u8,
    //
    pub decimals: u8,
    pub is_stable: u8,
    pub _padding: [u8; 2],
    //
    pub pool: Pubkey,
    // /!\ The position of this field matters as we use the offset to get the mint in get_custody_mint_from_account_info
    pub mint: Pubkey,
    pub token_account: Pubkey,
    // Oracle of the token (i.e jitoSOL custody, oracle is jitoSOL price)
    pub oracle: Pubkey,
    // Oracle used to trade the token (i.e jitoSOL custody, trade_oracle is SOL price)
    // Used to calculate the value of the position
    pub trade_oracle: Pubkey,
    pub pricing: PricingParams,

    pub fees: Fees,
    pub borrow_rate: BorrowRateParams,
    // All time stats
    pub collected_fees: FeesStats,
    pub volume_stats: VolumeStats,
    pub trade_stats: TradeStats,
    // Real time stats
    pub assets: Assets,
    pub long_positions: PositionsAccounting,
    pub short_positions: PositionsAccounting,
    pub borrow_rate_state: BorrowRateState,
}

// pub fn get_custody_mint_from_account_info(account_info: &AccountInfo<'_>) -> Pubkey {
//     // anchor discriminator 8 + 8 (bumps + u8s) + 32 pool pubkey
//     let data_slice = &account_info.data.borrow()[48..80];

//     let mut pubkey_bytes = [0u8; 32];

//     pubkey_bytes.copy_from_slice(data_slice);

//     Pubkey::from(pubkey_bytes)
// }

// impl Fees {
//     pub fn validate(&self) -> bool {
//         self.swap_in as u128 <= Cortex::BPS_POWER
//             && self.swap_out as u128 <= Cortex::BPS_POWER
//             && self.stable_swap_in as u128 <= Cortex::BPS_POWER
//             && self.stable_swap_out as u128 <= Cortex::BPS_POWER
//             && self.add_liquidity as u128 <= Cortex::BPS_POWER
//             && self.remove_liquidity as u128 <= Cortex::BPS_POWER
//             && self.close_position as u128 <= Cortex::BPS_POWER
//             && self.liquidation as u128 <= Cortex::BPS_POWER
//             && self.fee_max as u128 <= Cortex::BPS_POWER
//     }
// }

// impl PricingParams {
//     pub fn validate(&self) -> bool {
//         (MIN_INITIAL_LEVERAGE as u128) >= Cortex::BPS_POWER
//             && MIN_INITIAL_LEVERAGE <= self.max_initial_leverage
//             && self.max_initial_leverage <= self.max_leverage
//     }
// }

// impl BorrowRateParams {
//     pub fn validate(&self) -> bool {
//         // Between 0%+ and 100%
//         self.max_hourly_borrow_interest_rate > 0
//             && (self.max_hourly_borrow_interest_rate as u128) <= Cortex::RATE_POWER
//     }
// }

// impl Custody {
//     pub const LEN: usize = 8 + std::mem::size_of::<Custody>();

//     pub fn validate(&self) -> bool {
//         self.token_account != Pubkey::default()
//             && self.mint != Pubkey::default()
//             && self.oracle != Pubkey::default()
//             && self.pricing.validate()
//             && self.fees.validate()
//             && self.borrow_rate.validate()
//     }

//     pub fn is_stable(&self) -> bool {
//         self.is_stable == 1
//     }

//     pub fn allow_trade(&self) -> bool {
//         self.allow_trade == 1
//     }

//     pub fn allow_swap(&self) -> bool {
//         self.allow_swap == 1
//     }

//     pub fn lock_funds(&mut self, amount: u64) -> Result<()> {
//         self.assets.locked += amount;

//         if self.assets.owned < self.assets.locked {
//             Err(ProgramError::InsufficientFunds.into())
//         } else {
//             Ok(())
//         }
//     }

//     pub fn unlock_funds(&mut self, amount: u64) -> Result<()> {
//         if amount > self.assets.locked {
//             self.assets.locked = 0;
//         } else {
//             self.assets.locked -= amount;
//         }

//         Ok(())
//     }

//     // Returns the interest amount that has accrued since the last position cumulative interest snapshot update
//     pub fn get_interest_amount_usd(&self, position: &Position, current_time: i64) -> Result<u64> {
//         if position.borrow_size_usd == 0 {
//             return Ok(0);
//         }

//         let cumulative_interest = self.get_cumulative_interest(current_time)?;

//         let position_interest =
//             if cumulative_interest > position.cumulative_interest_snapshot.to_u128() {
//                 cumulative_interest - position.cumulative_interest_snapshot.to_u128()
//             } else {
//                 return Ok(0);
//             };

//         math::checked_as_u64(
//             (position_interest * position.borrow_size_usd as u128) / Cortex::RATE_POWER,
//         )
//     }

//     pub fn get_cumulative_interest(&self, current_time: i64) -> Result<u128> {
//         if current_time > self.borrow_rate_state.last_update {
//             let cumulative_interest = math::checked_ceil_div(
//                 (current_time - self.borrow_rate_state.last_update) as u128
//                     * self.borrow_rate_state.current_rate as u128,
//                 3_600,
//             )?;

//             Ok(self.borrow_rate_state.cumulative_interest.to_u128() + cumulative_interest)
//         } else {
//             Ok(self.borrow_rate_state.cumulative_interest.to_u128())
//         }
//     }

//     pub fn update_borrow_rate(&mut self, current_time: i64) -> Result<()> {
//         if self.assets.owned == 0 {
//             self.borrow_rate_state.current_rate = 0;
//             self.borrow_rate_state.last_update =
//                 std::cmp::max(current_time, self.borrow_rate_state.last_update);
//             return Ok(());
//         }

//         if current_time > self.borrow_rate_state.last_update {
//             // compute interest accumulated since previous update
//             self.borrow_rate_state.cumulative_interest =
//                 self.get_cumulative_interest(current_time)?.into();
//             self.borrow_rate_state.last_update = current_time;
//         }

//         // the borrow rate is now a function of the current utilization, where its max value is the max_hourly_borrow_interest_rate and the min value near 0
//         let hourly_rate = math::checked_ceil_div(
//             (self.assets.locked as u128
//                 * self.borrow_rate.max_hourly_borrow_interest_rate as u128
//                 * Cortex::RATE_POWER)
//                 / self.assets.owned as u128,
//             Cortex::RATE_POWER,
//         )?;

//         self.borrow_rate_state.current_rate = math::checked_as_u64(hourly_rate)?;

//         require!(
//             self.borrow_rate_state.current_rate <= self.borrow_rate.max_hourly_borrow_interest_rate,
//             AdrenaError::InvalidCustodyConfig
//         );

//         Ok(())
//     }
