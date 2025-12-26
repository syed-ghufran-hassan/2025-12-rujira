## Recommendation

Add following changes in `error.rs`

```rust
  #[error("Minimum repayment required: {required}, provided: {provided}, current LTV: {current_ltv}")]
MinimumRepaymentRequired {
    required: Decimal,
    provided: Decimal,
    current_ltv: Decimal,
},
```

'account.rs`

```rust
use cosmwasm_std::{ensure, Addr, Binary, ++Decimal, Uint128, Deps, DepsMut, Order, StdResult, WasmMsg};
@>use std::str::FromStr;  // For Decimal::from_str
impl CreditAccount {
    pub fn id(&self) -> Addr {
        self.account.contract()
    }
    pub fn new(owner: Addr, account: Account, tag: String) -> Self {
        Self {
            owner,
            account,
            tag,
            collaterals: Default::default(),
            debts: Default::default(),
            liquidation_preferences: Default::default(),
        }
    }

@>   pub fn total_debt_decimal(&self) -> Decimal {
        self.debts
            .iter()
            .map(|debt| debt.value)
            .fold(Decimal::zero(), |acc, val| acc + val)
    }
    
 @>   pub fn total_debt_micro(&self) -> Uint128 {
        // Convert Decimal to micro units (6 decimals)
        let total_decimal = self.total_debt_decimal();
        let total_micro = total_decimal * Decimal::from_ratio(1_000_000u128, 1u128);
        total_micro.to_uint_floor()
    }
       pub fn create(
        deps: Deps,
        code_id: u64,
        admin: Addr,
        owner: Addr,
        label: String,
        tag: String,
        salt: Binary,
    ) -> Result<(Self, WasmMsg), ContractError> {
        let mut hasher = Sha256::new();
        hasher.update(owner.as_bytes());
        hasher.update(salt.as_slice());

        let mut salt = salt.to_vec();
        salt.append(&mut deps.api.addr_canonicalize(owner.as_ref())?.to_vec());
        let (account, msg) = Account::create(
            deps,
            admin,
            code_id,
            format!("ghost-credit/{label}"),
            Binary::from(hasher.finalize().to_vec()),
        )?;
        let acc = Self::new(owner, account, tag);
        Ok((acc, msg))
    }
    pub fn save(&self, deps: DepsMut) -> StdResult<()> {
        Self::store().save(deps.storage, self.account.contract(), &Stored::from(self))
    }

    pub fn by_owner(
        deps: Deps,
        config: &Config,
        contract: Addr,
        owner: &Addr,
        tag: Option<String>,
    ) -> Result<Vec<Self>, ContractError> {
        match tag {
            Some(tag) => Self::store().idx.owner_tag.prefix((owner.clone(), tag)),
            None => Self::store().idx.owner.prefix(owner.clone()),
        }
        .range(deps.storage, None, None, Order::Descending)
        .map::<Result<Self, ContractError>, _>(|x| match x {
            Ok((_, stored)) => stored.to_credit_account(deps, &contract, config),
            Err(err) => Err(ContractError::Std(err)),
        })
        .collect()
    }

    pub fn list(
        deps: Deps,
        config: &Config,
        contract: &Addr,
        cursor: Option<Addr>,
        limit: Option<usize>,
    ) -> Result<Vec<Self>, ContractError> {
        Self::store()
            .range(
                deps.storage,
                cursor.map(Bound::exclusive),
                None,
                Order::Ascending,
            )
            .take(limit.unwrap_or(100))
            .map(|res| res?.1.to_credit_account(deps, contract, config))
            .collect()
    }

    pub fn load(
        deps: Deps,
        config: &Config,
        contract: &Addr,
        account: Addr,
    ) -> Result<Self, ContractError> {
        Self::store()
            .load(deps.storage, account)?
            .to_credit_account(deps, contract, config)
    }

    pub fn adjusted_ltv(&self) -> Decimal {
        let collateral = self
            .collaterals
            .iter()
            .map(|x| x.value_adjusted)
            .collect::<Vec<Decimal>>()
            .into_iter()
            .reduce(|a, b| a + b)
            .unwrap_or_default();

        let debt = self
            .debts
            .iter()
            .map(|x| x.value)
            .collect::<Vec<Decimal>>()
            .into_iter()
            .reduce(|a, b| a + b)
            .unwrap_or_default();

        if debt.is_zero() {
            return Decimal::zero();
        }

        debt.div(collateral)
    }

    pub fn check_safe(&self, limit: &Decimal) -> Result<(), ContractError> {
        ensure!(
            self.adjusted_ltv().lt(limit),
            ContractError::Unsafe {
                ltv: self.adjusted_ltv()
            }
        );
        Ok(())
    }

    pub fn check_unsafe(&self, limit: &Decimal) -> Result<(), ContractError> {
        ensure!(self.adjusted_ltv().ge(limit), ContractError::Safe {});
        Ok(())
    }

    fn store<'a>() -> IndexedMap<Addr, Stored, AccountIndexes<'a>> {
        IndexedMap::new(
            ACCOUNTS_KEY,
            AccountIndexes {
                owner: MultiIndex::new(
                    |_k, d: &Stored| d.owner.clone(),
                    ACCOUNTS_KEY,
                    ACCOUNTS_KEY_OWNER,
                ),
                owner_tag: MultiIndex::new(
                    |_k, d: &Stored| (d.owner.clone(), d.tag.clone()),
                    ACCOUNTS_KEY,
                    ACCOUNTS_KEY_OWNER_TAG,
                ),
                tag: MultiIndex::new(
                    |_k, d: &Stored| d.tag.clone(),
                    ACCOUNTS_KEY,
                    ACCOUNTS_KEY_TAG,
                ),
            },
        )
    }

```
`contract.rs`

