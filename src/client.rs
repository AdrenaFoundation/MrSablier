use {
    adrena_abi::{
        main_pool::USDC_CUSTODY_ID,
        types::{Cortex, Position},
    },
    anchor_client::{
        anchor_lang::AccountDeserialize, solana_sdk::signer::keypair::read_keypair_file, Client,
        Cluster,
    },
    backoff::{future::retry, ExponentialBackoff},
    clap::Parser,
    futures::{future::TryFutureExt, stream::StreamExt},
    handlers::{
        get_mean_prioritization_fee_by_percentile, GetRecentPrioritizationFeesByPercentileConfig,
    },
    pyth::PriceUpdateV2,
    solana_client::{
        nonblocking::rpc_client::RpcClient,
        rpc_filter::{Memcmp, RpcFilterType},
    },
    solana_sdk::{pubkey::Pubkey, signature::Keypair},
    std::{
        collections::{HashMap, HashSet},
        env,
        error::Error,
        sync::Arc,
        time::Duration,
    },
    tokio::{
        sync::{Mutex, RwLock},
        time::interval,
    },
    tonic::transport::channel::ClientTlsConfig,
    utils::OraclePrice,
    yellowstone_grpc_client::{GeyserGrpcClient, Interceptor},
    yellowstone_grpc_proto::{
        geyser::{
            subscribe_update::UpdateOneof, SubscribeRequest, SubscribeRequestFilterAccountsFilter,
            SubscribeRequestFilterAccountsFilterMemcmp, SubscribeUpdate,
        },
        prelude::{
            subscribe_request_filter_accounts_filter::Filter as AccountsFilterDataOneof,
            subscribe_request_filter_accounts_filter_memcmp::Data as AccountsFilterMemcmpOneof,
            CommitmentLevel, SubscribeRequestFilterAccounts,
        },
    },
};

type AccountFilterMap = HashMap<String, SubscribeRequestFilterAccounts>;

type IndexedPositionsThreadSafe = Arc<RwLock<HashMap<Pubkey, adrena_abi::types::Position>>>;
type IndexedCustodiesThreadSafe = Arc<RwLock<HashMap<Pubkey, adrena_abi::types::Custody>>>;

// pub const PYTH_ORACLE_PROGRAM: &str = "FsJ3A3u2vn5cTVofAjvy6y5kwABJAqYWpe4975bi2epH";
// https://solscan.io/account/rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ
pub const PYTH_RECEIVER_PROGRAM: &str = "rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ";

pub mod handlers;
pub mod pyth;
pub mod utils;

const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:10000";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MEAN_PRIORITY_FEE_PERCENTILE: u64 = 3000; // 30th
const PRIORITY_FEE_REFRESH_INTERVAL: Duration = Duration::from_secs(5); // seconds
pub const CLOSE_POSITION_LONG_CU_LIMIT: u32 = 380_000;
pub const CLOSE_POSITION_SHORT_CU_LIMIT: u32 = 250_000;

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
            .tls_config(ClientTlsConfig::new())?
            .connect()
            .await
            .map_err(Into::into)
    }
}

pub fn get_position_anchor_discriminator() -> Vec<u8> {
    utils::derive_discriminator("Position").to_vec()
}

fn create_accounts_filter_map(trade_oracles_keys: Vec<Pubkey>) -> AccountFilterMap {
    let mut accounts_filter_map: AccountFilterMap = HashMap::new();

    // Positions
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
        "positions".to_owned(),
        SubscribeRequestFilterAccounts {
            account: vec![],
            owner: position_owner,
            filters: vec![position_filter_discriminator],
        },
    );

    // Price update v2 pdas
    let price_feed_owner = vec![PYTH_RECEIVER_PROGRAM.to_owned()];
    accounts_filter_map.insert(
        "price_feeds".to_owned(),
        SubscribeRequestFilterAccounts {
            account: trade_oracles_keys.iter().map(|p| p.to_string()).collect(),
            owner: price_feed_owner,
            filters: vec![],
        },
    );

    accounts_filter_map
}

