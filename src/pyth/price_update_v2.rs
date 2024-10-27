use {
    borsh::{BorshDeserialize, BorshSchema, BorshSerialize},
    solana_sdk::pubkey::Pubkey,
};

#[derive(BorshSerialize, BorshDeserialize, Copy, Clone, PartialEq, BorshSchema, Debug)]
pub enum VerificationLevel {
    Partial { num_signatures: u8 },
    Full,
}

/// Id of a feed producing the message. One feed produces one or more messages.
pub type FeedId = [u8; 32];

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct PriceFeedMessage {
    pub feed_id: FeedId,
    pub price: i64,
    pub conf: u64,
    pub exponent: i32,
    /// The timestamp of this price update in seconds
    pub publish_time: i64,
    /// The timestamp of the previous price update. This field is intended to allow users to
    /// identify the single unique price update for any moment in time:
    /// for any time t, the unique update is the one such that prev_publish_time < t <= publish_time.
    ///
    /// Note that there may not be such an update while we are migrating to the new message-sending logic,
    /// as some price updates on pythnet may not be sent to other chains (because the message-sending
    /// logic may not have triggered). We can solve this problem by making the message-sending mandatory
    /// (which we can do once publishers have migrated over).
    ///
    /// Additionally, this field may be equal to publish_time if the message is sent on a slot where
    /// where the aggregation was unsuccesful. This problem will go away once all publishers have
    /// migrated over to a recent version of pyth-agent.
    pub prev_publish_time: i64,
    pub ema_price: i64,
    pub ema_conf: u64,
}

/// A price update account. This account is used by the Pyth Receiver program to store a verified price update from a Pyth price feed.
/// It contains:
/// - `write_authority`: The write authority for this account. This authority can close this account to reclaim rent or update the account to contain a different price update.
/// - `verification_level`: The [`VerificationLevel`] of this price update. This represents how many Wormhole guardian signatures have been verified for this price update.
/// - `price_message`: The actual price update.
/// - `posted_slot`: The slot at which this price update was posted.
#[derive(BorshSchema, BorshSerialize, BorshDeserialize)]
pub struct PriceUpdateV2 {
    pub write_authority: Pubkey,
    pub verification_level: VerificationLevel,
    pub price_message: PriceFeedMessage,
    pub posted_slot: u64,
}

impl PriceUpdateV2 {
    pub const LEN: usize = 8 + 32 + 2 + 32 + 8 + 8 + 4 + 8 + 8 + 8 + 8 + 8;
}
