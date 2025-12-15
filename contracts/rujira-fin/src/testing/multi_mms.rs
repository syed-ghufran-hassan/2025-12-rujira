#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use cosmwasm_std::{coin, coins, Addr, Decimal, Event, Uint128};
    use cw_multi_test::Executor;

    use rujira_bow::mock::Bow;
    use rujira_ghost_vault::mock::GhostVault;
    use rujira_rs::{
        fin::{Denoms, ExecuteMsg, Price, Side, SwapRequest},
        Layer1Asset,
    };
    use rujira_rs_testing::{mock_rujira_app, RujiraApp};
    use rujira_thorchain_swap::mock::ThorchainSwap;

    use crate::mock::Fin;

    /// Setup a comprehensive multi-market maker test environment with:
    /// - Ghost vault for BTC
    /// - Ghost vault for RUNE
    /// - Thorchain swap contract using the ghost vault
    /// - BOW contract RUNE - BTC
    /// - FIN market with both thorchain swap and BOW as market makers
    fn setup(
        app: &mut RujiraApp,
        owner: &Addr,
        fees: &Addr,
    ) -> (Fin, GhostVault, GhostVault, ThorchainSwap, Bow) {
        // 1. Setup Ghost Vaults for BTC and RUNE
        let btc_ghost = GhostVault::create(app, owner, "btc-btc");
        let rune_ghost = GhostVault::create(app, owner, "rune");

        // 2. Setup Thorchain Swap with both ghost vaults
        let tc = ThorchainSwap::create(app, owner);

        // Configure thorchain swap to use both vaults
        tc.set_vault(app, "btc-btc", Some(&btc_ghost)).unwrap();
        tc.set_vault(app, "rune", Some(&rune_ghost)).unwrap();

        // Set borrower permissions for thorchain swap
        btc_ghost
            .set_borrower(app, tc.addr().as_str(), Uint128::MAX)
            .unwrap();
        rune_ghost
            .set_borrower(app, tc.addr().as_str(), Uint128::MAX)
            .unwrap();

        // 3. Setup BOW contract
        let bow = Bow::create(app, owner, "btc-btc", "rune", Uint128::from(10_000u128));

        // 4. Setup FIN market with both market makers
        let fin = Fin::create(
            app,
            owner,
            fees,
            &[tc.addr().clone(), bow.addr().clone()],
            Denoms::new("rune", "btc-btc"),
            Some([
                Layer1Asset::new("THOR", "RUNE"),
                Layer1Asset::new("BTC", "BTC"),
            ]),
        );

        // Configure thorchain swap to allow fin to use it as a market maker
        tc.set_market(app, fin.addr().as_str(), true).unwrap();

        (fin, btc_ghost, rune_ghost, tc, bow)
    }

    #[test]
    fn test_swap_with_multi_market_makers_same_price() {
        let mut app: RujiraApp = mock_rujira_app();
        let owner = app.api().addr_make("owner");
        let fees = app.api().addr_make("fees");

        // Setup
        let (fin, btc_ghost, rune_ghost, _, bow) = setup(&mut app, &owner, &fees);

        // Initialize owner with funds
        app.init_modules(|router, _, storage| {
            router.bank.init_balance(
                storage,
                &owner,
                vec![
                    coin(1_000_000_000_000_000_000_000, "btc-btc"),
                    coin(9_500_000_000_000_000_000, "rune"),
                ],
            )
        })
        .unwrap();

        // Add liquidity to ghost vaults (which thorchain swap will use)
        btc_ghost
            .deposit(&mut app, &owner, 500_000_000, "btc-btc")
            .unwrap();
        rune_ghost
            .deposit(&mut app, &owner, 500_000_000_000, "rune")
            .unwrap();

        //  0.000058642175 price of rune/btc (tc)
        //  0.000058642175 price of rune/btc (bow)

        // Add liquidity to BOW with exact ratio: 0.000058642175 RUNE per BTC
        bow.deposit(
            &mut app,
            &owner,
            &[
                coin(68_602_648_901, "btc-btc"),
                coin(1_171_021_570_000_000, "rune"), // replicate the same state of the thorchain swap
            ],
            None,
        )
        .unwrap();

        let balance_rune_before = app.wrap().query_balance(owner.to_string(), "rune").unwrap();

        // Execute a swap that should use both market makers
        //  for a swap of 10_000 BTC
        //  i should receive 10_000 / 0.000058642175 = 170_525_735 RUNE before fees
        //  fees must be 1.5% of total amount due to the configuration of mock fin
        //  so i should receive 170_525_735 * 0.985 = 167_967_815 RUNE
        let swap_amount = 10_000u128;
        let res = app
            .execute_contract(
                owner.clone(),
                fin.addr().clone(),
                &ExecuteMsg::Swap(SwapRequest::Yolo {
                    to: None,
                    callback: None,
                }),
                &coins(swap_amount, "btc-btc"),
            )
            .unwrap();

        let balance_rune_after = app.wrap().query_balance(owner.to_string(), "rune").unwrap();

        assert_eq!(
            balance_rune_after.amount,
            balance_rune_before.amount + Uint128::from(167_967_815u128)
        );

        res.assert_event(&Event::new("wasm-rujira-bow/swap"));
        res.assert_event(&Event::new("wasm-rujira-thorchain-swap/swap"));
    }

    #[test]
    fn test_swap_with_multi_market_makers_same_price_and_single_manual_order() {
        let mut app: RujiraApp = mock_rujira_app();
        let owner = app.api().addr_make("owner");
        let fees = app.api().addr_make("fees");

        // Setup
        let (fin, btc_ghost, rune_ghost, _, bow) = setup(&mut app, &owner, &fees);

        // Initialize owner with funds
        app.init_modules(|router, _, storage| {
            router.bank.init_balance(
                storage,
                &owner,
                vec![
                    coin(1_000_000_000_000_000_000_000, "btc-btc"),
                    coin(9_500_000_000_000_000_000, "rune"),
                ],
            )
        })
        .unwrap();

        // Add liquidity to ghost vaults (which thorchain swap will use)
        btc_ghost
            .deposit(&mut app, &owner, 500_000_000, "btc-btc")
            .unwrap();
        rune_ghost
            .deposit(&mut app, &owner, 500_000_000_000, "rune")
            .unwrap();

        //  0.000058642175 price of rune/btc (tc)
        //  0.000058642175 price of rune/btc (bow)

        // put selling order of 50_000 RUNE @  0.0000585 RUNE per BTC
        fin.place_orders(
            &mut app,
            &owner,
            vec![(
                Side::Base,
                Price::Fixed(Decimal::from_str("0.0000585").unwrap()),
                Some(Uint128::from(50_000u128)),
            )],
            &coins(50_000, "rune"),
        )
        .unwrap();

        // Add liquidity to BOW with exact ratio: 0.000058642175 RUNE per BTC
        bow.deposit(
            &mut app,
            &owner,
            &[
                coin(68_602_648_901, "btc-btc"),
                coin(1_171_021_570_000_000, "rune"), // replicate the same state of the thorchain swap
            ],
            None,
        )
        .unwrap();

        // Execute a swap:
        // Should consume the order of 50_000 RUNE @ 0.0000585
        //  and the remaining should be split between the market makers

        let swap_amount = 10_000u128;
        let res = app
            .execute_contract(
                owner.clone(),
                fin.addr().clone(),
                &ExecuteMsg::Swap(SwapRequest::Yolo {
                    to: None,
                    callback: None,
                }),
                &coins(swap_amount, "btc-btc"),
            )
            .unwrap();

        res.assert_event(
            &Event::new("wasm-rujira-fin/trade")
                .add_attribute("rate", "0.0000585")
                .add_attribute("side", "base"),
        );

        res.assert_event(
            &Event::new("wasm-rujira-fin/trade")
                .add_attribute("rate", "0.000058642175")
                .add_attribute("side", "base"),
        );

        res.assert_event(&Event::new("wasm-rujira-bow/swap"));
        res.assert_event(&Event::new("wasm-rujira-thorchain-swap/swap"));
    }
}
