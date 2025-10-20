use near_contract_standards::fungible_token::receiver::FungibleTokenReceiver;
use near_sdk::{
    env::panic_str, json_types::U128, near, require, serde_json, AccountId, PromiseOrValue,
};

use crate::{grant::GrantApi, Contract, ContractExt, Role};
use near_sdk_contract_tools::rbac::Rbac;

#[near(serializers = [json])]
#[serde(tag = "type", content = "data")]
#[serde(rename_all = "snake_case")]
pub enum FtMessage {
    TopUp,
    Issue(IssueData),
    Migrate(Vec<(AccountId, u32, U128, U128)>),
}

#[near(serializers = [json])]
pub struct IssueData {
    pub issue_date: u32,
    pub grants: Vec<(AccountId, U128)>,
}

#[near]
impl FungibleTokenReceiver for Contract {
    fn ft_on_transfer(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        msg: String,
    ) -> PromiseOrValue<U128> {
        let message: FtMessage =
            serde_json::from_str(&msg).unwrap_or_else(|_| panic_str("Failed to parse the message"));

        match message {
            FtMessage::TopUp => self.on_top_up(&sender_id, amount),
            FtMessage::Issue(issue_data) => self.on_issue(&sender_id, amount, issue_data),
            FtMessage::Migrate(accounts) => self.on_migrate(&sender_id, amount, accounts),
        }

        PromiseOrValue::Value(0.into())
    }
}

impl Contract {
    fn on_top_up(&mut self, sender_id: &AccountId, amount: U128) {
        Self::has_role(sender_id, &Role::Issuer);

        self.spare_balance.0 += amount.0;
    }

    fn on_issue(&mut self, sender_id: &AccountId, amount: U128, issue_data: IssueData) {
        Self::has_role(sender_id, &Role::Issuer);

        let total_amount: u128 = issue_data.grants.iter().map(|(_, amount)| amount.0).sum();
        require!(
            total_amount == amount.0,
            "Transferred amount doesn't match total grants amount"
        );

        self.issue(issue_data.issue_date, issue_data.grants);
    }

    fn on_migrate(
        &mut self,
        sender_id: &AccountId,
        amount: U128,
        accounts: Vec<(AccountId, u32, U128, U128)>,
    ) {
        Self::has_role(sender_id, &Role::Predecessor);

        let total_amount: u128 = accounts
            .iter()
            .map(|(_, _, total_amount, claimed_amount)| total_amount.0 - claimed_amount.0)
            .sum();
        require!(
            total_amount == amount.0,
            "Transferred amount doesn't match total grants amount"
        );

        for (account_id, issue_date, total_amount, claimed_amount) in accounts.into_iter() {
            self.create_grant_internal(&account_id, issue_date, total_amount, Some(claimed_amount));
        }
    }
}
