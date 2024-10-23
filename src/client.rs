use {
    backoff::{future::retry, ExponentialBackoff},
    clap::{Parser, ValueEnum},
    futures::{future::TryFutureExt, stream::StreamExt},
    log::{error, info},
    solana_sdk::{account::Account, account_info::AccountInfo, pubkey::Pubkey},
    std::{collections::HashMap, env, sync::Arc, time::Duration},
    tokio::sync::Mutex,
    tonic::transport::channel::ClientTlsConfig,
    yellowstone_grpc_client::{GeyserGrpcClient, Interceptor},
    yellowstone_grpc_proto::prelude::{
        subscribe_request_filter_accounts_filter::Filter as AccountsFilterDataOneof,
        subscribe_request_filter_accounts_filter_memcmp::Data as AccountsFilterMemcmpOneof,
        CommitmentLevel, SubscribeRequestFilterAccounts, SubscribeRequestFilterBlocks,
        SubscribeRequestFilterBlocksMeta, SubscribeRequestFilterEntry, SubscribeRequestFilterSlots,
        SubscribeRequestFilterTransactions,
    },
};

// type SlotsFilterMap = HashMap<String, SubscribeRequestFilterSlots>;
// type AccountFilterMap = HashMap<String, SubscribeRequestFilterAccounts>;
// type TransactionsFilterMap = HashMap<String, SubscribeRequestFilterTransactions>;
// type TransactionsStatusFilterMap = HashMap<String, SubscribeRequestFilterTransactions>;
// type EntryFilterMap = HashMap<String, SubscribeRequestFilterEntry>;
// type BlocksFilterMap = HashMap<String, SubscribeRequestFilterBlocks>;
// type BlocksMetaFilterMap = HashMap<String, SubscribeRequestFilterBlocksMeta>;

pub mod adrena;
pub mod state;

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
enum ArgsCommitment {
    #[default]
    Processed,
    Confirmed,
    Finalized,
}

impl From<ArgsCommitment> for CommitmentLevel {
    fn from(commitment: ArgsCommitment) -> Self {
        match commitment {
            ArgsCommitment::Processed => CommitmentLevel::Processed,
            ArgsCommitment::Confirmed => CommitmentLevel::Confirmed,
            ArgsCommitment::Finalized => CommitmentLevel::Finalized,
        }
    }
}

#[derive(Debug, Clone, Parser)]
#[clap(author, version, about)]
struct Args {
    #[clap(short, long, default_value_t = String::from("http://127.0.0.1:10000"))]
    /// Service endpoint
    endpoint: String,

    #[clap(long)]
    x_token: Option<String>,

    /// Commitment level: processed, confirmed or finalized
    #[clap(long)]
    commitment: Option<ArgsCommitment>,
}

impl Args {
    fn get_commitment(&self) -> Option<CommitmentLevel> {
        Some(self.commitment.unwrap_or_default().into())
    }

    async fn connect(&self) -> anyhow::Result<GeyserGrpcClient<impl Interceptor>> {
        GeyserGrpcClient::build_from_shared(self.endpoint.clone())?
            .x_token(self.x_token.clone())?
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(10))
            .tls_config(ClientTlsConfig::new().with_native_roots())?
            .connect()
            .await
            .map_err(Into::into)
    }
}

// Function to derive PDA (to be implemented)
fn derive_pda(_seeds: &[&[u8]], _program_id: &Pubkey) -> (Pubkey, u8) {
    unimplemented!("PDA derivation function not implemented yet")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env::set_var(
        env_logger::DEFAULT_FILTER_ENV,
        env::var_os(env_logger::DEFAULT_FILTER_ENV).unwrap_or_else(|| "info".into()),
    );
    env_logger::init();

    let args = Args::parse();
    let zero_attempts = Arc::new(Mutex::new(true));

    // The default exponential backoff strategy intervals:
    // [500ms, 750ms, 1.125s, 1.6875s, 2.53125s, 3.796875s, 5.6953125s,
    // 8.5s, 12.8s, 19.2s, 28.8s, 43.2s, 64.8s, 97s, ... ]
    retry(ExponentialBackoff::default(), move || {
        let args = args.clone();
        let zero_attempts = Arc::clone(&zero_attempts);

        async move {
            let mut zero_attempts = zero_attempts.lock().await;
            if *zero_attempts {
                *zero_attempts = false;
            } else {
                info!("Retry to connect to the server");
            }
            drop(zero_attempts);

            let commitment = args.get_commitment();
            let mut client = args.connect().await.map_err(backoff::Error::transient)?;
            info!("Connected");

            // SL/TP / Liquidations

            let positions: Vec<(Pubkey, Account)> = vec![]; //Vec<adrena::Position> = vec![];
            let price_feeds: Vec<(Pubkey, Account)> = vec![]; //Vec<pyth::PriceFeedV2> = vec![];

            let position_accounts_discriminator_filter = SubscribeRequestFilterAccountsFilter {
                filter: Some(AccountsFilterDataOneof::Memcmp(
                    SubscribeRequestFilterAccountsFilterMemcmp {
                        offset: 0,
                        data: Some(AccountsFilterMemcmpOneof::Base58(
                            adrena::Position::discriminator().to_vec(),
                        )),
                    },
                )),
            };
            // let price_feed_accounts_filter = SubscribeRequestFilterAccountsFilter {
            //     filter: Some(AccountsFilterDataOneof::Memcmp(
            //         SubscribeRequestFilterAccountsFilterMemcmp {
            //             offset: 0
            //                 .parse()
            //                 .map_err(|_| anyhow::anyhow!("invalid offset"))?,
            //             data: todo!(),
            //         },
            //     )),
            // };
            let mut position_accounts: AccountFilterMap = HashMap::new();
            // let mut price_feed_accounts: AccountFilterMap = HashMap::new();

            accounts.insert(
                "client".to_owned(),
                SubscribeRequestFilterAccounts {
                    account: accounts_account,
                    owner: args.accounts_owner.clone(),
                    filters,
                },
            );

            // retrieve all existing Adrena::PositionPDA, index them
            //    Each time a new position is indexed, check the Custody/Oracle and if it doesn't exist yet, index the Pyth::PriceFeedV2
            // Now we got all our existing position and price feeds we can start the loop.

            // go over arrays and convert them to the correct data types (adrena::Position, pyth::PriceFeedV2)

            // LOOP/subscribe to updates on indexed PDA OR index new PDA
            //    For each position,
            //      Check if it has SL/TP set, if it does, check if triggered based on the price feed
            //        If it's triggered, do a CPI to closePosition
            //        If not, do nothing
            //      Check position Liquidation conditions (based on position current leverage, based on borrow fees)
            //        If it's in liquidation territory, do a CPI to liquidatePosition
            //        If not, do nothing

            Ok::<(), backoff::Error<anyhow::Error>>(())
        }
        .inspect_err(|error| error!("failed to connect: {error}"))
    })
    .await
    .map_err(Into::into)
}
