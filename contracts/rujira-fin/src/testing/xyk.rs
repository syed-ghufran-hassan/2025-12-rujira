use std::str::FromStr;

use crate::contract::{execute, instantiate, query};
use cosmwasm_std::{coin, coins, Addr, Decimal, Event, Uint128};
use cw_multi_test::{ContractWrapper, Executor};
use rujira_bow::contract as bow_contract;
use rujira_rs::{
    bow::{self, Xyk},
    fin::{
        BookItemResponse, BookResponse, Denoms, ExecuteMsg, InstantiateMsg, OrderResponse,
        OrdersResponse, Price, QueryMsg, Side, SwapRequest, Tick,
    },
    Layer1Asset, TokenMetadata,
};
use rujira_rs_testing::{mock_rujira_app, RujiraApp};

fn setup(app: &mut RujiraApp, owner: &Addr, fees: &Addr) -> (Addr, Addr) {
    let fin_code = Box::new(ContractWrapper::new(execute, instantiate, query));
    let bow_code = Box::new(ContractWrapper::new(
        bow_contract::execute,
        bow_contract::instantiate,
        bow_contract::query,
    ));
    let fin_code_id = app.store_code(fin_code);
    let bow_code_id = app.store_code(bow_code);

    let bow = app
        .instantiate_contract(
            bow_code_id,
            owner.clone(),
            &bow::InstantiateMsg {
                metadata: TokenMetadata {
                    description: "".to_string(),
                    display: "".to_string(),
                    name: "".to_string(),
                    symbol: "".to_string(),
                    uri: None,
                    uri_hash: None,
                },
                strategy: bow::Strategies::Xyk(Xyk::new(
                    "btc".to_string(),
                    "usdc".to_string(),
                    Decimal::permille(1),
                    Uint128::from(10_000u128),
                    Decimal::zero(),
                )),
            },
            &[],
            "bow",
            None,
        )
        .unwrap();

    let fin = app
        .instantiate_contract(
            fin_code_id,
            owner.clone(),
            &InstantiateMsg {
                denoms: Denoms::new("btc", "usdc"),
                oracles: Some([
                    Layer1Asset::new("BTC", "BTC"),
                    Layer1Asset::new("ETH", "USDC-0XA0B86991C6218B36C1D19D4A2E9EB0CE3606EB48"),
                ]),
                market_makers: vec![bow.to_string()],
                tick: Tick::new(8),
                fee_taker: Decimal::from_ratio(15u128, 1000u128),
                fee_maker: Decimal::from_ratio(30u128, 1000u128),
                fee_address: fees.to_string(),
            },
            &[],
            "fin",
            None,
        )
        .unwrap();
    (fin, bow)
}

