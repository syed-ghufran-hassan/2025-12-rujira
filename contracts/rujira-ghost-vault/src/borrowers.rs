use crate::{state::State, ContractError};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    from_json, to_json_vec, Addr, Decimal, Order, StdError, StdResult, Storage, Uint128,
};
use cw_storage_plus::{Bound, Map};
use rujira_rs::SharePool;
use std::{
    cmp::min,
    ops::{Add, Sub},
};

static BORROWERS: Map<Addr, Borrower> = Map::new("borrowers");
// Delegated shares for a borrower
static DELEGATE_SHARES: Map<(Addr, Addr), Uint128> = Map::new("delegates");

#[cw_serde]
pub struct Borrower {
    pub addr: Addr,
    pub limit: Uint128,
    pub shares: Uint128,
}

impl Borrower {
    pub fn load(storage: &dyn Storage, addr: Addr) -> Result<Self, ContractError> {
        match BORROWERS.load(storage, addr.clone()) {
            Ok(x) => Ok(x),
            Err(StdError::NotFound { .. }) => Err(ContractError::UnauthorizedBorrower {}),
            Err(err) => Err(ContractError::Std(err)),
        }
    }

    pub fn save(&self, storage: &mut dyn Storage) -> StdResult<()> {
        BORROWERS.save(storage, self.addr.clone(), self)
    }

    pub fn delegate_shares(&self, storage: &dyn Storage, delegate: Addr) -> Uint128 {
        DELEGATE_SHARES
            .load(storage, (self.addr.clone(), delegate))
            .unwrap_or_default()
    }

    pub fn delegate_borrow(
        &mut self,
        storage: &mut dyn Storage,
        delegate: Addr,
        pool: &SharePool,
        shares: Uint128,
    ) -> Result<(), ContractError> {
        DELEGATE_SHARES.update(
            storage,
            (self.addr.clone(), delegate),
            |v| -> Result<Uint128, ContractError> { Ok(v.unwrap_or_default().add(shares)) },
        )?;
        self.borrow(storage, pool, shares)
    }

    pub fn borrow(
        &mut self,
        storage: &mut dyn Storage,
        pool: &SharePool,
        shares: Uint128,
    ) -> Result<(), ContractError> {
        if pool.ownership(self.shares.add(shares)).gt(&self.limit) {
            return Err(ContractError::BorrowLimitReached { limit: self.limit });
        }
        self.shares += shares;
        Ok(self.save(storage)?)
    }

    pub fn repay(
        &mut self,
        storage: &mut dyn Storage,
        shares: Uint128,
    ) -> Result<Uint128, ContractError> {
        let repaid = min(shares, self.shares);
        self.shares -= repaid;
        self.save(storage)?;
        Ok(shares.sub(repaid))
    }

    pub fn delegate_repay(
        &mut self,
        storage: &mut dyn Storage,
        delegate: Addr,
        shares: Uint128,
    ) -> Result<Uint128, ContractError> {
        let k = (self.addr.clone(), delegate);
        let delegate = DELEGATE_SHARES.load(storage, k.clone())?;
        let repaid = min(shares, delegate);
        DELEGATE_SHARES.save(storage, k, &delegate.checked_sub(repaid)?)?;
        self.repay(storage, repaid)?;
        Ok(shares.sub(repaid))
    }

    pub fn set(storage: &mut dyn Storage, addr: Addr, limit: Uint128) -> StdResult<()> {
        let mut borrower = BORROWERS.load(storage, addr.clone()).unwrap_or(Borrower {
            addr: addr.clone(),
            limit: Default::default(),
            shares: Default::default(),
        });
        borrower.limit = limit;
        BORROWERS.save(storage, addr, &borrower)
    }

    pub fn list(
        storage: &dyn Storage,
        limit: Option<u8>,
        start_after: Option<Addr>,
    ) -> impl Iterator<Item = StdResult<Self>> + '_ {
        let limit = limit.unwrap_or(100) as usize;
        let min = start_after.map(Bound::exclusive);
        BORROWERS
            .range(storage, min, None, Order::Ascending)
            .take(limit)
            .map(|x| x.map(|(_, v)| v))
    }
}

// ------------ Migration ------------
#[cw_serde]
struct TempSharePool {
    size: Uint128,
    shares: Decimal,
}

pub fn migrate(storage: &mut dyn Storage) -> StdResult<()> {
    let total_borrower_shares: Uint128 = BORROWERS
        .range(storage, None, None, Order::Ascending)
        .map(|r| r.map(|(_, b)| b.shares))
        .sum::<StdResult<Uint128>>()?;

    let mut state = State::load(storage)?;
    let serialized = to_json_vec(&TempSharePool {
        size: state.debt_pool.size(),
        shares: Decimal::from_ratio(total_borrower_shares, 1u128),
    })?;
    state.debt_pool = from_json(&serialized)?;
    state.save(storage)
}
