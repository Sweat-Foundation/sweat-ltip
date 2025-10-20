pub mod auth;
mod ft_receiver;
pub mod grant;
pub mod init;

#[cfg(test)]
pub mod testing_api;

use std::collections::HashMap;

use near_sdk::{
    json_types::U128, near, store::IterableMap, AccountId, BorshStorageKey, PanicOnDefault,
};
use near_sdk_contract_tools::{Owner, Rbac, Upgrade};

pub use auth::Role;

#[derive(BorshStorageKey)]
#[near]
pub(crate) enum StorageKey {
    Accounts,
}

#[near(contract_state)]
#[derive(Owner, PanicOnDefault, Rbac, Upgrade)]
#[rbac(roles = Role)]
#[upgrade(serializer = "borsh", hook = "owner")]
pub struct Contract {
    pub token_id: AccountId,
    pub accounts: IterableMap<AccountId, Account>,
    pub config: Config,
    pub spare_balance: U128,
    pub pending_transfers: HashMap<AccountId, Vec<(u32, U128)>>,
}

#[near(serializers = [borsh, json])]
#[derive(Clone)]
pub struct Account {
    pub grants: HashMap<u32, Grant>,
}

#[near(serializers = [borsh, json])]
#[derive(Clone)]
pub struct Grant {
    pub total_amount: U128,
    pub claimed_amount: U128,
    pub order_amount: U128,
}

#[near(serializers = [borsh, json])]
pub struct Config {
    pub cliff_duration: u32,
    pub full_unlock_duration: u32,
}
