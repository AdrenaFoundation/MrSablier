pub trait Action {
    fn is_triggered() -> bool;

    fn get_args_and_instruction() -> (impl InstructionData, impl ToAccountMetas);

    fn execute(
        &self,
        args: impl InstructionData,
        accounts: impl ToAccountMetas,
    ) -> Result<(), backoff::Error<anyhow::Error>> {
        self.program
            .request()
            .args(args)
            .accounts(accounts)
            .send()
            .await
            .map_err(|e| backoff::Error::transient(e.into()))?;
    }
}
