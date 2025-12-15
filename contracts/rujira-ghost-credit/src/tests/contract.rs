use std::str::FromStr;

use cosmwasm_std::{
    coin, coins, to_json_binary, Addr, BankMsg, Binary, CosmosMsg, Decimal, Event, Uint128, WasmMsg,
};
use cw_multi_test::Executor;
use rujira_bow::mock::Bow;
use rujira_ghost_vault::mock::GhostVault;
use rujira_rs::{
    bow,
    ghost::credit::{AccountMsg, AccountResponse, AccountsResponse, Collateral, ExecuteMsg},
};
use rujira_rs_testing::{mock_rujira_app, RujiraApp};

use crate::{
    mock::GhostCredit,
    tests::support::{BTC, ETH, USDC},
};

#[test]
fn account_lifecycle() {
    let mut app = mock_rujira_app();
    app.init_modules(|router, _, _| {
        router.stargate.with_prices(vec![
            ("USDC", Decimal::from_str("1.00034562").unwrap()),
            ("BTC", Decimal::from_str("1.56").unwrap()),
        ]);
    });

    let owner = app.api().addr_make("owner");
    let fees = app.api().addr_make("fee");
    let credit = GhostCredit::create(&mut app, &owner, &fees);
    let account = create(&mut app, &credit, owner.clone());
    fund(&mut app, &credit, &account);
    configure(&mut app, &credit);
    let res = credit.query_accounts(&app, &owner, None);
    assert_eq!(res.accounts[1].account, account.account);
    let collateral = res.accounts[1].collaterals[0].clone();
    assert_eq!(collateral.collateral, Collateral::Coin(coin(1000, USDC)));
    assert_eq!(
        collateral.value_adjusted,
        Decimal::from_str("900.311058").unwrap()
    );
    assert_eq!(
        collateral.value_full,
        Decimal::from_str("1000.34562").unwrap()
    );
    let vault = GhostVault::create(&mut app, &owner, USDC);

    vault
        .set_borrower(&mut app, credit.addr().as_str(), Uint128::MAX)
        .unwrap();
    credit.set_vault(&mut app, &vault);
    vault.deposit(&mut app, &owner, 2000, USDC).unwrap();

    borrow_usdc(&mut app, &credit, &account);
    let bow = Bow::create(&mut app, &owner, BTC, USDC, Uint128::from(10000u128));
    bow.deposit(
        &mut app,
        &owner,
        &[coin(1000000, BTC), coin(1560000, USDC)],
        None,
    )
    .unwrap();

    swap_collateral(&mut app, &credit, &account, &bow);

    // We now have 1000.34562 of debt and 1280 BTC with a CR of 0.9.
    // BTC needs to drop to 0.868 for the account to be unsafe
    app.init_modules(|router, _, _| {
        router
            .stargate
            .with_price("BTC", Decimal::from_str("0.85").unwrap());
    });

    liquidate(&mut app, &credit, &account, &bow);

    let next = credit.query_next(&app, &owner, Binary::new(vec![1]));
    assert_eq!(
        next,
        Addr::unchecked("cosmwasm1pmlhxn2c0xdmntx55vjqpe4rvdlcmgxl4pchcxskytky3pxk9gkslu4pal")
    );

    let all = credit.query_all_accounts(&app, None);
    assert_eq!(all.accounts.len(), 2);
    let page = credit.query_all_accounts(&app, Some(all.accounts[0].clone().account));
    assert_eq!(page.accounts.len(), 1);
    close(&mut app, &credit, &account);
}

fn create(app: &mut RujiraApp, credit: &GhostCredit, owner: Addr) -> AccountResponse {
    let res = credit.create_account(app, &owner, "", "", Binary::new(vec![0]));
    res.assert_event(
        &Event::new("wasm-rujira-ghost-credit/account.create").add_attributes(vec![
            ("owner", owner.as_str()),
            (
                "address",
                "cosmwasm1jgtw6zkp5fcv9htjrxwye5fhmefeyt9fet76eu6q8kqka0lsldwqz6wz4l",
            ),
        ]),
    );
    credit.create_account(app, &owner, "", "", Binary::new(vec![1]));
    let accounts: AccountsResponse = credit.query_accounts(app, &owner, None);

    assert_eq!(
        accounts.accounts[0].account,
        Addr::unchecked("cosmwasm1pmlhxn2c0xdmntx55vjqpe4rvdlcmgxl4pchcxskytky3pxk9gkslu4pal")
    );
    assert_eq!(
        accounts.accounts[1].account,
        Addr::unchecked("cosmwasm1jgtw6zkp5fcv9htjrxwye5fhmefeyt9fet76eu6q8kqka0lsldwqz6wz4l")
    );
    accounts.accounts[1].clone()
}

