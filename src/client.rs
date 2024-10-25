use {
    adrena_abi::{
        types::{Cortex, Position},
        ADX_MINT, ALP_MINT, SABLIER_THREAD_PROGRAM_ID, SPL_ASSOCIATED_TOKEN_PROGRAM_ID,
        SPL_TOKEN_PROGRAM_ID, USDC_CUSTODY_ID, USDC_MINT,
    },
    anchor_client::{
        anchor_lang::AccountDeserialize, solana_sdk::signer::keypair::read_keypair_file, Client,
        Cluster,
    },
    backoff::{future::retry, ExponentialBackoff},
    clap::Parser,
    futures::{future::TryFutureExt, stream::StreamExt},
    solana_client::rpc_filter::{Memcmp, RpcFilterType},
    solana_sdk::{pubkey::Pubkey, signature::Keypair},
    std::{
        collections::{HashMap, HashSet},
        env,
        rc::Rc,
        sync::Arc,
        time::Duration,
    },
    tokio::sync::{Mutex, RwLock},
    tonic::transport::channel::ClientTlsConfig,
    yellowstone_grpc_client::{GeyserGrpcClient, Interceptor},
    yellowstone_grpc_proto::{
        geyser::{
            subscribe_update::UpdateOneof, SubscribeRequest, SubscribeRequestFilterAccountsFilter,
            SubscribeRequestFilterAccountsFilterMemcmp,
        },
        prelude::{
            subscribe_request_filter_accounts_filter::Filter as AccountsFilterDataOneof,
            subscribe_request_filter_accounts_filter_memcmp::Data as AccountsFilterMemcmpOneof,
            CommitmentLevel, SubscribeRequestFilterAccounts,
        },
    },
};

type AccountFilterMap = HashMap<String, SubscribeRequestFilterAccounts>;

// pub const PYTH_ORACLE_PROGRAM: &str = "FsJ3A3u2vn5cTVofAjvy6y5kwABJAqYWpe4975bi2epH";
// https://solscan.io/account/rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ
pub const PYTH_RECEIVER_PROGRAM: &str = "rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ";

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
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(10))
            .tls_config(ClientTlsConfig::new())?
            .connect()
            .await
            .map_err(Into::into)
    }
}

pub fn get_position_anchor_discriminator() -> Vec<u8> {
    utils::derive_discriminator("Position").to_vec()
}

