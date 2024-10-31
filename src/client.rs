use {
    crate::{
        process_stream_message::process_stream_message, update_indexes::update_indexed_custodies,
    },
    adrena_abi::{
        main_pool::USDC_CUSTODY_ID,
        types::{Cortex, Position},
    },
    anchor_client::{solana_sdk::signer::keypair::read_keypair_file, Client, Cluster},
    backoff::{future::retry, ExponentialBackoff},
    clap::Parser,
    futures::{StreamExt, TryFutureExt},
    priority_fees::fetch_mean_priority_fee,
    solana_client::rpc_filter::{Memcmp, RpcFilterType},
    solana_sdk::pubkey::Pubkey,
    std::{collections::HashMap, env, sync::Arc, time::Duration},
    tokio::{
        sync::{Mutex, RwLock},
        task::JoinHandle,
        time::interval,
    },
    tonic::transport::channel::ClientTlsConfig,
    yellowstone_grpc_client::{GeyserGrpcClient, Interceptor},
    yellowstone_grpc_proto::{
        geyser::{
            SubscribeRequest, SubscribeRequestFilterAccountsFilter,
            SubscribeRequestFilterAccountsFilterMemcmp,
        },
        prelude::{
            subscribe_request_filter_accounts_filter::Filter as AccountsFilterDataOneof,
            subscribe_request_filter_accounts_filter_memcmp::Data as AccountsFilterMemcmpOneof,
            CommitmentLevel, SubscribeRequestFilterAccounts, SubscribeRequestPing,
        },
    },
};

type AccountFilterMap = HashMap<String, SubscribeRequestFilterAccounts>;

type IndexedPositionsThreadSafe = Arc<RwLock<HashMap<Pubkey, adrena_abi::types::Position>>>;
type IndexedCustodiesThreadSafe = Arc<RwLock<HashMap<Pubkey, adrena_abi::types::Custody>>>;

// https://solscan.io/account/rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ
pub const PYTH_RECEIVER_PROGRAM: &str = "rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ";

pub mod evaluate_and_run_automated_orders;
pub mod handlers;
pub mod priority_fees;
pub mod process_stream_message;
pub mod update_indexes;
pub mod utils;

const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:10000";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MEAN_PRIORITY_FEE_PERCENTILE: u64 = 5000; // 50th
const PRIORITY_FEE_REFRESH_INTERVAL: Duration = Duration::from_secs(5); // seconds
pub const CLOSE_POSITION_LONG_CU_LIMIT: u32 = 380_000;
pub const CLOSE_POSITION_SHORT_CU_LIMIT: u32 = 280_000;
pub const CLEANUP_POSITION_CU_LIMIT: u32 = 60_000;
pub const LIQUIDATE_LONG_CU_LIMIT: u32 = 310_000;
pub const LIQUIDATE_SHORT_CU_LIMIT: u32 = 210_000;

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
    #[clap(short, long, default_value_t = String::from(DEFAULT_ENDPOINT))]
    /// Service endpoint
    endpoint: String,

    #[clap(long)]
    x_token: Option<String>,

    /// Commitment level: processed, confirmed or finalized
    #[clap(long)]
    commitment: Option<ArgsCommitment>,

    /// Path to the payer keypair
    #[clap(long)]
    payer_keypair: String,
}

impl Args {
    fn get_commitment(&self) -> Option<CommitmentLevel> {
        Some(self.commitment.unwrap_or_default().into())
    }

    async fn connect(&self) -> anyhow::Result<GeyserGrpcClient<impl Interceptor>> {
        GeyserGrpcClient::build_from_shared(self.endpoint.clone())?
            .x_token(self.x_token.clone())?
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .tls_config(ClientTlsConfig::new().with_native_roots())?
            .connect()
            .await
            .map_err(Into::into)
    }
}

pub fn get_position_anchor_discriminator() -> Vec<u8> {
    utils::derive_discriminator("Position").to_vec()
}

