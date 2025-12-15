use cosmwasm_std::{Addr, Binary, Coin, Event, Uint128};
use cw_utils::NativeBalance;

use crate::account::CreditAccount;

pub fn event_create_account(account: &CreditAccount) -> Event {
    Event::new(format!("{}/account.create", env!("CARGO_PKG_NAME")))
        .add_attribute("owner", account.owner.clone())
        .add_attribute("address", account.id().to_string())
}

pub fn event_execute_account(account: &CreditAccount) -> Event {
    Event::new(format!("{}/account.msg", env!("CARGO_PKG_NAME")))
        .add_attribute("owner", account.owner.clone())
        .add_attribute("address", account.id().to_string())
}

pub fn event_execute_account_borrow(amount: &Coin) -> Event {
    Event::new(format!("{}/account.msg/borrow", env!("CARGO_PKG_NAME")))
        .add_attribute("amount", amount.to_string())
}

pub fn event_execute_account_repay(amount: &Coin) -> Event {
    Event::new(format!("{}/account.msg/repay", env!("CARGO_PKG_NAME")))
        .add_attribute("amount", amount.to_string())
}

pub fn event_execute_account_execute(
    contract_addr: &String,
    msg: &Binary,
    funds: &NativeBalance,
) -> Event {
    Event::new(format!("{}/account.msg/execute", env!("CARGO_PKG_NAME")))
        .add_attribute("contract_addr", contract_addr.to_string())
        .add_attribute("msg", msg.to_string())
        .add_attribute("funds", funds.to_string())
}

pub fn event_execute_account_send(to_address: &String, funds: &NativeBalance) -> Event {
    Event::new(format!("{}/account.msg/send", env!("CARGO_PKG_NAME")))
        .add_attribute("to_address", to_address.to_string())
        .add_attribute("funds", funds.to_string())
}

pub fn event_execute_account_transfer(recipient: &Addr) -> Event {
    Event::new(format!("{}/account.msg/transfer", env!("CARGO_PKG_NAME")))
        .add_attribute("to_adrecipientdress", recipient.to_string())
}

pub fn event_execute_account_set_preference_order(denom: &String, after: &Option<String>) -> Event {
    Event::new(format!(
        "{}/account.msg/set_preference_order",
        env!("CARGO_PKG_NAME")
    ))
    .add_attribute("denom", denom.to_string())
    .add_attribute("after", after.clone().unwrap_or_default().to_string())
}

pub fn event_execute_account_set_preference_msgs() -> Event {
    Event::new(format!(
        "{}/account.msg/set_preference_msgs",
        env!("CARGO_PKG_NAME")
    ))
}

pub fn event_execute_liquidate(account: &CreditAccount, caller: &Addr) -> Event {
    Event::new(format!("{}/account.liquidate", env!("CARGO_PKG_NAME")))
        .add_attribute("owner", account.owner.clone())
        .add_attribute("address", account.id().to_string())
        .add_attribute("caller", caller.to_string())
}

pub fn event_execute_liquidate_preference_error(msg: String) -> Event {
    Event::new(format!(
        "{}/liquidate.msg/preference.error",
        env!("CARGO_PKG_NAME")
    ))
    .add_attribute("error", msg.to_string())
}

pub fn event_execute_liquidate_repay(
    amount: &Coin,
    repay_amount: Uint128,
    fee_liquidation: Uint128,
    fee_liquidator: Uint128,
) -> Event {
    Event::new(format!("{}/liquidate.msg/repay", env!("CARGO_PKG_NAME")))
        .add_attribute("amount", amount.to_string())
        .add_attribute("repay_amount", repay_amount.to_string())
        .add_attribute("fee_liquidation", fee_liquidation.to_string())
        .add_attribute("fee_liquidator", fee_liquidator.to_string())
}

pub fn event_execute_liquidate_execute(
    contract_addr: &String,
    msg: &Binary,
    funds: &NativeBalance,
) -> Event {
    Event::new(format!("{}/liquidate.msg/execute", env!("CARGO_PKG_NAME")))
        .add_attribute("contract_addr", contract_addr.to_string())
        .add_attribute("msg", msg.to_string())
        .add_attribute("funds", funds.to_string())
}
