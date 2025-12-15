use cosmwasm_std::StdError;
use rujira_rs::query::PoolError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    Pool(#[from] PoolError),
}
