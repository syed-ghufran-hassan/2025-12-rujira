use cosmwasm_std::{QuerierWrapper, Storage};
use itertools::{EitherOrBoth, Itertools};
use rujira_rs::{exchange::Swappable, fin::Side, Premiumable};

use crate::market_maker::MarketMakerOffer;
use crate::{config::Config, pool::Pool};

pub struct SwapIter<'a> {
    querier: QuerierWrapper<'a>,
    config: &'a Config,
}

impl<'a> SwapIter<'a> {
    pub fn new(querier: QuerierWrapper<'a>, config: &'a Config) -> Self {
        Self { querier, config }
    }

    pub fn iter(
        &self,
        storage: &'a dyn Storage,
        side: &'a Side,
        oracle: &'a impl Premiumable,
    ) -> impl Iterator<Item = EitherOrBoth<EitherOrBoth<Pool>, Vec<MarketMakerOffer>>> + 'a {
        Pool::iter(storage, side, oracle).merge_join_by(
            self.config.market_makers.iter(
                self.querier,
                &self.config.denoms,
                &self.config.tick,
                side,
            ),
            move |x, y| match side {
                Side::Base => x.rate().cmp(&y.rate()),
                Side::Quote => y.rate().cmp(&x.rate()),
            },
        )
    }
}
