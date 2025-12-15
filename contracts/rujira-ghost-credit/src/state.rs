use cw_storage_plus::Map;
use rujira_rs::ghost::vault::Vault;

/// Contracts and messages to borrow String denom
pub static BORROW: Map<String, Vault> = Map::new("borrow");
