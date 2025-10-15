use near_contract_standards::fungible_token::receiver::FungibleTokenReceiver;
use near_sdk::{
    env::panic_str, json_types::U128, near, require, serde_json, AccountId, PromiseOrValue,
};

use crate::{Contract, ContractExt};

#[near(serializers = [json])]
pub enum FtMessage {
    TopUp,
    Issue(IssueData),
}

#[near(serializers = [json])]
pub struct IssueData {
    pub issue_timestamp: u32,
    pub grants: Vec<(AccountId, U128)>,
}

#[near]
impl FungibleTokenReceiver for Contract {
    fn ft_on_transfer(
        &mut self,
        _sender_id: AccountId,
        amount: U128,
        msg: String,
    ) -> PromiseOrValue<U128> {
        let message: FtMessage =
            serde_json::from_str(&msg).unwrap_or_else(|_| panic_str("Failed to parse the message"));

        match message {
            FtMessage::TopUp => self.on_top_up(amount),
            FtMessage::Issue(issue_data) => self.on_issue(amount, issue_data),
        }

        PromiseOrValue::Value(0.into())
    }
}

impl Contract {
    fn on_top_up(&mut self, amount: U128) {
        self.spare_balance.0 += amount.0;
    }

    fn on_issue(&mut self, amount: U128, issue_data: IssueData) {
        let total_amount: u128 = issue_data.grants.iter().map(|(_, amount)| amount.0).sum();
        require!(
            total_amount == amount.0,
            "Transferred amount doesn't match total grants amount"
        );

        self.issue(issue_data.issue_timestamp, issue_data.grants);
    }
}
