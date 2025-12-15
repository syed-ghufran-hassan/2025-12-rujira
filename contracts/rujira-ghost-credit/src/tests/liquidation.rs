use std::str::FromStr;

use cosmwasm_std::{coin, coins, Addr, Binary, Decimal, Uint128};
use cw_multi_test::Executor;
use rujira_bow::mock::Bow;
use rujira_fin::mock::Fin;
use rujira_ghost_vault::mock::GhostVault;
use rujira_rs::{
    fin::{self, Denoms},
    ghost::{
        credit::{AccountResponse, Collateral, CollateralResponse, Debt},
        vault::{BorrowerResponse, DelegateResponse},
    },
};
use rujira_rs_testing::{mock_rujira_app, RujiraApp};

use crate::{
    mock::GhostCredit,
    tests::support::{BTC, ETH, USDC, USDT},
};

struct Ctx {
    fin_btc_usdc: Fin,
    fin_btc_usdt: Fin,
    fin_eth_usdc: Fin,
    // fin_eth_usdt: Fin,
    ghost_credit: GhostCredit,
    account: AccountResponse,
}

fn setup(app: &mut RujiraApp, owner: &Addr) -> Ctx {
    let bow_btc_usdc = Bow::create(app, owner, BTC, USDC, Uint128::from(1000u128));
    let bow_btc_usdt = Bow::create(app, owner, BTC, USDT, Uint128::from(1000u128));
    let bow_eth_usdc = Bow::create(app, owner, ETH, USDC, Uint128::from(1000u128));
    let bow_eth_usdt = Bow::create(app, owner, ETH, USDT, Uint128::from(1000u128));

    let fin_btc_usdc = Fin::create(
        app,
        owner,
        owner,
        &[bow_btc_usdc.addr().clone()],
        Denoms::new(BTC, USDC),
        None,
    );
    let fin_btc_usdt = Fin::create(
        app,
        owner,
        owner,
        &[bow_btc_usdt.addr().clone()],
        Denoms::new(BTC, USDT),
        None,
    );
    let fin_eth_usdc = Fin::create(
        app,
        owner,
        owner,
        &[bow_eth_usdc.addr().clone()],
        Denoms::new(ETH, USDC),
        None,
    );
    let _fin_eth_usdt = Fin::create(
        app,
        owner,
        owner,
        &[bow_eth_usdt.addr().clone()],
        Denoms::new(ETH, USDT),
        None,
    );

    // Deposit into BOW pools at current oracle prices
    app.init_modules(|router, _api, storage| {
        router.stargate.with_prices(vec![
            ("BTC", Decimal::from_str("111000").unwrap()),
            ("ETH", Decimal::from_str("3500").unwrap()),
            ("USDC", Decimal::from_str("0.9995").unwrap()),
            ("USDT", Decimal::from_str("1.0003").unwrap()),
        ]);
        router
            .bank
            .init_balance(
                storage,
                owner,
                vec![
                    coin(10000000000000000, BTC),
                    coin(10000000000000000, ETH),
                    coin(10000000000000000, USDC),
                    coin(10000000000000000, USDT),
                ],
            )
            .unwrap();
    });

    bow_btc_usdc
        .deposit(
            app,
            owner,
            &[coin(1000000000, BTC), coin(111445783132530, USDC)],
            None,
        )
        .unwrap();

    bow_btc_usdt
        .deposit(
            app,
            owner,
            &[coin(1000000000, BTC), coin(110667996011964, USDT)],
            None,
        )
        .unwrap();

    bow_eth_usdc
        .deposit(
            app,
            owner,
            &[coin(10000000000, ETH), coin(35017508754377, USDC)],
            None,
        )
        .unwrap();

    bow_eth_usdt
        .deposit(
            app,
            owner,
            &[coin(10000000000, ETH), coin(34989503149055, USDT)],
            None,
        )
        .unwrap();

    let ghost_vault_btc = GhostVault::create(app, owner, BTC);
    let ghost_vault_eth = GhostVault::create(app, owner, ETH);
    let ghost_vault_usdc = GhostVault::create(app, owner, USDC);
    let ghost_vault_usdt = GhostVault::create(app, owner, USDT);
    let ghost_credit = GhostCredit::create(app, owner, owner);

    ghost_credit.set_collateral(app, BTC, "0.8");
    ghost_credit.set_collateral(app, ETH, "0.7");
    ghost_credit.set_collateral(app, USDC, "0.9");
    ghost_credit.set_collateral(app, USDT, "0.9");

    ghost_credit.set_vault(app, &ghost_vault_btc);
    ghost_credit.set_vault(app, &ghost_vault_eth);
    ghost_credit.set_vault(app, &ghost_vault_usdc);
    ghost_credit.set_vault(app, &ghost_vault_usdt);

    ghost_vault_btc
        .deposit(app, owner, 111000000000000, BTC)
        .unwrap();
    ghost_vault_eth
        .deposit(app, owner, 3500000000000, ETH)
        .unwrap();
    ghost_vault_usdc
        .deposit(app, owner, 100000000000000, USDC)
        .unwrap();
    ghost_vault_usdt
        .deposit(app, owner, 100000000000000, USDT)
        .unwrap();

    ghost_vault_btc
        .set_borrower(app, ghost_credit.addr().as_str(), Uint128::MAX)
        .unwrap();
    ghost_vault_eth
        .set_borrower(app, ghost_credit.addr().as_str(), Uint128::MAX)
        .unwrap();
    ghost_vault_usdc
        .set_borrower(app, ghost_credit.addr().as_str(), Uint128::MAX)
        .unwrap();
    ghost_vault_usdt
        .set_borrower(app, ghost_credit.addr().as_str(), Uint128::MAX)
        .unwrap();

    ghost_credit.create_account(app, owner, "", "", Binary::new(vec![0]));
    let account = ghost_credit.query_account(
        app,
        &Addr::unchecked("cosmwasm10x7mxuxfufc9naqthz6zpc07vzfvyppx2aamqk0ljdkwnl6d48qq3xx860"),
    );

    Ctx {
        account,
        fin_btc_usdc,
        fin_btc_usdt,
        fin_eth_usdc,
        // fin_eth_usdt,
        ghost_credit,
    }
}

