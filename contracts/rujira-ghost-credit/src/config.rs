use std::collections::BTreeMap;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, StdResult, Storage};
use cw_storage_plus::Item;
use rujira_rs::ghost::credit::{ConfigResponse, ConfigUpdate, InstantiateMsg};

use crate::ContractError;

static CONFIG: Item<Config> = Item::new("config");
pub type CollateralRatios = BTreeMap<String, Decimal>;

#[cw_serde]
pub struct Config {
    pub code_id: u64,
    pub collateral_ratios: CollateralRatios,
    pub fee_liquidation: Decimal,
    pub fee_liquidator: Decimal,
    pub fee_address: Addr,
    pub liquidation_max_slip: Decimal,
    pub liquidation_threshold: Decimal,
    pub adjustment_threshold: Decimal,
}

impl From<InstantiateMsg> for Config {
    fn from(value: InstantiateMsg) -> Self {
        Self {
            code_id: value.code_id,
            collateral_ratios: BTreeMap::default(),
            fee_liquidation: value.fee_liquidation,
            fee_liquidator: value.fee_liquidator,
            fee_address: value.fee_address,
            liquidation_max_slip: value.liquidation_max_slip,
            liquidation_threshold: value.liquidation_threshold,
            adjustment_threshold: value.adjustment_threshold,
        }
    }
}

impl From<Config> for ConfigResponse {
    fn from(value: Config) -> Self {
        Self {
            code_id: value.code_id,
            collateral_ratios: value.collateral_ratios,
            fee_liquidation: value.fee_liquidation,
            fee_liquidator: value.fee_liquidator,
            fee_address: value.fee_address,
            liquidation_max_slip: value.liquidation_max_slip,
            liquidation_threshold: value.liquidation_threshold,
            adjustment_threshold: value.adjustment_threshold,
        }
    }
}

impl Config {
    pub fn load(storage: &dyn Storage) -> StdResult<Self> {
        CONFIG.load(storage)
    }

    pub fn update(&mut self, update: &ConfigUpdate) {
        if let Some(code_id) = update.code_id {
            self.code_id = code_id;
        }
        if let Some(fee_liquidation) = update.fee_liquidation {
            self.fee_liquidation = fee_liquidation;
        }
        if let Some(fee_liquidator) = update.fee_liquidator {
            self.fee_liquidator = fee_liquidator;
        }
        if let Some(fee_address) = &update.fee_address {
            self.fee_address = fee_address.clone();
        }
        if let Some(liquidation_max_slip) = update.liquidation_max_slip {
            self.liquidation_max_slip = liquidation_max_slip;
        }
        if let Some(liquidation_threshold) = update.liquidation_threshold {
            self.liquidation_threshold = liquidation_threshold;
        }
        if let Some(adjustment_threshold) = update.adjustment_threshold {
            self.adjustment_threshold = adjustment_threshold;
        }
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        if self.liquidation_threshold <= self.adjustment_threshold {
            return Err(ContractError::InvalidConfig {
                key: "adjustment_threshold".to_string(),
                value: self.adjustment_threshold.to_string(),
            });
        }
        if self.liquidation_max_slip >= Decimal::one() {
            return Err(ContractError::InvalidConfig {
                key: "liquidation_max_slip".to_string(),
                value: self.liquidation_max_slip.to_string(),
            });
        }
        for (k, v) in self.collateral_ratios.iter() {
            if v > &Decimal::one() {
                return Err(ContractError::InvalidConfig {
                    key: format!("#{k} collateral_ratio"),
                    value: v.to_string(),
                });
            }
        }

        if self.fee_liquidation >= Decimal::percent(5) {
            return Err(ContractError::InvalidConfig {
                key: "fee_liquidation".to_string(),
                value: self.fee_liquidation.to_string(),
            });
        }

        if self.fee_liquidator >= Decimal::percent(5) {
            return Err(ContractError::InvalidConfig {
                key: "fee_liquidator".to_string(),
                value: self.fee_liquidator.to_string(),
            });
        }

        Ok(())
    }

    pub fn save(&self, storage: &mut dyn Storage) -> StdResult<()> {
        CONFIG.save(storage, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation() {
        Config {
            code_id: 1,
            collateral_ratios: BTreeMap::default(),
            fee_liquidation: Decimal::percent(1),
            fee_liquidator: Decimal::percent(1),
            fee_address: Addr::unchecked(""),
            liquidation_max_slip: Decimal::percent(30),
            liquidation_threshold: Decimal::percent(100),
            adjustment_threshold: Decimal::percent(90),
        }
        .validate()
        .unwrap();
    }
}
