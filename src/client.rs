use {
    crate::{process_stream_message::process_stream_message, update_indexes::update_indexed_custodies},
    adrena_abi::{
        main_pool::USDC_CUSTODY_ID,
        types::{Cortex, Position},
        LimitOrderBook, Pool,
    },
    anchor_client::{solana_sdk::signer::keypair::read_keypair_file, Client, Cluster},
    clap::Parser,
    futures::StreamExt,
    solana_client::rpc_filter::{Memcmp, RpcFilterType},
    solana_sdk::pubkey::Pubkey,
    std::{
        collections::{BTreeMap, HashMap},
        env,
        sync::Arc,
        time::Duration,
    },
    tokio::{sync::RwLock, task::JoinHandle, time::timeout},
    yellowstone_fumarole_client::{config::FumaroleConfig, proto, FumaroleClientBuilder},
    yellowstone_grpc_proto::{
        geyser::{
            subscribe_request_filter_accounts_filter::Filter as AccountsFilterDataOneof,
            subscribe_request_filter_accounts_filter_memcmp::Data as AccountsFilterMemcmpOneof,
        },
        prelude::*,
    },
};

// Thread-safe types
type AccountFilterMap = HashMap<String, SubscribeRequestFilterAccounts>;
type IndexedPositionsThreadSafe = Arc<RwLock<HashMap<Pubkey, adrena_abi::types::Position>>>;
type IndexedCustodiesThreadSafe = Arc<RwLock<HashMap<Pubkey, adrena_abi::types::Custody>>>;
type IndexedLimitOrderBooksThreadSafe = Arc<RwLock<HashMap<Pubkey, adrena_abi::types::LimitOrderBook>>>;
type PriorityFeesThreadSafe = Arc<RwLock<PriorityFees>>;
type SolPriceThreadSafe = Arc<RwLock<f64>>;

// Constants
pub const PYTH_RECEIVER_PROGRAM: &str = "rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ";

// Modules
pub mod evaluate_and_run_automated_orders;
pub mod handlers;
pub mod priority_fees;
pub mod process_stream_message;
pub mod sol_price;
pub mod update_indexes;
pub mod utils;

const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:10000";
const MEDIAN_PRIORITY_FEE_PERCENTILE: u64 = 5000; // 50th
const HIGH_PRIORITY_FEE_PERCENTILE: u64 = 7000; // 70th
const ULTRA_PRIORITY_FEE_PERCENTILE: u64 = 9000; // 90th
pub const CLOSE_POSITION_LONG_CU_LIMIT: u32 = 385_000;
pub const CLOSE_POSITION_SHORT_CU_LIMIT: u32 = 285_000;
pub const CLEANUP_POSITION_CU_LIMIT: u32 = 60_000;
pub const LIQUIDATE_LONG_CU_LIMIT: u32 = 355_000;
pub const LIQUIDATE_SHORT_CU_LIMIT: u32 = 256_000;
pub const EXECUTE_LIMIT_ORDER_LONG_CU_LIMIT: u32 = 200_000;
pub const EXECUTE_LIMIT_ORDER_SHORT_CU_LIMIT: u32 = 200_000;

#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
enum ArgsCommitment {
    #[default]
    Processed,
    Confirmed,
    Finalized,
}

#[derive(Debug, Clone, Parser)]
#[clap(author, version, about)]
struct Args {
    #[clap(short, long, default_value_t = String::from(DEFAULT_ENDPOINT))]
    /// Service endpoint
    endpoint: String,

    #[clap(long)]
    /// Fumarole endpoint
    fumarole_endpoint: String,

    #[clap(long)]
    /// Auth token for Service endpoint
    x_token: Option<String>,

    #[clap(long)]
    /// Auth token for Fumarole service
    fumarole_x_token: Option<String>,

    /// Path to the payer keypair
    #[clap(long)]
    payer_keypair: String,

    /// Name of the existing consumer group to connect to
    #[clap(long)]
    consumer_group: String,

    /// Member ID within the consumer group (0-5)
    #[clap(long, default_value_t = 0)]
    member_id: u32,
}

pub fn get_position_anchor_discriminator() -> Vec<u8> {
    utils::derive_discriminator("Position").to_vec()
}

pub fn get_limit_order_book_anchor_discriminator() -> Vec<u8> {
    utils::derive_discriminator("LimitOrderBook").to_vec()
}

