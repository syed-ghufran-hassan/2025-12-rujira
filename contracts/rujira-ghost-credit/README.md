# Rujira Credit Accounts

A Credit Account (Account) is a generic primitive design to allow overcollateralised borrowing of assets on Rujira.

A single instance of `rujira-ghost-credit` (the Registry) is deployed, and each Account is a freshly instantiated `rujira-account`, which holds the collateral, owns the debt, and creates/manages any 3rd party collateral such as limit orders on `rujira-fin`. It is managed by an `owner` via the Registry, depositing collateral, borrowing funds, exchanging collateral etc. When the LTV exceeds the `config.liquidation_threshold`, a liquidator may step in and bring the Account back into good standing.

## Creation

An account is simply by executing the Registry with `{"create": {}}`. This will instantiate a new Account with its own address.

## Collateralisation

Funding your Account is simply a case of sending it funds. Any tokens that are held by the contract are considered collateral by the Registry. They can be withdrawn via the Registry.

Tokens owned by an Account are counted as collateral. They are valued at `price * ratio` where `price` is the Oracle price as provided by [THORChain's Enshrined Oracle](https://x.com/THORChain/status/1958263114601820162), and `ratio` is the collateral ratio as configured in the Registry. This can be queried with `{"config":{}}` on the Registry contract, under the `collateral_ratios` response key.

## Debt

Once collateral has been added to the Account, tokens can be borrowed. To borrow eg 100 USDC you would execute the Registry contract with the following. Under the hood, the Registry borrows those tokens from the `rujira-ghost-vault` and transfers them to your Account.

Your Account now has a debt allocated to it on the Vault, and the borrowed tokens are allocated against the global debt limit for this asset.

At this point, the USDC has only been sent to your Account. It now holds the collateral you posted, plus the USDC.

```json
{
  "account": {
    "idx": "0",
    "msgs": [
      {
        "borrow": {
          "amount": "10000000000",
          "denom": "eth-usdc-0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
        }
      }
    ]
  }
}
```

Anything that the `Collateral` type can value can be used as collateral. Currently this is limited to Secured Asset tokens held by the Account, but this will be extended to include Limit Orders, LP tokens etc:

```rust
#[cw_serde]
pub enum Collateral {
    Coin(Coin),
}
```

## Actions

Now you have an Account that contains some funds from your own wallet, plus the USDC you've borrowed. What can you do now?

### Option 1: Retreive borrowed funds

This is the simplest option; simply receive the funds that you just borrowed into your own wallet. Execute the Registry with the following message. This will transfer 100 USDC from your Account, to your wallet, validating the final LTV of the Account before the transaction is approved.

```json
{
  "account": {
    "idx": "0",
    "msgs": [
      {
        "send": {
          "to_address": "{your-address}",
          "funds": [
            {
              "amount": "10000000000",
              "denom": "eth-usdc-0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
            }
          ]
        }
      }
    ]
  }
}
```

### Option 2: Execute a contract to swap borrowed funds

Alternatively you might want to swap the USDC for the same token as your collateral. This is equivalent to taking a margin long position on an exchange. Execute the following on the Registry, and it will call eg the BTC/USDC pair on Rujira Trade, swapping the USDC for BTC, returning the BTC to your Account.

This is a completely generic way to call any other smart contract - as long as the token that you get back can be valued by the Registry and the Account has a safe LTV, the transaction will succeed.

```json
{
  "account": {
    "idx": "0",
    "msgs": [
      {
        "execute": {
          "contract_addr": "{rujira-fin/btc/usdc}",
          // base64 encoded `{"swap": {}}`
          "msg": "eyJzd2FwIjp7fX0=",
          "funds": [
            {
              "amount": "10000000000",
              "denom": "eth-usdc-0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
            }
          ]
        }
      }
    ]
  }
}
```

## Health

An Account must keep its LTV below `config.liquidation_threshold` or it risks Liquidation (see below). Each Collateral type has a `collateralization_ratio` which adjusts its $ value in order to ensure collateral can be sold in a timely manner without its value dipping below the debts owed by the Protocol. This allows the Protocol to support multiple collateral types on a single Account with different risk profiles for each type.

Any adjustment of an Account by its Owner must result in the overall LTV of the Account being less than `config.adjustment_threshold`. This is necessarily lower than `config.liquidation_threshold` for two main reasons; to protect users from creating an Account that is instantly liqudiated if the collateral value dips, and to force an Account holder to make a decision to either bring their Account back into good standing, or roll the dice and risk liquidation if the LTV is in "no man's land" between these two values.

## Liquidation

If an Account's adjusted LTV rises above the `config.liquidation_threshold`, the Account can be liquidated. This requires a manual execution which defines the optimal way to acquire the Debt Tokens owed by the Account, in exchange for the Collateral Assets it owns. Currently this will be a simple Market Order on `rujira-trade`, however as more Collateral Types are added, and Accounts have multiple Debt and multiple Collateral types, as well as complex Preferences, Liquidation Routes will become more complex.

A simple example for the Account above; it currently owns BTC, which we need to exchange for USDC to repay the debt:

```json
{
  "liquidate": {
    "account": "{rujira-account-address}",
    "msgs": [
      // Step 1: Execute the swap of BTC to USDC
      // This will return USDC to the Account
      {
        "execute": {
          "contract_addr": "{rujira-fin/btc/usdc}",
          // base64 encoded `{"swap": {}}`
          "msg": "eyJzd2FwIjp7fX0=",
          "funds": [
            {
              "amount": "10000",
              "denom": "btc-btc"
            }
          ]
        }
      },
      // Step 2: Repay the debt
      // This will send the USDC from the Account to the Regsitry, and finally to the Vault, repaying the Account's debt
      {
        "repay": {
          "amount": "10000000000",
          "denom": "eth-usdc-0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
        }
      }
    ]
  }
}
```

This model for Liquidations is designed to support the flexibility offered by multi-Collateral and multi-Debt Credit Accounts, incentivising off-chain solvers to find the liquidation routes that would often be impractical or impossible to calculate on-chain, whilst still internalising the liquidation volume across the Rujira Ecosystem.

Account LTV calculation requires atomic execution of swaps, and as such this guarantees that liquidation volume will flow through Rujira Trade, where users can use Oracle orders to automatically adjust with the Enshrined Oracle price, and catch local tops and local bottoms during periods of strong movement and liquidations.

### Rewards

As a reward for solving and executing a liquidation, the account that calls the liquidation earns `config.fee_liquidator * repay` in a fee. This fee is paid only when the debt is repaid during a Liquidation, aligning incentives between the Protocol and the Liquidator. The Liquidator must plan a _route_ as a `Vec<LiquidationMsg>` in order exchange collateral for debt, and repay the debt.

### Constraints

Whenever a Liquidation Step `execute`s a contract, the output from that execution is validated against a set of rules to ensure proper Liquidation.

- The Account's LTV must must be strictly **less** than `config.liquidation_threshold` (e.g. 100% LTV), otherwise the transaction will fail.
- The Account's LTV must must be **equal to or above** the `config.adjustment_threshold` (e.g. 90% LTV), otherwise the transaction will fail. This protects the Account from over-liquidation.
- All tokens _received_ by the Account in the contract execution **must** be one of the Debt tokens owed by the Account. This is to prevent unnecessary exchanges of a user's collateral.
- All tokens _received_ by the Account in the contract execution **must** be equal to or less than the corresponding debt owed. This is to prevent over-liquidation of a user's collateral.
- All tokens _sent_ by the Account in the contract execution must match an Account's `Preferences`.
- The $ value when collateral is exchanged for debt must not exceed `config.liquidation_max_slip`.

### Account Liquidation Preferences

An Account can configure its own Liquidation Preferences, which are rules applied when a Liquidation is processed.

#### Specified Messages

This allows an Account Owner to inject a set of `LiquidateMsg`s at the start of the liquidation route that a Liquidator provides. This allows an Account Owner to exactly specify how their position should be liquidated, if it reaches that point.

These messages are executed on a "best-effort" basis, and if execution fails then the error is ignored and a liquidator may choose an alternative route. They are also still subject to the Constraints outlined above.

Execute the following to say "The first action of any liquidation must be to swap BTC for USDC on the rujira-fin pair"

The Liquidator will therefore have to calculate the expected return from this swap and submit `repay` as the first message

```json
{
  "account": {
    "idx": "0",
    "msgs": [
      {
        "set_preference_messages": [
          {
            "execute": {
              "contract_addr": "{rujira-fin/btc/usdc}",
              // base64 encoded `{"swap": {}}`
              "msg": "eyJzd2FwIjp7fX0=",
              "funds": [
                {
                  "amount": "10000",
                  "denom": "btc-btc"
                }
              ]
            }
          }
        ]
      }
    ]
  }
}
```

#### Liquidation Order

This allows an Account owner to specify whether a certain Collateral Token is allowed to be Liquidated, depending on what other Collateral Tokens the Account still holds:

Execute the following to say "Don't liquidate BTC whilst the Account still has ETH Collateral"

```json
{
  "account": {
    "idx": "0",
    "msgs": [
      {
        "set_preference_msgs": {
          "denom": "btc-btc",
          "required": "eth-eth"
        }
      }
    ]
  }
}
```

and clear the preference with

```json
{
  "account": {
    "idx": "0",
    "msgs": [
      {
        "set_preference_order": {
          "denom": "btc-btc",
          "required": null
        }
      }
    ]
  }
}
```
