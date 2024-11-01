use crate::{cli::CliConfig, error::Result, AlloyCacheDB};

pub trait TestSwap {
    async fn test_swap(config: &CliConfig, db: &mut AlloyCacheDB) -> Result<()>;
}