fn create_accounts_filter_map(price_update_v2_pdas_pubkeys: Vec<String>) -> AccountFilterMap {
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
        "position".to_owned(),
        SubscribeRequestFilterAccounts {
            account: vec![],
            owner: position_owner,
            filters: vec![position_filter_discriminator],
        },
    );

    // Price update v2 pdas
    let price_feed_owner = vec![PYTH_RECEIVER_PROGRAM.to_owned()];
    accounts_filter_map.insert(
        "price_update_v2".to_owned(),
        SubscribeRequestFilterAccounts {
            account: price_update_v2_pdas_pubkeys,
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
    let indexed_positions: Arc<RwLock<HashMap<Pubkey, adrena_abi::types::Position>>> =
        Arc::new(RwLock::new(HashMap::new()));
    // The array of indexed custodies - These are not directly observed, but are needed for instructions and to keep track of which price update v2 accounts are observed
    let indexed_custodies: Arc<RwLock<HashMap<Pubkey, adrena_abi::types::Custody>>> =
        Arc::new(RwLock::new(HashMap::new()));

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
            // let rpc = RpcClient::new(args.endpoint.clone());

            let payer = read_keypair_file(args.payer_keypair.clone()).unwrap();
            let client = Client::new(
                Cluster::Custom(args.endpoint.clone(), args.endpoint.clone()),
                Rc::new(payer),
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
            log::info!("1 - Retrieving and indexing existing positions...");
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
            }
            // ////////////////////////////////////////////////////////////////

            // ////////////////////////////////////////////////////////////////
            let price_update_v2_pdas = {
                log::info!(
                    "1.2 - Parsing existing positions to retrieve their associated price feeds..."
                );
                let price_update_v2_pdas = get_associated_price_update_v2_keys(
                    &program,
                    &indexed_positions,
                    &indexed_custodies,
                )
                .await?;
                log::info!(
                    "  <> # of price update v2 pdas fetched: {}",
                    price_update_v2_pdas.len()
                );
                price_update_v2_pdas
            };
            // ////////////////////////////////////////////////////////////////

            // ////////////////////////////////////////////////////////////////
            // The account filter map is what is provided to the subscription request
            // to inform the server about the accounts we are interested in observing changes to
            // ////////////////////////////////////////////////////////////////
            log::info!("2 - Initializing account filter map for...");
            let accounts_filter_map = {
                let price_update_v2_pdas_pubkeys = price_update_v2_pdas
                    .iter()
                    .map(|p| p.1.to_string())
                    .collect();
                create_accounts_filter_map(price_update_v2_pdas_pubkeys)
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
            // CORE LOOP
            //
            // Here we wait for new messages from the stream and process them
            // if coming from the price update v2 accounts, we check for liquidation/sl/tp conditions on the already indexed positions
            // if coming from the position accounts, we update the indexed positions map
            // ////////////////////////////////////////////////////////////////
            while let Some(message) = stream.next().await {
                match message {
                    Ok(msg) => {
                        match msg.update_oneof {
                            Some(UpdateOneof::Account(sua)) => {
                                let account = sua.account.expect("Account should be defined");
                                let account_key =
                                    Pubkey::try_from(account.pubkey).expect("valid pubkey");
                                let account_data = account.data.to_vec();

                                log::info!(
                                    "new account update: filters {:?}, account: {:#?}",
                                    msg.filters,
                                    account_key
                                );

                                if msg.filters.contains(&"price_update_v2".to_owned()) {
                                    // Check liquidation/sl/tp conditions for related positions
                                    check_liquidation_sl_tp_conditions(
                                        &account_key,
                                        &account_data,
                                        &indexed_positions,
                                        &indexed_custodies,
                                        &price_update_v2_pdas,
                                        &program,
                                        &cortex,
                                    )
                                    .await?;
                                }

                                if msg.filters.contains(&"position".to_owned()) {
                                    // Update, Create or Delete indexed positions based on the account received
                                    update_indexed_positions(
                                        &account_key,
                                        &account_data,
                                        &indexed_positions,
                                        &indexed_custodies,
                                        &program,
                                    )
                                    .await?;
                                }

                                continue;
                            }
                            _ => {}
                        }
                        log::debug!("new message: {msg:?}")
                    }
                    Err(error) => {
                        log::error!("error: {error:?}");
                        break;
                    }
                }
            }

            log::info!("  <> stream closed");

            Ok::<(), backoff::Error<anyhow::Error>>(())
        }
        .inspect_err(|error| log::error!("failed to connect: {error}"))
    })
    .await
    .map_err(Into::into)
}

