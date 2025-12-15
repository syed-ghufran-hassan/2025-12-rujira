use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    Addr, Attribute, Binary, Decimal, Fraction, QuerierWrapper, StdResult, Storage, Uint128,
};
use rujira_rs::{
    bow,
    exchange::{Commitment, SwapError, Swappable},
    fin::{Denoms, Side, Tick},
};
use std::{
    cmp::Ordering,
    ops::{Div, Mul},
};

pub struct MarketMaker<'a> {
    q: QuerierWrapper<'a>,
    denoms: Denoms,
    side: Side,
    contract: Addr,
    tick: Tick,
    bid: Uint128,
    ask: Uint128,
    last_price: Option<Decimal>,
    data: Option<Binary>,
}

impl<'a> MarketMaker<'a> {
    pub fn new(
        q: QuerierWrapper<'a>,
        denoms: Denoms,
        side: Side,
        contract: Addr,
        tick: Tick,
    ) -> Self {
        Self {
            q,
            denoms,
            side,
            contract,
            tick,
            bid: Uint128::zero(),
            ask: Uint128::zero(),
            last_price: None,
            data: None,
        }
    }
}

#[cw_serde]
pub struct MarketMakerOffer {
    pub price: Decimal,
    pub total: Uint128,
    side: Side,
    rate: Decimal,
    contract: Addr,
    commitment: (Uint128, Uint128),
}

impl Swappable for MarketMakerOffer {
    fn rate(&self) -> Decimal {
        self.rate
    }

    fn attributes(&self) -> Vec<Attribute> {
        let price = match self.side {
            Side::Base => self.price.inv().unwrap().to_string(),
            Side::Quote => self.price.to_string(),
        };
        vec![
            Attribute::new("price", format!("{}:{price}", self.contract)),
            Attribute::new("side", self.side.to_string()),
        ]
    }

    fn total(&self) -> Uint128 {
        self.total
    }

    fn swap(&mut self, offer: Uint128) -> Result<(Uint128, Uint128), SwapError> {
        let offer_value = Decimal::from_ratio(offer, 1u128)
            .mul(self.price)
            .to_uint_floor();
        let pool_value = Decimal::from_ratio(self.total, 1u128)
            .div(self.price)
            .to_uint_floor();
        let res = match self.total.cmp(&offer_value) {
            Ordering::Greater => {
                self.total -= offer_value;
                (offer, offer_value)
            }
            // Complete fill
            _ => {
                let size = self.total;
                self.total = Uint128::zero();
                (pool_value, size)
            }
        };

        self.commitment = res;
        Ok(res)
    }

    fn commit(&self, _storage: &mut dyn Storage) -> Result<Commitment, SwapError> {
        Ok(Commitment::new(&self.contract, self.commitment))
    }
}

impl<'a> MarketMaker<'a> {
    fn query_next(&mut self, contract: Addr) -> Option<MarketMakerOffer> {
        let res: StdResult<Option<bow::QuoteResponse>> = self.q.query_wasm_smart(
            contract.to_string(),
            &bow::QueryMsg::Quote(bow::QuoteRequest {
                min_price: self.last_price,
                ask_denom: self.denoms.bid(&self.side).to_string(),
                offer_denom: self.denoms.ask(&self.side).to_string(),
                data: self.data.clone(),
            }),
        );

        match res {
            Ok(Some(res)) => {
                self.bid += Decimal::from_ratio(res.size, 1u128)
                    .mul(res.price)
                    .to_uint_ceil();
                self.ask += res.size;
                self.data = res.data;
                let price = res.price;
                self.last_price = Some(price);
                // Rate is used for presentation, and for sorting and grouping `Iter<impl Swappable>`
                // Truncate to "worse" price so that execution price is fractionally better than
                // the price rendered
                let rate = match self.side {
                    Side::Base => self.tick.truncate_ceil(&res.price.inv().unwrap()),
                    Side::Quote => self.tick.truncate_floor(&res.price),
                };

                Some(MarketMakerOffer {
                    contract,
                    price,
                    rate,
                    total: res.size,
                    side: self.side.clone(),
                    commitment: Default::default(),
                })
            }
            _ => None,
        }
    }
}

impl Iterator for MarketMaker<'_> {
    type Item = MarketMakerOffer;

    fn next(&mut self) -> Option<Self::Item> {
        self.query_next(self.contract.clone())
    }
}

#[cfg(test)]

mod test {
    use super::*;
    use cosmwasm_std::{
        from_json, testing::MockQuerier, to_json_binary, ContractResult, SystemError, SystemResult,
        WasmQuery,
    };
    use std::ops::Add;
    use std::str::FromStr;

