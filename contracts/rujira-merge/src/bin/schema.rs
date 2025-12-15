use cosmwasm_schema::write_api;

use rujira_rs::merge;

fn main() {
    write_api! {
        instantiate: merge::InstantiateMsg,
        execute: merge::ExecuteMsg,
        query: merge::QueryMsg,
    }
}
