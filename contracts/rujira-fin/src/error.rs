use cosmwasm_std::{CheckedFromRatioError, Coin, ConversionOverflowError, OverflowError, StdError};
use cw_utils::{NativeBalance, PaymentError};
use rujira_rs::{
    bid_pool::BidPoolError, exchange::SwapError, fin::TickError, query::PoolError, OracleError,
};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    Payment(#[from] PaymentError),

    #[error("{0}")]
    CheckedFromRatio(#[from] CheckedFromRatioError),

    #[error("{0}")]
    ConversionOverflow(#[from] ConversionOverflowError),

    #[error("{0}")]
    BidPool(#[from] BidPoolError),

    #[error("{0}")]
    Pool(#[from] PoolError),

    #[error("{0}")]
    Tick(#[from] TickError),

    #[error("{0}")]
    Overflow(#[from] OverflowError),

    #[error("{0}")]
    Swap(#[from] SwapError),

    #[error("{0}")]
    Oracle(#[from] OracleError),

    #[error("Insufficient Return. expected {requested} got {returned}")]
    InsufficientReturn { requested: Coin, returned: Coin },

    #[error("InsufficientFunds required {required} got {available}")]
    InsufficientFunds {
        required: NativeBalance,
        available: NativeBalance,
    },

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("NotFound")]
    NotFound {},

    #[error("Invalid: {0}")]
    Invalid(String),
    // Add any other custom errors you like here.
    // Look at https://docs.rs/thiserror/1.0.21/thiserror/ for details.
}