// Check liquidation/sl/tp conditions for related positions
// Based on the provided price_update_v2 account, check if any of the related positions have crossed their liquidation/sl/tp conditions
// First go over the price update v2 account to get the price, then go over the indexed positions to check if any of them have crossed their conditions
pub async fn check_liquidation_sl_tp_conditions(
    price_update_v2_key: &Pubkey,
    price_update_v2_data: &[u8],
    indexed_positions: &Arc<RwLock<HashMap<Pubkey, adrena_abi::types::Position>>>,
    indexed_custodies: &Arc<RwLock<HashMap<Pubkey, adrena_abi::types::Custody>>>,
    price_update_v2_pdas: &HashMap<Pubkey, Pubkey>,
    program: &anchor_client::Program<Rc<Keypair>>,
    cortex: &Cortex,
) -> Result<(), backoff::Error<anyhow::Error>> {
    log::debug!("    <> Checking liquidation/sl/tp conditions");

    // Deserialize price update v2 account
    let price_update_v2: pyth::price_update_v2::PriceUpdateV2 =
        borsh::BorshDeserialize::deserialize(&mut &price_update_v2_data[8..])
            .map_err(|e| backoff::Error::transient(e.into()))?;
    log::debug!("    <> Deserialized price_update_v2");

    // TODO: Optimize this by not creating the OraclePrice struct from the price update v2 account but just using the price and conf directly
    // Create an OraclePrice struct from the price update v2 account
    let oracle_price: utils::oracle_price::OraclePrice =
        utils::oracle_price::OraclePrice::new_from_pyth_price_update_v2(&price_update_v2)
            .map_err(|e| backoff::Error::transient(e.into()))?;
    log::debug!("    <> Created OraclePrice struct from price_update_v2");

    // check SL/TP/LIQ conditions for all indexed positions
    for (position_key, position) in indexed_positions.read().await.iter() {
        let position_custody = position.custody;

        // Check if the price update v2 account is associated with the position's custody
        if price_update_v2_pdas.get(&position_custody) == Some(&price_update_v2_key) {
            // check if the position has a SL set
            if position.stop_loss_thread_is_set != 0 {
                match position.side {
                    1 => {
                        // Long side is 1
                        // check if the price has crossed the SL
                        if oracle_price.price <= position.stop_loss_limit_price {
                            log::info!("SL condition met for LONG position {:#?}", position_key);
                            let indexed_custodies_read = indexed_custodies.read().await;
                            let custody = indexed_custodies_read.get(&position.custody).unwrap();
                            let collateral_custody = indexed_custodies_read
                                .get(&position.collateral_custody)
                                .unwrap();
                            let staking_reward_token_custody =
                                indexed_custodies_read.get(&USDC_CUSTODY_ID).unwrap();

                            // let mint = custody.mint;
                            let collateral_mint = collateral_custody.mint;

                            let receiving_account = Pubkey::find_program_address(
                                &[
                                    &position.owner.to_bytes(),
                                    &SPL_TOKEN_PROGRAM_ID.to_bytes(),
                                    &collateral_mint.to_bytes(),
                                ],
                                &SPL_ASSOCIATED_TOKEN_PROGRAM_ID,
                            )
                            .0;

                            let user_profile_pda =
                                adrena_abi::pda::get_user_profile_pda(&position.owner).0;

                            let transfer_authority_pda =
                                adrena_abi::pda::get_transfer_authority_pda().0;

                            let position_take_profit_pda = adrena_abi::pda::get_sablier_thread_pda(
                                &transfer_authority_pda,
                                position.take_profit_thread_id.to_le_bytes().to_vec(),
                                Some(position.owner.to_bytes().to_vec()),
                            )
                            .0;

                            let position_stop_loss_pda = adrena_abi::pda::get_sablier_thread_pda(
                                &transfer_authority_pda,
                                position.stop_loss_thread_id.to_le_bytes().to_vec(),
                                Some(position.owner.to_bytes().to_vec()),
                            )
                            .0;

                            program
                                .request()
                                .args(adrena_abi::instruction::ClosePositionLong {
                                    params: adrena_abi::types::ClosePositionLongParams {
                                        price: Some(position.stop_loss_close_position_price), // The limit price (slippage)
                                    },
                                })
                                .accounts(adrena_abi::accounts::ClosePositionLong {
                                    caller: program.payer(),
                                    owner: position.owner,
                                    receiving_account,
                                    transfer_authority: transfer_authority_pda,
                                    lm_staking: adrena_abi::pda::get_staking_pda(&ADX_MINT).0,
                                    lp_staking: adrena_abi::pda::get_staking_pda(&USDC_MINT).0,
                                    cortex: adrena_abi::ID,
                                    pool: adrena_abi::ID,
                                    position: *position_key,
                                    staking_reward_token_custody: USDC_CUSTODY_ID,
                                    staking_reward_token_custody_oracle:
                                        staking_reward_token_custody.oracle,
                                    staking_reward_token_custody_token_account:
                                        staking_reward_token_custody.token_account,
                                    custody: position_custody,
                                    custody_oracle: custody.oracle,
                                    custody_trade_oracle: custody.trade_oracle,
                                    custody_token_account: custody.token_account,
                                    lm_staking_reward_token_vault:
                                        adrena_abi::pda::get_staking_reward_token_vault_pda(
                                            &ADX_MINT,
                                        )
                                        .0,
                                    lp_staking_reward_token_vault:
                                        adrena_abi::pda::get_staking_reward_token_vault_pda(
                                            &ALP_MINT,
                                        )
                                        .0,
                                    lp_token_mint: ALP_MINT,
                                    protocol_fee_recipient: cortex.protocol_fee_recipient,
                                    user_profile: Some(user_profile_pda),
                                    take_profit_thread: position_take_profit_pda,
                                    stop_loss_thread: position_stop_loss_pda,
                                    token_program: SPL_TOKEN_PROGRAM_ID,
                                    adrena_program: adrena_abi::ID,
                                    sablier_program: SABLIER_THREAD_PROGRAM_ID,
                                })
                                .send()
                                .await
                                .map_err(|e| backoff::Error::transient(e.into()))?;
                        }
                    }
                    0 => {
                        // Short side is 0 - update abi
                        // check if the price has crossed the SL
                        if oracle_price.price >= position.stop_loss_limit_price {
                            log::info!("SL condition met for SHORT position {:#?}", position_key);
                            // TODO: a RPC call to close the position @ position.stop_loss_close_position_price
                            log::warn!("TODO: a RPC call to close the position @ position.stop_loss_close_position_price");
                        }
                    }
                    _ => {}
                }
            }

            // check if the position has a TP set
            if position.take_profit_thread_is_set != 0 {
                // check if the price has crossed the TP
                match position.side {
                    1 => {
                        if oracle_price.price >= position.take_profit_limit_price {
                            log::info!("TP condition met for LONG position {:#?}", position_key);
                            log::warn!("TODO: a RPC call to close the position @ position.take_profit_close_position_price");
                        }
                    }
                    0 => {
                        if oracle_price.price <= position.take_profit_limit_price {
                            log::info!("TP condition met for SHORT position {:#?}", position_key);
                            log::warn!("TODO: a RPC call to close the position @ position.take_profit_close_position_price");
                        }
                    }
                    _ => {}
                }
            }

            // check LIQ
            // TODO : recalculate the liquidation price and check if the price has crossed it
            // log::warn!("TODO: check LIQ");
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
    indexed_positions: &Arc<RwLock<HashMap<Pubkey, adrena_abi::types::Position>>>,
    indexed_custodies: &Arc<RwLock<HashMap<Pubkey, adrena_abi::types::Custody>>>,
    program: &anchor_client::Program<Rc<Keypair>>,
) -> Result<(), backoff::Error<anyhow::Error>> {
    if position_account_data.is_empty() {
        // Position was closed
        log::info!(
            "update_indexed_positions: {:#?} was CLOSED",
            position_account_key
        );
        indexed_positions
            .write()
            .await
            .remove(&position_account_key);
    } else {
        if let Some(_) = indexed_positions.read().await.get(&position_account_key) {
            // Position was modified
            log::info!(
                "update_indexed_positions: {:#?} was MODIFIED",
                position_account_key
            );
        } else {
            // New position
            log::info!(
                "update_indexed_positions: {:#?} was CREATED",
                position_account_key
            );
        }
        // Either way, update or create, this operation is the same
        let position: adrena_abi::types::Position =
            adrena_abi::types::Position::try_deserialize(&mut &position_account_data[..])
                .map_err(|e| backoff::Error::transient(e.into()))?;
        indexed_positions
            .write()
            .await
            .insert(position_account_key.clone(), position);

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

// Provided an array of existing positions pdas, get the associated price update v2 pdas indexed by custody
async fn get_associated_price_update_v2_keys(
    program: &anchor_client::Program<Rc<Keypair>>,
    indexed_positions: &Arc<RwLock<HashMap<Pubkey, adrena_abi::types::Position>>>,
    indexed_custodies: &Arc<RwLock<HashMap<Pubkey, adrena_abi::types::Custody>>>,
) -> Result<HashMap<Pubkey, Pubkey>, backoff::Error<anyhow::Error>> {
    let mut custody_keys: HashSet<Pubkey> = HashSet::new();

    for (_, p) in indexed_positions.read().await.iter() {
        custody_keys.insert(p.custody);
    }
    log::info!(
        "  <> Existing positions have {} unique custodies",
        custody_keys.len()
    );

    // If there are no custodies, return an empty set
    if custody_keys.is_empty() {
        return Ok(HashMap::new());
    }

    let mut oracle_price_feed_keys: HashMap<Pubkey, Pubkey> = HashMap::new();

    for custody_key in custody_keys {
        let custody = program
            .account::<adrena_abi::types::Custody>(custody_key)
            .await
            .map_err(|e| backoff::Error::transient(e.into()))?;

        // insert into the oracle_price_feeds map
        oracle_price_feed_keys.insert(custody_key, custody.trade_oracle);
        // also insert into the indexed_custodies map
        indexed_custodies.write().await.insert(custody_key, custody);
    }
    log::info!(
        "  <> fetched {} oracle price feeds",
        oracle_price_feed_keys.len()
    );

    Ok(oracle_price_feed_keys)
}