#[test]
fn mm_book() {
    let mut app = mock_rujira_app();
    let owner = app.api().addr_make("owner");
    let fees = app.api().addr_make("fees");
    let (fin, bow) = setup(&mut app, &owner, &fees);
    let book: BookResponse = app
        .wrap()
        .query_wasm_smart(
            fin.clone(),
            &QueryMsg::Book {
                limit: None,
                offset: None,
            },
        )
        .unwrap();

    assert_eq!(book.base.len(), 0);
    assert_eq!(book.quote.len(), 0);

    app.init_modules(|router, _, storage| {
        router.bank.init_balance(
            storage,
            &owner,
            vec![
                coin(1_000_000_000_000, "btc"),
                coin(1_000_000_000_000, "usdc"),
            ],
        )
    })
    .unwrap();

    // First deposit
    app.execute_contract(
        owner.clone(),
        bow.clone(),
        &bow::ExecuteMsg::Deposit {
            callback: None,
            min_return: None,
        },
        // 2 BTC @ 100k USDC
        &[coin(200_000_000, "btc"), coin(200_000_000_000, "usdc")],
    )
    .unwrap();

    let book: BookResponse = app
        .wrap()
        .query_wasm_smart(
            fin.clone(),
            &QueryMsg::Book {
                limit: None,
                offset: None,
            },
        )
        .unwrap();

    assert_eq!(book.base.len(), 100);
    assert_eq!(book.quote.len(), 100);

    // Just assert spread and direction
    // Quoting accuracy tested in Strategy

    assert_side(
        book.quote,
        vec![
            ("999.00099", 199_800_198u128),
            ("997.00598", 199_600_598u128),
            ("995.01496", 199_400_998u128),
        ],
    );

    assert_side(
        book.base,
        vec![
            ("1001.0061", 199_799u128),
            ("1003.0111", 199_599u128),
            ("1005.0161", 199_400u128),
        ],
    );

    let res = app
        .execute_contract(
            owner.clone(),
            fin.clone(),
            &ExecuteMsg::Swap(SwapRequest::Yolo {
                to: None,
                callback: None,
            }),
            &coins(100_000_000, "usdc"),
        )
        .unwrap();

    // Single trade in first MM pool
    res.assert_event(&Event::new("wasm-rujira-fin/trade").add_attributes(vec![
        ("rate", "1001.0061"),
        ("price", "cosmwasm1uzyszmsnca8euusre35wuqj4el3hyj8jty84kwln7du5stwwxyns2z5hxp:1001.006011041096301783"),
        ("offer", "100000000"),
        ("bid", "99899"),
        ("side", "base"),
    ]));

    // BOW returns the BTC for the trade
    res.assert_event(&Event::new("transfer").add_attributes(vec![
        ("recipient", fin.to_string()),
        ("sender", bow.to_string()),
        ("amount", "99899btc".to_string()),
    ]));

    // Returns the amount less fees to the buyer
    res.assert_event(&Event::new("transfer").add_attributes(vec![
        ("recipient", owner.to_string()),
        ("sender", fin.to_string()),
        ("amount", "98400btc".to_string()),
    ]));

    // And the fees to the fee address
    res.assert_event(&Event::new("transfer").add_attributes(vec![
        ("recipient", fees.to_string()),
        ("sender", fin.to_string()),
        ("amount", "1499btc".to_string()),
    ]));

    #[allow(deprecated)]
    let balances = app.wrap().query_all_balances(bow.clone()).unwrap();
    assert_eq!(
        balances,
        vec![coin(199900101, "btc"), coin(200100000000, "usdc"),]
    );

    let book: BookResponse = app
        .wrap()
        .query_wasm_smart(
            fin.clone(),
            &QueryMsg::Book {
                limit: Some(10),
                offset: None,
            },
        )
        .unwrap();

    assert_eq!(book.base.len(), 10);
    assert_eq!(book.quote.len(), 10);
    assert_side(
        book.quote,
        vec![
            ("1000.0002", 199_900_048u128),
            ("0998.00324", 199_699_451u128),
            ("0996.01022", 199_500_849u128),
        ],
    );

    assert_side(
        book.base,
        vec![
            ("1002.0081", 199_699u128),
            ("1004.0156", 199_499u128),
            ("1006.0231", 199_300u128),
        ],
    );

    // Larger swap, opposite direction
    let res = app
        .execute_contract(
            owner.clone(),
            fin.clone(),
            &ExecuteMsg::Swap(SwapRequest::Yolo {
                to: None,
                callback: None,
            }),
            &coins(500_000, "btc"),
        )
        .unwrap();

    // 3 trade events
    res.assert_event(&Event::new("wasm-rujira-fin/trade").add_attributes(vec![
        ("rate", "1000.0002"),
        ("price", "cosmwasm1uzyszmsnca8euusre35wuqj4el3hyj8jty84kwln7du5stwwxyns2z5hxp:1000.000240120060030015"),
        ("offer", "199900"),
        ("bid", "199900048"),
        ("side", "quote"),
    ]));

    res.assert_event(&Event::new("wasm-rujira-fin/trade").add_attributes(vec![
        ("rate", "998.00324"),
        ("price", "cosmwasm1uzyszmsnca8euusre35wuqj4el3hyj8jty84kwln7du5stwwxyns2z5hxp:998.003243394519712742"),
        ("offer", "200099"),
        ("bid", "199699451"),
        ("side", "quote"),
    ]));
    res.assert_event(&Event::new("wasm-rujira-fin/trade").add_attributes(vec![
        ("rate", "996.01022"),
        ("price", "cosmwasm1uzyszmsnca8euusre35wuqj4el3hyj8jty84kwln7du5stwwxyns2z5hxp:996.010229655516724912"),
        ("offer", "100001"),
        ("bid", "99602018"),
        ("side", "quote"),
    ]));

    // BOW returns the USDC for the trade
    res.assert_event(&Event::new("transfer").add_attributes(vec![
        ("recipient", fin.to_string()),
        ("sender", bow.to_string()),
        ("amount", "499201517usdc".to_string()),
    ]));

    // Returns the amount less fees to the buyer
    res.assert_event(&Event::new("transfer").add_attributes(vec![
        ("recipient", owner.to_string()),
        ("sender", fin.to_string()),
        ("amount", "491713494usdc".to_string()),
    ]));

    // And the fees to the fee address
    res.assert_event(&Event::new("transfer").add_attributes(vec![
        ("recipient", fees.to_string()),
        ("sender", fin.to_string()),
        ("amount", "7488023usdc".to_string()),
    ]));

    // Check large in first dfirection succeeds
    app.execute_contract(
        owner.clone(),
        fin.clone(),
        &ExecuteMsg::Swap(SwapRequest::Yolo {
            to: None,
            callback: None,
        }),
        &coins(600_000_000, "usdc"),
    )
    .unwrap();
}

