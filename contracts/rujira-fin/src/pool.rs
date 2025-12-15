use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    Addr, Attribute, Decimal, Decimal256, Fraction, StdResult, Storage, Timestamp, Uint128, Uint256,
};
use cw_storage_plus::Map;
use itertools::{EitherOrBoth, Itertools};
use rujira_rs::{
    bid_pool::{self, SumSnapshot},
    exchange::{Commitment, SwapError, Swappable},
    fin::{Price, Side},
    DecimalScaled, Premiumable,
};

use crate::{
    order::Order,
    pool_key::{PoolKey, PoolType},
    ContractError,
};
const SNAPSHOTS: Map<(PoolKey, bid_pool::SumSnapshotKey), DecimalScaled> = Map::new("snapshots");
// The POOLS Map is used simply as an indicator that there is a non-zero BidPool at this key
// The BID_POOLS Map is used to store the BidPool itself, and the Key is used to populate the Pool values
const POOLS: Map<PoolKey, ()> = Map::new("pools");
const BID_POOLS: Map<PoolKey, bid_pool::Pool> = Map::new("bid-pools");

/// A wrapper around a BidPool to provide a side & price, used for keying orders and
/// storing pools for iterating during execution
#[cw_serde]
pub struct Pool {
    pub price: Price,
    pub side: Side,
    pool: bid_pool::Pool,
    rate: Decimal,
    #[serde(skip)]
    pending_sum_snapshots: Vec<SumSnapshot>,
}

impl Pool {
    pub fn iter<'a>(
        storage: &'a dyn Storage,
        side: &'a Side,
        oracle: &'a impl Premiumable,
    ) -> impl Iterator<Item = EitherOrBoth<Pool>> + 'a {
        let order = match side {
            Side::Base => cosmwasm_std::Order::Ascending,
            Side::Quote => cosmwasm_std::Order::Descending,
        };

        let populate = |x: StdResult<(Price, ())>| -> Option<Self> {
            match x {
                Ok((price, _)) => Some(Self {
                    price: price.clone(),
                    side: side.clone(),
                    rate: price.to_rate(oracle),
                    // The presence of the key indicates a BidPool should be present,
                    // so we should panic if this is incorrect
                    pool: BID_POOLS
                        .load(storage, PoolKey::new(side.clone(), price))
                        .unwrap(),
                    pending_sum_snapshots: vec![],
                }),
                Err(_) => None,
            }
        };

        let fixed = POOLS
            .prefix((side.clone(), PoolType::Fixed))
            .range(storage, None, None, order)
            .filter_map(populate);

        let oracle = POOLS
            .prefix((side.clone(), PoolType::Oracle))
            .range(storage, None, None, order)
            .filter_map(populate);

        fixed.merge_join_by(oracle, move |f, o| match side {
            Side::Base => f.rate().cmp(&o.rate()),
            Side::Quote => o.rate().cmp(&f.rate()),
        })
    }

    pub fn load(
        storage: &dyn Storage,
        price: &Price,
        side: &Side,
        oracle: &impl Premiumable,
    ) -> Self {
        let key = PoolKey::new(side.clone(), price.clone());
        Self {
            price: price.clone(),
            rate: price.to_rate(oracle),
            side: side.clone(),
            pool: BID_POOLS.load(storage, key).unwrap_or_default(),
            pending_sum_snapshots: vec![],
        }
    }

    fn key(&self) -> PoolKey {
        PoolKey::new(self.side.clone(), self.price.clone())
    }

    pub fn create_order(
        &mut self,
        storage: &mut dyn Storage,
        timestamp: &Timestamp,
        owner: &Addr,
        offer: Uint128,
    ) -> Result<Order, ContractError> {
        let order = Order {
            owner: owner.clone(),
            offer,
            updated_at: *timestamp,
            bid: self.pool.new_bid(offer.into()),
        };
        self.commit(storage)?;
        order.save(storage, self)?;
        Ok(order)
    }

    pub fn load_order(&self, storage: &dyn Storage, owner: &Addr) -> Result<Order, ContractError> {
        let mut order = Order::load(storage, owner, &self.side, &self.price)?;
        self.sync_order(storage, &mut order)?;
        Ok(order)
    }

    pub fn increase_order(
        &mut self,
        storage: &mut dyn Storage,
        order: &mut Order,
        timestamp: &Timestamp,
        amount: Uint128,
    ) -> Result<Uint128, ContractError> {
        order.bid.increase(&mut self.pool, amount.into())?;
        order.offer = order.amount();
        order.updated_at = *timestamp;
        order.save(storage, self)?;
        self.commit(storage)?;
        Ok(amount)
    }

    pub fn retract_order(
        &mut self,
        storage: &mut dyn Storage,
        order: &mut Order,
        timestamp: &Timestamp,
        amount: Option<Uint128>,
    ) -> Result<Uint128, ContractError> {
        let amount256 = amount.map(Uint256::from);
        let refund_amount = order.bid.retract(&mut self.pool, amount256)?;
        order.offer = order.amount();
        order.updated_at = *timestamp;
        order.save(storage, self)?;
        self.commit(storage)?;
        Ok(Uint128::try_from(refund_amount)?)
    }

    pub fn claim_order(
        &mut self,
        storage: &mut dyn Storage,
        order: &mut Order,
    ) -> Result<Uint128, ContractError> {
        let claimed = order.bid.claim_filled();
        order.save(storage, self)?;
        Ok(Uint128::try_from(claimed)?)
    }

    pub fn sync_order(
        &self,
        storage: &dyn Storage,
        order: &mut Order,
    ) -> Result<(), ContractError> {
        let sum_snapshot = self.sum_snapshot(storage, &order.bid).ok();
        Ok(self.pool.sync_bid(&mut order.bid, sum_snapshot)?)
    }

    fn sum_snapshot(&self, storage: &dyn Storage, bid: &bid_pool::Bid) -> StdResult<DecimalScaled> {
        let key = (
            PoolKey::new(self.side.clone(), self.price.clone()),
            bid.sum_snapshot_key(),
        );
        SNAPSHOTS.load(storage, key)
    }
}

