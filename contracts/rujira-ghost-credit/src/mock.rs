use std::str::FromStr;

use cosmwasm_std::{coin, coins, to_json_binary, Addr, Binary, Coin, Decimal};
use cw_multi_test::{AppResponse, ContractWrapper, Executor};
use rujira_ghost_vault::mock::GhostVault;
use rujira_rs::ghost::credit::{
    AccountMsg, AccountResponse, AccountsResponse, ConfigResponse, ExecuteMsg, InstantiateMsg,
    LiquidateMsg, QueryMsg, SudoMsg,
};
use rujira_rs_testing::RujiraApp;
use serde::Serialize;

use crate::contract::{execute, instantiate, query, reply, sudo};

/// Wrapper struct for Ghost Vault contract with convenience methods
#[derive(Debug, Clone)]
pub struct GhostCredit(pub Addr);

impl GhostCredit {
    /// Get the contract address
    pub fn addr(&self) -> &Addr {
        &self.0
    }

    pub fn create_account(
        &self,
        app: &mut RujiraApp,
        owner: &Addr,
        label: &str,
        tag: &str,
        salt: Binary,
    ) -> AppResponse {
        app.execute_contract(
            owner.clone(),
            self.0.clone(),
            &ExecuteMsg::Create {
                salt,
                label: label.to_string(),
                tag: tag.to_string(),
            },
            &[],
        )
        .unwrap()
    }

    pub fn account(
        &self,
        app: &mut RujiraApp,
        account: &AccountResponse,
        msgs: Vec<AccountMsg>,
    ) -> anyhow::Result<AppResponse> {
        app.execute_contract(
            account.owner.clone(),
            self.0.clone(),
            &ExecuteMsg::Account {
                addr: account.account.to_string(),
                msgs,
            },
            &[],
        )
    }

    pub fn account_borrow(
        &self,
        app: &mut RujiraApp,
        account: &AccountResponse,
        amount: u128,
        denom: &str,
    ) -> anyhow::Result<AppResponse> {
        self.account(app, account, vec![AccountMsg::Borrow(coin(amount, denom))])
    }

    pub fn account_execute<T>(
        &self,
        app: &mut RujiraApp,
        account: &AccountResponse,
        contract: &Addr,
        msg: T,
        funds: Vec<Coin>,
    ) -> anyhow::Result<AppResponse>
    where
        T: Serialize + Sized,
    {
        self.account(
            app,
            account,
            vec![AccountMsg::Execute {
                contract_addr: contract.to_string(),
                msg: to_json_binary(&msg).unwrap(),
                funds,
            }],
        )
    }

    pub fn account_send(
        &self,
        app: &mut RujiraApp,
        account: &AccountResponse,
        amount: u128,
        denom: &str,
        recipient: &Addr,
    ) -> anyhow::Result<AppResponse> {
        self.account(
            app,
            account,
            vec![AccountMsg::Send {
                to_address: recipient.to_string(),
                funds: coins(amount, denom),
            }],
        )
    }

    pub fn account_preference_order(
        &self,
        app: &mut RujiraApp,
        account: &AccountResponse,
        denom: &str,
        after: Option<&str>,
    ) -> anyhow::Result<AppResponse> {
        self.account(
            app,
            account,
            vec![AccountMsg::SetPreferenceOrder {
                denom: denom.to_string(),
                after: after.map(|x| x.to_string()),
            }],
        )
    }

    pub fn liquidate(
        &self,
        app: &mut RujiraApp,
        account: &AccountResponse,
        msgs: Vec<LiquidateMsg>,
    ) -> anyhow::Result<AppResponse> {
        app.execute_contract(
            account.owner.clone(),
            self.0.clone(),
            &ExecuteMsg::Liquidate {
                addr: account.account.to_string(),
                msgs,
            },
            &[],
        )
    }

    pub fn liquidate_execute<T>(
        &self,
        app: &mut RujiraApp,
        account: &AccountResponse,
        contract: &Addr,
        msg: T,
        funds: Vec<Coin>,
    ) -> anyhow::Result<AppResponse>
    where
        T: Serialize + Sized,
    {
        self.liquidate(
            app,
            account,
            vec![LiquidateMsg::Execute {
                contract_addr: contract.to_string(),
                msg: to_json_binary(&msg).unwrap(),
                funds,
            }],
        )
    }