#[test]
fn combined_book() {
    let mut app = mock_rujira_app();
    let owner = app.api().addr_make("owner");
    let fees = app.api().addr_make("fees");
    let (fin, bow) = setup(&mut app, &owner, &fees);
    app.init_modules(|router, _, storage| {
        router.bank.init_balance(
            storage,
            &owner,
            vec![
                coin(100_000_000_000, "btc"),
                coin(10_000_000_000_000, "usdc"),
            ],
        )
    })
    .unwrap();

    app.execute_contract(
        owner.clone(),
        bow.clone(),
        &bow::ExecuteMsg::Deposit {
            callback: None,
            min_return: None,
        },
        // 2 BTC @ 100k USDC
        &[coin(20_000_000, "btc"), coin(2_000_000_000_000, "usdc")],
    )
    .unwrap();

    let book: BookResponse = app
        .wrap()
        .query_wasm_smart(
            fin.clone(),
            &QueryMsg::Book {
                limit: Some(10),
                offset: None,
            },
        )
        .unwrap();

    assert_side(
        book.quote,
        vec![
            ("99900.099", 1_998_001_997u128),
            ("99700.598", 1_996_005_991u128),
            ("99501.496", 1_994_009_990u128),
        ],
    );
    // Now let's add some user orders

    app.execute_contract(
        owner.clone(),
        fin.clone(),
        &ExecuteMsg::Order((
            vec![
                (
                    Side::Quote,
                    Price::Fixed(Decimal::from_str("99700.598").unwrap()),
                    Some(Uint128::from(50_000_000u128)),
                ),
                (
                    Side::Quote,
                    Price::Fixed(Decimal::from_str("99600").unwrap()),
                    Some(Uint128::from(50_000_000u128)),
                ),
                (
                    Side::Quote,
                    Price::Oracle(0),
                    Some(Uint128::from(50_000_000u128)),
                ),
            ],
            None,
        )),
        &coins(150_000_000, "usdc"),
    )
    .unwrap();

    let book: BookResponse = app
        .wrap()
        .query_wasm_smart(
            fin.clone(),
            &QueryMsg::Book {
                limit: Some(5),
                offset: None,
            },
        )
        .unwrap();

    assert_side(
        book.quote,
        vec![
            ("100000", 50_000_000u128),
            ("99900.099", 1_998_001_997u128),
            ("99700.598", 2_046_005_991u128),
            ("99600", 50_000_000u128),
            ("99501.496", 1_994_009_990u128),
        ],
    );

    // Swap across them all
    let res = app
        .execute_contract(
            owner.clone(),
            fin.clone(),
            &ExecuteMsg::Swap(SwapRequest::Yolo {
                to: None,
                callback: None,
            }),
            &coins(45_500, "btc"),
        )
        .unwrap();

    res.assert_event(&Event::new("wasm-rujira-fin/trade").add_attributes(vec![
        ("rate", "100000"),
        ("price", "oracle:0"),
        ("offer", "500"),
        ("bid", "50000000"),
        ("side", "quote"),
    ]));

    res.assert_event(&Event::new("wasm-rujira-fin/trade").add_attributes(vec![
        ("rate", "99900.099"),
        (
            "price",
            "cosmwasm1uzyszmsnca8euusre35wuqj4el3hyj8jty84kwln7du5stwwxyns2z5hxp:99900.09985",
        ),
        ("offer", "20000"),
        ("bid", "1998001997"),
        ("side", "quote"),
    ]));

    res.assert_event(&Event::new("wasm-rujira-fin/trade").add_attributes(vec![
        ("rate", "99700.598"),
        ("offer", "20521"),
        ("bid", "2046005991"),
        ("price", "fixed:99700.598"),
        ("price", "cosmwasm1uzyszmsnca8euusre35wuqj4el3hyj8jty84kwln7du5stwwxyns2z5hxp:99700.598951048951048951"),
        ("side", "quote"),
    ]));
    res.assert_event(&Event::new("wasm-rujira-fin/trade").add_attributes(vec![
        ("rate", "99600"),
        ("price", "fixed:99600"),
        ("offer", "502"),
        ("bid", "50000000"),
        ("side", "quote"),
    ]));
    res.assert_event(&Event::new("wasm-rujira-fin/trade").add_attributes(vec![
        ("rate", "99501.496"),
        ("price", "cosmwasm1uzyszmsnca8euusre35wuqj4el3hyj8jty84kwln7du5stwwxyns2z5hxp:99501.496506986027944111"),
        ("offer", "3977"),
        ("bid", "395717451"),
        ("side", "quote"),
    ]));

    // Check orders and withdraw
    let order: OrderResponse = app
        .wrap()
        .query_wasm_smart(
            fin.to_string(),
            &QueryMsg::Order((
                owner.to_string(),
                Side::Quote,
                Price::Fixed(Decimal::from_str("99600").unwrap()),
            )),
        )
        .unwrap();
    assert_eq!(order.filled, Uint128::from(502u128));
    assert_eq!(order.remaining, Uint128::zero());
    let orders: OrdersResponse = app
        .wrap()
        .query_wasm_smart(
            fin.to_string(),
            &QueryMsg::Orders {
                owner: owner.to_string(),
                side: None,
                offset: None,
                limit: None,
            },
        )
        .unwrap();
    assert_eq!(orders.orders[0].clone(), order);

    let res = app
        .execute_contract(
            owner.clone(),
            fin.clone(),
            &ExecuteMsg::Order((
                vec![(
                    Side::Quote,
                    Price::Fixed(Decimal::from_str("99600").unwrap()),
                    Some(Uint128::zero()),
                )],
                None,
            )),
            &coins(45_500, "btc"),
        )
        .unwrap();

    res.assert_event(
        &Event::new("wasm-rujira-fin/order.withdraw").add_attributes(vec![
            ("owner", owner.as_str()),
            ("side", "quote"),
            ("price", "fixed:99600"),
            ("amount", "502"),
        ]),
    );

    res.assert_event(&Event::new("transfer").add_attributes(vec![
        ("recipient", owner.as_str()),
        ("sender", fin.as_str()),
        ("amount", "45986btc"),
    ]));

    res.assert_event(&Event::new("transfer").add_attributes(vec![
        ("recipient", fees.as_str()),
        ("sender", fin.as_str()),
        ("amount", "16btc"),
    ]));
}

