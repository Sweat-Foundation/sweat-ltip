pub mod auth;
pub mod common;
pub mod config;
pub mod event;
mod ft_receiver;
pub mod grant;
pub mod init;
#[cfg(test)]
pub mod tests;
pub mod vesting;

#[cfg(test)]
pub mod testing_api;

use std::collections::HashMap;

use near_sdk::{
    env::panic_str, json_types::U128, near, store::IterableMap, AccountId, BorshStorageKey,
    PanicOnDefault,
};
use near_sdk_contract_tools::{Owner, Pause, Rbac, Upgrade};

pub use auth::Role;

#[derive(BorshStorageKey)]
#[near]
pub(crate) enum StorageKey {
    Accounts,
    Pause,
}

#[near(contract_state)]
#[derive(Owner, PanicOnDefault, Pause, Rbac, Upgrade)]
#[pause(storage_key = StorageKey::Pause)]
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

impl Contract {
    pub(crate) fn get_account_mut(&mut self, account_id: &AccountId) -> &mut Account {
        self.accounts
            .get_mut(account_id)
            .unwrap_or_else(|| panic_str(format!("Account {account_id} was not found.").as_str()))
    }
}

impl Account {
    pub(crate) fn get_grant_mut(&mut self, issued_at: &u32) -> &mut Grant {
        self.grants.get_mut(issued_at).unwrap_or_else(|| {
            panic_str(
                format!("The grant issued at {issued_at} for the account was not found.").as_str(),
            );
        })
    }
}

#[near(serializers = [borsh, json])]
#[derive(Clone)]
pub struct Grant {
    pub total_amount: U128,
    pub claimed_amount: U128,
    pub order_amount: U128,
    pub terminated_at: Option<u32>,
}

#[near(serializers = [borsh, json])]
#[derive(Clone)]
pub struct Config {
    pub cliff_duration: u32,
    pub vesting_duration: u32,
}
