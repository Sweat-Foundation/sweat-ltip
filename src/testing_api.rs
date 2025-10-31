#![cfg(test)]

use near_sdk::{
    json_types::U128,
    test_utils::{accounts, VMContextBuilder},
    testing_env, AccountId,
};

use crate::{auth::Role, init::InitApi, Contract};
use near_sdk_contract_tools::rbac::Rbac;

pub const DEFAULT_CLIFF: u32 = 1_000;
pub const DEFAULT_UNLOCK: u32 = 2_000;

pub fn get_context(predecessor_account_id: AccountId) -> VMContextBuilder {
    let mut builder = VMContextBuilder::new();
    builder
        .current_account_id(accounts(0))
        .signer_account_id(predecessor_account_id.clone())
        .predecessor_account_id(predecessor_account_id);
    builder
}

pub fn set_predecessor(account: &AccountId, timestamp_in_seconds: u64) {
    let mut context = get_context(account.clone());
    context.block_timestamp(timestamp_in_seconds * 1_000_000);
    testing_env!(context.build());
}

pub fn set_predecessor_with_time(account: &AccountId, timestamp: u64) {
    set_predecessor(account, timestamp);
}

pub fn init_contract_with_spare(spare_balance: u128) -> Contract {
    let mut contract = Contract::new(accounts(0), DEFAULT_CLIFF, DEFAULT_UNLOCK, accounts(0));

    contract.spare_balance = spare_balance.into();
    contract.add_role(&accounts(0), &Role::Executor);
    contract.add_role(&accounts(0), &Role::Issuer);

    contract
}

pub fn init_contract_with_grant(total_amount: U128) -> Contract {
    let mut contract = init_contract_with_spare(total_amount.0);
    contract.create_grant_internal(&accounts(1), DEFAULT_CLIFF, total_amount, None);

    contract
}
