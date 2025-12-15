use cosmwasm_schema::write_api;

use rujira_rs::ghost::credit;

fn main() {
    write_api! {
        instantiate: credit::InstantiateMsg,
        execute: credit::ExecuteMsg,
        query: credit::QueryMsg,
        sudo: credit::SudoMsg,
    }
}
