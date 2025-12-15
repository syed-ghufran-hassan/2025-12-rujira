use cosmwasm_schema::cw_serde;
use cosmwasm_std::{StdResult, Storage, Timestamp, Uint128};
use cw_storage_plus::Item;
use rujira_rs::merge::{ConfigResponse, InstantiateMsg};

use crate::ContractError;

static CONFIG: Item<Config> = Item::new("config");

#[cw_serde]
pub struct Config {
    pub merge_denom: String,
    pub merge_supply: Uint128,
    pub ruji_denom: String,
    pub ruji_allocation: Uint128,
    pub decay_starts_at: Timestamp,
    pub decay_ends_at: Timestamp,
}

impl From<InstantiateMsg> for Config {
    fn from(value: InstantiateMsg) -> Self {
        Self {
            merge_denom: value.merge_denom,
            merge_supply: value.merge_supply,
            ruji_denom: value.ruji_denom,
            ruji_allocation: value.ruji_allocation,
            decay_starts_at: value.decay_starts_at,
            decay_ends_at: value.decay_ends_at,
        }
    }
}

impl From<Config> for ConfigResponse {
    fn from(value: Config) -> Self {
        Self {
            merge_denom: value.merge_denom,
            merge_supply: value.merge_supply,
            ruji_denom: value.ruji_denom,
            ruji_allocation: value.ruji_allocation,
            decay_starts_at: value.decay_starts_at,
            decay_ends_at: value.decay_ends_at,
        }
    }
}

impl Config {
    pub fn load(storage: &dyn Storage) -> StdResult<Self> {
        CONFIG.load(storage)
    }

    pub fn validate(&self, now: &Timestamp) -> Result<(), ContractError> {
        if self.decay_starts_at.lt(now) {
            return Err(ContractError::Invalid("decay_starts_at".to_string()));
        }
        if self.decay_ends_at.lt(&self.decay_starts_at) {
            return Err(ContractError::Invalid("decay_ends_at".to_string()));
        }
        if self.merge_denom.is_empty() {
            return Err(ContractError::Invalid("merge_denom".to_string()));
        }
        if self.merge_supply.eq(&Uint128::zero()) {
            return Err(ContractError::Invalid("merge_supply".to_string()));
        }
        if self.ruji_denom.is_empty() {
            return Err(ContractError::Invalid("ruji_denom".to_string()));
        }
        if self.ruji_allocation.eq(&Uint128::zero()) {
            return Err(ContractError::Invalid("ruji_allocation".to_string()));
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
            merge_denom: "".to_string(),
            merge_supply: Uint128::from(100u128),
            ruji_denom: "uruji".to_string(),
            ruji_allocation: Uint128::from(100u128),
            decay_starts_at: Timestamp::from_seconds(0),
            decay_ends_at: Timestamp::from_seconds(100),
        }
        .validate(&Timestamp::default())
        .unwrap_err();

        Config {
            merge_denom: "ukuji".to_string(),
            merge_supply: Uint128::zero(),
            ruji_denom: "uruji".to_string(),
            ruji_allocation: Uint128::from(100u128),
            decay_starts_at: Timestamp::from_seconds(0),
            decay_ends_at: Timestamp::from_seconds(100),
        }
        .validate(&Timestamp::default())
        .unwrap_err();

        Config {
            merge_denom: "ukuji".to_string(),
            merge_supply: Uint128::from(100u128),
            ruji_denom: "".to_string(),
            ruji_allocation: Uint128::from(100u128),
            decay_starts_at: Timestamp::from_seconds(0),
            decay_ends_at: Timestamp::from_seconds(100),
        }
        .validate(&Timestamp::default())
        .unwrap_err();

        Config {
            merge_denom: "ukuji".to_string(),
            merge_supply: Uint128::from(100u128),
            ruji_denom: "uruji".to_string(),
            ruji_allocation: Uint128::zero(),
            decay_starts_at: Timestamp::from_seconds(0),
            decay_ends_at: Timestamp::from_seconds(100),
        }
        .validate(&Timestamp::default())
        .unwrap_err();

        Config {
            merge_denom: "ukuji".to_string(),
            merge_supply: Uint128::from(100u128),
            ruji_denom: "uruji".to_string(),
            ruji_allocation: Uint128::from(100u128),
            decay_starts_at: Timestamp::default(),
            decay_ends_at: Timestamp::from_seconds(100),
        }
        .validate(&Timestamp::from_seconds(100))
        .unwrap_err();

        Config {
            merge_denom: "ukuji".to_string(),
            merge_supply: Uint128::from(100u128),
            ruji_denom: "uruji".to_string(),
            ruji_allocation: Uint128::from(100u128),
            decay_starts_at: Timestamp::from_seconds(200),
            decay_ends_at: Timestamp::from_seconds(150),
        }
        .validate(&Timestamp::from_seconds(100))
        .unwrap_err();
    }
}