impl Swappable for Pool {
    fn swap(&mut self, offer: Uint128) -> Result<(Uint128, Uint128), SwapError> {
        let rate = match self.side {
            Side::Base => self.rate.inv().unwrap(),
            Side::Quote => self.rate,
        };
        let res = self
            .pool
            .distribute(offer.into(), &Decimal256::from(rate))?;
        self.pending_sum_snapshots = res.snapshots;

        Ok((
            res.consumed_offer.try_into()?,
            res.consumed_bids.try_into()?,
        ))
    }

    fn commit(&self, storage: &mut dyn Storage) -> Result<Commitment, SwapError> {
        for s in self.pending_sum_snapshots.clone() {
            SNAPSHOTS.save(storage, (self.key(), s.key()), &s.sum)?;
        }

        BID_POOLS.save(storage, self.key(), &self.pool)?;
        // Clear empty pools so they're not iterated over during a swap
        if self.pool.is_zero() {
            POOLS.remove(storage, self.key());
            return Ok(Commitment::default());
        }

        POOLS.save(storage, self.key(), &())?;
        Ok(Commitment::default())
    }

    fn attributes(&self) -> Vec<Attribute> {
        vec![
            Attribute::new("price", self.price.to_string()),
            Attribute::new("side", self.side.to_string()),
        ]
    }

    fn rate(&self) -> Decimal {
        self.rate
    }

