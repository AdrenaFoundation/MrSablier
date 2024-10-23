use {crate::adrena::u128_split::U128Split, solana_sdk::pubkey::Pubkey};

#[derive(PartialEq, Copy, Clone, Default, Debug)]
pub enum Side {
    None = 0,
    #[default]
    Long = 1,
    Short = 2,
}

impl From<Side> for u8 {
    fn from(val: Side) -> Self {
        match val {
            Side::None => 0,
            Side::Long => 1,
            Side::Short => 2,
        }
    }
}

impl TryFrom<u8> for Side {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        Ok(match value {
            0 => Side::None,
            1 => Side::Long,
            2 => Side::Short,
            // Return an error if unknown value
            _ => Err(anyhow::anyhow!("Invalid position side"))?,
        })
    }
}

#[repr(C)]
pub struct Position {
    pub bump: u8,
    pub side: u8, // Side
    pub take_profit_thread_is_set: u8,
    pub stop_loss_thread_is_set: u8,
    pub pending_cleanup_and_close: u8,
    pub _padding: [u8; 3],

    pub owner: Pubkey,
    pub pool: Pubkey,
    pub custody: Pubkey,
    pub collateral_custody: Pubkey,

    pub open_time: i64,
    pub update_time: i64,
    pub price: u64,
    pub size_usd: u64,
    pub borrow_size_usd: u64,
    pub collateral_usd: u64,
    // Borrowing fee to be paid (increasing after increase position)
    pub unrealized_interest_usd: u64,
    pub cumulative_interest_snapshot: U128Split,
    pub locked_amount: u64,
    pub collateral_amount: u64,

    // Estimations at open - When actually closing, the confidence will be applied
    // Before being used again at close time, these values are updated to their true values
    pub exit_fee_usd: u64,
    pub liquidation_fee_usd: u64,

    pub take_profit_thread_id: u64,
    pub take_profit_limit_price: u64,
    pub stop_loss_thread_id: u64,
    pub stop_loss_limit_price: u64,
    // 0 means no slippage
    pub stop_loss_close_position_price: u64,
}

impl Position {
    pub const LEN: usize = 8 + std::mem::size_of::<Position>();

    pub fn is_pending_cleanup_and_close(&self) -> bool {
        self.pending_cleanup_and_close != 0
    }

    pub fn get_side(&self) -> Side {
        // Consider value in the struct always good
        Side::try_from(self.side).unwrap()
    }

    pub fn take_profit_is_set(&self) -> bool {
        self.take_profit_thread_is_set != 0
    }

    pub fn stop_loss_is_set(&self) -> bool {
        self.stop_loss_thread_is_set != 0
    }

    pub fn take_profit_reached(&self, price: u64) -> bool {
        if self.take_profit_limit_price == 0 {
            return false;
        }

        if self.get_side() == Side::Long {
            price >= self.take_profit_limit_price
        } else {
            price <= self.take_profit_limit_price
        }
    }

    pub fn stop_loss_reached(&self, price: u64) -> bool {
        if self.stop_loss_limit_price == 0 {
            return false;
        }

        if self.get_side() == Side::Long {
            price <= self.stop_loss_limit_price
        } else {
            price >= self.stop_loss_limit_price
        }
    }

    pub fn stop_loss_slippage_ok(&self, price: u64) -> bool {
        // 0 means no slippage
        if self.stop_loss_close_position_price == 0 {
            return true;
        }

        if self.get_side() == Side::Long {
            price >= self.stop_loss_close_position_price
        } else {
            price <= self.stop_loss_close_position_price
        }
    }
}