#[test]
fn test_arbitrage_quote_single() {
    // Test arbitrage with a single match
    let mut app = mock_rujira_app();
    let owner = app.api().addr_make("owner");
    let fees = app.api().addr_make("fees");
    let (fin, bow) = setup(&mut app, &owner, &fees);
    app.init_modules(|router, _, storage| {
        router.bank.init_balance(
            storage,
            &owner,
            vec![
                coin(1_000_000_000_000, "btc"),
                coin(1_000_000_000_000, "usdc"),
            ],
        )
    })
    .unwrap();

    app.execute_contract(
        owner.clone(),
        bow.clone(),
        &bow::ExecuteMsg::Deposit {
            callback: None,
            min_return: None,
        },
        // 2 BTC @ 100k USDC
        &[coin(200_000_000, "btc"), coin(200_000_000_000, "usdc")],
    )
    .unwrap();

    let book: BookResponse = app
        .wrap()
        .query_wasm_smart(
            fin.clone(),
            &QueryMsg::Book {
                limit: Some(10),
                offset: None,
            },
        )
        .unwrap();

    assert_side(
        book.quote,
        vec![
            ("999.00099", 199_800_198u128),
            ("997.00598", 199_600_598u128),
            ("995.01496", 199_400_998u128),
        ],
    );

    // Here we have an arbitrage profit that's captured in USDC, but an local orderbook that only has a
    // single BTC in local balances - no USDC
    // Options:
    // 1: Execute the mm swap for arbitrage and send fees in a callback
    // 2: Assume there is a float on the contract (including balances from user orders) and just check that the Arb MM swap is executed correctly
    // 3: Ensure arbitrage profit is captured in the local token, also ensure arb swap is executed

    // We have a maker maker order buying with
    // 199_800_199 at 999.00099  and a user order selling
    // 10_000 at 998

    // Buy 10000 BTC off the market maker for 9990009 USDC
    // Sell 10000 BTC to the buyer for 9,980,000 USDC
    // Profit 10,009 USDC
    // Request the full 10000 swap from the market maker
    // in exchange for 9990010 USDC - sell order value (9,980,000) + profit 10,009
    let res = app
        .execute_contract(
            owner.clone(),
            fin.clone(),
            &ExecuteMsg::Order((
                vec![(
                    Side::Base,
                    Price::Fixed(Decimal::from_str("998").unwrap()),
                    Some(Uint128::from(10_000u128)),
                )],
                None,
            )),
            &coins(10_000, "btc"),
        )
        .unwrap();

    // Ensure BOW has returned the funds
    res.assert_event(&Event::new("transfer").add_attributes(vec![
        ("recipient", fin.to_string()),
        ("sender", bow.to_string()),
        ("amount", "9990009usdc".to_string()),
    ]));

    // Ensure owner receives swap output
    res.assert_event(&Event::new("transfer").add_attributes(vec![
        ("sender", fin.as_str()),
        ("recipient", owner.as_str()),
        ("amount", "9840158usdc"),
    ]));
}

