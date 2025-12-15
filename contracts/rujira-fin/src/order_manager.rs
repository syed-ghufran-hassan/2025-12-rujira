use cosmwasm_schema::cw_serde;
use cosmwasm_std::{coin, Addr, CosmosMsg, Decimal, Event, Fraction, Storage, Timestamp, Uint128};
use cw_utils::NativeBalance;
use rujira_rs::exchange::Swappable;
use rujira_rs::exchange::Swapper;
use rujira_rs::fin::{Price, Side, SwapRequest};
use rujira_rs::Premiumable;
use std::cmp::Ordering;
use std::ops::{Mul, Sub};

use crate::config::Config;
use crate::swap_iter::SwapIter;
use crate::{
    events::{event_create_order, event_increase_order, event_retract_order, event_withdraw_order},
    order::Order,
    pool::Pool,
    ContractError,
};

#[cw_serde]
pub struct OrderManager {
    config: Config,
    owner: Addr,
    timestamp: Timestamp,
    // NativeBalance can't be negative. Store in and out separately and we'll validate
    // no negative balances at the end
    // What we receive from the user and withdrawn and retracted orders
    receive: NativeBalance,
    // What we spend creating and increasing orders
    send: NativeBalance,
    fees: NativeBalance,
    events: Vec<Event>,
    messages: Vec<CosmosMsg>,
}

impl OrderManager {
    pub fn new(config: &Config, owner: Addr, timestamp: Timestamp, funds: NativeBalance) -> Self {
        Self {
            config: config.clone(),
            owner,
            timestamp,
            receive: funds,
            send: NativeBalance::default(),
            fees: NativeBalance::default(),
            events: vec![],
            messages: vec![],
        }
    }

    pub fn execute_orders(
        &mut self,
        storage: &mut dyn Storage,
        swap_iter: &SwapIter,
        o: Vec<(Side, Price, Option<Uint128>)>,
        oracle: &impl Premiumable,
    ) -> Result<ExecutionResult, ContractError> {
        for (side, price, target) in o {
            if let Price::Fixed(x) = price {
                self.config.tick.validate_price(&x)?;
            }
            let mut pool = Pool::load(storage, &price, &side, oracle);
            match pool.load_order(storage, &self.owner) {
                Ok(mut order) => {
                    self.execute_existing_order(storage, &mut pool, &mut order, &side, target)?
                }
                Err(ContractError::NotFound {}) => {
                    self.execute_new_order(storage, swap_iter, &mut pool, &side, target, oracle)?
                }
                Err(err) => return Err(err),
            }
        }
        self.send.normalize();
        self.receive.normalize();
        for x in self.send.clone().into_vec() {
            self.receive =
                (self.receive.clone() - x).map_err(|_| ContractError::InsufficientFunds {
                    required: self.send.clone(),
                    available: self.receive.clone(),
                })?;
        }

        Ok(self.into())
    }

    fn execute_existing_order(
        &mut self,
        storage: &mut dyn Storage,
        pool: &mut Pool,
        order: &mut Order,
        side: &Side,
        target: Option<Uint128>,
    ) -> Result<(), ContractError> {
        self.maybe_withdraw(storage, pool, order)?;
        if let Some(target) = target {
            let amount = Uint128::try_from(order.bid.amount()).unwrap();
            match amount.cmp(&target) {
                Ordering::Less => {
                    let diff = target - amount;

                    let amount = pool.increase_order(storage, order, &self.timestamp, diff)?;
                    let coins = coin(amount.u128(), self.config.denoms.bid(side));
                    self.send += coins;
                    self.events.push(event_increase_order(pool, order, &diff));
                }
                Ordering::Greater => {
                    let diff = amount - target;
                    let amount = pool.retract_order(storage, order, &self.timestamp, Some(diff))?;
                    let coins = coin(amount.u128(), self.config.denoms.bid(side));
                    self.receive += coins;
                    self.events.push(event_retract_order(pool, order, &diff));
                }
                Ordering::Equal => {}
            }
        }

        Ok(())
    }

