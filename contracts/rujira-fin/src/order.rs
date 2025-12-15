use std::cmp::min;

use crate::{error::ContractError, pool::Pool, pool_key::PoolKey};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, StdResult, Storage, Timestamp, Uint128};
use cw_storage_plus::Map;
use rujira_rs::{
    bid_pool,
    fin::{Price, Side},
};

pub const ORDERS: Map<(Addr, Side, Price), (Timestamp, Uint128, bid_pool::Bid)> =
    Map::new("orders");
const MAX_LIMIT: u8 = 31;
const DEFAULT_LIMIT: u8 = 10;

#[cw_serde]
pub struct Order {
    pub owner: Addr,
    pub updated_at: Timestamp,
    /// Original offer amount, as it was at `updated_at` time
    pub offer: Uint128,
    pub bid: bid_pool::Bid,
}

impl Order {
    pub fn load(
        storage: &dyn Storage,
        owner: &Addr,
        side: &Side,
        price: &Price,
    ) -> Result<Self, ContractError> {
        let (updated_at, offer, bid) = ORDERS
            .load(storage, (owner.clone(), side.clone(), price.clone()))
            .map_err(|_| ContractError::NotFound {})?;
        Ok(Self {
            owner: owner.clone(),
            updated_at,
            offer,
            bid,
        })
    }

    pub fn by_owner(
        storage: &dyn Storage,
        owner: &Addr,
        side: Option<Side>,
        offset: Option<u8>,
        limit: Option<u8>,
    ) -> StdResult<Vec<(PoolKey, Self)>> {
        let limit = min(limit.unwrap_or(DEFAULT_LIMIT), MAX_LIMIT) as usize;
        let offset = offset.unwrap_or(0) as usize;
        match side {
            Some(side) => Self::by_owner_side(storage, owner, side, offset, limit),
            None => Self::by_owner_all(storage, owner, offset, limit),
        }
    }

    fn by_owner_all(
        storage: &dyn Storage,
        owner: &Addr,
        offset: usize,
        limit: usize,
    ) -> StdResult<Vec<(PoolKey, Self)>> {
        ORDERS
            .sub_prefix(owner.clone())
            .range(storage, None, None, cosmwasm_std::Order::Ascending)
            .skip(offset)
            .take(limit)
            .map(|x| {
                x.map(|(k, (updated_at, offer, bid))| {
                    (
                        PoolKey::new(k.0, k.1),
                        Self {
                            owner: owner.clone(),
                            updated_at,
                            offer,
                            bid,
                        },
                    )
                })
            })
            .collect()
    }

    fn by_owner_side(
        storage: &dyn Storage,
        owner: &Addr,
        side: Side,
        offset: usize,
        limit: usize,
    ) -> StdResult<Vec<(PoolKey, Self)>> {
        let order = match side {
            Side::Base => cosmwasm_std::Order::Ascending,
            Side::Quote => cosmwasm_std::Order::Descending,
        };

        ORDERS
            .prefix((owner.clone(), side.clone()))
            .range(storage, None, None, order)
            .skip(offset)
            .take(limit)
            .map(|x| {
                x.map(|(k, (updated_at, offer, bid))| {
                    (
                        PoolKey::new(side.clone(), k),
                        Self {
                            owner: owner.clone(),
                            updated_at,
                            offer,
                            bid,
                        },
                    )
                })
            })
            .collect()
    }

    pub fn amount(&self) -> Uint128 {
        self.bid.amount().try_into().unwrap()
    }

    pub fn save(&self, storage: &mut dyn Storage, pool: &Pool) -> StdResult<()> {
        if self.bid.is_empty() {
            self.remove(storage, pool);
            return Ok(());
        }
        ORDERS.save(
            storage,
            (self.owner.clone(), pool.side.clone(), pool.price.clone()),
            &(self.updated_at, self.offer, self.bid.clone()),
        )?;
        Ok(())
    }

    fn remove(&self, storage: &mut dyn Storage, pool: &Pool) {
        ORDERS.remove(
            storage,
            (self.owner.clone(), pool.side.clone(), pool.price.clone()),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use cosmwasm_std::{testing::MockStorage, Addr, Decimal, Timestamp, Uint128};
    use rujira_rs::{
        exchange::Swappable,
        fin::{Price, Side},
    };

    use crate::pool::Pool;

    #[test]
    fn query_order() {
        let mut store = MockStorage::new();
        let timestamp = Timestamp::default();
        let owner = Addr::unchecked("owner");
        let offer = Uint128::from(100u128);
        let oracle = Decimal::one();
        let mut pool = Pool::load(&store, &Price::Oracle(0), &Side::Quote, &oracle);
        pool.create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();

        pool.commit(&mut store).unwrap();

        let order = Order::load(&store, &owner, &Side::Quote, &Price::Oracle(0)).unwrap();
        assert_eq!(order.owner, owner);
        assert_eq!(order.offer, offer);
    }

    #[test]
    fn query_orders_by_owner() {
        let mut store = MockStorage::new();
        let timestamp = Timestamp::default();
        let owner = Addr::unchecked("owner");
        let owner2 = Addr::unchecked("owner2");
        let offer = Uint128::from(100u128);
        let oracle = Decimal::one();
        let mut pool1 = Pool::load(&store, &Price::Oracle(0), &Side::Quote, &oracle);
        let mut pool2 = Pool::load(&store, &Price::Oracle(1), &Side::Quote, &oracle);
        let mut pool3 = Pool::load(&store, &Price::Oracle(2), &Side::Quote, &oracle);
        let mut pool4 = Pool::load(
            &store,
            &Price::Fixed(Decimal::from_str("1.0").unwrap()),
            &Side::Quote,
            &oracle,
        );
        let mut pool5 = Pool::load(
            &store,
            &Price::Fixed(Decimal::from_str("1.1").unwrap()),
            &Side::Quote,
            &oracle,
        );
        let mut pool6 = Pool::load(
            &store,
            &Price::Fixed(Decimal::from_str("1.2").unwrap()),
            &Side::Quote,
            &oracle,
        );
        let mut pool7 = Pool::load(
            &store,
            &Price::Fixed(Decimal::from_str("1.2").unwrap()),
            &Side::Base,
            &oracle,
        );

        pool1
            .create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();
        pool2
            .create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();
        pool3
            .create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();
        pool4
            .create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();
        pool5
            .create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();
        pool6
            .create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();
        pool7
            .create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();

        pool1
            .create_order(&mut store, &timestamp, &owner2, offer)
            .unwrap();

        pool1.commit(&mut store).unwrap();

        let orders = Order::by_owner(&store, &owner, None, None, None).unwrap();
        assert_eq!(orders.len(), 7);
        assert_eq!(orders[0].1.owner, owner);
        assert_eq!(orders[0].1.offer, offer);

        let orders = Order::by_owner(&store, &owner, Some(Side::Quote), None, None).unwrap();
        assert_eq!(orders.len(), 6);
        assert_eq!(orders[0].1.owner, owner);
        assert_eq!(orders[0].1.offer, offer);
    }
}