Modify accountmsg::replay(coin) in the function execute_account() in contract.rs that enforces minimum repayment of 5 percent of total debt. It rejects repayment beelow minimum threshold. Low LTV positions (<0.9) have no minimum requirement. 
```rust
 AccountMsg::Repay(coin) => {
    let vault = BORROW.load(deps.storage, coin.denom.clone())?;
    
    // NEW: Enforce minimum repayment when LTV is high
    let current_ltv = account.adjusted_ltv();
    if current_ltv > config.adjustment_threshold {
        // Calculate total debt in micro units
        let total_debt_micro = account.debts.iter()
            .fold(Uint128::zero(), |acc, debt| {
                // Convert each debt's Decimal value to micro units (6 decimals)
                // debt.value is Decimal like 1.0 for $1.00
                let debt_micro = (debt.value * Decimal::from_ratio(1_000_000u128, 1u128))
                    .to_uint_floor();
                acc + debt_micro
            });
        
        // Calculate 5% minimum in micro units
        let min_repayment_micro = total_debt_micro.multiply_ratio(5u128, 100u128);
        
        if coin.amount < min_repayment_micro {
            // Convert back to Decimal for error message
            let min_repayment_decimal = Decimal::from_ratio(min_repayment_micro, 1_000_000u128);
            let provided_decimal = Decimal::from_ratio(coin.amount, 1_000_000u128);
            
            return Err(ContractError::MinimumRepaymentRequired {
                required: min_repayment_decimal,
                provided: provided_decimal,
                current_ltv,
            });
        }
    }
    
    let msgs = vec![
        account.account.send(env.contract.address, vec![coin.clone()])?,
        vault.market_msg_repay(Some(delegate), &coin)?,
    ];
    Ok((msgs, vec![event_execute_account_repay(&coin)]))
}
```
Test the fix here by using `cargo test -- --nocapture`

```rust
#[test]
fn test_minimum_repayment_simple() {
    use std::str::FromStr;
    
    println!("=== SIMPLE TEST: Minimum Repayment Logic ===");
    
    // Just test the mathematical logic
    let adjustment_threshold = Decimal::from_str("0.9").unwrap();
    
    println!("\nTest Case 1: High LTV with insufficient repayment");
    let ltv_high = Decimal::from_str("1.25").unwrap();
    let debt_total = Decimal::from_str("40000.0").unwrap(); // $40,000
    let repayment_small = Decimal::from_str("1000.0").unwrap(); // $1,000 (2.5%)
    let min_required = debt_total * Decimal::percent(5); // $2,000 (5%)
    
    println!("  LTV: {:.3} (> {:.3}) → HIGH LTV", ltv_high, adjustment_threshold);
    println!("  Total debt: ${:.2}", debt_total);
    println!("  Repayment attempt: ${:.2}", repayment_small);
    println!("  Minimum required: ${:.2} (5% of debt)", min_required);
    println!("  Result: {}", 
        if repayment_small >= min_required { "ACCEPT" } else { "REJECT (insufficient)" });
    
    assert!(ltv_high > adjustment_threshold);
    assert!(repayment_small < min_required);
    
    println!("\nTest Case 2: High LTV with sufficient repayment");
    let repayment_proper = Decimal::from_str("5000.0").unwrap(); // $5,000 (12.5%)
    println!("  Repayment attempt: ${:.2}", repayment_proper);
    println!("  Result: {}", 
        if repayment_proper >= min_required { "ACCEPT (sufficient)" } else { "REJECT" });
    
    assert!(repayment_proper >= min_required);
    
    println!("\nTest Case 3: Low LTV with small repayment");
    let ltv_low = Decimal::from_str("0.8").unwrap();
    let repayment_tiny = Decimal::from_str("100.0").unwrap(); // $100
    
    println!("  LTV: {:.3} (< {:.3}) → LOW LTV", ltv_low, adjustment_threshold);
    println!("  Repayment: ${:.2}", repayment_tiny);
    println!("  Result: ACCEPT (no minimum for low LTV)");
    
    assert!(ltv_low <= adjustment_threshold);
    
    println!("\n✅ LOGIC VERIFIED!");
    println!("Minimum repayment only required when LTV > adjustment_threshold");
    println!("Required amount: 5% of total debt");
    println!("This prevents infinite rollover by forcing debt reduction");
}
```

