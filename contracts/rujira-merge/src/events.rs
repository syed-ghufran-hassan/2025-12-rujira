use cosmwasm_std::{Addr, Event, Uint128};

pub fn event_deposit(account: Addr, amount: Uint128, shares: Uint128) -> Event {
    Event::new(format!("{}/deposit", env!("CARGO_PKG_NAME")))
        .add_attribute("account", account)
        .add_attribute("amount", amount)
        .add_attribute("shares", shares)
}

pub fn event_withdraw(account: Addr, share_amount: Uint128, amount: Uint128) -> Event {
    Event::new(format!("{}/withdraw", env!("CARGO_PKG_NAME")))
        .add_attribute("account", account)
        .add_attribute("shares", share_amount)
        .add_attribute("amount", amount)
}