async fn generate_accounts_filter_map(
    indexed_custodies: &IndexedCustodiesThreadSafe,
    indexed_positions: &IndexedPositionsThreadSafe,
) -> AccountFilterMap {
    // Retrieve the price update v2 pdas from the indexed custodies
    let trade_oracle_keys: Vec<String> = indexed_custodies
        .read()
        .await
        .values()
        .map(|c| c.trade_oracle.to_string())
        .collect();

    // Retrieve the existing positions keys - they are monitored for close events
    let existing_positions_keys: Vec<String> = indexed_positions
        .read()
        .await
        .keys()
        .map(|p| p.to_string())
        .collect();

    // Create the accounts filter map (on all positions based on discriminator, and the above price update v2 pdas)
    let mut accounts_filter_map: AccountFilterMap = HashMap::new();

    // Positions (will catch new positions created and modified positions)
    let position_filter_discriminator = SubscribeRequestFilterAccountsFilter {
        filter: Some(AccountsFilterDataOneof::Memcmp(
            SubscribeRequestFilterAccountsFilterMemcmp {
                offset: 0,
                data: Some(AccountsFilterMemcmpOneof::Bytes(
                    get_position_anchor_discriminator(),
                )),
            },
        )),
    };
    let position_owner = vec![adrena_abi::ID.to_string()];
    accounts_filter_map.insert(
        "positions_create_update".to_owned(),
        SubscribeRequestFilterAccounts {
            account: vec![],
            owner: position_owner,
            filters: vec![position_filter_discriminator],
        },
    );

    // Existing positions - We monitor these to catch when they are closed
    accounts_filter_map.insert(
        "positions_close".to_owned(),
        SubscribeRequestFilterAccounts {
            account: existing_positions_keys,
            owner: vec![],
            filters: vec![],
        },
    );

    // Price update v2 pdas - the price updates
    let price_feed_owner = vec![PYTH_RECEIVER_PROGRAM.to_owned()];
    accounts_filter_map.insert(
        "price_feeds".to_owned(),
        SubscribeRequestFilterAccounts {
            account: trade_oracle_keys,
            owner: price_feed_owner,
            filters: vec![],
        },
    );

    accounts_filter_map
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

    // The array of indexed positions - These positions are the ones that we are watching for any updates from the stream and for SL/TP/LIQ conditions
    let indexed_positions: IndexedPositionsThreadSafe = Arc::new(RwLock::new(HashMap::new()));
    // The array of indexed custodies - These are not directly observed, but are needed for instructions and to keep track of which price update v2 accounts are observed
    let indexed_custodies: IndexedCustodiesThreadSafe = Arc::new(RwLock::new(HashMap::new()));

    // The default exponential backoff strategy intervals:
    // [500ms, 750ms, 1.125s, 1.6875s, 2.53125s, 3.796875s, 5.6953125s,
    // 8.5s, 12.8s, 19.2s, 28.8s, 43.2s, 64.8s, 97s, ... ]
    retry(ExponentialBackoff::default(), move || {
        let args = args.clone();
        let zero_attempts = Arc::clone(&zero_attempts);
        let indexed_positions = Arc::clone(&indexed_positions);
        let indexed_custodies = Arc::clone(&indexed_custodies);
        let mut periodical_priority_fees_fetching_task: Option<JoinHandle<Result<(), backoff::Error<anyhow::Error>>>> = None;

        async move {
            // In case it errored out, abort the fee task (will be recreated)
            if let Some(t) = periodical_priority_fees_fetching_task.take() {
                t.abort();
            }

            let mut zero_attempts = zero_attempts.lock().await;
            if *zero_attempts {
                *zero_attempts = false;
            } else {
                log::info!("Retry to connect to the server");
            }
            drop(zero_attempts);

            let commitment = args.get_commitment();
            let mut grpc = args
                .connect()
                .await
                .map_err(backoff::Error::transient)?;

            let payer = read_keypair_file(args.payer_keypair.clone()).unwrap();
            let payer = Arc::new(payer);
            let client = Client::new(
                Cluster::Custom(args.endpoint.clone(), args.endpoint.clone()),
                Arc::clone(&payer),
            );
            let program = client
                .program(adrena_abi::ID)
                .map_err(|e| backoff::Error::transient(e.into()))?;
            log::info!("  <> gRPC, RPC clients connected!");

            // Fetched once
            let cortex: Cortex = program
                .account::<Cortex>(adrena_abi::CORTEX_ID)
                .await
                .map_err(|e| backoff::Error::transient(e.into()))?;

            // Index USDC custody once
            let usdc_custody = program
                .account::<adrena_abi::types::Custody>(USDC_CUSTODY_ID)
                .await
                .map_err(|e| backoff::Error::transient(e.into()))?;
            indexed_custodies
                .write()
                .await
                .insert(USDC_CUSTODY_ID, usdc_custody);

            // ////////////////////////////////////////////////////////////////
            log::info!("1 - Retrieving and indexing existing positions and their custodies...");
            {
                let position_pda_filter = RpcFilterType::Memcmp(Memcmp::new_base58_encoded(
                    0,
                    &get_position_anchor_discriminator(),
                ));
                let filters = vec![position_pda_filter];
                let mut existing_positions_accounts = program
                    .accounts::<Position>(filters)
                    .await
                    .map_err(|e| backoff::Error::transient(e.into()))?;
                {
                    let mut indexed_positions = indexed_positions.write().await;

                    // filter out the positions that are pending cleanup and close
                    existing_positions_accounts.retain(|(_, position)| {
                        !position.is_pending_cleanup_and_close()
                    });

                    indexed_positions.extend(existing_positions_accounts);
                }
                log::info!(
                    "  <> # of existing positions parsed and loaded: {}",
                    indexed_positions.read().await.len()
                );

                // Update the indexed custodies map based on the indexed positions
                update_indexed_custodies(&program, &indexed_positions, &indexed_custodies).await?;
            }
            // ////////////////////////////////////////////////////////////////

            // ////////////////////////////////////////////////////////////////
            // The account filter map is what is provided to the subscription request
            // to inform the server about the accounts we are interested in observing changes to
            // ////////////////////////////////////////////////////////////////
            log::info!("2 - Generate subscription request and open stream...");
            let accounts_filter_map =
                generate_accounts_filter_map(&indexed_custodies, &indexed_positions).await;
            log::info!("  <> Account filter map initialized");
            let (mut subscribe_tx, mut stream) = {
                let request = SubscribeRequest {
                    ping: None,//Some(SubscribeRequestPing { id: 1 }),
                    accounts: accounts_filter_map,
                    commitment: commitment.map(|c| c.into()),
                    ..Default::default()
                };
                log::debug!("  <> Sending subscription request: {:?}", request);
                let (subscribe_tx, stream) = grpc
                    .subscribe_with_request(Some(request))
                    .await
                    .map_err(|e| backoff::Error::transient(e.into()))?;
                log::info!("  <> stream opened");
                (subscribe_tx, stream)
            };


            // ////////////////////////////////////////////////////////////////
            // Side thread to fetch the median priority fee every 5 seconds
            // ////////////////////////////////////////////////////////////////
            let median_priority_fee = Arc::new(Mutex::new(0u64));
            // Spawn a task to poll priority fees every 5 seconds
            log::info!("3 - Spawn a task to poll priority fees every 5 seconds...");
            #[allow(unused_assignments)]
            {
            periodical_priority_fees_fetching_task = Some({
                let median_priority_fee = Arc::clone(&median_priority_fee);
                tokio::spawn(async move {
                    let mut fee_refresh_interval = interval(PRIORITY_FEE_REFRESH_INTERVAL);
                    loop {
                        fee_refresh_interval.tick().await;
                        if let Ok(fee) =
                            fetch_mean_priority_fee(&client, MEAN_PRIORITY_FEE_PERCENTILE).await
                        {
                            let mut fee_lock = median_priority_fee.lock().await;
                            *fee_lock = fee;
                            log::debug!(
                                "  <> Updated median priority fee 30th percentile to : {} µLamports / cu",
                                fee
                            );
                        }
                    }
                    })
                });
            }

            // ////////////////////////////////////////////////////////////////
            // CORE LOOP
            //
            // Here we wait for new messages from the stream and process them
            // if coming from the price update v2 accounts, we check for 
            // liquidation/sl/tp conditions on the already indexed positions if 
            // coming from the position accounts, we update the indexed positions map
            // ////////////////////////////////////////////////////////////////
            log::info!("4 - Start core loop: processing gRPC stream...");
            loop {
                if let Some(message) = stream.next().await {
                    match process_stream_message(
                        message.map_err(|e| backoff::Error::transient(e.into())),
                        &indexed_positions,
                        &indexed_custodies,
                        &payer,
                        &args.endpoint.clone(),
                        &cortex,
                        &mut subscribe_tx,
                        *median_priority_fee.lock().await,
                    )
                    .await
                    {
                        Ok(_) => continue,
                        Err(backoff::Error::Permanent(e)) => {
                            log::error!("Permanent error: {:?}", e);
                            break;
                        }
                        Err(backoff::Error::Transient { err, .. }) => {
                            log::warn!("Transient error: {:?}", err);
                            // Handle transient error without breaking the loop
                        }
                    }
                }
            }

            // log::info!("  <> stream closed");

            Ok::<(), backoff::Error<anyhow::Error>>(())
        }
        .inspect_err(|error| log::error!("failed to connect: {error}"))
    })
    .await
    .map_err(Into::into)
}
