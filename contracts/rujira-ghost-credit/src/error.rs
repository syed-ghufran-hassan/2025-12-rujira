use cosmwasm_std::{Addr, CheckedFromRatioError, Decimal, Instantiate2AddressError, StdError};
use cw_utils::{ParseReplyError, PaymentError};
use rujira_rs::{
    account::AccountError,
    ghost::credit::{CollateralError, DebtError, LiquidationPreferenceOrderError},
    OracleError,
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
    Instantiate2Address(#[from] Instantiate2AddressError),

    #[error("{0}")]
    ParseReply(#[from] ParseReplyError),

    #[error("{0}")]
    Account(#[from] AccountError),

    #[error("{0}")]
    Collateral(#[from] CollateralError),

    #[error("{0}")]
    Debt(#[from] DebtError),

    #[error("{0}")]
    Oracle(#[from] OracleError),

    #[error("{0}")]
    LiquidationPreferenceOrder(#[from] LiquidationPreferenceOrderError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("InsufficientFunds")]
    InsufficientFunds {},

    #[error("ZeroDebtTokens {denom}")]
    ZeroDebtTokens { denom: String },

    #[error("Invalid Config: {key}:{value}")]
    InvalidConfig { key: String, value: String },

    #[error("{denom} not registered as valid collateral")]
    InvalidCollateral { denom: String },

    #[error("Reply Error: {0}")]
    ReplyError(String),

    #[error("Account Safe")]
    Safe {},

    #[error("Account Unsafe: ltv {ltv}")]
    Unsafe { ltv: Decimal },

    #[error("Account Not Found: {addr}")]
    NotFound { addr: Addr },

    #[error("LTV increased from {from} to {to}")]
    LtvIncreased { from: Decimal, to: Decimal },

    #[error("Over liquidation max {max} actual {actual}")]
    OverLiquidation { max: Decimal, actual: Decimal },

    #[error("Max Slip exceeded during liquidation: #{slip}")]
    LiquidationMaxSlipExceeded { slip: Decimal },
    // Add any other custom errors you like here.
    // Look at https://docs.rs/thiserror/1.0.21/thiserror/ for details.
}