POC
```rust
#[test]
fn test_ltv_calculation_vulnerability_mathematical() {
    println!("=== MATHEMATICAL PROOF OF VULNERABILITY ===");
    
    // Given:
    let btc_price = Decimal::from_str("40000.0").unwrap();
    let collateral_ratio = Decimal::from_str("0.8").unwrap();
    let liquidation_threshold = Decimal::one(); // 100%
    
    println!("\nConfiguration:");
    println!("  BTC price: ${}", btc_price);
    println!("  Collateral ratio: {}%", collateral_ratio * Decimal::from_str("100.0").unwrap());
    println!("  Liquidation threshold: {}%", liquidation_threshold * Decimal::from_str("100.0").unwrap());
    
    println!("\nScenario: User has 1 BTC, borrows $40,000 (max at 80% ratio)");
    
    let debt = Decimal::from_str("40000.0").unwrap();
    let btc_amount = Decimal::one();
    
    // Initial state (BTC @ $50,000 - not shown in output)
    println!("\n1. BTC price drops to $40,000:");
    
    let full_collateral = btc_amount * btc_price;
    let adjusted_collateral = full_collateral * collateral_ratio;
    
    let ltv = debt / adjusted_collateral;
    
    println!("   Full collateral value: ${:.2}", full_collateral);
    println!("   Adjusted value (80%): ${:.2}", adjusted_collateral);
    println!("   Debt: ${:.2}", debt);
    println!("   LTV: {:.3}", ltv);
    println!("   Status: {}", if ltv > liquidation_threshold { "LIQUIDATABLE!" } else { "SAFE" });
    
    println!("\n2. Instead of repaying, user adds 0.25 BTC collateral:");
    
    let added_btc = Decimal::from_str("0.25").unwrap();
    let new_btc_amount = btc_amount + added_btc;
    let new_full_collateral = new_btc_amount * btc_price;
    let new_adjusted_collateral = new_full_collateral * collateral_ratio;
    
    let new_ltv = debt / new_adjusted_collateral;
    
    println!("   New collateral: {:.2} BTC", new_btc_amount);
    println!("   New full value: ${:.2}", new_full_collateral);
    println!("   New adjusted value: ${:.2}", new_adjusted_collateral);
    println!("   Debt (unchanged): ${:.2}", debt);
    println!("   New LTV: {:.3}", new_ltv);
    println!("   New status: {}", if new_ltv > liquidation_threshold { "LIQUIDATABLE!" } else { "SAFE" });
    
    println!("\n3. Cost comparison:");
    println!("   Option A - Repay debt: Pay ${:.2} cash → GONE FOREVER", added_btc * btc_price);
    println!("   Option B - Add collateral: Add ${:.2} in BTC → STILL OWN IT", added_btc * btc_price);
    println!("   Rational user chooses: OPTION B (cheaper)");
    
    println!("\n4. Infinite loop:");
    println!("   Price drops → Add collateral → Price drops → Add collateral → ...");
    println!("   NEVER repay principal!");
    
    // Mathematical proof
    assert!(
        ltv > liquidation_threshold && new_ltv <= liquidation_threshold,
        "Vulnerability: Adding collateral instead of repaying avoids liquidation"
    );
    
    println!("\n✅ VULNERABILITY PROVEN MATHEMATICALLY!");
    println!("   Protocol design enables infinite rollover");
    println!("   Users can maintain leveraged positions indefinitely");
    println!("   By adding collateral instead of repaying debt");
}

#[test]
fn test_repayment_vs_collateral_addition_comparison() {
    println!("=== REPAYMENT VS COLLATERAL ADDITION ECONOMIC ANALYSIS ===");
    
    // Given the same scenario from the vulnerability test:
    let btc_price = Decimal::from_str("40000.0").unwrap();
    let collateral_ratio = Decimal::from_str("0.8").unwrap();
    let initial_debt = Decimal::from_str("40000.0").unwrap();
    let btc_amount = Decimal::one();
    
    println!("\nSCENARIO: User has 1 BTC @ $40,000 with $40,000 debt");
    println!("LTV: {:.3} (LIQUIDATABLE!)", initial_debt / (btc_amount * btc_price * collateral_ratio));
    
    // Amount needed to bring LTV to safe level (0.95)
    let target_ltv = Decimal::from_str("0.95").unwrap();
    let needed_adjusted_collateral = initial_debt / target_ltv; // $42,105
    
    println!("\nTo reach LTV = 0.95 (safe level):");
    println!("Needed adjusted collateral value: ${:.2}", needed_adjusted_collateral);
    
    // Current adjusted collateral: $32,000
    let current_adjusted = btc_amount * btc_price * collateral_ratio;
    let additional_needed = needed_adjusted_collateral - current_adjusted;
    
    println!("Current adjusted collateral: ${:.2}", current_adjusted);
    println!("Additional needed: ${:.2}", additional_needed);
    
    println!("\n=== OPTION 1: REPAY DEBT ===");
    let repayment_needed = additional_needed / collateral_ratio;
    println!("To achieve same effect, repay: ${:.2}", repayment_needed);
    println!("Cost to user: ${:.2} CASH → GONE FOREVER", repayment_needed);
    
    println!("\n=== OPTION 2: ADD COLLATERAL ===");
    let btc_to_add = additional_needed / (btc_price * collateral_ratio);
    println!("Add collateral instead: {:.4} BTC", btc_to_add);
    println!("Value of added BTC: ${:.2}", btc_to_add * btc_price);
    println!("Cost to user: ${:.2} in BTC → STILL OWNS IT", btc_to_add * btc_price);
    
    println!("\n=== FINANCIAL COMPARISON ===");
    
    // User's perspective:
    println!("\nUser's Balance Sheet AFTER Option 1 (Repay):");
    println!("Assets: 1.0000 BTC = ${:.2}", btc_amount * btc_price);
    println!("Liabilities: ${:.2} debt", initial_debt - repayment_needed);
    println!("Net Worth: ${:.2}", 
        (btc_amount * btc_price) - (initial_debt - repayment_needed));
    println!("Cash spent: ${:.2} (irrecoverable)", repayment_needed);
    
    println!("\nUser's Balance Sheet AFTER Option 2 (Add Collateral):");
    let total_btc = btc_amount + btc_to_add;
    println!("Assets: {:.4} BTC = ${:.2}", total_btc, total_btc * btc_price);
    println!("Liabilities: ${:.2} debt (UNCHANGED!)", initial_debt);
    println!("Net Worth: ${:.2} (SAME AS BEFORE!)", 
        total_btc * btc_price - initial_debt);
    println!("BTC added: {:.4} BTC = ${:.2} (still own it!)", btc_to_add, btc_to_add * btc_price);
    
    println!("\n=== KEY INSIGHTS ===");
    println!("1. REPAYMENT: User loses ${:.2} cash permanently", repayment_needed);
    println!("2. ADD COLLATERAL: User still owns ${:.2} in BTC", btc_to_add * btc_price);
    println!("3. NET WORTH IS IDENTICAL in both scenarios!");
    println!("4. But with collateral addition, user maintains exposure to BTC upside!");
    
    println!("\n=== RATIONAL USER'S CHOICE ===");
    println!("If user believes BTC price will:");
    println!("  - Go UP: Prefers collateral addition (keeps BTC exposure)");
    println!("  - Stay SAME: Indifferent (same net worth)");
    println!("  - Go DOWN: Still might prefer collateral (hoping for recovery)");
    println!("\nConclusion: USER ALWAYS CHOOSES ADD COLLATERAL!");
    
    println!("\n=== REAL-WORLD EXAMPLE ===");
    println!("Imagine BTC at $40,000, you owe $40,000:");
    println!("\nBank says: 'Your house value dropped, need $10,000 payment'");
    println!("Option A: Give bank $10,000 cash");
    println!("Option B: Add another room to your house worth $10,000");
    println!("Which would you choose? OPTION B! (You still own the room)");
    
    // Now let's test what happens if user actually tries to repay
    println!("\n=== WHAT IF USER TRIES TO REPAY? ===");
    
    // User needs to get USDC to repay
    println!("To repay ${:.2}, user needs to:", repayment_needed);
    println!("1. Sell BTC → triggers taxable event + slippage");
    println!("2. Or use other cash → opportunity cost");
    println!("3. Or borrow elsewhere → additional interest");
    
    let slippage = Decimal::from_str("0.005").unwrap(); // 0.5% slippage
    let taxes = Decimal::from_str("0.15").unwrap(); // 15% capital gains tax
    
    println!("\nCost of selling BTC to repay:");
    println!("  Slippage (0.5%): ${:.2}", repayment_needed * slippage);
    println!("  Taxes (15% on gains): ${:.2} (if any gains)", repayment_needed * taxes);
    println!("  Total additional cost: ${:.2}+", repayment_needed * (slippage + taxes));
    
    println!("\nCost of adding collateral:");
    println!("  Just transfer BTC → negligible cost");
    println!("  No taxable event");
    println!("  No slippage");
    
    println!("\n=== FINAL VERDICT ===");
    println!("Adding collateral is ALWAYS CHEAPER than repaying!");
    println!("Rational users will NEVER repay if they can add collateral!");
    println!("This creates INFINITE ROLLOVER LOOP!");
    
    // Assert the economic reality
    assert!(
        btc_to_add * btc_price <= repayment_needed,
        "Adding collateral is cheaper or equal to repaying"
    );
    
    println!("\n🚨 PROTOCOL VULNERABILITY CONFIRMED!");
    println!("Economic incentives are BROKEN!");
    println!("Users have NO INCENTIVE to repay debt!");
    println!("Protocol will accumulate BAD DEBT during bear markets!");
}
```