#[test]
fn liquidation_preference_order() {
    let mut app = mock_rujira_app();
    let owner = app.api().addr_make("owner");
    let ctx = setup(&mut app, &owner);

    // 0.1BTC + 2 ETH
    app.send_tokens(
        owner.clone(),
        ctx.account.account.clone(),
        &[coin(10000000, BTC), coin(200000000, ETH)],
    )
    .unwrap();

    let account = ctx.ghost_credit.query_account(&app, &ctx.account.account);

    // Total value adjusted = 888000000000 + 490000000000 = $13,780.00000000
    ctx.ghost_credit
        .account_borrow(&mut app, &account, 1309100000000, USDC)
        .unwrap();

    ctx.ghost_credit
        .account_send(&mut app, &account, 1309100000000, USDC, &owner)
        .unwrap();
    ctx.ghost_credit
        .account_preference_order(&mut app, &account, BTC, Some(ETH))
        .unwrap();

    app.init_modules(|router, _api, _storage| {
        router.stargate.with_prices(vec![
            ("BTC", Decimal::from_str("104450").unwrap()),
            ("ETH", Decimal::from_str("3225").unwrap()),
        ]);
    });

    let account = ctx.ghost_credit.query_account(&app, &ctx.account.account);

    assert!(account.ltv > Decimal::one());
    let account = ctx.ghost_credit.query_account(&app, &ctx.account.account);
    // Verify that liquidation BTC fails

    let err = ctx
        .ghost_credit
        .liquidate_execute_repay(
            &mut app,
            &account,
            ctx.fin_btc_usdc.addr(),
            fin::ExecuteMsg::Swap(fin::SwapRequest::Yolo {
                to: None,
                callback: None,
            }),
            coins(1000000, BTC),
            USDC,
        )
        .unwrap_err();

    let err = err.root_cause();
    let msg = format!("{err:?}");
    assert!(msg.contains("LiquidationPreferenceOrder"));
    assert!(msg.contains("coin: Coin { 1000000 \"btc-btc\" }"));
    assert!(msg.contains("before: \"eth-eth\""));

    ctx.ghost_credit
        .liquidate_execute_repay(
            &mut app,
            &account,
            ctx.fin_eth_usdc.addr(),
            fin::ExecuteMsg::Swap(fin::SwapRequest::Yolo {
                to: None,
                callback: None,
            }),
            coins(30000000, ETH),
            USDC,
        )
        .unwrap();

    let account = ctx.ghost_credit.query_account(&app, &ctx.account.account);
    assert_eq!(
        account.ltv,
        Decimal::from_str("0.989750185753791901").unwrap()
    );
    assert_eq!(
        account.collaterals[0].clone().collateral,
        Collateral::Coin(coin(10000000, BTC))
    );
    assert_eq!(
        account.collaterals[1].clone().collateral,
        Collateral::Coin(coin(170000000, ETH))
    );
    assert_eq!(
        account.debts[0].debt,
        Debt::from(DelegateResponse {
            borrower: BorrowerResponse {
                addr: ctx.ghost_credit.addr().to_string(),
                denom: USDC.to_string(),
                limit: Uint128::from(340282366920938463463374607431768211455u128),
                current: Uint128::from(1207480372940u128),
                shares: Uint128::from(1207480372940u128),
                available: Uint128::from(98792519627060u128),
            },
            addr: ctx.account.account.to_string(),
            current: Uint128::from(1207480372940u128),
            shares: Uint128::from(1207480372940u128),
        })
    );
    // btc value adjusted : 835600000000
    // eth value adjusted : 383775000000
    // drop price so that we can liquidate all eth and then btc
    let account = ctx.ghost_credit.query_account(&app, &ctx.account.account);

    app.init_modules(|router, _api, _storage| {
        router.stargate.with_prices(vec![
            ("BTC", Decimal::from_str("81335").unwrap()),
            ("ETH", Decimal::from_str("2000").unwrap()),
        ]);
    });

    ctx.ghost_credit
        .liquidate_execute_repay(
            &mut app,
            &account,
            ctx.fin_eth_usdc.addr(),
            fin::ExecuteMsg::Swap(fin::SwapRequest::Yolo {
                to: None,
                callback: None,
            }),
            coins(170000000, ETH),
            USDC,
        )
        .unwrap();
    let account = ctx.ghost_credit.query_account(&app, &ctx.account.account);

    assert_eq!(
        account.collaterals,
        vec![CollateralResponse {
            collateral: Collateral::Coin(coin(10000000, BTC),),
            value_full: Decimal::from_str("813350000000").unwrap(),
            value_adjusted: Decimal::from_str("650680000000").unwrap(),
        },]
    );

    // Finally drive the BTC price down and liquidate the BTC
    app.init_modules(|router, _api, _storage| {
        router
            .stargate
            .with_prices(vec![("BTC", Decimal::from_str("78335").unwrap())]);
    });

    ctx.ghost_credit
        .liquidate_execute_repay(
            &mut app,
            &account,
            ctx.fin_btc_usdc.addr(),
            fin::ExecuteMsg::Swap(fin::SwapRequest::Yolo {
                to: None,
                callback: None,
            }),
            coins(500000, BTC),
            USDC,
        )
        .unwrap();
    let account = ctx.ghost_credit.query_account(&app, &ctx.account.account);

    assert_eq!(
        account.collaterals,
        vec![CollateralResponse {
            collateral: Collateral::Coin(coin(9500000, BTC),),
            value_full: Decimal::from_str("744182500000").unwrap(),
            value_adjusted: Decimal::from_str("595346000000").unwrap(),
        },]
    );
}