fn fund(app: &mut RujiraApp, credit: &GhostCredit, account: &AccountResponse) {
    app.init_modules(|router, _api, storage| {
        router.bank.init_balance(
            storage,
            &account.owner,
            vec![
                coin(1000000000000, BTC),
                coin(1000000000000, ETH),
                coin(1000000000000, USDC),
            ],
        )
    })
    .unwrap();

    app.send_tokens(
        account.owner.clone(),
        account.account.clone(),
        &coins(1000, USDC),
    )
    .unwrap();

    let res = credit.query_accounts(app, &account.owner, None);
    // Nothing whitelisted, no collaterals counted
    assert_eq!(res.accounts[0].collaterals, vec![]);
    assert_eq!(res.accounts[0].debts, vec![]);
}

fn configure(app: &mut RujiraApp, credit: &GhostCredit) {
    credit.set_collateral(app, USDC, "0.9");
    credit.set_collateral(app, BTC, "0.9");
    let res = credit.query_config(app);
    assert_eq!(
        res.collateral_ratios.get(USDC),
        Some(&Decimal::from_str("0.9").unwrap())
    );
}

fn borrow_usdc(app: &mut RujiraApp, credit: &GhostCredit, account: &AccountResponse) {
    let res = credit.account_borrow(app, account, 1000, USDC).unwrap();
    res.assert_event(
        &Event::new("wasm-rujira-ghost-credit/account.msg").add_attributes(vec![
            ("owner", account.owner.as_str()),
            (
                "address",
                "cosmwasm1jgtw6zkp5fcv9htjrxwye5fhmefeyt9fet76eu6q8kqka0lsldwqz6wz4l",
            ),
        ]),
    );

    res.assert_event(
        &Event::new("wasm-rujira-ghost-credit/account.msg/borrow").add_attributes(vec![(
            "amount",
            "1000eth-usdc-0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
        )]),
    );

    res.assert_event(
        &Event::new("wasm-rujira-ghost-vault/borrow").add_attributes(vec![
            ("borrower", credit.addr().as_str()),
            (
                "delegate",
                "cosmwasm1jgtw6zkp5fcv9htjrxwye5fhmefeyt9fet76eu6q8kqka0lsldwqz6wz4l",
            ),
            ("amount", "1000"),
        ]),
    );

    let res = credit.query_accounts(app, &account.owner, None);
    let debt = res.accounts[1].debts[0].clone();
    assert_eq!(debt.value, Decimal::from_str("1000.34562").unwrap());

    // Ensure idx 1 can't borrow anything - no collateral
    credit
        .account_borrow(app, &res.accounts[0], 1000, USDC)
        .unwrap_err();

    credit
        .account_borrow(app, &res.accounts[0], 1, USDC)
        .unwrap_err();
}

fn swap_collateral(
    app: &mut RujiraApp,
    credit: &GhostCredit,
    account: &AccountResponse,
    bow: &Bow,
) {
    // We have 2000 USDC collateral. Swap it to BTC
    credit
        .account_execute(
            app,
            account,
            bow.addr(),
            &bow::ExecuteMsg::Swap {
                min_return: coin(1280, BTC),
                to: None,
                callback: None,
            },
            coins(2000, USDC),
        )
        .unwrap();
    let res = credit.query_accounts(app, &account.owner, None);
    let account = res.accounts[1].clone();
    assert_eq!(account.collaterals.len(), 1);
    assert_eq!(account.debts.len(), 1);
    let collateral = account.collaterals[0].clone();
    let debt = account.debts[0].clone();
    assert_eq!(collateral.collateral, Collateral::Coin(coin(1280, BTC)));
    assert_eq!(
        collateral.value_adjusted,
        Decimal::from_str("1797.12").unwrap()
    );
    assert_eq!(collateral.value_full, Decimal::from_str("1996.8").unwrap());
    assert_eq!(debt.value, Decimal::from_str("1000.34562").unwrap());
}