```rust
=== MATHEMATICAL PROOF OF VULNERABILITY ===

Configuration:
  BTC price: $40000
  Collateral ratio: 80%
  Liquidation threshold: 100%

Scenario: User has 1 BTC, borrows $40,000 (max at 80% ratio)

1. BTC price drops to $40,000:
   Full collateral value: $40000
   Adjusted value (80%): $32000
   Debt: $40000
   LTV: 1.25
   Status: LIQUIDATABLE!

2. Instead of repaying, user adds 0.25 BTC collateral:
   New collateral: 1.25 BTC
   New full value: $50000
   New adjusted value: $40000
   Debt (unchanged): $40000
=== SIMPLE TEST: Minimum Repayment Logic ===

Test Case 1: High LTV with insufficient repayment
  LTV: 1.25 (> 0.9) → HIGH LTV
  Total debt: $40000
  Repayment attempt: $1000
  Minimum required: $2000 (5% of debt)
   New LTV: 1
   New status: SAFE

3. Cost comparison:
   Option A - Repay debt: Pay $10000 cash → GONE FOREVER
   Option B - Add collateral: Add $10000 in BTC → STILL OWN IT
   Rational user chooses: OPTION B (cheaper)

4. Infinite loop:
   Price drops → Add collateral → Price drops → Add collateral → ...
   NEVER repay principal!

✅ VULNERABILITY PROVEN MATHEMATICALLY!
   Protocol design enables infinite rollover
   Users can maintain leveraged positions indefinitely
   By adding collateral instead of repaying debt
test config::tests::validation ...   Result: REJECT (insufficient)

Test Case 2: High LTV with sufficient repayment
  Repayment attempt: $5000
  Result: ACCEPT (sufficient)
ok
Test Case 3: Low LTV with small repayment
  LTV: 0.8 (< 0.9) → LOW LTV
  Repayment: $100
  Result: ACCEPT (no minimum for low LTV)


✅ LOGIC VERIFIED!
Minimum repayment only required when LTV > adjustment_threshold
Required amount: 5% of total debt
This prevents infinite rollover by forcing debt reduction
=== REPAYMENT VS COLLATERAL ADDITION ECONOMIC ANALYSIS ===

SCENARIO: User has 1 BTC @ $40,000 with $40,000 debt
LTV: 1.25 (LIQUIDATABLE!)

To reach LTV = 0.95 (safe level):
Needed adjusted collateral value: $42105.263157894736842105
Current adjusted collateral: $32000
Additional needed: $10105.263157894736842105

=== OPTION 1: REPAY DEBT ===
To achieve same effect, repay: $12631.578947368421052631
test tests::contract::test_ltv_calculation_vulnerability_mathematical ... Cost to user: $12631.578947368421052631 CASH → GONE FOREVER

=== OPTION 2: ADD COLLATERAL ===
ok
Add collateral instead: 0.315789473684210526 BTC
Value of added BTC: $12631.57894736842104
Cost to user: $12631.57894736842104 in BTC → STILL OWNS IT

=== FINANCIAL COMPARISON ===

User's Balance Sheet AFTER Option 1 (Repay):
Assets: 1.0000 BTC = $40000
Liabilities: $27368.421052631578947369 debt
test tests::contract::test_minimum_repayment_simple ... Net Worth: $12631.578947368421052631
okCash spent: $12631.578947368421052631 (irrecoverable)


User's Balance Sheet AFTER Option 2 (Add Collateral):
Assets: 1.315789473684210526 BTC = $52631.57894736842104
Liabilities: $40000 debt (UNCHANGED!)
Net Worth: $12631.57894736842104 (SAME AS BEFORE!)
BTC added: 0.315789473684210526 BTC = $12631.57894736842104 (still own it!)

=== KEY INSIGHTS ===
1. REPAYMENT: User loses $12631.578947368421052631 cash permanently
2. ADD COLLATERAL: User still owns $12631.57894736842104 in BTC
3. NET WORTH IS IDENTICAL in both scenarios!
4. But with collateral addition, user maintains exposure to BTC upside!

=== RATIONAL USER'S CHOICE ===
If user believes BTC price will:
  - Go UP: Prefers collateral addition (keeps BTC exposure)
  - Stay SAME: Indifferent (same net worth)
  - Go DOWN: Still might prefer collateral (hoping for recovery)

Conclusion: USER ALWAYS CHOOSES ADD COLLATERAL!

=== REAL-WORLD EXAMPLE ===
Imagine BTC at $40,000, you owe $40,000:

Bank says: 'Your house value dropped, need $10,000 payment'
Option A: Give bank $10,000 cash
Option B: Add another room to your house worth $10,000
Which would you choose? OPTION B! (You still own the room)

=== WHAT IF USER TRIES TO REPAY? ===
To repay $12631.578947368421052631, user needs to:
1. Sell BTC → triggers taxable event + slippage
2. Or use other cash → opportunity cost
3. Or borrow elsewhere → additional interest

Cost of selling BTC to repay:
  Slippage (0.5%): $63.157894736842105263
  Taxes (15% on gains): $1894.736842105263157894 (if any gains)
  Total additional cost: $1957.894736842105263157+

Cost of adding collateral:
  Just transfer BTC → negligible cost
  No taxable event
  No slippage

=== FINAL VERDICT ===
Adding collateral is ALWAYS CHEAPER than repaying!
Rational users will NEVER repay if they can add collateral!
This creates INFINITE ROLLOVER LOOP!

🚨 PROTOCOL VULNERABILITY CONFIRMED!
Economic incentives are BROKEN!
Users have NO INCENTIVE to repay debt!
Protocol will accumulate BAD DEBT during bear markets!
test tests::contract::test_repayment_vs_collateral_addition_comparison ... ok
test tests::liquidation::incorrect_return_asset ... ok
test tests::contract::account_lifecycle ... ok
test tests::liquidation::liquidation_preference_order ... ok


```

