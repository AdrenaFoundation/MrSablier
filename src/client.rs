use {
    backoff::{future::retry, ExponentialBackoff},
    clap::Parser,
    futures::{future::TryFutureExt, stream::StreamExt, SinkExt},
    log::{error, info},
    solana_account_decoder::UiAccountEncoding,
    solana_client::{
        nonblocking::rpc_client::RpcClient,
        rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig},
        rpc_filter::{Memcmp, RpcFilterType},
    },
    solana_sdk::{account::Account, pubkey::Pubkey},
    std::{
        collections::{HashMap, HashSet},
        env,
        str::FromStr,
        sync::Arc,
        time::Duration,
    },
    tokio::sync::Mutex,
    tonic::transport::channel::ClientTlsConfig,
    utils::{AccountPretty, TransactionPretty, TransactionStatusPretty},
    yellowstone_grpc_client::{GeyserGrpcClient, Interceptor},
    yellowstone_grpc_proto::{
        geyser::{
            subscribe_update::UpdateOneof, SubscribeRequest, SubscribeRequestFilterAccountsFilter,
            SubscribeRequestFilterAccountsFilterMemcmp, SubscribeRequestPing,
        },
        prelude::{
            subscribe_request_filter_accounts_filter::Filter as AccountsFilterDataOneof,
            subscribe_request_filter_accounts_filter_memcmp::Data as AccountsFilterMemcmpOneof,
            CommitmentLevel, SubscribeRequestFilterAccounts,
        },
    },
};

type AccountFilterMap = HashMap<String, SubscribeRequestFilterAccounts>;

pub const ADRENA_PROGRAM: &str = "13gDzEXCdocbj8iAiqrScGo47NiSuYENGsRqi3SEAwet";
// pub const PYTH_ORACLE_PROGRAM: &str = "FsJ3A3u2vn5cTVofAjvy6y5kwABJAqYWpe4975bi2epH";

// https://solscan.io/account/rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ
pub const PYTH_RECEIVER_PROGRAM: &str = "rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ";

pub mod adrena;
pub mod pyth;
pub mod utils;

#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
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
            let mut grpc = args
                .connect()
                .await
                .map_err(|e| backoff::Error::transient(e.into()))?;
            let rpc = RpcClient::new(args.endpoint.clone());
            info!("Connected");

            // TODO:
            // - implement a new step to check for price feeds required by the positions and subscribe to them

            info!("1 - Retrieving existing positions..."); /////////////////////////////////////////////////
            let existing_positions_pda =
                fetch_existing_positions(&rpc, &Pubkey::from_str(ADRENA_PROGRAM).unwrap()).await?;
            info!(
                "  <> # of existing positions retrieved: {}",
                existing_positions_pda.len()
            );

            info!("1.1 - parse existing positions to retrieve their associated price feeds...");
            let price_update_v2_pdas =
                get_associated_price_update_v2_pdas(&rpc, &existing_positions_pda).await?;

            info!("1 - Preparing account filter map for [position_pdas, price_update_v2_pdas]..."); //////////////////////////////////////////////////////////////////////
            let accounts_filter_map = {
                let mut accounts_filter_map: AccountFilterMap = HashMap::new();

                // // Positions
                // let existing_positions_pubkeys = existing_positions_pda
                //     .iter()
                //     .map(|p| p.0.to_string())
                //     .collect();
                // // let position_filter_discriminator = SubscribeRequestFilterAccountsFilter {
                // //     filter: Some(AccountsFilterDataOneof::Memcmp(
                // //         SubscribeRequestFilterAccountsFilterMemcmp {
                // //             offset: 0,
                // //             data: Some(AccountsFilterMemcmpOneof::Bytes(
                // //                 adrena::Position::get_anchor_discriminator(),
                // //             )),
                // //         },
                // //     )),
                // // };
                // let position_filter_size = SubscribeRequestFilterAccountsFilter {
                //     filter: Some(AccountsFilterDataOneof::Datasize(
                //         adrena::Position::LEN as u64,
                //     )),
                // };
                // let position_owner = vec![ADRENA_PROGRAM.to_owned()];
                // accounts_filter_map.insert(
                //     "position".to_owned(),
                //     SubscribeRequestFilterAccounts {
                //         // Subscribe to all existing positions
                //         account: existing_positions_pubkeys,
                //         // Subscribe to positions owned by Adrena program
                //         owner: position_owner,
                //         // Filter by size
                //         filters: vec![position_filter_size],
                //     },
                // );

                // Price update v2 pdas
                let price_update_v2_pdas_pubkeys: Vec<String> =
                    price_update_v2_pdas.iter().map(|p| p.to_string()).collect();
                info!(
                    "  <> # of price update v2 pdas: {}",
                    price_update_v2_pdas_pubkeys.len()
                );
                info!(
                    "  <> price update v2 pdas: {:?}",
                    price_update_v2_pdas_pubkeys
                );
                let price_feed_owner = vec![PYTH_RECEIVER_PROGRAM.to_owned()];
                // let price_update_v2_filter_size = SubscribeRequestFilterAccountsFilter {
                //     filter: Some(AccountsFilterDataOneof::Datasize(
                //         pyth::PriceUpdateV2::PriceUpdateV2::LEN as u64,
                //     )),
                // };
                accounts_filter_map.insert(
                    "price_update_v2".to_owned(),
                    SubscribeRequestFilterAccounts {
                        account: price_update_v2_pdas_pubkeys,
                        owner: vec![], //price_feed_owner,
                        filters: vec![],
                    },
                );

                accounts_filter_map
            };

            info!("3 - Opening stream..."); ///////////////////////////////////////////////////////////////////////////////////////////////
            let (mut subscribe_tx, mut stream) = {
                let request = SubscribeRequest {
                    // This is necessary to keep load balancers that expect client pings alive. If your load balancer doesn't
                    // require periodic client pings then this is unnecessary
                    ping: Some(SubscribeRequestPing { id: 1 }),
                    accounts: accounts_filter_map,
                    commitment: commitment.map(|c| c.into()),
                    ..Default::default()
                };
                info!("Sending subscription request: {:?}", request);
                let (subscribe_tx, stream) = grpc
                    .subscribe_with_request(Some(request))
                    .await
                    .map_err(|e| backoff::Error::transient(e.into()))?;
                (subscribe_tx, stream)
            };

            info!("4 - Processing stream..."); ////////////////////////////////////////////////////////////////////////////////////////////
            while let Some(message) = stream.next().await {
                match message {
                    Ok(msg) => {
                        match msg.update_oneof {
                            Some(UpdateOneof::Account(account)) => {
                                let account: AccountPretty = account.into();
                                info!(
                                    "new account update: filters {:?}, account: {:#?}",
                                    msg.filters, account
                                );
                                continue;
                            }
                            Some(UpdateOneof::Transaction(tx)) => {
                                let tx: TransactionPretty = tx.into();
                                info!(
                                    "new transaction update: filters {:?}, transaction: {:#?}",
                                    msg.filters, tx
                                );
                                continue;
                            }
                            Some(UpdateOneof::TransactionStatus(status)) => {
                                let status: TransactionStatusPretty = status.into();
                                info!(
                                "new transaction update: filters {:?}, transaction status: {:?}",
                                msg.filters, status
                            );
                                continue;
                            }
                            Some(UpdateOneof::Ping(_)) => {
                                // This is necessary to keep load balancers that expect client pings alive. If your load balancer doesn't
                                // require periodic client pings then this is unnecessary
                                subscribe_tx
                                    .send(SubscribeRequest {
                                        ping: None, //Some(SubscribeRequestPing { id: 1 }),
                                        ..Default::default()
                                    })
                                    .await
                                    .map_err(|e| backoff::Error::transient(e.into()))?;
                            }
                            _ => {}
                        }
                        info!("new message: {msg:?}")
                    }
                    Err(error) => {
                        error!("error: {error:?}");
                        break;
                    }
                }
            }

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

