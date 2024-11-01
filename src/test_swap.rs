use crate::{cli::CliConfig, error::Result, AlloyCacheDB};

pub trait TestSwap {
    async fn test_swap(&self, config: &CliConfig, db: &mut AlloyCacheDB) -> Result<()>;
}