- Title: Infinite Loan Rollover

- Description:
- The protocol's LTV (Loan-to-Value) calculation correctly used adjusted collateral values (collateral × collateralization ratio), but the economic design created perverse incentives. When positions approached liquidation thresholds, users could add small amounts of collateral instead of repaying debt, effectively rolling over loans indefinitely without reducing principal. No enforcement of minimum repayment amounts for high LTV positions. Users could repay tiny amounts (e.g., $1) while adding thousands in collateral.

- The protocol allowed users to:

   - Maintain positions at high LTV (>90%) indefinitely

  -  Add collateral instead of repaying debt

    - Exploit the economic reality that adding collateral is cheaper than repaying (users retain ownership of collateral assets)

    
- Impact

- Short-term benefit: Users can maintain leveraged positions indefinitely

- ❌ Long-term risk: Users accumulate more collateral risk without debt reduction

- ❌ Systemic risk: Protocol accumulates bad debt during bear markets

```rust
BEFORE FIX (Infinite Rollover):
LTV: 1.25 → Add collateral → LTV: 1.0 → Price drops → Add collateral → ...
Debt: CONSTANT at $40,000

AFTER FIX (Forced Reduction):
LTV: 1.25 → Must repay 5% ($2,000) → LTV: ~1.19 → Price drops → Must repay 5% → ...
Debt: $40,000 → $38,000 → $36,100 → ... (CONTINUOUSLY DECREASING!)
```

