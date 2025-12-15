use cosmwasm_std::{Addr, Coin, Event};

pub fn event_deposit(owner: Addr, minted: Coin) -> Event {
    Event::new(format!("{}/deposit", env!("CARGO_PKG_NAME")))
        .add_attribute("owner", owner.clone())
        .add_attribute("minted", minted.to_string())
}

pub fn event_withdraw(owner: Addr, share: Coin) -> Event {
    Event::new(format!("{}/withdraw", env!("CARGO_PKG_NAME")))
        .add_attribute("owner", owner.clone())
        .add_attribute("share", share.to_string())
}

pub fn event_swap(offer: Coin, ask: Coin, fee: Coin, surplus: Coin) -> Event {
    Event::new(format!("{}/swap", env!("CARGO_PKG_NAME")))
        .add_attribute("offer", offer.to_string())
        .add_attribute("ask", ask.to_string())
        .add_attribute("fee", fee.to_string())
        .add_attribute("surplus", surplus.to_string())
}
