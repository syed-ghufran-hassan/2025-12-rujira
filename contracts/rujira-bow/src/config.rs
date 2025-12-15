use cosmwasm_schema::cw_serde;
use cosmwasm_std::{StdResult, Storage};
use cw_storage_plus::Item;
use rujira_rs::bow::{InstantiateMsg, Strategies, Strategy};

use crate::ContractError;

#[cw_serde]
pub struct Config {
    pub strategy: Strategies,
}

impl From<InstantiateMsg> for Config {
    fn from(v: InstantiateMsg) -> Self {
        Self {
            strategy: v.strategy,
        }
    }
}

impl Config {
    pub fn load(storage: &dyn Storage) -> StdResult<Self> {
        Item::new("config").load(storage)
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        Ok(self.strategy.validate()?)
    }

    pub fn save(&self, storage: &mut dyn Storage) -> StdResult<()> {
        Item::new("config").save(storage, self)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn validation() {
        // Config { st }.validate().unwrap();
    }
}
