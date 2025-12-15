use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Api, Decimal, Deps, DepsMut, StdResult, Storage};
use cw_storage_plus::Item;
use rujira_rs::{
    fin::{ConfigResponse, Denoms, InstantiateMsg, Tick},
    Layer1Asset, Oracle,
};

use crate::{market_makers::MarketMakers, ContractError};

pub static CONFIG: Item<Config> = Item::new("config");

#[cw_serde]
pub struct Config {
    pub denoms: Denoms,
    pub oracles: Option<[Layer1Asset; 2]>,
    pub market_makers: MarketMakers,
    pub tick: Tick,
    pub fee_maker: Decimal,
    pub fee_taker: Decimal,
    pub fee_address: Addr,
}

impl Config {
    pub fn new(api: &dyn Api, value: InstantiateMsg) -> StdResult<Self> {
        Ok(Self {
            denoms: value.denoms.clone(),
            oracles: value.oracles,
            market_makers: MarketMakers::new(api, value.market_makers)?,
            tick: value.tick,
            fee_taker: value.fee_taker,
            fee_maker: value.fee_maker,
            fee_address: api.addr_validate(value.fee_address.as_str())?,
        })
    }

    pub fn validate(&self, deps: Deps) -> Result<(), ContractError> {
        self.denoms.validate()?;
        if let Some(oracles) = self.oracles.clone() {
            oracles[0].tor_price(deps.querier)?;
            oracles[1].tor_price(deps.querier)?;
        }
        if self.fee_maker >= Decimal::one() {
            return Err(ContractError::Invalid("fee_maker >= 1".into()));
        }
        if self.fee_taker >= Decimal::one() {
            return Err(ContractError::Invalid("fee_take >= 1".into()));
        }
        self.tick.validate()?;
        Ok(())
    }

    pub fn save(&self, storage: &mut dyn Storage) -> StdResult<()> {
        CONFIG.save(storage, self)
    }

    pub fn update(
        &mut self,
        tick: Option<Tick>,
        market_makers: Option<MarketMakers>,
        fee_taker: Option<Decimal>,
        fee_maker: Option<Decimal>,
        fee_address: Option<Addr>,
        oracles: Option<[Layer1Asset; 2]>,
    ) {
        if let Some(tick) = tick {
            self.tick = tick;
        }
        if let Some(market_makers) = market_makers {
            self.market_makers = market_makers;
        }
        if let Some(fee_taker) = fee_taker {
            self.fee_taker = fee_taker;
        }
        if let Some(fee_maker) = fee_maker {
            self.fee_maker = fee_maker;
        }
        if let Some(fee_address) = fee_address {
            self.fee_address = fee_address;
        }
        if let Some(oracles) = oracles {
            self.oracles = Some(oracles);
        }
    }

    pub fn migrate(deps: DepsMut) -> StdResult<()> {
        #[cw_serde]
        pub struct Legacy {
            pub denoms: Denoms,
            pub oracles: Option<[Layer1Asset; 2]>,
            pub market_maker: Option<Addr>,
            pub tick: Tick,
            pub fee_maker: Decimal,
            pub fee_taker: Decimal,
            pub fee_address: Addr,
        }
        let legacy: Legacy = Item::new("config").load(deps.storage)?;
        Self {
            denoms: legacy.denoms,
            oracles: legacy.oracles,
            market_makers: MarketMakers {
                contracts: legacy.market_maker.into_iter().collect(),
            },
            tick: legacy.tick,
            fee_maker: legacy.fee_maker,
            fee_taker: legacy.fee_taker,
            fee_address: legacy.fee_address,
        }
        .save(deps.storage)
    }
}

impl From<Config> for ConfigResponse {
    fn from(value: Config) -> Self {
        Self {
            denoms: value.denoms,
            oracles: value.oracles,
            market_makers: value
                .market_makers
                .contracts
                .iter()
                .map(|x| x.to_string())
                .collect(),
            tick: value.tick,
            fee_maker: value.fee_maker,
            fee_taker: value.fee_taker,
            fee_address: value.fee_address.to_string(),
        }
    }
}