async fn generate_accounts_filter_map(
    indexed_custodies: &IndexedCustodiesThreadSafe,
    indexed_positions: &IndexedPositionsThreadSafe,
    indexed_limit_order_books: &IndexedLimitOrderBooksThreadSafe,
) -> AccountFilterMap {
    // Retrieve the price update v2 pdas from the indexed custodies
    let trade_oracle_keys: Vec<String> = indexed_custodies
        .read()
        .await
        .values()
        .map(|c| c.trade_oracle.to_string())
        .collect();

    // Retrieve the existing positions keys - they are monitored for close events
    let existing_positions_keys: Vec<String> = indexed_positions.read().await.keys().map(|p| p.to_string()).collect();

    // Retrieve the existing limit order book keys - they are monitored for close events
    let existing_limit_order_books_keys: Vec<String> =
        indexed_limit_order_books.read().await.keys().map(|p| p.to_string()).collect();

    // Create the accounts filter map (on all positions based on discriminator, and the above price update v2 pdas)
    let mut accounts_filter_map: AccountFilterMap = HashMap::new();

    // Positions (will catch new positions created and modified positions)
    let position_filter_discriminator = SubscribeRequestFilterAccountsFilter {
        filter: Some(AccountsFilterDataOneof::Memcmp(SubscribeRequestFilterAccountsFilterMemcmp {
            offset: 0,
            data: Some(AccountsFilterMemcmpOneof::Bytes(get_position_anchor_discriminator())),
        })),
    };

    let position_owner = vec![adrena_abi::ID.to_string()];

    accounts_filter_map.insert(
        "positions_create_update".to_owned(),
        SubscribeRequestFilterAccounts {
            account: vec![],
            owner: position_owner,
            filters: vec![position_filter_discriminator],
            nonempty_txn_signature: None,
        },
    );

    if !existing_positions_keys.is_empty() {
        // Existing positions - We monitor these to catch when they are closed
        accounts_filter_map.insert(
            "positions_close".to_owned(),
            SubscribeRequestFilterAccounts {
                account: existing_positions_keys,
                owner: vec![],
                filters: vec![],
                nonempty_txn_signature: None,
            },
        );
    }

    let limit_order_book_owner = vec![adrena_abi::ID.to_string()];

    let limit_order_book_filter_discriminator = SubscribeRequestFilterAccountsFilter {
        filter: Some(AccountsFilterDataOneof::Memcmp(SubscribeRequestFilterAccountsFilterMemcmp {
            offset: 0,
            data: Some(AccountsFilterMemcmpOneof::Bytes(get_limit_order_book_anchor_discriminator())),
        })),
    };

    accounts_filter_map.insert(
        "limit_order_books_create_update".to_owned(),
        SubscribeRequestFilterAccounts {
            account: vec![],
            owner: limit_order_book_owner,
            filters: vec![limit_order_book_filter_discriminator],
            nonempty_txn_signature: None,
        },
    );

    if !existing_limit_order_books_keys.is_empty() {
        // Existing limit order books - We monitor these to catch when they are closed
        accounts_filter_map.insert(
            "limit_order_books_close".to_owned(),
            SubscribeRequestFilterAccounts {
                account: existing_limit_order_books_keys,
                owner: vec![],
                filters: vec![],
                nonempty_txn_signature: None,
            },
        );
    }

    // Price update v2 pdas - the price updates
    let price_feed_owner = vec![PYTH_RECEIVER_PROGRAM.to_owned()];
    accounts_filter_map.insert(
        "price_feeds".to_owned(),
        SubscribeRequestFilterAccounts {
            account: trade_oracle_keys,
            owner: price_feed_owner,
            filters: vec![],
            nonempty_txn_signature: None,
        },
    );

    accounts_filter_map
}

#[derive(Debug, Clone, Copy)]
pub struct PriorityFees {
    pub median: u64,
    pub high: u64,
    pub ultra: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env::set_var(
        env_logger::DEFAULT_FILTER_ENV,
        env::var_os(env_logger::DEFAULT_FILTER_ENV).unwrap_or_else(|| "info".into()),
    );
    env_logger::init();

    let args = Args::parse();