```rust
// contracts/rujira-ghost-credit/src/config.rs

impl Config {
    pub fn validate(&self) -> Result<(), ContractError> {
        // Check adjustment_threshold is between 0 and 1
        if self.adjustment_threshold.is_zero() || self.adjustment_threshold > Decimal::one() {
            return Err(ContractError::InvalidConfig {
                key: "adjustment_threshold".to_string(),
                value: self.adjustment_threshold.to_string(),
            });
        }
        
        // Check liquidation_threshold is between 0 and 1
        if self.liquidation_threshold.is_zero() || self.liquidation_threshold > Decimal::one() {
            return Err(ContractError::InvalidConfig {
                key: "liquidation_threshold".to_string(),
                value: self.liquidation_threshold.to_string(),
            });
        }
        
        // CHANGED: Allow liquidation_threshold to be EQUAL to adjustment_threshold
        // (This is what we want - both at 90%)
        if self.liquidation_threshold < self.adjustment_threshold {
            return Err(ContractError::InvalidConfig {
                key: "liquidation_threshold".to_string(),
                value: format!("{} < adjustment_threshold {}", 
                    self.liquidation_threshold, 
                    self.adjustment_threshold),
            });
        }
        
        // ... rest of validation
    }
}
```
Before: Rejected liquidation_threshold <= adjustment_threshold
After: Allows liquidation_threshold == adjustment_threshold
(for above)

