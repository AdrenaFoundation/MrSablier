use {
    adrena_abi::{
        main_pool::USDC_CUSTODY_ID,
        types::{Cortex, Position},
        Side,
    },
    anchor_client::{
        anchor_lang::AccountDeserialize, solana_sdk::signer::keypair::read_keypair_file, Client,
        Cluster,
    },
    backoff::{future::retry, ExponentialBackoff},
    clap::Parser,
    futures::{channel::mpsc::SendError, Sink, SinkExt, StreamExt, TryFutureExt},
    handlers::{
        cleanup_position_stop_loss, cleanup_position_take_profit,
        get_mean_prioritization_fee_by_percentile, GetRecentPrioritizationFeesByPercentileConfig,
    },
    pyth::PriceUpdateV2,
    solana_client::rpc_filter::{Memcmp, RpcFilterType},
    solana_sdk::{pubkey::Pubkey, signature::Keypair},
    std::{
        collections::{HashMap, HashSet},
        env,
        sync::Arc,
        time::Duration,
    },
    tokio::{
        sync::{Mutex, RwLock},
        task::JoinHandle,
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

    // Existing positions - We monitor these to check when they are closed
    // (during a close the Anchor Discriminator is also removed so previous filter won't catch it)
    // let position_filter_zeroed = SubscribeRequestFilterAccountsFilter {
    //     filter: Some(AccountsFilterDataOneof::Datasize(0)),
    // };
    accounts_filter_map.insert(
        "positions_close".to_owned(),
        SubscribeRequestFilterAccounts {
            account: existing_positions_keys,
            owner: vec![],
            filters: vec![],
        },
    );

    // Price update v2 pdas
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

async fn fetch_priority_fee(client: &Client<Arc<Keypair>>) -> Result<u64, anyhow::Error> {
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
                    ping: None,
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
                        if let Ok(fee) = fetch_priority_fee(&client).await {
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

async fn process_stream_message<S>(
    message: Result<SubscribeUpdate, backoff::Error<anyhow::Error>>,
    indexed_positions: &IndexedPositionsThreadSafe,
    indexed_custodies: &IndexedCustodiesThreadSafe,
    payer: &Arc<Keypair>,
    endpoint: &str,
    cortex: &Cortex,
    subscribe_tx: &mut S,
    median_priority_fee: u64,
) -> Result<(), backoff::Error<anyhow::Error>>
where
    S: Sink<SubscribeRequest, Error = SendError> + Unpin,
{
    let mut subscriptions_update_required = false;

    match message {
        Ok(msg) => {
            if let Some(UpdateOneof::Account(sua)) = msg.update_oneof {
                let account = sua.account.expect("Account should be defined");
                let account_key = Pubkey::try_from(account.pubkey).expect("valid pubkey");
                let account_data = account.data.to_vec();
                // Each loop iteration we check if we need to update the subscription request based on what previously happened

                if msg.filters.contains(&"price_feeds".to_owned()) {
                    evaluate_and_run_automated_orders(
                        &account_key,
                        &account_data,
                        indexed_positions,
                        indexed_custodies,
                        payer,
                        endpoint,
                        cortex,
                        median_priority_fee,
                    )
                    .await?;
                    // No subscriptions update needed here as this will trigger the update in the next cycle for position change (and update filters there)
                }

                if msg.filters.contains(&"positions_create_update".to_owned()) {
                    // Updates the indexed positions map
                    let update =
                        update_indexed_positions(&account_key, &account_data, indexed_positions)
                            .await?;

                    // Update the indexed custodies map and the subscriptions request if a new position was created
                    match update {
                        PositionUpdate::Created(new_position) => {
                            log::info!("(pcu) New position created: {:#?}", account_key);
                            // If the new position's custody is not yet in the indexed custodies map, fetch it from the RPC
                            if !indexed_custodies
                                .read()
                                .await
                                .contains_key(&new_position.custody)
                            {
                                let client = Client::new(
                                    Cluster::Custom(endpoint.to_string(), endpoint.to_string()),
                                    Arc::clone(payer),
                                );
                                let program = client
                                    .program(adrena_abi::ID)
                                    .map_err(|e| backoff::Error::transient(e.into()))?;

                                let custody = program
                                    .account::<adrena_abi::types::Custody>(new_position.custody)
                                    .await
                                    .map_err(|e| backoff::Error::transient(e.into()))?;

                                // Update the indexed custodies map
                                indexed_custodies
                                    .write()
                                    .await
                                    .insert(new_position.custody, custody);
                                log::info!(
                                    "(pcu) Fetched new custody {:#?} from RPC",
                                    new_position.custody
                                );
                            }

                            // We need to update the subscriptions request to include the new position (and maybe the new custody's price update v2 account)
                            subscriptions_update_required = true;
                        }
                        PositionUpdate::PendingCleanupAndClose(position) => {
                            log::info!(
                                "(pcu) Position pending cleanup and close: {:#?}",
                                account_key
                            );
                            // Do the cleanup and close for the position in stasis
                            let client = Client::new(
                                Cluster::Custom(endpoint.to_string(), endpoint.to_string()),
                                Arc::clone(payer),
                            );
                            cleanup_position(&account_key, &position, &client, median_priority_fee)
                                .await?;
                        }
                        PositionUpdate::Modified(_position) => {
                            log::info!("(pcu) Position modified: {:#?}", account_key);
                        }
                        PositionUpdate::Closed => {
                            log::info!("(pcu) Position closed: {:#?}", account_key);
                        }
                    }
                }
                /* Else if is important as we only want to end up here if the message is not about a  positions_create_update*/
                else if msg.filters.contains(&"positions_close".to_owned()) {
                    // Updates the indexed positions map
                    let update =
                        update_indexed_positions(&account_key, &account_data, indexed_positions)
                            .await?;
                    match update {
                        PositionUpdate::Created(_) => {
                            panic!("New position created in positions_close filter");
                        }
                        PositionUpdate::Modified(_) => {
                            panic!("New position created in positions_close filter");
                        }
                        PositionUpdate::PendingCleanupAndClose(_) => {
                            panic!("New position created in positions_close filter");
                        }
                        PositionUpdate::Closed => {
                            log::info!("(pc) Position closed: {:#?}", account_key);
                            // We need to update the subscriptions request to remove the closed position
                            subscriptions_update_required = true;
                        }
                    }
                }
            }
        }
        Err(error) => {
            log::error!("error: {error:?}");
            return Err(error);
        }
    }

    // Update the subscriptions request if needed
    if subscriptions_update_required {
        log::info!("  <> Update subscriptions request");
        let accounts_filter_map =
            generate_accounts_filter_map(indexed_custodies, indexed_positions).await;
        let request = SubscribeRequest {
            // ping: None,
            accounts: accounts_filter_map,
            // commitment: commitment.map(|c| c.into()),
            ..Default::default()
        };
        subscribe_tx
            .send(request)
            .await
            .map_err(|e| backoff::Error::transient(e.into()))?;
    }
    Ok(())
}

// Check liquidation/sl/tp conditions for related positions
// Based on the provided price_update_v2 account, check if any of the related positions have crossed their liquidation/sl/tp conditions
// First go over the price update v2 account to get the price, then go over the indexed positions to check if any of them have crossed their conditions
pub async fn evaluate_and_run_automated_orders(
    trade_oracle_key: &Pubkey,
    trade_oracle_data: &[u8],
    indexed_positions: &IndexedPositionsThreadSafe,
    indexed_custodies: &IndexedCustodiesThreadSafe,
    payer: &Arc<Keypair>,
    endpoint: &str,
    cortex: &Cortex,
    median_priority_fee: u64,
) -> Result<(), backoff::Error<anyhow::Error>> {
    // Deserialize price update v2 account
    let trade_oracle: PriceUpdateV2 =
        borsh::BorshDeserialize::deserialize(&mut &trade_oracle_data[8..])
            .map_err(|e| backoff::Error::transient(e.into()))?;

    // TODO: Optimize this by not creating the OraclePrice struct from the price update v2 account but just using the price and conf directly
    // Create an OraclePrice struct from the price update v2 account
    let oracle_price: OraclePrice = OraclePrice::new_from_pyth_price_update_v2(&trade_oracle)
        .map_err(backoff::Error::transient)?;

    // Find the custody key associated with the trade oracle key
    let associated_custody_key = indexed_custodies
        .read()
        .await
        .iter()
        .find(|(_, c)| c.trade_oracle == *trade_oracle_key)
        .map(|(k, _)| *k)
        .ok_or(anyhow::anyhow!("No custody found for trade oracle key"))?;

    log::debug!(
        "    <> Pricefeed {:#?} update (price: {})",
        associated_custody_key,
        oracle_price.price
    );

    // check SL/TP/LIQ conditions for all indexed positions associated with the associated_custody_key
    // and that are not pending cleanup and close (just in case the position was partially handled by sablier)

    // make a clone of the indexed positions map to iterate over (while we modify the original map)
    let positions_shallow_clone = indexed_positions.read().await.clone();
    let mut tasks = vec![];

    for (position_key, position) in positions_shallow_clone
        .iter()
        .filter(|(_, p)| p.custody == associated_custody_key && !p.is_pending_cleanup_and_close())
    {
        let position_key = *position_key;
        let position = *position;
        let indexed_custodies = Arc::clone(indexed_custodies);
        let cortex = *cortex;
        let median_priority_fee = median_priority_fee;

        let client = Client::new(
            Cluster::Custom(endpoint.to_string(), endpoint.to_string()),
            Arc::clone(payer),
        );
        let task = tokio::spawn(async move {
            let result: Result<(), anyhow::Error> = async {
                match position.get_side() {
                    Side::Long => {
                        // Check SL
                        if position.stop_loss_is_set() {
                            if let Err(e) = handlers::sl_long::sl_long(
                                &position_key,
                                &position,
                                &oracle_price,
                                &indexed_custodies,
                                &client,
                                &cortex,
                                median_priority_fee,
                            )
                            .await
                            {
                                log::error!("Error in sl_long: {}", e);
                            }
                        }

                        // Check TP
                        if position.take_profit_is_set() {
                            if let Err(e) = handlers::tp_long::tp_long(
                                &position_key,
                                &position,
                                &oracle_price,
                                &indexed_custodies,
                                &client,
                                &cortex,
                                median_priority_fee,
                            )
                            .await
                            {
                                log::error!("Error in tp_long: {}", e);
                            }
                        }

                        // Check LIQ
                        if let Err(e) = handlers::liquidate_long::liquidate_long(
                            &position_key,
                            &position,
                            &oracle_price,
                            &indexed_custodies,
                            &client,
                            &cortex,
                            median_priority_fee,
                        )
                        .await
                        {
                            log::error!("Error in liquidate_long: {}", e);
                        }
                    }
                    Side::Short => {
                        // Check SL
                        if position.stop_loss_is_set() {
                            if let Err(e) = handlers::sl_short::sl_short(
                                &position_key,
                                &position,
                                &oracle_price,
                                &indexed_custodies,
                                &client,
                                &cortex,
                                median_priority_fee,
                            )
                            .await
                            {
                                log::error!("Error in sl_short: {}", e);
                            }
                        }

                        // Check TP
                        if position.take_profit_is_set() {
                            if let Err(e) = handlers::tp_short::tp_short(
                                &position_key,
                                &position,
                                &oracle_price,
                                &indexed_custodies,
                                &client,
                                &cortex,
                                median_priority_fee,
                            )
                            .await
                            {
                                log::error!("Error in tp_short: {}", e);
                            }
                        }

                        // Check LIQ
                        if let Err(e) = handlers::liquidate_short::liquidate_short(
                            &position_key,
                            &position,
                            &oracle_price,
                            &indexed_custodies,
                            &client,
                            &cortex,
                            median_priority_fee,
                        )
                        .await
                        {
                            log::error!("Error in liquidate_short: {}", e);
                        }
                    }
                    _ => {}
                }
                Ok::<(), anyhow::Error>(())
            }
            .await;

            if let Err(e) = result {
                log::error!("Error processing position {}: {:?}", position_key, e);
            }
        });

        tasks.push(task);
    }

    // Wait for all tasks to complete
    for task in tasks {
        if let Err(e) = task.await {
            log::error!("Task failed: {:?}", e);
        }
    }
    Ok(())
}

pub enum PositionUpdate {
    Created(Position),
    Modified(Position),
    PendingCleanupAndClose(Position),
    Closed,
}

// Update the indexed positions map with the account received from the stream
// The update can be about creating or deleting or modifying a position
// This is based on the currently indexed positions and the pubkey/data of the account received
// - If the position is not currently indexed, it means it is a new position and we need to create an entry for it in the map
// - If the account data is empty, it means the position was closed and we need to delete it from the map
// - If the position is currently indexed and is in the received account with non-empty data, it means the position was modified and we need to update the existing entry in the map with the new data
//
// Return the updated position if it was created, None if it was closed or modified
pub async fn update_indexed_positions(
    position_account_key: &Pubkey,
    position_account_data: &[u8],
    indexed_positions: &IndexedPositionsThreadSafe,
) -> Result<PositionUpdate, backoff::Error<anyhow::Error>> {
    if position_account_data.is_empty() {
        indexed_positions.write().await.remove(position_account_key);
        return Ok(PositionUpdate::Closed);
    }
    let position: adrena_abi::types::Position =
        adrena_abi::types::Position::try_deserialize(&mut &position_account_data[..])
            .map_err(|e| backoff::Error::transient(e.into()))?;

    // Special case - If Sablier race us for the Close/liquidate, we want to remove the position (if closed by sablier it will remain in cleanup and close mode)
    if position.is_pending_cleanup_and_close() {
        indexed_positions.write().await.remove(position_account_key);
        return Ok(PositionUpdate::PendingCleanupAndClose(position));
    }
    let mut positions = indexed_positions.write().await;
    let is_new_position = !positions.contains_key(position_account_key);
    // Either way, update or create, this operation is the sames
    positions.insert(*position_account_key, position);

    if is_new_position {
        return Ok(PositionUpdate::Created(position));
    }
    Ok(PositionUpdate::Modified(position))
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

pub async fn cleanup_position(
    position_key: &Pubkey,
    position: &adrena_abi::types::Position,
    client: &Client<Arc<Keypair>>,
    median_priority_fee: u64,
) -> Result<(), backoff::Error<anyhow::Error>> {
    if position.take_profit_is_set() && position.stop_loss_is_set() {
        handlers::cleanup_position_sl_tp::cleanup_position(
            position_key,
            position,
            client,
            median_priority_fee,
        )
        .await?;
    } else if position.stop_loss_is_set() {
        cleanup_position_stop_loss(position_key, position, client, median_priority_fee).await?;
    } else if position.take_profit_is_set() {
        cleanup_position_take_profit(position_key, position, client, median_priority_fee).await?;
    }
    Ok(())
}
