use cosmwasm_schema::write_api;

use rujira_rs::bow;

fn main() {
    write_api! {
        instantiate: bow::InstantiateMsg,
        execute: bow::ExecuteMsg,
        query: bow::QueryMsg,
        sudo: bow::SudoMsg,
    }
}