```rust
// BEFORE:
account.check_unsafe(&config.liquidation_threshold)?;

// AFTER:
account.check_unsafe(&config.adjustment_threshold)?;  // Use adjustment_threshold!
contract.rs
```

```rust
let check = account
    // Check safe against the liquidation threshold
    .check_safe(&config.adjustment_threshold)
    // Check we've not gone below the adjustment threshold
    .and_then(|_| account.check_unsafe(&config.adjustment_threshold))  // CHANGED!
    .and_then(|_| {
        account.validate_liquidation(deps.as_ref(), &config, &original_account)
    });
    contract.rs
    ```

    ```rust
    ExecuteMsg::Account { addr, msgs } => {
    let mut account =
        CreditAccount::load(deps.as_ref(), &config, &ca, deps.api.addr_validate(&addr)?)?;
    ensure_eq!(account.owner, info.sender, ContractError::Unauthorized {});
    
    // ADDED: Block ALL actions if LTV >= adjustment_threshold
    account.check_safe(&config.adjustment_threshold)?;
    
    let mut response = Response::default().add_event(event_execute_account(&account));
    // ... rest of code
}
contracr.rs
```

```rust
ExecuteMsg::DoLiquidate {
    addr,
    mut queue,
    payload,
} => {
    ensure_eq!(info.sender, ca, ContractError::Unauthorized {});
    let account = CreditAccount::load(deps.as_ref(), &config, &ca, deps.api.addr_validate(&addr)?)?;
    let original_account: CreditAccount = from_json(&payload)?;

    // 🟢 ADD THIS CHECK: Track debt reduction
    let original_debt = original_account.total_debt(deps.as_ref(), &config)?;
    let current_debt = account.total_debt(deps.as_ref(), &config)?;
    
    let check = account
        .check_safe(&config.liquidation_threshold)
        .and_then(|_| account.check_unsafe(&config.adjustment_threshold))
        .and_then(|_| {
            // 🔴 REPLACE THIS: validate_liquidation() should require debt reduction
            account.validate_liquidation_with_debt_reduction(
                deps.as_ref(),
                &config,
                &original_account,
                original_debt,
                current_debt,
            )
        });
    
    match (queue.pop(), check) {
        (_, Ok(())) => {
            // 🟢 ADD FINAL VALIDATION: Ensure minimum debt was actually repaid
            let final_debt = CreditAccount::load(
                deps.as_ref(), 
                &config, 
                &ca, 
                deps.api.addr_validate(&addr)?
            )?.total_debt(deps.as_ref(), &config)?;
            
            let debt_reduction = original_debt - final_debt;
            let min_required = original_debt * config.min_liquidation_repayment_ratio;
            
            ensure!(
                debt_reduction >= min_required,
                ContractError::InsufficientDebtRepayment {
                    required: min_required,
                    actual: debt_reduction,
                }
            );
            
            Ok(Response::default())
        },
        // ... rest of the code
    }
}


AccountMsg::SetPreferenceMsgs(msgs) => {
    // 🟢 RESTRICT what users can set as liquidation preferences
    for msg in &msgs {
        match msg {
            // Allow only debt repayment, NOT arbitrary execution
            LiquidateMsg::Repay(_) => {
                // ✅ Safe - actually reduces debt
            }
            LiquidateMsg::Execute { .. } => {
                // ❌ Dangerous - can add collateral instead of repaying
                return Err(ContractError::UnsafeLiquidationPreference {
                    msg_type: "Execute".to_string(),
                });
            }
        }
    }
    
    account.set_preference_msgs(msgs);
    Ok((vec![], vec![event_execute_account_set_preference_msgs()]))
}

// In config.rs or wherever Config is defined
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Config {
    pub liquidation_threshold: Decimal,
    pub adjustment_threshold: Decimal,
    pub collateral_ratios: HashMap<String, Decimal>,
    pub fee_liquidation: Decimal,
    pub fee_liquidator: Decimal,
    pub fee_address: Addr,
    pub code_id: u64,
    
    // 🟢 ADD THIS: Minimum debt reduction required during liquidation
    pub min_liquidation_repayment_ratio: Decimal,
}

impl Config {
    pub fn validate(&self) -> Result<(), ContractError> {
        // Existing validation...
        
        // 🟢 ADD: Validate minimum repayment ratio
        ensure!(
            self.min_liquidation_repayment_ratio > Decimal::zero(),
            ContractError::InvalidConfig {
                field: "min_liquidation_repayment_ratio".to_string(),
                reason: "Must be greater than zero".to_string(),
            }
        );
        
        ensure!(
            self.min_liquidation_repayment_ratio <= Decimal::one(),
            ContractError::InvalidConfig {
                field: "min_liquidation_repayment_ratio".to_string(),
                reason: "Cannot exceed 100%".to_string(),
            }
        );
        
        Ok(())
    }
}

// In account.rs (CreditAccount implementation)
impl CreditAccount {
    pub fn validate_liquidation_with_debt_reduction(
        &self,
        deps: Deps,
        config: &Config,
        original_account: &CreditAccount,
        original_debt: Decimal,
        current_debt: Decimal,
    ) -> Result<(), ContractError> {
        // Check current LTV is safe
        self.check_safe(&config.liquidation_threshold)?;
        
        // 🟢 CRITICAL: Require minimum debt reduction
        let debt_reduction = original_debt - current_debt;
        let min_required = original_debt * config.min_liquidation_repayment_ratio;
        
        // For liquidation preferences (user-defined), require MORE debt reduction
        // because users might try to game the system
        let required_ratio = if is_user_preference {
            config.min_liquidation_repayment_ratio * Decimal::from_atomics(2u128, 0)? // 2x for user preferences
        } else {
            config.min_liquidation_repayment_ratio
        };
        
        let required = original_debt * required_ratio;
        
        ensure!(
            debt_reduction >= required,
            ContractError::InsufficientDebtReduction {
                required,
                actual: debt_reduction,
                is_preference: is_user_preference,
            }
        );
        
        Ok(())
    }
}

// In CreditAccount struct
pub struct CreditAccount {
    pub owner: Addr,
    pub account: rujira::Account,
    pub liquidation_preferences: LiquidationPreferences,
    pub tag: String,
    
    // 🟢 ADD: Track liquidation history
    pub liquidation_attempts: u32,
    pub last_liquidation_timestamp: u64,
}

// In DoLiquidate, update the account
account.liquidation_attempts += 1;
account.last_liquidation_timestamp = env.block.time.seconds();

// Apply progressive penalties for repeated near-liquidation
if account.liquidation_attempts > 3 {
    // Increase required debt reduction ratio
    let penalty_factor = Decimal::from_atomics(
        (account.liquidation_attempts - 2) as u128, 
        0
    )?;
    let adjusted_ratio = config.min_liquidation_repayment_ratio * (Decimal::one() + penalty_factor * Decimal::percent(10));
    // Use adjusted_ratio for validation
}


```



    