    fn execute_new_order(
        &mut self,
        storage: &mut dyn Storage,
        swap_iter: &SwapIter,
        pool: &mut Pool,
        side: &Side,
        target: Option<Uint128>,
        oracle: &impl Premiumable,
    ) -> Result<(), ContractError> {
        if let Some(target) = target {
            let opposite = side.other();
            let mut swapper = Swapper::new(
                env!("CARGO_PKG_NAME"),
                target,
                SwapRequest::Limit {
                    price: match opposite {
                        Side::Base => pool.rate(),
                        Side::Quote => pool.rate().inv().unwrap(),
                    },
                    to: None,
                    callback: None,
                },
                self.config.fee_taker,
            );
            let mut swap = {
                let mut iter = swap_iter.iter(storage, &opposite, oracle);
                swapper.swap(&mut iter)?
            };
            let order =
                pool.create_order(storage, &self.timestamp, &self.owner, swap.remaining_offer)?;
            if !swap.return_amount.is_zero() {
                let commit = swapper.commit(storage)?;
                self.events.append(&mut swap.events);
                self.messages
                    .append(&mut commit.to_msgs(&self.config.denoms, &opposite)?);
                // Allocate the swap return to funds sent from user
                self.receive += coin(swap.return_amount.u128(), self.config.denoms.ask(side));
                self.receive = (self.receive.clone()
                    - coin(swap.consumed_offer.u128(), self.config.denoms.bid(side)))?;
            }
            // Allocate order size as received amount
            self.send += coin(order.amount().u128(), self.config.denoms.bid(side));
            self.events.push(event_create_order(pool, &order));
        }
        Ok(())
    }

    fn maybe_withdraw(
        &mut self,
        storage: &mut dyn Storage,
        pool: &mut Pool,
        order: &mut Order,
    ) -> Result<(), ContractError> {
        if order.bid.filled().is_zero() {
            return Ok(());
        }
        let amount = pool.claim_order(storage, order)?;
        let fees = Decimal::from_ratio(amount, 1u128)
            .mul(self.config.fee_maker)
            .to_uint_ceil();

        let receive = coin(amount.sub(fees).u128(), self.config.denoms.ask(&pool.side));
        let fees = coin(fees.u128(), self.config.denoms.ask(&pool.side));

        self.receive += receive;
        self.fees += fees;
        self.events.push(event_withdraw_order(pool, order, &amount));
        Ok(())
    }
}

impl From<&mut OrderManager> for ExecutionResult {
    fn from(e: &mut OrderManager) -> Self {
        e.fees.normalize();
        e.receive.normalize();
        Self {
            withdraw: e.receive.clone(),
            fees: e.fees.clone(),
            events: e.events.clone(),
            messages: e.messages.clone(),
        }
    }
}

#[derive(Debug)]
pub struct ExecutionResult {
    pub withdraw: NativeBalance,
    pub fees: NativeBalance,
    pub events: Vec<Event>,
    pub messages: Vec<CosmosMsg>,
}

#[cfg(test)]

mod tests {

    use crate::market_makers::MarketMakers;

    use super::*;
    use cosmwasm_std::{
        coins,
        testing::{message_info, mock_dependencies, mock_env},
    };
    use rujira_rs::fin::{Denoms, Price, Tick};
    use std::str::FromStr;

    #[test]
    fn test_simple_success() {
        let mut deps = mock_dependencies();
        let mut_deps = deps.as_mut();
        let env = mock_env();
        let info = message_info(&Addr::unchecked("addr0000"), &[]);
        let oracle = Decimal::from_str("1.0").unwrap();
        let mut funds = NativeBalance::default();
        funds += coin(1000, "usdc");
        let config = Config {
            denoms: Denoms::new("ruji", "usdc"),
            oracles: None,
            market_makers: MarketMakers::new(mut_deps.api, vec![]).unwrap(),
            tick: Tick::new(4),
            fee_maker: Decimal::from_str("0.001").unwrap(),
            fee_taker: Decimal::from_str("0.002").unwrap(),
            fee_address: Addr::unchecked(""),
        };
        let swap_iter = SwapIter::new(mut_deps.querier, &config);
        let mut e = OrderManager::new(&config, info.sender, env.block.time, funds);

        let res = e
            .execute_orders(
                mut_deps.storage,
                &swap_iter,
                vec![(
                    Side::Quote,
                    Price::Fixed(Decimal::from_str("1.0").unwrap()),
                    Some(Uint128::from(1000u128)),
                )],
                &oracle,
            )
            .unwrap();

        assert_eq!(res.withdraw, NativeBalance::default());
        let event = res.events[0].clone();
        assert_eq!(event.ty, "rujira-fin/order.create");
        assert_eq!(event.attributes[0].key, "owner");
        assert_eq!(event.attributes[0].value, "addr0000");
        assert_eq!(event.attributes[1].key, "side");
        assert_eq!(event.attributes[1].value, "quote");
        assert_eq!(event.attributes[2].key, "price");
        assert_eq!(event.attributes[2].value, "fixed:1");
        assert_eq!(event.attributes[3].key, "offer");
        assert_eq!(event.attributes[3].value, "1000");
    }