async fn fetch_priority_fee(client: &RpcClient) -> Result<u64, anyhow::Error> {
    // Change the return type to anyhow::Error
    let config = GetRecentPrioritizationFeesByPercentileConfig {
        percentile: Some(MEAN_PRIORITY_FEE_PERCENTILE),
        fallback: false,
        locked_writable_accounts: vec![], //adrena_abi::MAIN_POOL_ID, adrena_abi::CORTEX_ID],
    };
    get_mean_prioritization_fee_by_percentile(client, &config, None)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch mean priority fee: {:?}", e))
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

        async move {
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
                .map_err(|e| backoff::Error::transient(e.into()))?;

            let payer = read_keypair_file(args.payer_keypair.clone()).unwrap();
            let client = Client::new(
                Cluster::Custom(args.endpoint.clone(), args.endpoint.clone()),
                Arc::new(payer), // Change Rc to Arc
            );
            let program = client
                .program(adrena_abi::ID)
                .map_err(|e| backoff::Error::transient(e.into()))?;
            log::info!("  <> gRPC, RPC clients connected!");

            // Fetched once
            let cortex: Cortex = program
                .account::<Cortex>(adrena_abi::pda::get_cortex_pda().0)
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
                let existing_positions_accounts = program
                    .accounts::<Position>(filters)
                    .await
                    .map_err(|e| backoff::Error::transient(e.into()))?;
                {
                    let mut indexed_positions = indexed_positions.write().await;
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
            // to inform the server about the accounts we are interested in observing changes  to
            // ////////////////////////////////////////////////////////////////
            log::info!("2 - Initializing account filter map for...");
            let accounts_filter_map = {
                // Retrieve the price update v2 pdas from the indexed custodies
                // They'll be monitored for updates
                let trade_oracle_keys: Vec<Pubkey> = indexed_custodies
                    .read()
                    .await
                    .values()
                    .map(|c| c.trade_oracle)
                    .collect();

                // Create the accounts filter map (on all positions based on discriminator, and the above price update v2 pdas)
                create_accounts_filter_map(trade_oracle_keys)
            };
            // ////////////////////////////////////////////////////////////////

            // ////////////////////////////////////////////////////////////////
            log::info!("3 - Opening stream...");
            let (mut _subscribe_tx, mut stream) = {
                let request = SubscribeRequest {
                    ping: None,
                    accounts: accounts_filter_map,
                    commitment: commitment.map(|c| c.into()),
                    ..Default::default()
                };
                log::debug!("Sending subscription request: {:?}", request);
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
            let rpc_client = program.async_rpc();
            // Spawn a task to poll priority fees every 5 seconds
            let _fee_task = {
                let median_priority_fee = Arc::clone(&median_priority_fee);
                tokio::spawn(async move {
                    let mut fee_refresh_interval = interval(PRIORITY_FEE_REFRESH_INTERVAL);
                    loop {
                        fee_refresh_interval.tick().await;
                        if let Ok(fee) = fetch_priority_fee(&rpc_client).await {
                            let mut fee_lock = median_priority_fee.lock().await;
                            *fee_lock = fee;
                            log::info!(
                            "  <> Updated median priority fee 30th percentile to : {} µLamports / cu",
                            fee
                        );
                        }
                    }
                })
            };

            // ////////////////////////////////////////////////////////////////
            // CORE LOOP
            //
            // Here we wait for new messages from the stream and process them
            // if coming from the price update v2 accounts, we check for 
            // liquidation/sl/tp conditions on the already indexed positions if 
            // coming from the position accounts, we update the indexed positions map
            // ////////////////////////////////////////////////////////////////
            loop {
                if let Some(message) = stream.next().await {
                    match process_stream_message(
                        message.map_err(|e| backoff::Error::transient(e.into())),
                        &indexed_positions,
                        &indexed_custodies,
                        &program,
                        &cortex,
                        *median_priority_fee.lock().await,
                    )
                    .await
                    {
                        Ok(_) => continue,
                        Err(backoff::Error::Permanent(e)) => {
                            log::error!("Permanent error: {:?}", e);
                            break; // or handle differently
                        }
                        Err(backoff::Error::Transient { err, .. }) => {
                            log::warn!("Transient error: {:?}", err);
                            // Handle transient error without breaking the loop
                        }
                    }
                }
            }

            // log::info!("  <> stream closed");

            // Ensure the fee task is awaited or handled properly
            // fee_task.await.unwrap();

            
            Ok::<(), backoff::Error<anyhow::Error>>(())
        }
        .inspect_err(|error| log::error!("failed to connect: {error}"))
    })
    .await
    .map_err(Into::into)
}

