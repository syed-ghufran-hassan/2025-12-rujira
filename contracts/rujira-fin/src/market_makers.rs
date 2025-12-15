use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Api, QuerierWrapper, StdResult};
use rujira_rs::{
    exchange::Swappable,
    fin::{Denoms, Side, Tick},
    MergeNByIter,
};
use schemars::Set;

use crate::market_maker::{MarketMaker, MarketMakerOffer};

#[cw_serde]
pub struct MarketMakers {
    pub contracts: Set<Addr>,
}

impl MarketMakers {
    pub fn new(api: &dyn Api, market_makers: Vec<String>) -> StdResult<Self> {
        Ok(Self {
            contracts: market_makers
                .into_iter()
                .map(|x| api.addr_validate(&x))
                .collect::<StdResult<Set<Addr>>>()?,
        })
    }

    pub fn iter<'a>(
        &'a self,
        querier: QuerierWrapper<'a>,
        denoms: &'a Denoms,
        tick: &'a Tick,
        side: &'a Side,
    ) -> impl Iterator<Item = Vec<MarketMakerOffer>> + 'a {
        MergeNByIter::new(
            self.contracts.iter().map(|addr| {
                MarketMaker::new(
                    querier,
                    denoms.clone(),
                    side.clone(),
                    addr.clone(),
                    tick.clone(),
                )
            }),
            move |a, b| match side.clone() {
                Side::Base => a.rate().cmp(&b.rate()),
                Side::Quote => b.rate().cmp(&a.rate()),
            },
        )
    }
}