    fn total(&self) -> Uint128 {
        self.pool.total().try_into().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use crate::pool_key::PoolType;

    use super::*;
    use cosmwasm_std::{testing::MockStorage, Decimal};
    use cw_storage_plus::Bound;
    use itertools::EitherOrBoth;
    use rujira_rs::fin::{Price, Side};
    use std::str::FromStr;

    #[test]
    // Verify that when a Pool is removed from storage, the BidPool is retained and the correct values used for syncing bids
    fn pool_bid_pool_replacement() {
        let mut store = MockStorage::new();
        let timestamp = Timestamp::default();
        let owner = Addr::unchecked("owner");
        let offer = Uint128::from(100u128);
        let price = Price::Fixed(Decimal::one());
        let oracle = Decimal::one();
        let mut pool = Pool::load(&store, &price, &Side::Quote, &oracle);
        let k = PoolKey::new(Side::Quote, price.clone());
        pool.create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();

        pool.commit(&mut store).unwrap();
        // Check both the pool and bid pool are stored
        POOLS.load(&store, k.clone()).unwrap();
        BID_POOLS.load(&store, k.clone()).unwrap();

        pool.swap(Uint128::from(100u128)).unwrap();

        // Bid Pool should have been emptied, so the container Pool shold be cleared, but the BidPool should remain
        pool.commit(&mut store).unwrap();
        POOLS.load(&store, k.clone()).unwrap_err();
        let bp = BID_POOLS.load(&store, k.clone()).unwrap();
        // Check it's different from the default
        assert_ne!(bp, bid_pool::Pool::default());

        // Now check it's restored to the pool correctly
        let pool = Pool::load(&store, &price, &Side::Quote, &oracle);
        assert_eq!(pool.pool, bp);
    }

    #[test]
    fn pools_map() {
        let mut store = MockStorage::new();
        let timestamp = Timestamp::default();
        let owner = Addr::unchecked("owner");
        let offer = Uint128::from(100u128);
        let oracle = Decimal::one();
        Pool::load(&store, &Price::Oracle(0), &Side::Quote, &oracle)
            .create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();

        Pool::load(&store, &Price::Oracle(-1), &Side::Quote, &oracle)
            .create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();

        Pool::load(&store, &Price::Oracle(1), &Side::Quote, &oracle)
            .create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();

        Pool::load(&store, &Price::Oracle(2), &Side::Quote, &oracle)
            .create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();

        Pool::load(&store, &Price::Oracle(2), &Side::Base, &oracle)
            .create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();

        Pool::load(&store, &Price::Fixed(Decimal::one()), &Side::Base, &oracle)
            .create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();

        Pool::load(
            &store,
            &Price::Fixed(Decimal::from_ratio(10u128, 9u128)),
            &Side::Quote,
            &oracle,
        )
        .create_order(&mut store, &timestamp, &owner, offer)
        .unwrap();

        let pool = Pool::load(&store, &Price::Oracle(0), &Side::Quote, &oracle);
        assert_eq!(pool.price, Price::Oracle(0));
        assert_eq!(pool.side, Side::Quote);

        let iter = POOLS.range(&store, None, None, cosmwasm_std::Order::Ascending);

        let ordered = iter.collect::<StdResult<Vec<(PoolKey, ())>>>().unwrap();
        assert_eq!(ordered.len(), 7);

        // Result should be ordered by Side, then Price Type, then Price Value
        let (key, _) = ordered[0].clone();
        assert_eq!(key.side, Side::Base);
        assert_eq!(key.price, Price::Fixed(Decimal::one()));

        let (key, _) = ordered[1].clone();
        assert_eq!(key.side, Side::Base);
        assert_eq!(key.price, Price::Oracle(2));

        let (key, _) = ordered[2].clone();
        assert_eq!(key.side, Side::Quote);
        assert_eq!(
            key.price,
            Price::Fixed(Decimal::from_str("1.111111111111111111").unwrap())
        );

        let (key, _) = ordered[3].clone();
        assert_eq!(key.side, Side::Quote);
        assert_eq!(key.price, Price::Oracle(-1));

        let (key, _) = ordered[4].clone();
        assert_eq!(key.side, Side::Quote);
        assert_eq!(key.price, Price::Oracle(0));

        let (key, _) = ordered[5].clone();
        assert_eq!(key.side, Side::Quote);
        assert_eq!(key.price, Price::Oracle(1));

        let (key, _) = ordered[6].clone();
        assert_eq!(key.side, Side::Quote);
        assert_eq!(key.price, Price::Oracle(2));

        let iter =
            POOLS
                .sub_prefix(Side::Base)
                .range(&store, None, None, cosmwasm_std::Order::Ascending);

        let ordered = iter
            .collect::<StdResult<Vec<((PoolType, Price), ())>>>()
            .unwrap();

        assert_eq!(ordered.len(), 2);

        let (key, _) = ordered[0].clone();
        assert_eq!(key.0, PoolType::Fixed);
        assert_eq!(key.1, Price::Fixed(Decimal::one()));

        let (key, _) = ordered[1].clone();
        assert_eq!(key.0, PoolType::Oracle);
        assert_eq!(key.1, Price::Oracle(2));

        let iter = POOLS.prefix((Side::Quote, PoolType::Oracle)).range(
            &store,
            None,
            None,
            cosmwasm_std::Order::Ascending,
        );

        let ordered = iter.collect::<StdResult<Vec<(Price, ())>>>().unwrap();

        assert_eq!(ordered.len(), 4);

        let (price, _) = ordered[0].clone();
        assert_eq!(price, Price::Oracle(-1));

        let (price, _) = ordered[1].clone();
        assert_eq!(price, Price::Oracle(0));

        let (price, _) = ordered[2].clone();
        assert_eq!(price, Price::Oracle(1));

        let (price, _) = ordered[3].clone();
        assert_eq!(price, Price::Oracle(2));

        let iter = POOLS.prefix((Side::Quote, PoolType::Oracle)).range(
            &store,
            Some(Bound::exclusive(Price::Oracle(0))),
            None,
            cosmwasm_std::Order::Ascending,
        );

        let ordered = iter.collect::<StdResult<Vec<(Price, ())>>>().unwrap();

        assert_eq!(ordered.len(), 2);

        let (price, _) = ordered[0].clone();
        assert_eq!(price, Price::Oracle(1));

        let (price, _) = ordered[1].clone();
        assert_eq!(price, Price::Oracle(2));

        // assert_eq!(key_bytes.len(), 2);
    }

    #[test]
    fn side_iter() {
        let mut store = MockStorage::new();
        let timestamp = Timestamp::default();
        let owner = Addr::unchecked("owner");
        let offer = Uint128::from(100u128);
        let oracle = Decimal::one();
        Pool::load(&store, &Price::Oracle(0), &Side::Quote, &oracle)
            .create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();

        Pool::load(&store, &Price::Oracle(-1), &Side::Quote, &oracle)
            .create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();

        Pool::load(&store, &Price::Oracle(1), &Side::Quote, &oracle)
            .create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();

        Pool::load(&store, &Price::Oracle(2), &Side::Quote, &oracle)
            .create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();

        Pool::load(&store, &Price::Oracle(2), &Side::Base, &oracle)
            .create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();

        Pool::load(&store, &Price::Fixed(Decimal::one()), &Side::Base, &oracle)
            .create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();

        Pool::load(&store, &Price::Fixed(Decimal::one()), &Side::Quote, &oracle)
            .create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();

        Pool::load(&store, &Price::Oracle(-1), &Side::Base, &oracle)
            .create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();

        Pool::load(&store, &Price::Oracle(1), &Side::Base, &oracle)
            .create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();

        Pool::load(&store, &Price::Oracle(2), &Side::Base, &oracle)
            .create_order(&mut store, &timestamp, &owner, offer)
            .unwrap();

        Pool::load(
            &store,
            &Price::Fixed(Decimal::from_ratio(10u128, 9u128)),
            &Side::Quote,
            &oracle,
        )
        .create_order(&mut store, &timestamp, &owner, offer)
        .unwrap();

        Pool::load(
            &store,
            &Price::Fixed(Decimal::from_ratio(15u128, 9u128)),
            &Side::Quote,
            &oracle,
        )
        .create_order(&mut store, &timestamp, &owner, offer)
        .unwrap();

        Pool::load(
            &store,
            &Price::Fixed(Decimal::from_ratio(21u128, 9u128)),
            &Side::Quote,
            &oracle,
        )
        .create_order(&mut store, &timestamp, &owner, offer)
        .unwrap();

        let iter = Pool::iter(&store, &Side::Quote, &oracle);
        let ordered: Vec<EitherOrBoth<Pool>> = iter.collect();
        assert_eq!(ordered.len(), 7);

        let x = ordered[0].clone();
        assert_eq!(
            x.clone().left().unwrap().rate(),
            Decimal::from_ratio(21u128, 9u128)
        );
        assert_eq!(x.right(), None);

        let x = ordered[1].clone();
        assert_eq!(
            x.clone().left().unwrap().rate(),
            Decimal::from_ratio(15u128, 9u128)
        );
        assert_eq!(x.right(), None);

        let x = ordered[2].clone();
        assert_eq!(
            x.clone().left().unwrap().rate(),
            Decimal::from_ratio(10u128, 9u128)
        );
        assert_eq!(x.right(), None);

        let x = ordered[3].clone();
        assert_eq!(x.clone().left(), None);
        assert_eq!(
            x.right().unwrap().rate(),
            Decimal::from_ratio(10002u128, 10000u128)
        );

        let x = ordered[4].clone();
        assert_eq!(x.clone().left(), None);
        assert_eq!(
            x.right().unwrap().rate(),
            Decimal::from_ratio(10001u128, 10000u128)
        );

        let x = ordered[5].clone();
        assert_eq!(x.clone().left().unwrap().rate(), Decimal::one());
        assert_eq!(
            x.clone().right().unwrap().rate(),
            x.clone().left().unwrap().rate(),
        );

        let x = ordered[6].clone();
        assert_eq!(x.clone().left(), None);
        assert_eq!(
            x.right().unwrap().rate(),
            Decimal::from_ratio(9999u128, 10000u128)
        );

        let iter = Pool::iter(&store, &Side::Base, &oracle);
        let ordered: Vec<EitherOrBoth<Pool>> = iter.collect();

        assert_eq!(ordered.len(), 4);

        let x = ordered[0].clone();
        assert_eq!(x.clone().left(), None);
        assert_eq!(
            x.right().unwrap().rate(),
            Decimal::from_ratio(9999u128, 10000u128)
        );

        let x = ordered[1].clone();
        assert_eq!(x.clone().left().unwrap().rate(), Decimal::one());
        assert_eq!(x.right(), None);

        let x = ordered[2].clone();
        assert_eq!(x.clone().left(), None);
        assert_eq!(
            x.right().unwrap().rate(),
            Decimal::from_ratio(10001u128, 10000u128)
        );

        let x = ordered[3].clone();
        assert_eq!(x.clone().left(), None);
        assert_eq!(
            x.right().unwrap().rate(),
            Decimal::from_ratio(10002u128, 10000u128)
        );
    }
}