async fn process_stream_message(
    message: Result<SubscribeUpdate, backoff::Error<anyhow::Error>>,
    indexed_positions: &IndexedPositionsThreadSafe,
    indexed_custodies: &IndexedCustodiesThreadSafe,
    program: &anchor_client::Program<Arc<Keypair>>,
    cortex: &Cortex,
    median_priority_fee: u64,
) -> Result<(), backoff::Error<anyhow::Error>> {
    match message {
        Ok(msg) => {
            if let Some(UpdateOneof::Account(sua)) = msg.update_oneof {
                let account = sua.account.expect("Account should be defined");
                let account_key = Pubkey::try_from(account.pubkey).expect("valid pubkey");
                let account_data = account.data.to_vec();

                if msg.filters.contains(&"price_feeds".to_owned()) {
                    check_liquidation_sl_tp_conditions(
                        &account_key,
                        &account_data,
                        indexed_positions,
                        indexed_custodies,
                        program,
                        cortex,
                        median_priority_fee,
                    )
                    .await?;
                }

                if msg.filters.contains(&"positions".to_owned()) {
                    let indexed_custodies_count_before = indexed_custodies.read().await.len();
                    // Also updates the indexed custodies map based on the new position
                    update_indexed_positions(
                        &account_key,
                        &account_data,
                        indexed_positions,
                        indexed_custodies,
                        program,
                    )
                    .await?;
                    let indexed_custodies_count_after = indexed_custodies.read().await.len();

                    // if the indexed custodies map has a new custody, update the subscription request
                    if indexed_custodies_count_after > indexed_custodies_count_before {
                        // TODO: update the subscription request
                    }
                }
            }
            // log::debug!("new message: {msg:?}")
        }
        Err(error) => {
            log::error!("error: {error:?}");
            return Err(error);
        }
    }
    Ok(())
}

// Check liquidation/sl/tp conditions for related positions
// Based on the provided price_update_v2 account, check if any of the related positions have crossed their liquidation/sl/tp conditions
// First go over the price update v2 account to get the price, then go over the indexed positions to check if any of them have crossed their conditions
pub async fn check_liquidation_sl_tp_conditions(
    trade_oracle_key: &Pubkey,
    trade_oracle_data: &[u8],
    indexed_positions: &IndexedPositionsThreadSafe,
    indexed_custodies: &IndexedCustodiesThreadSafe,
    program: &anchor_client::Program<Arc<Keypair>>,
    cortex: &Cortex,
    median_priority_fee: u64,
) -> Result<(), backoff::Error<anyhow::Error>> {
    log::debug!("    <> Checking liquidation/sl/tp conditions");

    // Deserialize price update v2 account
    let trade_oracle: PriceUpdateV2 =
        borsh::BorshDeserialize::deserialize(&mut &trade_oracle_data[8..])
            .map_err(|e| backoff::Error::transient(e.into()))?;
    log::debug!("    <> Deserialized price_update_v2");

    // TODO: Optimize this by not creating the OraclePrice struct from the price update v2 account but just using the price and conf directly
    // Create an OraclePrice struct from the price update v2 account
    let oracle_price: OraclePrice = OraclePrice::new_from_pyth_price_update_v2(&trade_oracle)
        .map_err(|e| backoff::Error::transient(e.into()))?;
    log::debug!("    <> Created OraclePrice struct from price_update_v2");

    // Find the custody key associated with the trade oracle key
    let associated_custody_key = indexed_custodies
        .read()
        .await
        .iter()
        .find(|(_, c)| c.trade_oracle == *trade_oracle_key)
        .map(|(k, _)| k.clone())
        .ok_or(anyhow::anyhow!("No custody found for trade oracle key"))?;

    log::info!("    <> Pricefeed {:#?} update (price: {})", associated_custody_key, oracle_price.price);
    // check SL/TP/LIQ conditions for all indexed positions associated with the associated_custody_key
    // and that are not pending cleanup and close (just in case the position was partially handled by sablier)
    for (position_key, position) in
        indexed_positions.read().await.iter().filter(|(_, p)| {
            p.custody == associated_custody_key && p.pending_cleanup_and_close == 0
        })
    {
        match position.side {
            1 => {
                /* LONG */

                // Check SL
                if position.stop_loss_thread_is_set != 0 {
                    handlers::sl_long::sl_long(
                        position_key,
                        position,
                        &oracle_price,
                        indexed_custodies,
                        program,
                        cortex,
                        median_priority_fee,
                    )
                    .await?;
                }

                // Check TP
                if position.take_profit_thread_is_set != 0 {
                    handlers::tp_long::tp_long(
                        position_key,
                        position,
                        &oracle_price,
                        indexed_custodies,
                        program,
                        cortex,
                        median_priority_fee,
                    )
                    .await?;
                }

                // Check LIQ
                // TODO : recalculate the liquidation price and check if the price has crossed it
                // log::warn!("TODO: check LIQ");
            }
            0 => {
                /* SHORT */

                // Check SL
                if position.stop_loss_thread_is_set != 0 {
                    handlers::sl_short::sl_short(
                        position_key,
                        position,
                        &oracle_price,
                        indexed_custodies,
                        program,
                        cortex,
                        median_priority_fee,
                    )
                    .await?;
                }

                // Check TP
                if position.take_profit_thread_is_set != 0 {
                    handlers::tp_short::tp_short(
                        position_key,
                        position,
                        &oracle_price,
                        indexed_custodies,
                        program,
                        cortex,
                        median_priority_fee,
                    )
                    .await?;
                }

                // Check LIQ
                // TODO : recalculate the liquidation price and check if the price has crossed it
                // log::warn!("TODO: check LIQ");
            }
            _ => {}
        }
    }
    Ok(())
}

