use std::collections::HashMap;

use near_sdk::{env::panic_str, json_types::U128, near, AccountId};
use near_sdk_contract_tools::Nep297;

#[derive(Nep297)]
#[near(serializers = [json])]
#[nep297(standard = "nep171", version = "0.1.0", rename_all = "snake_case")]
pub enum LtipEvent {
    OrderUpdate(Vec<OrderUpdateData>),
    Terminate(TerminationData),
    Buy(BuyData),
}

#[near(serializers = [json])]
pub struct OrderUpdateData {
    pub issue_at: u32,
    pub amount: U128,
}

#[near(serializers = [json])]
pub struct TerminationData {
    pub account_id: AccountId,
    pub unvested_amounts: HashMap<u32, U128>,
}

impl TerminationData {
    pub fn new(account_id: &AccountId) -> Self {
        Self {
            account_id: account_id.clone(),
            unvested_amounts: HashMap::new(),
        }
    }

    pub fn push(&mut self, issued_at: u32, amount: u128) {
        self.unvested_amounts.insert(issued_at, amount.into());
    }

    pub fn get_total_amount(&self) -> u128 {
        self.unvested_amounts
            .iter()
            .map(|(_, amount)| amount.0)
            .sum()
    }
}

#[near(serializers = [json])]
pub struct BuyData {
    pub bought_amounts: HashMap<AccountId, U128>,
    pub total_amount: U128,
}

impl BuyData {
    pub fn new() -> Self {
        Self {
            bought_amounts: HashMap::new(),
            total_amount: 0.into(),
        }
    }

    pub fn push(&mut self, account_id: &AccountId, amount: u128) {
        if !self.bought_amounts.contains_key(account_id) {
            self.bought_amounts.insert(account_id.clone(), 0.into());
        }

        self.bought_amounts
            .get_mut(account_id)
            .unwrap_or_else(|| panic_str("Failed to get entry."))
            .0 += amount;
        self.total_amount.0 += amount;
    }
}
