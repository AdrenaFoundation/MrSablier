use solana_sdk::{signature::Signature, transaction::TransactionError};
use yellowstone_grpc_proto::geyser::SubscribeUpdateTransactionStatus;

#[allow(dead_code)]
#[derive(Debug)]
pub struct TransactionStatusPretty {
    slot: u64,
    signature: Signature,
    is_vote: bool,
    index: u64,
    err: Option<TransactionError>,
}

impl From<SubscribeUpdateTransactionStatus> for TransactionStatusPretty {
    fn from(status: SubscribeUpdateTransactionStatus) -> Self {
        Self {
            slot: status.slot,
            signature: Signature::try_from(status.signature.as_slice()).expect("valid signature"),
            is_vote: status.is_vote,
            index: status.index,
            err: yellowstone_grpc_proto::convert_from::create_tx_error(status.err.as_ref())
                .expect("valid tx err"),
        }
    }
}