    #[test]
    fn test_multiple_orders() {
        let mut deps = mock_dependencies();
        let mut_deps = deps.as_mut();

        let env = mock_env();
        let info = message_info(&Addr::unchecked("addr0000"), &[]);
        let oracle = Decimal::from_str("1.0").unwrap();
        let mut funds = NativeBalance::default();
        funds += coin(10000, "usdc");
        funds += coin(10000, "ruji");
        let config = Config {
            denoms: Denoms::new("ruji", "usdc"),
            oracles: None,
            market_makers: MarketMakers::new(mut_deps.api, vec![]).unwrap(),
            tick: Tick::new(4),
            fee_maker: Decimal::from_str("0.001").unwrap(),
            fee_taker: Decimal::from_str("0.002").unwrap(),
            fee_address: Addr::unchecked(""),
        };
        let swap_iter = SwapIter::new(mut_deps.querier, &config);

        let mut e = OrderManager::new(&config, info.sender, env.block.time, funds);

        let res = e
            .execute_orders(
                mut_deps.storage,
                &swap_iter,
                vec![
                    (
                        Side::Quote,
                        Price::Fixed(Decimal::from_str("1.0").unwrap()),
                        Some(Uint128::from(1000u128)),
                    ),
                    (Side::Quote, Price::Oracle(0), Some(Uint128::from(2000u128))),
                    (
                        Side::Base,
                        Price::Oracle(100),
                        Some(Uint128::from(1200u128)),
                    ),
                    (
                        Side::Base,
                        Price::Fixed(Decimal::from_str("1.3").unwrap()),
                        Some(Uint128::from(1300u128)),
                    ),
                ],
                &oracle,
            )
            .unwrap();
        let returned = NativeBalance(vec![coin(7500, "ruji"), coin(7000, "usdc")]);
        assert_eq!(res.withdraw, returned);
        let event = res.events[0].clone();
        assert_eq!(event.ty, "rujira-fin/order.create");
        assert_eq!(event.attributes[0].key, "owner");
        assert_eq!(event.attributes[0].value, "addr0000");
        assert_eq!(event.attributes[1].key, "side");
        assert_eq!(event.attributes[1].value, "quote");
        assert_eq!(event.attributes[2].key, "price");
        assert_eq!(event.attributes[2].value, "fixed:1");
        assert_eq!(event.attributes[3].key, "offer");
        assert_eq!(event.attributes[3].value, "1000");

        let event = res.events[1].clone();
        assert_eq!(event.ty, "rujira-fin/order.create");
        assert_eq!(event.attributes[0].key, "owner");
        assert_eq!(event.attributes[0].value, "addr0000");
        assert_eq!(event.attributes[1].key, "side");
        assert_eq!(event.attributes[1].value, "quote");
        assert_eq!(event.attributes[2].key, "price");
        assert_eq!(event.attributes[2].value, "oracle:0");
        assert_eq!(event.attributes[3].key, "offer");
        assert_eq!(event.attributes[3].value, "2000");

        let event = res.events[2].clone();
        assert_eq!(event.ty, "rujira-fin/order.create");
        assert_eq!(event.attributes[0].key, "owner");
        assert_eq!(event.attributes[0].value, "addr0000");
        assert_eq!(event.attributes[1].key, "side");
        assert_eq!(event.attributes[1].value, "base");
        assert_eq!(event.attributes[2].key, "price");
        assert_eq!(event.attributes[2].value, "oracle:100");
        assert_eq!(event.attributes[3].key, "offer");
        assert_eq!(event.attributes[3].value, "1200");

        let event = res.events[3].clone();
        assert_eq!(event.ty, "rujira-fin/order.create");
        assert_eq!(event.attributes[0].key, "owner");
        assert_eq!(event.attributes[0].value, "addr0000");
        assert_eq!(event.attributes[1].key, "side");
        assert_eq!(event.attributes[1].value, "base");
        assert_eq!(event.attributes[2].key, "price");
        assert_eq!(event.attributes[2].value, "fixed:1.3");
        assert_eq!(event.attributes[3].key, "offer");
        assert_eq!(event.attributes[3].value, "1300");
    }