    // The indexed positions structure is a map of position pubkey -> position
    let indexed_positions: IndexedPositionsThreadSafe = Arc::new(RwLock::new(HashMap::new()));
    let indexed_custodies: IndexedCustodiesThreadSafe = Arc::new(RwLock::new(HashMap::new()));
    let indexed_limit_order_books: IndexedLimitOrderBooksThreadSafe = Arc::new(RwLock::new(HashMap::new()));
    let priority_fees: PriorityFeesThreadSafe = Arc::new(RwLock::new(PriorityFees {
        median: 0,
        high: 0,
        ultra: 0,
    }));

    // Initialize SOL price tracking
    let sol_price: SolPriceThreadSafe = Arc::new(RwLock::new(150.0)); // Default price

    // Process message handling loop
    let mut periodical_priority_fees_fetching_task: Option<JoinHandle<anyhow::Result<()>>> = None;

    // Initialize SOL price tracking task
    let mut sol_price_update_task: Option<JoinHandle<()>> = None;

    // Main loop
    loop {
        // ////////////////////////////////////////////////////////////////
        // The account filter map is what is provided to the subscription request
        // to inform the server about the accounts we are interested in observing changes to
        // ////////////////////////////////////////////////////////////////
        log::info!("1 - Generate the accounts filter map...");
        let accounts_filter_map =
            generate_accounts_filter_map(&indexed_custodies, &indexed_positions, &indexed_limit_order_books).await;
        log::info!("  <> Account filter map initialized");

        // ////////////////////////////////////////////////////////////////
        // Creating the Fumarole request from the accounts filter map
        // ////////////////////////////////////////////////////////////////
        log::info!("2 - Create the SuscribeRequest from the accounts filter map...");

        let request = proto::SubscribeRequest {
            consumer_group_label: args.consumer_group.clone(),
            consumer_id: Some(args.member_id),
            accounts: accounts_filter_map,
            transactions: HashMap::new(),
        };

        // ////////////////////////////////////////////////////////////////
        //  Open the connection to Fumarole
        // ////////////////////////////////////////////////////////////////
        log::info!("3 - Connect to Fumarole...");
        let config = FumaroleConfig {
            endpoint: args.fumarole_endpoint.clone(),
            x_token: args.fumarole_x_token.clone(),
            max_decoding_message_size_bytes: 512_000_000,
            x_metadata: BTreeMap::new(),
        };
        let mut fumarole = match FumaroleClientBuilder::default().connect(config).await {
            Ok(client) => client,
            Err(e) => {
                log::error!("Failed to connect to Fumarole: {:?}", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        // ////////////////////////////////////////////////////////////////
        // Subscribe to the stream
        // ////////////////////////////////////////////////////////////////
        log::info!("4 - Subscribe to Fumarole request...");
        let response = match fumarole.subscribe_with_request(request).await {
            Ok(response) => response,
            Err(e) => {
                log::error!("Failed to subscribe to Fumarole: {:?}", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        // ////////////////////////////////////////////////////////////////
        //  Process the stream
        // ////////////////////////////////////////////////////////////////
        log::info!("5 - Process Fumarole stream...");
        let mut rx = response.into_inner();

        // In case it errored out, abort the fee task (will be recreated)
        if let Some(t) = periodical_priority_fees_fetching_task.take() {
            t.abort();
        }

        // Also abort the SOL price task if it exists
        if let Some(t) = sol_price_update_task.take() {
            t.abort();
        }

        // ////////////////////////////////////////////////////////////////
        //  Connect to the RPC and fetch the program accounts
        // ////////////////////////////////////////////////////////////////
        log::info!("6 - Connect to the RPC and fetch the program accounts...");
        let payer = read_keypair_file(args.payer_keypair.clone()).unwrap();
        let payer = Arc::new(payer);
        let client = Client::new(
            Cluster::Custom(args.endpoint.clone(), args.endpoint.clone()),
            Arc::clone(&payer),
        );
        let program = client
            .program(adrena_abi::ID)
            .map_err(|e| anyhow::anyhow!("Failed to create program client: {}", e))?;

        log::info!("  <> Fumarole, RPC clients connected!");

        // Fetched once
        let cortex: Cortex = program
            .account::<Cortex>(adrena_abi::CORTEX_ID)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch cortex: {e}"))?;

        // Fetched once
        let pool = program
            .account::<Pool>(adrena_abi::MAIN_POOL_ID)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch pool: {e}"))?;

        // Index USDC custody once
        let usdc_custody = program
            .account::<adrena_abi::types::Custody>(USDC_CUSTODY_ID)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch USDC custody: {e}"))?;
        indexed_custodies.write().await.insert(USDC_CUSTODY_ID, usdc_custody);

        // ////////////////////////////////////////////////////////////////
        //  Retrieving and indexing existing positions and their custodies
        // ////////////////////////////////////////////////////////////////
        log::info!("7 - Retrieve and index existing positions and their custodies...");
        {
            let position_pda_filter = RpcFilterType::Memcmp(Memcmp::new_base58_encoded(0, &get_position_anchor_discriminator()));
            let filters = vec![position_pda_filter];
            let existing_positions_accounts = program
                .accounts::<Position>(filters)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to fetch existing positions: {e}"))?;
            // Extend the indexed positions map with the existing positions
            indexed_positions.write().await.extend(existing_positions_accounts);
            log::info!(
                "  <> # of existing positions parsed and loaded: {}",
                indexed_positions.read().await.len()
            );
        }

        // ////////////////////////////////////////////////////////////////
        //  Retrieving and indexing existing limit order books
        // ////////////////////////////////////////////////////////////////
        log::info!("8 - Retrieve and index existing limit order books...");
        {
            let limit_order_book_pda_filter =
                RpcFilterType::Memcmp(Memcmp::new_base58_encoded(0, &get_limit_order_book_anchor_discriminator()));
            let filters = vec![limit_order_book_pda_filter];
            let existing_limit_order_book_accounts = program
                .accounts::<LimitOrderBook>(filters)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to fetch existing limit order books: {e}"))?;
            // Extend the indexed limit order books map with the existing limit order books
            indexed_limit_order_books
                .write()
                .await
                .extend(existing_limit_order_book_accounts);
            log::info!(
                "  <> # of existing limit order books parsed and loaded: {}",
                indexed_limit_order_books.read().await.len()
            );
        }

        // ////////////////////////////////////////////////////////////////
        //  Update the indexed custodies map based on the indexed positions
        // ////////////////////////////////////////////////////////////////
        log::info!("9 - Update the indexed custodies map based on the indexed positions...");
        if let Err(e) =
            update_indexed_custodies(&program, &indexed_positions, &indexed_limit_order_books, &indexed_custodies).await
        {
            log::error!("Failed to update indexed custodies: {:?}", e);
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        }
        // ////////////////////////////////////////////////////////////////

        // ////////////////////////////////////////////////////////////////
        // Side thread to fetch the median priority fee every 5 seconds
        // ////////////////////////////////////////////////////////////////
        log::info!("10 - Spawn a task to poll priority fees every 5 seconds...");
        periodical_priority_fees_fetching_task = Some(priority_fees::start_priority_fees_updates(
            args.endpoint.clone(),
            Arc::clone(&priority_fees),
        ));

        // ////////////////////////////////////////////////////////////////
        // Start a task that fetches SOL price every 20 seconds
        // This is used for calculating liquidation fees in USD terms
        // ////////////////////////////////////////////////////////////////
        log::info!("11 - Spawn a task to poll SOL price every 20 seconds...");
        sol_price_update_task = Some(sol_price::start_sol_price_update_task(
            args.endpoint.clone(),
            Arc::clone(&sol_price),
        ));

        // ////////////////////////////////////////////////////////////////
        // CORE LOOP
        //
        // Here we wait for new messages from the stream and process them
        // if coming from the price update v2 accounts, we check for
        // liquidation/sl/tp conditions on the already indexed positions if
        // coming from the position accounts, we update the indexed positions map
        // ////////////////////////////////////////////////////////////////
        log::info!("12 - Start core loop: process Fumarole stream...");
        while let Ok(Some(message)) = timeout(Duration::from_secs(11), rx.next()).await {
            if let Err(e) = process_stream_message(
                message.map_err(|e| backoff::Error::transient(anyhow::anyhow!("Stream error: {}", e))),
                &indexed_positions,
                &indexed_custodies,
                &indexed_limit_order_books,
                &payer,
                &args.endpoint,
                &cortex,
                &pool,
                &priority_fees,
                &sol_price,
            )
            .await
            {
                log::error!("Error processing message: {:?}", e);
                break;
            }
        }

        log::info!("Connection lost, reconnecting in 5 seconds...");
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}
