#![cfg(test)]

use near_sdk::{test_utils::VMContextBuilder, testing_env, AccountId, Gas, PromiseResult};

use super::fixtures::{executor, issuer};

fn contract_account_id() -> AccountId {
    "contract.test.near".parse().unwrap()
}

#[derive(Default)]
pub struct TestContext {
    builder: VMContextBuilder,
}

impl TestContext {
    pub fn new() -> Self {
        let mut builder = VMContextBuilder::new();
        builder
            .current_account_id(contract_account_id())
            .predecessor_account_id(contract_account_id())
            .signer_account_id(contract_account_id());

        testing_env!(builder.build());

        Self { builder }
    }

    pub fn set_block_timestamp_in_seconds(&mut self, timestamp: u32) {
        testing_env!(self
            .builder
            .block_timestamp(u64::from(timestamp) * 1_000_000_000)
            .build());
    }

    pub fn switch_account(&mut self, account_id: &AccountId) {
        testing_env!(self
            .builder
            .signer_account_id(account_id.clone())
            .predecessor_account_id(account_id.clone())
            .build())
    }

    pub fn switch_to_issuer(&mut self) {
        self.switch_account(&issuer());
    }

    pub fn switch_to_executor(&mut self) {
        self.switch_account(&executor());
    }

    pub fn set_promise_results(&mut self, results: Vec<PromiseResult>) {
        testing_env!(
            self.builder.build(),
            near_sdk::test_vm_config(),
            near_sdk::RuntimeFeesConfig::test(),
            Default::default(),
            results,
        );
    }

    pub fn with_gas_attached<F, R>(&mut self, gas: Gas, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        testing_env!(self.builder.prepaid_gas(gas).build());

        let result = f();

        testing_env!(self.builder.prepaid_gas(Gas::from_tgas(100)).build());

        result
    }
}