    pub fn liquidate_execute_repay<T>(
        &self,
        app: &mut RujiraApp,
        account: &AccountResponse,
        contract: &Addr,
        msg: T,
        funds: Vec<Coin>,
        repay: &str,
    ) -> anyhow::Result<AppResponse>
    where
        T: Serialize + Sized,
    {
        self.liquidate(
            app,
            account,
            vec![
                LiquidateMsg::Execute {
                    contract_addr: contract.to_string(),
                    msg: to_json_binary(&msg).unwrap(),
                    funds,
                },
                LiquidateMsg::Repay(repay.to_string()),
            ],
        )
    }

    pub fn set_collateral(&self, app: &mut RujiraApp, denom: &str, ratio: &str) -> AppResponse {
        app.wasm_sudo(
            self.0.clone(),
            &SudoMsg::SetCollateral {
                denom: denom.to_string(),
                collateralization_ratio: Decimal::from_str(ratio).unwrap(),
            },
        )
        .unwrap()
    }

    pub fn set_vault(&self, app: &mut RujiraApp, vault: &GhostVault) -> AppResponse {
        app.wasm_sudo(
            self.0.clone(),
            &SudoMsg::SetVault {
                address: vault.addr().to_string(),
            },
        )
        .unwrap()
    }

    pub fn query_account(&self, app: &RujiraApp, addr: &Addr) -> AccountResponse {
        app.wrap()
            .query_wasm_smart(self.0.clone(), &QueryMsg::Account(addr.to_string()))
            .unwrap()
    }

    pub fn query_accounts(
        &self,
        app: &RujiraApp,
        owner: &Addr,
        tag: Option<String>,
    ) -> AccountsResponse {
        app.wrap()
            .query_wasm_smart(
                self.0.clone(),
                &QueryMsg::Accounts {
                    owner: owner.to_string(),
                    tag,
                },
            )
            .unwrap()
    }

    pub fn query_all_accounts(&self, app: &RujiraApp, cursor: Option<Addr>) -> AccountsResponse {
        app.wrap()
            .query_wasm_smart(
                self.0.clone(),
                &QueryMsg::AllAccounts {
                    cursor: cursor.map(|x| x.to_string()),
                    limit: None,
                },
            )
            .unwrap()
    }

    pub fn query_config(&self, app: &RujiraApp) -> ConfigResponse {
        app.wrap()
            .query_wasm_smart(self.0.clone(), &QueryMsg::Config {})
            .unwrap()
    }

    pub fn query_next(&self, app: &RujiraApp, owner: &Addr, salt: Binary) -> Addr {
        app.wrap()
            .query_wasm_smart(
                self.0.clone(),
                &QueryMsg::Predict {
                    salt,
                    owner: owner.to_string(),
                },
            )
            .unwrap()
    }

    pub fn create(app: &mut RujiraApp, owner: &Addr, fees: &Addr) -> Self {
        let account_code = Box::new(
            ContractWrapper::new(
                rujira_account::contract::execute,
                rujira_account::contract::instantiate,
                rujira_account::contract::query,
            )
            .with_sudo(rujira_account::contract::sudo),
        );
        let account_code_id = app.store_code(account_code);

        let code = Box::new(
            ContractWrapper::new(execute, instantiate, query)
                .with_sudo(sudo)
                .with_reply(reply),
        );
        let code_id = app.store_code(code);
        let addr = app
            .instantiate_contract(
                code_id,
                owner.clone(),
                &InstantiateMsg {
                    code_id: account_code_id,
                    fee_liquidation: Decimal::from_str("0.01").unwrap(),
                    fee_liquidator: Decimal::from_str("0.005").unwrap(),
                    liquidation_max_slip: Decimal::from_str("0.3").unwrap(),
                    liquidation_threshold: Decimal::one(),
                    adjustment_threshold: Decimal::from_str("0.95").unwrap(),
                    fee_address: fees.clone(),
                },
                &[],
                "template",
                None,
            )
            .unwrap();
        Self(addr)
    }
}