async fn fetch_existing_positions(
    rpc: &RpcClient,
    program_id: &Pubkey,
) -> Result<Vec<(Pubkey, Account)>, backoff::Error<anyhow::Error>> {
    let account_type_filter = RpcFilterType::Memcmp(Memcmp::new_base58_encoded(
        0,
        &adrena::Position::get_anchor_discriminator(),
    ));
    let config = RpcProgramAccountsConfig {
        filters: Some([vec![account_type_filter]].concat()),
        account_config: RpcAccountInfoConfig {
            encoding: Some(UiAccountEncoding::Base64),
            ..RpcAccountInfoConfig::default()
        },
        ..RpcProgramAccountsConfig::default()
    };

    let positions_pdas = rpc
        .get_program_accounts_with_config(program_id, config)
        .await
        .map_err(|e| backoff::Error::transient(e.into()))?;
    Ok(positions_pdas)
}

async fn get_associated_price_update_v2_pdas(
    rpc: &RpcClient,
    existing_positions: &[(Pubkey, Account)],
) -> Result<HashSet<Pubkey>, backoff::Error<anyhow::Error>> {
    let mut custodies: HashSet<Pubkey> = HashSet::new();

    for (_, position_acc) in existing_positions {
        // Deserialize position to get custody
        let position: adrena::Position =
            borsh::BorshDeserialize::deserialize(&mut &position_acc.data[8..])
                .map_err(|e| backoff::Error::transient(e.into()))?;
        custodies.insert(position.custody);
    }
    info!("  <> fetched {} custodies", custodies.len());

    // If there are no custodies, return an empty set
    if custodies.is_empty() {
        return Ok(HashSet::new());
    }

    let mut oracle_price_feeds: HashSet<Pubkey> = HashSet::new();

    for custody_key in custodies {
        let custody_acc = rpc
            .get_account(&custody_key)
            .await
            .map_err(|e| backoff::Error::transient(e.into()))?;
        // Deserialize to get oracle price
        let custody: adrena::Custody =
            borsh::BorshDeserialize::deserialize(&mut &custody_acc.data[8..])
                .map_err(|e| backoff::Error::transient(e.into()))?;
        info!("  <> custody: {:#?}", custody);
        oracle_price_feeds.insert(custody.trade_oracle);
    }
    info!(
        "  <> fetched {} oracle price feeds",
        oracle_price_feeds.len()
    );

    Ok(oracle_price_feeds)
}
