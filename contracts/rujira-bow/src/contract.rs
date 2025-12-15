#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    coin, coins, to_json_binary, BankMsg, Binary, CosmosMsg, Deps, DepsMut, Empty, Env,
    MessageInfo, Response,
};
use cw2::set_contract_version;
use cw_utils::{must_pay, one_coin, NativeBalance};
use rujira_rs::bow::{
    ExecuteMsg, InstantiateMsg, QueryMsg, Strategies, Strategy, StrategyResponse, StrategyState,
    SudoMsg,
};
use rujira_rs::TokenFactory;

use crate::config::Config;
use crate::error::ContractError;
use crate::events::{event_deposit, event_swap, event_withdraw};

// version info for migration info
const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    let config = Config::from(msg.clone());
    config.validate()?;
    config.save(deps.storage)?;
    Ok(Response::default().add_message(
        TokenFactory::new(&env, config.strategy.denom().as_str()).create_msg(msg.metadata),
    ))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    let config = Config::load(deps.storage)?;
    let mut state = config.strategy.load_state(deps.as_ref(), env.clone())?;

    match msg {
        ExecuteMsg::Swap {
            min_return,
            to,
            callback,
        } => {
            let offer = one_coin(&info)?;
            let (fee, surplus) =
                config
                    .strategy
                    .validate_swap(&mut state, offer.clone(), min_return.clone())?;
            let to_address = to
                .map(|x| deps.api.addr_validate(x.as_str()))
                .transpose()?
                .unwrap_or(info.sender);

            config.strategy.commit_state(deps, &state)?;
            let event = event_swap(offer, min_return.clone(), fee, surplus);

            match callback {
                None => Ok(Response::default()
                    .add_message(CosmosMsg::Bank(BankMsg::Send {
                        to_address: to_address.to_string(),
                        amount: vec![min_return],
                    }))
                    .add_event(event)),
                Some(cb) => Ok(Response::default()
                    .add_message(cb.to_message(&to_address, Empty {}, vec![min_return])?)
                    .add_event(event)),
            }
        }
        ExecuteMsg::Deposit {
            min_return,
            callback,
        } => {
            let t = TokenFactory::new(&env, config.strategy.denom().as_str());
            let minted = config
                .strategy
                .deposit(&mut state, NativeBalance(info.funds))?;

            config.strategy.commit_state(deps, &state)?;

            if let Some(min) = min_return {
                if minted.lt(&min) {
                    return Err(ContractError::InsufficientFunds {});
                };
            }

            match callback {
                None => Ok(Response::default()
                    .add_message(t.mint_msg(minted, info.sender.clone()))
                    .add_event(event_deposit(info.sender, coin(minted.u128(), t.denom())))),
                Some(cb) => Ok(Response::default()
                    .add_message(t.mint_msg(minted, env.contract.address))
                    .add_message(cb.to_message(
                        &info.sender,
                        Empty {},
                        coins(minted.u128(), t.denom()),
                    )?)
                    .add_event(event_deposit(info.sender, coin(minted.u128(), t.denom())))),
            }
        }
        ExecuteMsg::Withdraw { callback } => {
            let t = TokenFactory::new(&env, config.strategy.denom().as_str());
            let balance = must_pay(&info, t.denom().as_str())?;
            let withdrawn = config.strategy.withdraw(&mut state, balance)?;
            config.strategy.commit_state(deps, &state)?;

            match callback {
                None => Ok(Response::default()
                    .add_message(CosmosMsg::Bank(BankMsg::Send {
                        to_address: info.sender.to_string(),
                        amount: withdrawn.clone().into_vec(),
                    }))
                    .add_message(t.burn_msg(balance))
                    .add_event(event_withdraw(info.sender, coin(balance.u128(), t.denom())))),
                Some(cb) => Ok(Response::default()
                    .add_message(cb.to_message(
                        &info.sender,
                        Empty {},
                        withdrawn.clone().into_vec(),
                    )?)
                    .add_message(t.burn_msg(balance))
                    .add_event(event_withdraw(info.sender, coin(balance.u128(), t.denom())))),
            }
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn sudo(deps: DepsMut, _env: Env, msg: SudoMsg) -> Result<Response, ContractError> {
    let mut config = Config::load(deps.storage)?;

    match msg {
        SudoMsg::SetStrategy(strategy) => {
            config.strategy = strategy;
            config.validate()?;
            config.save(deps.storage)?;
            Ok(Response::default())
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> Result<Binary, ContractError> {
    let config = Config::load(deps.storage)?;
    let state = config.strategy.load_state(deps, env)?;
    match msg {
        QueryMsg::Quote(req) => {
            let q = config.strategy.quote(&state, req)?;
            Ok(to_json_binary(&q)?)
        }
        QueryMsg::Strategy {} => match (config.strategy, state) {
            (Strategies::Xyk(strategy), StrategyState::Xyk(state)) => {
                Ok(to_json_binary(&StrategyResponse::Xyk((strategy, state)))?)
            }
        },
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, _msg: ()) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    Ok(Response::default())
}

#[cfg(test)]
mod tests {

    use super::*;
    use cosmwasm_std::{Decimal, Event, Uint128};
    use cw_multi_test::{ContractWrapper, Executor};
    use rujira_rs::{
        bow::{Strategies, Xyk},
        TokenMetadata,
    };
    use rujira_rs_testing::mock_rujira_app;

    #[test]
    fn instantiation() {
        let mut app = mock_rujira_app();
        let owner = app.api().addr_make("owner");

        let code = Box::new(ContractWrapper::new(execute, instantiate, query));
        let code_id = app.store_code(code);
        app.instantiate_contract(
            code_id,
            owner,
            &InstantiateMsg {
                metadata: TokenMetadata {
                    description: "RUJI-USDC XYK Liquidity Pool Token".to_string(),
                    display: "LP RUJI-USDC".to_string(),
                    name: "LP RUJI-USDC".to_string(),
                    symbol: "LP/RUJI-USDC".to_string(),
                    uri: None,
                    uri_hash: None,
                },
                strategy: Strategies::Xyk(Xyk::new(
                    "ruji".to_string(),
                    "usdc".to_string(),
                    Decimal::permille(1u64),
                    Uint128::from(Xyk::MIN_MIN_QUOTE),
                    Decimal::zero(),
                )),
            },
            &[],
            "template",
            None,
        )
        .unwrap();
    }

    #[test]
    fn deposit_withdraw() {
        let mut app = mock_rujira_app();
        let addr = app.api().addr_make("depositor");
        app.init_modules(|router, _api, storage| {
            router
                .bank
                .init_balance(storage, &addr, vec![coin(1000, "ruji"), coin(1000, "usdc")])
        })
        .unwrap();
        let owner = app.api().addr_make("owner");

        let code = Box::new(ContractWrapper::new(execute, instantiate, query));
        let code_id = app.store_code(code);
        let contract = app
            .instantiate_contract(
                code_id,
                owner,
                &InstantiateMsg {
                    metadata: TokenMetadata {
                        description: "RUJI-USDC XYK Liquidity Pool Token".to_string(),
                        display: "LP RUJI-USDC".to_string(),
                        name: "LP RUJI-USDC".to_string(),
                        symbol: "LP/RUJI-USDC".to_string(),
                        uri: None,
                        uri_hash: None,
                    },
                    strategy: Strategies::Xyk(Xyk::new(
                        "ruji".to_string(),
                        "usdc".to_string(),
                        Decimal::permille(1u64),
                        Uint128::from(Xyk::MIN_MIN_QUOTE),
                        Decimal::zero(),
                    )),
                },
                &[],
                "template",
                None,
            )
            .unwrap();

        app.execute_contract(
            addr.clone(),
            contract.clone(),
            &ExecuteMsg::Deposit {
                min_return: Some(Uint128::from(1001u128)),
                callback: None,
            },
            &[coin(1000, "ruji"), coin(1000, "usdc")],
        )
        .unwrap_err();

        let res = app
            .execute_contract(
                addr.clone(),
                contract.clone(),
                &ExecuteMsg::Deposit {
                    min_return: Some(Uint128::from(1000u128)),
                    callback: None,
                },
                &[coin(1000, "ruji"), coin(1000, "usdc")],
            )
            .unwrap();

        res.assert_event(
            &Event::new("wasm-rujira-bow/deposit")
                .add_attributes(vec![("minted", "1000x/bow-xyk-ruji-usdc")]),
        );

        res.assert_event(&Event::new("mint").add_attributes(vec![
            ("amount", "1000"),
            ("denom", "x/bow-xyk-ruji-usdc"),
            ("recipient", addr.as_str()),
        ]));

        let res = app
            .execute_contract(
                addr.clone(),
                contract.clone(),
                &ExecuteMsg::Withdraw { callback: None },
                &[coin(200, "x/bow-xyk-ruji-usdc")],
            )
            .unwrap();

        res.assert_event(
            &Event::new("wasm-rujira-bow/withdraw")
                .add_attributes(vec![("share", "200x/bow-xyk-ruji-usdc")]),
        );

        res.assert_event(&Event::new("transfer").add_attributes(vec![
            ("amount", "200ruji,200usdc"),
            ("recipient", addr.as_str()),
            ("sender", contract.as_str()),
        ]));

        res.assert_event(
            &Event::new("burn")
                .add_attributes(vec![("amount", "200"), ("denom", "x/bow-xyk-ruji-usdc")]),
        );

        // Ensure the strategy state for withdraw has also been committed
        let res = app
            .execute_contract(
                addr.clone(),
                contract.clone(),
                &ExecuteMsg::Withdraw { callback: None },
                &[coin(200, "x/bow-xyk-ruji-usdc")],
            )
            .unwrap();

        res.assert_event(
            &Event::new("wasm-rujira-bow/withdraw")
                .add_attributes(vec![("share", "200x/bow-xyk-ruji-usdc")]),
        );

        res.assert_event(&Event::new("transfer").add_attributes(vec![
            ("amount", "200ruji,200usdc"),
            ("recipient", addr.as_str()),
            ("sender", contract.as_str()),
        ]));

        res.assert_event(
            &Event::new("burn")
                .add_attributes(vec![("amount", "200"), ("denom", "x/bow-xyk-ruji-usdc")]),
        );
    }
}