    #[test]
    fn test_out_of_funds() {
        let mut deps = mock_dependencies();
        let mut_deps = deps.as_mut();

        let env = mock_env();
        let info = message_info(&Addr::unchecked("addr0000"), &[]);

        let oracle = Decimal::from_str("1.0").unwrap();
        let funds = NativeBalance::default();
        let config = Config {
            denoms: Denoms::new("ruji", "usdc"),
            oracles: None,
            market_makers: MarketMakers::new(mut_deps.api, vec![]).unwrap(),
            tick: Tick::new(4),
            fee_maker: Decimal::from_str("0.001").unwrap(),
            fee_taker: Decimal::from_str("0.002").unwrap(),
            fee_address: Addr::unchecked(""),
        };
        let swap_iter = SwapIter::new(mut_deps.querier, &config);

        let mut e = OrderManager::new(&config, info.sender, env.block.time, funds);

        e.execute_orders(
            mut_deps.storage,
            &swap_iter,
            vec![(
                Side::Quote,
                Price::Fixed(Decimal::from_str("1.0").unwrap()),
                Some(Uint128::from(1000u128)),
            )],
            &oracle,
        )
        .unwrap_err();
    }

    #[test]
    fn test_moving_orders() {
        let mut deps = mock_dependencies();
        let mut_deps = deps.as_mut();

        let env = mock_env();
        let info = message_info(&Addr::unchecked("addr0000"), &[]);

        let oracle = Decimal::from_str("1.0").unwrap();
        let mut funds = NativeBalance::default();
        funds += coin(10000, "usdc");
        funds += coin(10000, "ruji");
        let config = Config {
            denoms: Denoms::new("ruji", "usdc"),
            oracles: None,
            market_makers: MarketMakers::new(mut_deps.api, vec![]).unwrap(),
            tick: Tick::new(4),
            fee_maker: Decimal::from_str("0.001").unwrap(),
            fee_taker: Decimal::from_str("0.002").unwrap(),
            fee_address: Addr::unchecked(""),
        };
        let swap_iter = SwapIter::new(mut_deps.querier, &config);

        let mut e = OrderManager::new(&config, info.sender.clone(), env.block.time, funds);

        // Same as above
        e.execute_orders(
            mut_deps.storage,
            &swap_iter,
            vec![
                (
                    Side::Base,
                    Price::Fixed(Decimal::from_str("1.3").unwrap()),
                    Some(Uint128::from(1300u128)),
                ),
                (
                    Side::Base,
                    Price::Oracle(100),
                    Some(Uint128::from(1200u128)),
                ),
                (Side::Quote, Price::Oracle(0), Some(Uint128::from(2000u128))),
                (
                    Side::Quote,
                    Price::Fixed(Decimal::from_str("1.0").unwrap()),
                    Some(Uint128::from(1000u128)),
                ),
            ],
            &oracle,
        )
        .unwrap();

        let mut e = OrderManager::new(
            &config,
            info.sender.clone(),
            env.block.time,
            NativeBalance::default(),
        );

        let res = e
            .execute_orders(
                mut_deps.storage,
                &swap_iter,
                vec![
                    // Split 1200 ito 2 x 600
                    (
                        Side::Base,
                        Price::Oracle(1000),
                        Some(Uint128::from(600u128)),
                    ),
                    (Side::Base, Price::Oracle(100), Some(Uint128::from(600u128))),
                    (
                        Side::Quote,
                        Price::Fixed(Decimal::from_str("1.0").unwrap()),
                        Some(Uint128::zero()),
                    ),
                    (
                        Side::Quote,
                        Price::Fixed(Decimal::from_str("0.99").unwrap()),
                        Some(Uint128::from(1000u128)),
                    ),
                ],
                &oracle,
            )
            .unwrap();

        let returned = NativeBalance::default();
        assert_eq!(res.withdraw, returned);
        assert_eq!(res.events.len(), 4);

        let event = res.events[0].clone();
        assert_eq!(event.ty, "rujira-fin/order.create");
        assert_eq!(event.attributes[0].key, "owner");
        assert_eq!(event.attributes[0].value, "addr0000");
        assert_eq!(event.attributes[1].key, "side");
        assert_eq!(event.attributes[1].value, "base");
        assert_eq!(event.attributes[2].key, "price");
        assert_eq!(event.attributes[2].value, "oracle:1000");
        assert_eq!(event.attributes[3].key, "offer");
        assert_eq!(event.attributes[3].value, "600");

        let event = res.events[1].clone();
        assert_eq!(event.ty, "rujira-fin/order.retract");
        assert_eq!(event.attributes[0].key, "owner");
        assert_eq!(event.attributes[0].value, "addr0000");
        assert_eq!(event.attributes[1].key, "side");
        assert_eq!(event.attributes[1].value, "base");
        assert_eq!(event.attributes[2].key, "price");
        assert_eq!(event.attributes[2].value, "oracle:100");
        assert_eq!(event.attributes[3].key, "amount");
        assert_eq!(event.attributes[3].value, "600");

        let event = res.events[2].clone();
        assert_eq!(event.ty, "rujira-fin/order.retract");
        assert_eq!(event.attributes[0].key, "owner");
        assert_eq!(event.attributes[0].value, "addr0000");
        assert_eq!(event.attributes[1].key, "side");
        assert_eq!(event.attributes[1].value, "quote");
        assert_eq!(event.attributes[2].key, "price");
        assert_eq!(event.attributes[2].value, "fixed:1");
        assert_eq!(event.attributes[3].key, "amount");
        assert_eq!(event.attributes[3].value, "1000");

        let event = res.events[3].clone();
        assert_eq!(event.ty, "rujira-fin/order.create");
        assert_eq!(event.attributes[0].key, "owner");
        assert_eq!(event.attributes[0].value, "addr0000");
        assert_eq!(event.attributes[1].key, "side");
        assert_eq!(event.attributes[1].value, "quote");
        assert_eq!(event.attributes[2].key, "price");
        assert_eq!(event.attributes[2].value, "fixed:0.99");
        assert_eq!(event.attributes[3].key, "offer");
        assert_eq!(event.attributes[3].value, "1000");

        let mut e = OrderManager::new(
            &config,
            info.sender.clone(),
            env.block.time,
            NativeBalance(coins(300, "ruji")),
        );

        // Move 300 over and increase by 300 more with fresh funds
        let res = e
            .execute_orders(
                mut_deps.storage,
                &swap_iter,
                vec![
                    (Side::Base, Price::Oracle(100), Some(Uint128::from(300u128))),
                    (
                        Side::Base,
                        Price::Oracle(1000),
                        Some(Uint128::from(1200u128)),
                    ),
                ],
                &oracle,
            )
            .unwrap();

        let returned = NativeBalance::default();
        assert_eq!(res.withdraw, returned);
        assert_eq!(res.events.len(), 2);

        let event = res.events[0].clone();
        assert_eq!(event.ty, "rujira-fin/order.retract");
        assert_eq!(event.attributes[0].key, "owner");
        assert_eq!(event.attributes[0].value, "addr0000");
        assert_eq!(event.attributes[1].key, "side");
        assert_eq!(event.attributes[1].value, "base");
        assert_eq!(event.attributes[2].key, "price");
        assert_eq!(event.attributes[2].value, "oracle:100");
        assert_eq!(event.attributes[3].key, "amount");
        assert_eq!(event.attributes[3].value, "300");

        let event = res.events[1].clone();
        assert_eq!(event.ty, "rujira-fin/order.increase");
        assert_eq!(event.attributes[0].key, "owner");
        assert_eq!(event.attributes[0].value, "addr0000");
        assert_eq!(event.attributes[1].key, "side");
        assert_eq!(event.attributes[1].value, "base");
        assert_eq!(event.attributes[2].key, "price");
        assert_eq!(event.attributes[2].value, "oracle:1000");
        assert_eq!(event.attributes[3].key, "amount");
        assert_eq!(event.attributes[3].value, "600");
    }

    #[test]
    fn test_tick_validation() {
        let mut deps = mock_dependencies();
        let mut_deps = deps.as_mut();
        let env = mock_env();
        let info = message_info(&Addr::unchecked("addr0000"), &[]);
        let oracle = Decimal::from_str("1.0").unwrap();
        let mut funds = NativeBalance::default();
        funds += coin(1000, "usdc");
        let config = Config {
            denoms: Denoms::new("ruji", "usdc"),
            oracles: None,
            market_makers: MarketMakers::new(mut_deps.api, vec![]).unwrap(),
            tick: Tick::new(2),
            fee_maker: Decimal::from_str("0.001").unwrap(),
            fee_taker: Decimal::from_str("0.002").unwrap(),
            fee_address: Addr::unchecked(""),
        };
        let swap_iter = SwapIter::new(mut_deps.querier, &config);
        let mut e = OrderManager::new(&config, info.sender, env.block.time, funds);

        e.execute_orders(
            mut_deps.storage,
            &swap_iter,
            vec![(
                Side::Quote,
                Price::Fixed(Decimal::from_str("1.001").unwrap()),
                Some(Uint128::from(1000u128)),
            )],
            &oracle,
        )
        .unwrap_err();
    }
}
