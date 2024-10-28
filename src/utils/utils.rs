use solana_sdk::pubkey::Pubkey;

pub fn get_sablier_thread_pda(
    transfer_authority_pda: &Pubkey,
    thread_id: u64,
    owner: &Pubkey,
) -> Pubkey {
    adrena_abi::pda::get_sablier_thread_pda(
        transfer_authority_pda,
        thread_id.to_le_bytes().to_vec(),
        Some(owner.to_bytes().to_vec()),
    )
    .0
}
