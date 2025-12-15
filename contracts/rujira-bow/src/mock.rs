use cosmwasm_std::{coins, Addr, Decimal, Uint128};
use cw_multi_test::{ContractWrapper, Executor};
use rujira_rs_testing::RujiraApp;

use rujira_rs::{
    bow::{self, Xyk},
    TokenMetadata,
};

/// Wrapper struct for BOW contract with convenience methods
#[derive(Debug, Clone)]
pub struct Bow(pub Addr);

impl Bow {
    /// Get the contract address
    pub fn addr(&self) -> &Addr {
        &self.0
    }

    /// Execute a deposit into the BOW pool
    pub fn deposit(
        &self,
        app: &mut RujiraApp,
        sender: &Addr,
        amounts: &[cosmwasm_std::Coin],
        min_return: Option<Uint128>,
    ) -> anyhow::Result<cw_multi_test::AppResponse> {
        app.execute_contract(
            sender.clone(),
            self.0.clone(),
            &bow::ExecuteMsg::Deposit {
                callback: None,
                min_return,
            },
            amounts,
        )
    }

    /// Execute a withdrawal from the BOW pool
    pub fn withdraw(
        &self,
        app: &mut RujiraApp,
        sender: &Addr,
        amount: Uint128,
    ) -> anyhow::Result<cw_multi_test::AppResponse> {
        app.execute_contract(
            sender.clone(),
            self.0.clone(),
            &bow::ExecuteMsg::Withdraw { callback: None },
            &coins(amount.u128(), "bow-lp"), // Assuming BOW LP token
        )
    }

    /// Execute a swap through the BOW pool
    pub fn swap(
        &self,
        app: &mut RujiraApp,
        sender: &Addr,
        offer_amount: u128,
        offer_denom: &str,
        min_return: Option<cosmwasm_std::Coin>,
    ) -> anyhow::Result<cw_multi_test::AppResponse> {
        app.execute_contract(
            sender.clone(),
            self.0.clone(),
            &bow::ExecuteMsg::Swap {
                min_return: min_return.unwrap_or_else(|| cosmwasm_std::coin(1, "usdc")),
                to: None,
                callback: None,
            },
            &coins(offer_amount, offer_denom),
        )
    }

    /// Query the BOW strategy
    pub fn query_strategy(&self, app: &RujiraApp) -> anyhow::Result<bow::StrategyResponse> {
        Ok(app
            .wrap()
            .query_wasm_smart(self.0.clone(), &bow::QueryMsg::Strategy {})?)
    }

    /// Query a quote from the BOW pool
    pub fn query_quote(
        &self,
        app: &RujiraApp,
        request: bow::QuoteRequest,
    ) -> anyhow::Result<bow::QuoteResponse> {
        Ok(app
            .wrap()
            .query_wasm_smart(self.0.clone(), &bow::QueryMsg::Quote(request))?)
    }

    /// Setup a BOW contract with XYK strategy
    pub fn create(
        app: &mut RujiraApp,
        owner: &Addr,
        base_denom: &str,
        quote_denom: &str,
        min_quote: Uint128,
    ) -> Self {
        let lp_denom = format!("{base_denom}-{quote_denom}");
        let bow_code = Box::new(ContractWrapper::new(
            crate::contract::execute,
            crate::contract::instantiate,
            crate::contract::query,
        ));
        let bow_code_id = app.store_code(bow_code);

        let bow_addr = app
            .instantiate_contract(
                bow_code_id,
                owner.clone(),
                &bow::InstantiateMsg {
                    metadata: TokenMetadata {
                        description: format!("{lp_denom} Description"),
                        display: lp_denom.to_string(),
                        name: format!("{lp_denom} Name"),
                        symbol: lp_denom.to_string(),
                        uri: None,
                        uri_hash: None,
                    },
                    strategy: bow::Strategies::Xyk(Xyk::new(
                        base_denom.to_string(),
                        quote_denom.to_string(),
                        Decimal::permille(1),
                        min_quote,
                        Decimal::zero(),
                    )),
                },
                &[],
                "bow",
                None,
            )
            .unwrap();

        Self(bow_addr)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use rujira_rs_testing::mock_rujira_app;

    #[test]
    fn test_bow_setup() {
        let mut app = mock_rujira_app();
        let owner = app.api().addr_make("owner");
        let bow = Bow::create(&mut app, &owner, "btc", "usdc", Uint128::from(1000u128));

        assert!(!bow.addr().as_str().is_empty());
    }
}