    #[test]
    fn test_iterator() {
        let mut q = MockQuerier::default();
        q.update_wasm(|x| match x {
            WasmQuery::Smart { msg, .. } => {
                let query: bow::QueryMsg = from_json(msg).unwrap();
                match query {
                    bow::QueryMsg::Quote(query) => {
                        let ask_total: Uint128 = query
                            .data
                            .map(from_json)
                            .transpose()
                            .unwrap()
                            .unwrap_or_default();
                        // Double the order size until we run out of funds
                        if ask_total.gt(&Uint128::from(1000000u128)) {
                            return SystemResult::Err(SystemError::Unknown {});
                        }

                        let size = if ask_total.is_zero() {
                            Uint128::from(100u128)
                        } else {
                            ask_total * Uint128::from(2u128)
                        };

                        SystemResult::Ok(ContractResult::Ok(
                            to_json_binary(&bow::QuoteResponse {
                                size,
                                data: to_json_binary(&size.add(ask_total)).ok(),
                                price: query
                                    .min_price
                                    .unwrap_or(Decimal::one())
                                    .add(Decimal::from_ratio(1u128, 4u128)),
                            })
                            .unwrap(),
                        ))
                    }
                    bow::QueryMsg::Strategy {} => todo!(),
                }
            }
            _ => SystemResult::Err(SystemError::Unknown {}),
        });
        let contract = Addr::unchecked("bow");
        let mut iter = MarketMaker::new(
            QuerierWrapper::new(&q),
            Denoms::new("btc", "usdc"),
            Side::Quote,
            contract.clone(),
            Tick::new(3),
        );

        let items: Vec<MarketMakerOffer> = iter.by_ref().take(5).collect();

        assert_eq!(
            items,
            vec![
                MarketMakerOffer {
                    contract: contract.clone(),
                    price: Decimal::from_str("1.25").unwrap(),
                    rate: Decimal::from_str("1.25").unwrap(),
                    total: Uint128::from(100u128),
                    side: Side::Quote,
                    commitment: Default::default()
                },
                MarketMakerOffer {
                    contract: contract.clone(),
                    price: Decimal::from_str("1.5").unwrap(),
                    rate: Decimal::from_str("1.5").unwrap(),
                    total: Uint128::from(200u128),
                    side: Side::Quote,
                    commitment: Default::default()
                },
                MarketMakerOffer {
                    contract: contract.clone(),
                    price: Decimal::from_str("1.75").unwrap(),
                    rate: Decimal::from_str("1.75").unwrap(),
                    total: Uint128::from(600u128),
                    side: Side::Quote,
                    commitment: Default::default()
                },
                MarketMakerOffer {
                    contract: contract.clone(),
                    price: Decimal::from_str("2").unwrap(),
                    rate: Decimal::from_str("2").unwrap(),
                    total: Uint128::from(1800u128),
                    side: Side::Quote,
                    commitment: Default::default()
                },
                MarketMakerOffer {
                    contract: contract.clone(),
                    price: Decimal::from_str("2.25").unwrap(),
                    rate: Decimal::from_str("2.25").unwrap(),
                    total: Uint128::from(5400u128),
                    side: Side::Quote,
                    commitment: Default::default()
                }
            ]
        );
    }
    #[test]
    fn test_market_market_item_swap() {
        let contract = Addr::unchecked("bow");

        // Quote partial fill
        let mut o = MarketMakerOffer {
            contract: contract.clone(),
            price: Decimal::from_str("1.25").unwrap(),
            rate: Decimal::from_str("1.25").unwrap(),
            total: Uint128::from(10000u128),
            side: Side::Quote,
            commitment: Default::default(),
        };
        let (bid_returned, offer_consumed) = o.swap(Uint128::from(500u128)).unwrap();
        assert_eq!(bid_returned, Uint128::from(500u128));
        assert_eq!(offer_consumed, Uint128::from(625u128));

        // Quote full fill
        let mut o = MarketMakerOffer {
            contract: contract.clone(),
            price: Decimal::from_str("1.25").unwrap(),
            rate: Decimal::from_str("1.25").unwrap(),
            total: Uint128::from(10000u128),
            side: Side::Quote,
            commitment: Default::default(),
        };
        let (bid_returned, offer_consumed) = o.swap(Uint128::from(500000u128)).unwrap();
        assert_eq!(bid_returned, Uint128::from(8000u128));
        assert_eq!(offer_consumed, Uint128::from(10000u128));

        // Base partial fill
        let mut o = MarketMakerOffer {
            contract: contract.clone(),
            price: Decimal::from_str("1.25").unwrap(),
            rate: Decimal::from_str("0.8").unwrap(),
            total: Uint128::from(10000u128),
            side: Side::Base,
            commitment: Default::default(),
        };
        let (bid_returned, offer_consumed) = o.swap(Uint128::from(500u128)).unwrap();
        assert_eq!(bid_returned, Uint128::from(500u128));
        assert_eq!(offer_consumed, Uint128::from(625u128));

        // Base full fill
        let mut o = MarketMakerOffer {
            contract: contract.clone(),
            price: Decimal::from_str("1.25").unwrap(),
            rate: Decimal::from_str("0.8").unwrap(),
            total: Uint128::from(10000u128),
            side: Side::Base,
            commitment: Default::default(),
        };
        let (bid_returned, offer_consumed) = o.swap(Uint128::from(500000u128)).unwrap();
        assert_eq!(bid_returned, Uint128::from(8000u128));
        assert_eq!(offer_consumed, Uint128::from(10000u128));

        let mut o = MarketMakerOffer {
            contract: contract.clone(),
            price: Decimal::from_str("0.00000999").unwrap(),
            total: Uint128::from(19980u128),
            side: Side::Base,
            rate: Decimal::from_str("100100.11").unwrap(),
            commitment: Default::default(),
        };
        let (bid_returned, offer_consumed) = o.swap(Uint128::from(50000000u128)).unwrap();
        assert_eq!(bid_returned, Uint128::from(50000000u128));
        assert_eq!(offer_consumed, Uint128::from(499u128));
    }
}
