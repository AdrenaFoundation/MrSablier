use {
    anchor_client::{Client, Cluster, Program},
    solana_sdk::signature::Keypair,
    std::sync::Arc,
};

/// A thread-safe wrapper around the Program client
#[derive(Clone)]
pub struct ProgramWrapper {
    program: Arc<Program<Arc<Keypair>>>,
}

impl ProgramWrapper {
    pub fn new(endpoint: &str, payer: Arc<Keypair>, program_id: &str) -> Result<Self, anyhow::Error> {
        let program_id = program_id
            .parse()
            .map_err(|e| anyhow::anyhow!("Failed to parse program ID: {}", e))?;

        let program = Client::new(
            Cluster::Custom(endpoint.to_string(), endpoint.to_string()),
            Arc::clone(&payer),
        )
        .program(program_id)
        .map_err(|e| anyhow::anyhow!("Failed to create program client: {}", e))?;

        Ok(Self {
            program: Arc::new(program),
        })
    }

    pub fn get_program(&self) -> &Program<Arc<Keypair>> {
        &self.program
    }
}