#[test]
fn incorrect_return_asset() {
    let mut app = mock_rujira_app();
    let owner = app.api().addr_make("owner");
    let ctx = setup(&mut app, &owner);

    // 0.1BTC + 2 ETH
    app.send_tokens(
        owner.clone(),
        ctx.account.account.clone(),
        &[coin(10000000, BTC), coin(200000000, ETH)],
    )
    .unwrap();

    let account = ctx.ghost_credit.query_account(&app, &ctx.account.account);

    // Total value adjusted = 888000000000 + 490000000000 = $13,780.00000000
    ctx.ghost_credit
        .account_borrow(&mut app, &account, 1309100000000, USDC)
        .unwrap();

    ctx.ghost_credit
        .account_send(&mut app, &account, 1309100000000, USDC, &owner)
        .unwrap();

    app.init_modules(|router, _api, _storage| {
        router.stargate.with_prices(vec![
            ("BTC", Decimal::from_str("104450").unwrap()),
            ("ETH", Decimal::from_str("3225").unwrap()),
        ]);
    });

    let account = ctx.ghost_credit.query_account(&app, &ctx.account.account);

    assert!(account.ltv > Decimal::one());
    let account = ctx.ghost_credit.query_account(&app, &ctx.account.account);
    let err = ctx
        .ghost_credit
        .liquidate_execute_repay(
            &mut app,
            &account,
            ctx.fin_btc_usdt.addr(),
            fin::ExecuteMsg::Swap(fin::SwapRequest::Yolo {
                to: None,
                callback: None,
            }),
            coins(1000000, BTC),
            USDT,
        )
        .unwrap_err();

    let err = err.root_cause();
    let msg = format!("{err:?}");
    // Zero Debt Pool in USDT
    assert_eq!(msg, "ZeroDebt");
}