fn liquidate(app: &mut RujiraApp, credit: &GhostCredit, account: &AccountResponse, bow: &Bow) {
    // Oracle price is now 0.85. Use this for min_return to emulate the pool having moved

    // Trying to liquidate anything should fail with no repay
    credit
        .liquidate_execute(
            app,
            account,
            bow.addr(),
            &bow::ExecuteMsg::Swap {
                min_return: coin(85, USDC),
                to: None,
                callback: None,
            },
            coins(100, BTC),
        )
        .unwrap_err();

    // Trying to liquidate everything should fail on ratio change
    credit
        .liquidate_execute_repay(
            app,
            account,
            bow.addr(),
            &bow::ExecuteMsg::Swap {
                min_return: coin(1088, USDC),
                to: None,
                callback: None,
            },
            coins(1280, BTC),
            USDC,
        )
        .unwrap_err();

    // Ensure we can't under-repay
    credit
        .liquidate_execute_repay(
            app,
            account,
            bow.addr(),
            &bow::ExecuteMsg::Swap {
                min_return: coin(544, USDC),
                to: None,
                callback: None,
            },
            coins(640, BTC),
            USDC,
        )
        .unwrap_err();

    let res = credit
        .liquidate_execute_repay(
            app,
            account,
            bow.addr(),
            &bow::ExecuteMsg::Swap {
                min_return: coin(544, USDC),
                to: None,
                callback: None,
            },
            coins(660, BTC),
            USDC,
        )
        .unwrap();

    res.assert_event(
        &Event::new("wasm-rujira-ghost-credit/account.liquidate").add_attributes(vec![
            ("owner", account.owner.as_str()),
            ("address", account.account.as_str()),
            ("caller", account.owner.as_str()),
        ]),
    );

    res.assert_event(
            &Event::new("wasm-rujira-ghost-credit/liquidate.msg/execute").add_attributes(vec![
                ("contract_addr", bow.addr().as_str()),
                ("msg", "eyJzd2FwIjp7Im1pbl9yZXR1cm4iOnsiZGVub20iOiJldGgtdXNkYy0weGEwYjg2OTkxYzYyMThiMzZjMWQxOWQ0YTJlOWViMGNlMzYwNmViNDgiLCJhbW91bnQiOiI1NDQifSwidG8iOm51bGwsImNhbGxiYWNrIjpudWxsfX0="),
                ("funds", "btc-btc660"),
            ]),
        );

    res.assert_event(&Event::new("wasm-rujira-bow/swap").add_attributes(vec![
        ("offer", "660btc-btc"),
        (
            "ask",
            "544eth-usdc-0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
        ),
    ]));

    res.assert_event(&Event::new("transfer").add_attributes(vec![
        ("recipient", account.account.as_str()),
        ("sender", bow.addr().as_str()),
        (
            "amount",
            "544eth-usdc-0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
        ),
    ]));

    // Ensure the reply is handled
    res.assert_event(&Event::new("reply").add_attributes(vec![("mode", "handle_success")]));

    res.assert_event(
        &Event::new("wasm-rujira-ghost-vault/repay").add_attributes(vec![
            ("borrower", credit.addr().as_str()),
            ("delegate", account.account.as_str()),
            ("amount", "537"),
        ]),
    );

    // Test it fails if re-attempted

    credit
        .liquidate_execute_repay(
            app,
            account,
            bow.addr(),
            &bow::ExecuteMsg::Swap {
                min_return: coin(544, USDC),
                to: None,
                callback: None,
            },
            coins(640, BTC),
            USDC,
        )
        .unwrap_err();
}

fn close(app: &mut RujiraApp, credit: &GhostCredit, account: &AccountResponse) {
    let repay = coin(463, USDC);
    app.execute_multi(
        account.owner.clone(),
        vec![
            CosmosMsg::Bank(BankMsg::Send {
                to_address: account.account.to_string(),
                amount: vec![repay.clone()],
            }),
            CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: credit.addr().to_string(),
                funds: vec![],
                msg: to_json_binary(&ExecuteMsg::Account {
                    addr: account.account.to_string(),
                    msgs: vec![
                        AccountMsg::Repay(repay),
                        AccountMsg::Send {
                            to_address: account.owner.to_string(),
                            funds: coins(620, BTC),
                        },
                    ],
                })
                .unwrap(),
            }),
        ],
    )
    .unwrap();

    let res = credit.query_account(app, &account.account);

    assert_eq!(res.collaterals.len(), 0);
    assert_eq!(res.debts.len(), 0);
    assert_eq!(res.ltv, Decimal::zero());
}