// Update the indexed positions map with the account received from the stream
// The update can be about creating or deleting or modifying a position
// This is based on the currently indexed positions and the pubkey/data of the account received
// - If the position is not currently indexed, it means it is a new position and we need to create an entry for it in the map
// - If the account data is empty, it means the position was closed and we need to delete it from the map
// - If the position is currently indexed and is in the received account with non-empty data, it means the position was modified and we need to update the existing entry in the map with the new data
pub async fn update_indexed_positions(
    position_account_key: &Pubkey,
    position_account_data: &[u8],
    indexed_positions: &IndexedPositionsThreadSafe,
    indexed_custodies: &IndexedCustodiesThreadSafe,
    program: &anchor_client::Program<Arc<Keypair>>,
) -> Result<(), backoff::Error<anyhow::Error>> {
    if position_account_data.is_empty() {
        log::info!("Position closed: {:#?}", position_account_key);
        indexed_positions
            .write()
            .await
            .remove(&position_account_key);
    } else {
        let position: adrena_abi::types::Position =
            adrena_abi::types::Position::try_deserialize(&mut &position_account_data[..])
                .map_err(|e| backoff::Error::transient(e.into()))?;

        let mut positions = indexed_positions.write().await;
        if positions.contains_key(&position_account_key) {
            log::info!("Position modified: {:#?}", position_account_key);
        } else {
            log::info!("New position created: {:#?}", position_account_key);
        }
        // Either way, update or create, this operation is the sames
        positions.insert(position_account_key.clone(), position);

        // If the indexed_custodies map doesn't have the position's custody, add it
        if !indexed_custodies
            .read()
            .await
            .contains_key(&position.custody)
        {
            let custody = program
                .account::<adrena_abi::types::Custody>(position.custody)
                .await
                .map_err(|e| backoff::Error::transient(e.into()))?;
            indexed_custodies
                .write()
                .await
                .insert(position.custody, custody);
        }
    }
    Ok(())
}

// Update the indexed custodies map based on the indexed positions
async fn update_indexed_custodies(
    program: &anchor_client::Program<Arc<Keypair>>,
    indexed_positions: &Arc<RwLock<HashMap<Pubkey, adrena_abi::types::Position>>>,
    indexed_custodies: &Arc<RwLock<HashMap<Pubkey, adrena_abi::types::Custody>>>,
) -> Result<(), backoff::Error<anyhow::Error>> {
    let custody_keys: HashSet<_> = indexed_positions
        .read()
        .await
        .values()
        .map(|p| p.custody)
        .collect();

    log::info!(
        "  <> Existing positions have {} unique custodies",
        custody_keys.len()
    );

    for custody_key in custody_keys {
        let custody = program
            .account::<adrena_abi::types::Custody>(custody_key)
            .await
            .map_err(|e| backoff::Error::transient(e.into()))?;
        indexed_custodies.write().await.insert(custody_key, custody);
    }

    Ok(())
}

