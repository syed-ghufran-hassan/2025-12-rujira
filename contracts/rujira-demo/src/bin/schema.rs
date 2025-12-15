use cosmwasm_schema::write_api;

use rujira_rs::demo;

fn main() {
    write_api! {
        query: demo::QueryMsg,
    }
}