#[test]
fn test_arbitrage_quote_multi() {
    // Now execute in the opposite direction, consuming:
    // - multiple market maker pools with a single order
    // - a single market maker pool with multiple orders

    let mut app = mock_rujira_app();
    let owner = app.api().addr_make("owner");
    let fees = app.api().addr_make("fees");
    let (fin, bow) = setup(&mut app, &owner, &fees);
    app.init_modules(|router, _, storage| {
        router.bank.init_balance(
            storage,
            &owner,
            vec![
                coin(1_000_000_000_000, "btc"),
                coin(1_000_000_000_000, "usdc"),
            ],
        )
    })
    .unwrap();

    app.execute_contract(
        owner.clone(),
        bow.clone(),
        &bow::ExecuteMsg::Deposit {
            callback: None,
            min_return: None,
        },
        // 2 BTC @ 100k USDC
        &[coin(200_000_000, "btc"), coin(200_000_000_000, "usdc")],
    )
    .unwrap();

    let book: BookResponse = app
        .wrap()
        .query_wasm_smart(
            fin.clone(),
            &QueryMsg::Book {
                limit: Some(10),
                offset: None,
            },
        )
        .unwrap();

    assert_side(
        book.base,
        vec![
            ("1001.0061", 199_799u128),
            ("1003.0111", 199_599u128),
            ("1005.0161", 199_400u128),
            ("1007.0312", 199_200u128),
            ("1009.0462", 199_001u128),
            ("1011.0613", 198_803u128),
        ],
    );

    let res = app
        .execute_contract(
            owner.clone(),
            fin.clone(),
            &ExecuteMsg::Order((
                vec![(
                    Side::Quote,
                    Price::Fixed(Decimal::from_str("1004").unwrap()),
                    Some(Uint128::from(600_000_000u128)),
                )],
                None,
            )),
            &coins(600_000_000u128, "usdc"),
        )
        .unwrap();

    // Ensure BOW has returned the funds
    res.assert_event(&Event::new("transfer").add_attributes(vec![
        ("recipient", fin.to_string()),
        ("sender", bow.to_string()),
        ("amount", "399398btc".to_string()),
    ]));

    // Now a small one that should take profit in BTC
    let book: BookResponse = app
        .wrap()
        .query_wasm_smart(
            fin.clone(),
            &QueryMsg::Book {
                limit: Some(10),
                offset: None,
            },
        )
        .unwrap();

    assert_side(book.base, vec![("1005.0161", 199_400u128)]);

    let res = app
        .execute_contract(
            owner.clone(),
            fin.clone(),
            &ExecuteMsg::Order((
                vec![(
                    Side::Quote,
                    Price::Fixed(Decimal::from_str("1006").unwrap()),
                    Some(Uint128::from(10_000_000u128)),
                )],
                None,
            )),
            &coins(10_000_000u128, "usdc"),
        )
        .unwrap();

    // Ensure BOW has returned the funds
    res.assert_event(&Event::new("transfer").add_attributes(vec![
        ("recipient", fin.to_string()),
        ("sender", bow.to_string()),
        ("amount", "9950btc".to_string()),
    ]));

    // And that the owner receives them, less fees
    res.assert_event(&Event::new("transfer").add_attributes(vec![
        ("sender", fin.to_string()),
        ("recipient", owner.to_string()),
        ("amount", "9800btc".to_string()),
    ]));
}

fn assert_side(side: Vec<BookItemResponse>, items: Vec<(&str, u128)>) {
    assert!(side.len().ge(&items.len()));
    for ((price, amount), item) in items.iter().zip(side) {
        let price = Decimal::from_str(price).unwrap();
        let amount = Uint128::from(*amount);
        assert_eq!(price, item.price);
        assert_eq!(amount, item.total);
    }
}
