use std::{
    cmp,
    collections::{HashMap, HashSet},
};

use crate::{
    common::{assert_gas, now},
    event::{LtipEvent, OrderUpdateData},
    vesting::calculate_vested_amount,
    Account, Config, Contract, ContractExt, Grant, Role,
};
use near_sdk::{
    env::{self, log_str, panic_str},
    json_types::U128,
    near, require, serde_json, AccountId, NearToken, Promise, PromiseResult,
};
use near_sdk_contract_tools::{
    ft::nep141::GAS_FOR_FT_TRANSFER_CALL, pause::Pause, rbac::Rbac, standard::nep297::Event,
};

const GAS_FOR_CALLBACK: near_sdk::Gas = near_sdk::Gas::from_tgas(5);

#[derive(Clone, Debug, PartialEq, Eq)]
#[near(serializers = [json])]
pub struct TransferKey {
    pub account_id: AccountId,
    pub issue_at: u32,
}

#[near(serializers = [json])]
pub struct AccountView {
    pub account_id: AccountId,
    pub grants: Vec<GrantView>,
}

#[near(serializers = [json])]
pub struct GrantView {
    pub issued_at: u32,
    pub cliff_end_at: u32,
    pub vesting_end_at: u32,
    pub total_amount: U128,
    pub claimed_amount: U128,
    pub order_amount: U128,
    pub vested_amount: U128,
    pub not_vested_amount: U128,
    pub claimable_amount: U128,
    pub terminated_at: Option<u32>,
}

/// GrantApi encapsulates vesting-related actions such as claiming, issuing, buybacks, and termination logic.
pub trait GrantApi {
    /// Processes the caller's grants and accrues any newly unlocked amounts into their order balance.
    fn claim(&mut self);

    /// Authorizes payment for outstanding orders on the supplied accounts using an optional basis-point percentage.
    fn authorize(&mut self, account_ids: Vec<AccountId>, percentage: Option<u32>);

    /// Callback invoked after batched FT transfers to reconcile pending transfers with on-chain state.
    fn on_authorize_complete(&mut self, transfer_keys: Vec<TransferKey>);

    /// Issues grants for the specified timestamp, reducing spare balance accordingly.
    fn issue(&mut self, issue_at: u32, grants: Vec<(AccountId, U128)>);

    /// Executes a buyback against the provided accounts by the given percentage (basis points).
    fn buy(&mut self, account_ids: Vec<AccountId>, percentage: u32);

    /// Returns all outstanding orders (account, issue date, order amount).
    fn get_orders(&self) -> Vec<(AccountId, u32, U128)>;

    /// Retrieves a copy of the stored account, if present.
    fn get_account(&self, account_id: &AccountId) -> Option<AccountView>;

    /// Returns the contract's spare balance.
    fn get_spare_balance(&self) -> U128;

    /// Returns a copy of the pending transfers accumulated during authorization flow.
    fn get_pending_transfers(&self) -> HashMap<AccountId, Vec<(u32, U128)>>;

    /// Terminates an account's grants at the provided timestamp, adjusting totals to reflect vested amounts.
    fn terminate(&mut self, account_id: AccountId, timestamp: u32);
}

#[near]
impl GrantApi for Contract {
    fn claim(&mut self) {
        Self::require_unpaused();

        let caller = env::predecessor_account_id();

        let pending_issue_ats: HashSet<u32> = self
            .pending_transfers
            .get(&caller)
            .map(|transfers| transfers.iter().map(|(date, _)| *date).collect())
            .unwrap_or_default();

        let account = self.accounts.entry(caller.clone()).or_insert(Account {
            grants: Default::default(),
        });

        if account.grants.is_empty() {
            return;
        }

        let mut event_data = vec![];

        for (issue_at, grant) in account.grants.iter_mut() {
            if pending_issue_ats.contains(issue_at) {
                continue;
            }

            let vested_amount = grant.get_vested_amount(*issue_at, &self.config);

            if vested_amount == 0 {
                continue;
            }

            grant.order_amount.0 = vested_amount - grant.claimed_amount.0;

            event_data.push(OrderUpdateData {
                issue_at: issue_at.clone(),
                amount: grant.order_amount,
            });
        }

        LtipEvent::OrderUpdate(event_data).emit();
    }

    fn authorize(&mut self, account_ids: Vec<AccountId>, percentage: Option<u32>) {
        Self::require_role(&Role::Executor);
        Self::require_unpaused();

        self.pause();

        let percentage = percentage.unwrap_or(10_000);
        if percentage == 0 {
            self.decline_orders(account_ids);
            return;
        }

        self.pending_transfers.clear();
        let mut transfers = Vec::new();
        let mut transfer_keys = Vec::new();

        for account_id in account_ids {
            let pending_issue_ats: HashSet<u32> = self
                .pending_transfers
                .get(&account_id)
                .map(|transfers| transfers.iter().map(|(date, _)| *date).collect())
                .unwrap_or_default();

            if let Some(account) = self.accounts.get_mut(&account_id) {
                let mut account_transfers = Vec::new();

                for (issue_at, grant) in account.grants.iter_mut() {
                    if pending_issue_ats.contains(issue_at) {
                        continue;
                    }

                    let order_amount = grant.order_amount.0;
                    if order_amount == 0 {
                        continue;
                    }

                    let authorized_amount = (order_amount * percentage as u128) / 10_000;
                    if authorized_amount == 0 {
                        continue;
                    }

                    grant.claimed_amount = U128::from(grant.claimed_amount.0 + authorized_amount);
                    transfers.push((account_id.clone(), authorized_amount));
                    transfer_keys.push(TransferKey {
                        account_id: account_id.clone(),
                        issue_at: *issue_at,
                    });
                    account_transfers.push((*issue_at, U128::from(authorized_amount)));
                    grant.order_amount = U128::from(0);
                }

                if !account_transfers.is_empty() {
                    self.pending_transfers
                        .insert(account_id.clone(), account_transfers);
                }
            }
        }

        if transfers.is_empty() {
            return;
        }

        assert_gas(
            (GAS_FOR_FT_TRANSFER_CALL.saturating_add(GAS_FOR_CALLBACK)).as_gas()
                * transfers.len() as u64,
            || "Transfer on `authorize` call.",
        );

        let mut batch_promise = Promise::new(self.token_id.clone());
        for (account_id, amount) in transfers {
            batch_promise = batch_promise.function_call(
                "ft_transfer".to_string(),
                serde_json::to_vec(&serde_json::json!({
                    "receiver_id": account_id,
                    "amount": amount.to_string()
                }))
                .unwrap(),
                NearToken::from_yoctonear(1),
                GAS_FOR_FT_TRANSFER_CALL,
            );
        }

        batch_promise.then(
            Promise::new(env::current_account_id()).function_call(
                "on_authorize_complete".to_string(),
                serde_json::to_vec(&serde_json::json!({
                    "transfer_keys": transfer_keys
                }))
                .unwrap(),
                NearToken::from_yoctonear(0),
                GAS_FOR_CALLBACK,
            ),
        );
    }

    #[private]
    fn on_authorize_complete(&mut self, transfer_keys: Vec<TransferKey>) {
        log_str(&format!(
            "Authorize batch completed: {} transfers processed",
            transfer_keys.len()
        ));
        Self::require_paused();

        for (transfer_index, transfer_key) in transfer_keys.iter().enumerate() {
            #[allow(unreachable_patterns)]
            match env::promise_result(transfer_index as u64) {
                PromiseResult::Successful(_) => {
                    log_str(&format!("Transfer {} succeeded", transfer_index));
                }
                PromiseResult::Failed => {
                    log_str(&format!(
                        "Transfer {} failed, reverting claimed_amount",
                        transfer_index
                    ));

                    let failed_amount = self
                        .pending_transfers
                        .get(&transfer_key.account_id)
                        .and_then(|account_transfers| {
                            account_transfers
                                .iter()
                                .find(|(issue_at, _)| issue_at == &transfer_key.issue_at)
                                .map(|(_, amount)| amount.0)
                        });

                    if let Some(amount) = failed_amount {
                        if let Some(account) = self.accounts.get_mut(&transfer_key.account_id) {
                            if let Some(grant) = account.grants.get_mut(&transfer_key.issue_at) {
                                grant.claimed_amount.0 -= amount;
                                grant.order_amount.0 += amount;
                            }
                        }
                    } else {
                        log_str(&format!(
                            "No pending transfer entry for {} at issue date {}",
                            transfer_key.account_id, transfer_key.issue_at
                        ));
                    }
                }
                _ => {}
            }
        }

        self.pending_transfers.clear();
        self.unpause();
    }

    fn issue(&mut self, issue_at: u32, grants: Vec<(AccountId, U128)>) {
        Self::require_role(&Role::Issuer);

        self.issue_internal(issue_at, grants);
    }

    fn buy(&mut self, account_ids: Vec<AccountId>, percentage: u32) {
        Self::require_role(&Role::Executor);
        Self::require_unpaused();

        if percentage == 0 {
            self.decline_orders(account_ids);
            return;
        }

        for account_id in account_ids {
            let pending_issue_ats: HashSet<u32> = self
                .pending_transfers
                .get(&account_id)
                .map(|transfers| transfers.iter().map(|(date, _)| *date).collect())
                .unwrap_or_default();

            if let Some(account) = self.accounts.get_mut(&account_id) {
                for (issue_at, grant) in account.grants.iter_mut() {
                    if pending_issue_ats.contains(issue_at) {
                        continue;
                    }

                    let order_amount = grant.order_amount.0;
                    if order_amount == 0 {
                        continue;
                    }

                    let bought_amount = (order_amount * percentage as u128) / 10_000;
                    grant.claimed_amount = U128::from(grant.claimed_amount.0 + bought_amount);
                    grant.order_amount = U128::from(0);
                    self.spare_balance.0 += bought_amount;
                }
            }
        }
    }

    fn get_orders(&self) -> Vec<(AccountId, u32, U128)> {
        let mut orders = Vec::new();
        for (account_id, account) in self.accounts.iter() {
            for (issue_at, grant) in account.grants.iter() {
                if grant.order_amount.0 > 0 {
                    orders.push((account_id.clone(), *issue_at, grant.order_amount));
                }
            }
        }
        orders
    }

    fn get_account(&self, account_id: &AccountId) -> Option<AccountView> {
        if let Some(account) = self.accounts.get(account_id) {
            let grants = account
                .grants
                .iter()
                .map(|(issue_at, grant)| {
                    let cliff_end_at = *issue_at + self.config.cliff_duration;
                    let vesting_end_at = cliff_end_at + self.config.vesting_duration;

                    let vested_amount = grant.get_vested_amount(*issue_at, &self.config);

                    GrantView {
                        issued_at: *issue_at,
                        cliff_end_at,
                        vesting_end_at,
                        total_amount: grant.total_amount,
                        claimed_amount: grant.claimed_amount,
                        order_amount: grant.order_amount,
                        vested_amount: vested_amount.into(),
                        not_vested_amount: (grant.total_amount.0 - vested_amount).into(),
                        claimable_amount: (vested_amount - grant.claimed_amount.0).into(),
                        terminated_at: grant.terminated_at,
                    }
                })
                .collect();

            return Some(AccountView {
                account_id: account_id.clone(),
                grants,
            });
        }

        None
    }

    fn get_spare_balance(&self) -> U128 {
        self.spare_balance
    }

    fn get_pending_transfers(&self) -> HashMap<AccountId, Vec<(u32, U128)>> {
        self.pending_transfers.clone()
    }

    fn terminate(&mut self, account_id: AccountId, timestamp: u32) {
        Self::require_role(&Role::Executor);
        Self::require_unpaused();

        if let Some(account) = self.accounts.get_mut(&account_id) {
            for (issue_at, grant) in account.grants.iter_mut() {
                let unvested_amount = grant.terminate(*issue_at, &self.config, timestamp);

                self.spare_balance.0 += unvested_amount;
            }
        }
    }
}

impl Contract {
    fn decline_orders(&mut self, account_ids: Vec<AccountId>) {
        for account_id in account_ids {
            let pending_issue_ats: HashSet<u32> = self
                .pending_transfers
                .get(&account_id)
                .map(|transfers| transfers.iter().map(|(date, _)| *date).collect())
                .unwrap_or_default();

            if let Some(account) = self.accounts.get_mut(&account_id) {
                for (issue_at, grant) in account.grants.iter_mut() {
                    if !pending_issue_ats.contains(issue_at) {
                        grant.order_amount = U128::from(0);
                    }
                }

                log_str(&format!(
                    "Declined orders for account {} (skipped pending transfers)",
                    account_id
                ));
            }
        }

        self.unpause();
    }

    pub(crate) fn create_grant_internal(
        &mut self,
        account_id: &AccountId,
        issue_at: u32,
        total_amount: U128,
        claimed_amount: Option<U128>,
    ) {
        let account = self.accounts.entry(account_id.clone()).or_insert(Account {
            grants: HashMap::new(),
        });

        require!(
            !account.grants.contains_key(&issue_at),
            "A grant has alredy been issued on this date"
        );

        let grant = Grant {
            total_amount,
            claimed_amount: claimed_amount.unwrap_or_else(|| U128::from(0)),
            order_amount: U128::from(0),
            terminated_at: None,
        };

        account.grants.insert(issue_at, grant);
    }

    pub(crate) fn issue_internal(&mut self, issue_at: u32, grants: Vec<(AccountId, U128)>) {
        Self::require_unpaused();

        let total_amount: u128 = grants.iter().map(|(_, amount)| amount.0).sum();
        if total_amount > self.spare_balance.0 {
            env::panic_str(&format!(
                "Insufficient spare balance: required {}, available {}",
                total_amount, self.spare_balance.0
            ));
        }

        for (account_id, amount) in grants {
            self.create_grant_internal(&account_id, issue_at, amount, None);
        }

        self.spare_balance = U128::from(self.spare_balance.0 - total_amount);

        log_str(&format!(
            "Issued grants with total amount {} at timestamp {}",
            total_amount, issue_at
        ));
    }
}

#[cfg(test)]
impl Contract {
    pub fn clear_pending_transfers(&mut self) {
        self.pending_transfers.clear();
    }
}

impl Grant {
    pub(crate) fn get_vested_amount(&self, issue_at: u32, config: &Config) -> u128 {
        let now = now();

        let cliff_end = issue_at + config.cliff_duration;
        let effective_vesting_duration = self
            .terminated_at
            .map_or(config.vesting_duration, |t| t.saturating_sub(cliff_end));

        calculate_vested_amount(
            now,
            cliff_end,
            cliff_end + effective_vesting_duration,
            self.total_amount.0,
        )
    }

    pub(crate) fn terminate(&mut self, issue_at: u32, config: &Config, terminate_at: u32) -> u128 {
        if self.terminated_at.is_some() {
            return 0;
        }

        let cliff_end = config.cliff_end(issue_at);
        let vesting_end = config.vesting_end(issue_at);

        if terminate_at > vesting_end {
            return 0;
        }

        let now = now();
        let vested_amount =
            calculate_vested_amount(now, cliff_end, vesting_end, self.total_amount.0);

        if vested_amount >= self.claimed_amount.0 {
            self.terminated_at = terminate_at.into();
            self.order_amount.0 =
                cmp::min(self.order_amount.0, vested_amount - self.claimed_amount.0);

            let unvested_amount = self.total_amount.0 - vested_amount;
            self.total_amount.0 = vested_amount;

            return unvested_amount;
        }

        let effective_vesting_duration =
            u32::try_from(self.claimed_amount.0 / self.performance(config.vesting_duration))
                .unwrap_or_else(|_| panic_str("Failed to evaluate effective vesting duration"));

        self.terminated_at = (issue_at + config.cliff_duration + effective_vesting_duration).into();
        self.order_amount.0 = 0;

        let unvested_amount = self.total_amount.0 - self.claimed_amount.0;
        self.total_amount = self.claimed_amount;

        return unvested_amount;
    }

    /// Amount of tokens being vested per second
    fn performance(&self, vesting_duration: u32) -> u128 {
        self.total_amount.0 / u128::from(vesting_duration)
    }
}

impl Config {
    fn cliff_end(&self, issue_at: u32) -> u32 {
        issue_at + self.cliff_duration
    }

    fn vesting_end(&self, issue_at: u32) -> u32 {
        self.cliff_end(issue_at) + self.vesting_duration
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{self, AssertUnwindSafe};

    use near_sdk::{json_types::U128, test_utils::accounts, AccountId, Gas, PromiseResult};
    use near_sdk_contract_tools::pause::Pause;
    use rstest::*;

    use crate::{
        common::{ToOtto, ONE_DAY_IN_SECONDS, ONE_YEAR_IN_SECONDS},
        grant::{GrantApi, TransferKey},
        testing_api::DEFAULT_CLIFF,
        tests::context::TestContext,
        tests::fixtures::*,
        Contract,
    };

    #[rstest]
    fn claim_during_cliff_does_nothing(
        mut context: TestContext,
        mut contract: Contract,
        alice: AccountId,
    ) {
        contract.create_grant_internal(&alice, 1_000, 10_000.into(), None);

        context.switch_account(&alice);
        context.set_block_timestamp_in_seconds(1_500);

        contract.claim();

        let account = contract.accounts.get(&alice).unwrap();
        let grant = account.grants.get(&1_000).unwrap();
        assert_eq!(grant.order_amount.0, 0);
    }

    #[rstest]
    fn claim_after_unlock_accumulates(
        mut context: TestContext,
        mut contract: Contract,
        alice: AccountId,
    ) {
        contract.create_grant_internal(&alice, 1_000, 10_000.into(), None);

        context.switch_account(&alice);
        context.set_block_timestamp_in_seconds(4_000);

        contract.claim();

        let account = contract.accounts.get(&alice).unwrap();
        let grant = account.grants.get(&1_000).unwrap();
        assert_eq!(grant.order_amount.0, 10_000);
    }

    #[rstest]
    fn buy_updates_claimed_and_spare_balance(
        mut context: TestContext,
        mut contract: Contract,
        alice: AccountId,
    ) {
        contract.create_grant_internal(&alice, 1_000, 10_000.into(), None);

        let initial_spare = contract.spare_balance.0;

        {
            let account = contract.accounts.get_mut(&alice).unwrap();
            let grant = account.grants.get_mut(&1_000).unwrap();
            grant.order_amount = U128::from(5_000);
        }

        context.switch_to_executor();
        contract.buy(vec![alice.clone()], 5_000);

        let account = contract.accounts.get(&alice).unwrap();
        let grant = account.grants.get(&1_000).unwrap();
        assert_eq!(grant.claimed_amount.0, 2_500);
        assert_eq!(grant.order_amount.0, 0);
        assert_eq!(contract.spare_balance.0, initial_spare + 2_500);
    }

    #[rstest]
    fn issue_reduces_spare_balance(mut context: TestContext, mut contract: Contract) {
        contract.spare_balance = 10_000.into();

        context.switch_to_issuer();
        contract.issue(
            1_000,
            vec![
                (accounts(1), U128::from(3_000)),
                (accounts(2), U128::from(2_000)),
            ],
        );

        assert_eq!(contract.spare_balance.0, 5_000);
        assert!(contract.accounts.get(&accounts(1)).is_some());
    }

    #[rstest]
    fn issue_requires_issuer_role(
        mut context: TestContext,
        mut contract: Contract,
        alice: AccountId,
        bob: AccountId,
    ) {
        contract.spare_balance = 10_000.into();

        context.switch_account(&alice);
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            contract.issue(1_000, vec![(bob.clone(), 1_000.into())]);
        }));

        assert!(result.is_err());
        assert!(contract.accounts.get(&bob).is_none());
        assert_eq!(contract.spare_balance.0, 10_000);
    }

    #[rstest]
    fn authorize_moves_order_into_pending_transfers(
        mut context: TestContext,
        mut contract: Contract,
        alice: AccountId,
    ) {
        contract.create_grant_internal(&alice, 1_000, 10_000.into(), None);

        {
            let account = contract.accounts.get_mut(&alice).unwrap();
            let grant = account.grants.get_mut(&1_000).unwrap();
            grant.order_amount = U128::from(4_000);
        }

        context.switch_to_executor();
        contract.authorize(vec![alice.clone()], Some(5_000));

        let pending = contract.get_pending_transfers();
        assert!(pending.contains_key(&alice));
        let transfers = pending.get(&alice).unwrap();
        assert_eq!(transfers[0].1 .0, 2_000);
    }

    #[rstest]
    fn authorize_requires_executor_role(
        mut context: TestContext,
        mut contract: Contract,
        alice: AccountId,
    ) {
        contract.create_grant_internal(&alice, 1_000, 10_000.into(), None);

        context.switch_account(&alice);
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            contract.authorize(vec![alice.clone()], Some(10_000));
        }));

        assert!(result.is_err());
        assert!(contract.get_pending_transfers().is_empty());
    }

    #[rstest]
    #[should_panic(expected = "Not enough gas left.")]
    fn authorize_fails_with_insufficient_gas(
        mut context: TestContext,
        mut contract: Contract,
        alice: AccountId,
    ) {
        contract.create_grant_internal(&alice, 1_000, 10_000.into(), None);

        {
            let grant = contract
                .accounts
                .get_mut(&alice)
                .unwrap()
                .grants
                .get_mut(&DEFAULT_CLIFF)
                .unwrap();
            grant.order_amount = U128::from(100);
        }

        context.switch_to_executor();
        context.with_gas_attached(Gas::from_tgas(1), || {
            contract.authorize(vec![alice.clone()], Some(10_000));
        });
    }
    #[rstest]
    fn on_authorize_complete_reverts_failed_transfers_using_keys(
        mut context: TestContext,
        mut contract: Contract,
        alice: AccountId,
        bob: AccountId,
    ) {
        contract.create_grant_internal(&alice, DEFAULT_CLIFF, U128::from(1_000), None);
        contract.create_grant_internal(&bob, DEFAULT_CLIFF, U128::from(1_000), None);

        {
            let grant = contract
                .accounts
                .get_mut(&alice)
                .unwrap()
                .grants
                .get_mut(&DEFAULT_CLIFF)
                .unwrap();
            grant.claimed_amount = U128::from(100);
        }

        {
            let grant = contract
                .accounts
                .get_mut(&bob)
                .unwrap()
                .grants
                .get_mut(&DEFAULT_CLIFF)
                .unwrap();
            grant.claimed_amount = U128::from(200);
        }

        contract
            .pending_transfers
            .insert(alice.clone(), vec![(DEFAULT_CLIFF, U128::from(100))]);
        contract
            .pending_transfers
            .insert(bob.clone(), vec![(DEFAULT_CLIFF, U128::from(200))]);

        context.set_promise_results(vec![
            PromiseResult::Successful(vec![]),
            PromiseResult::Failed,
        ]);

        contract.pause();
        contract.on_authorize_complete(vec![
            TransferKey {
                account_id: alice.clone(),
                issue_at: DEFAULT_CLIFF,
            },
            TransferKey {
                account_id: bob.clone(),
                issue_at: DEFAULT_CLIFF,
            },
        ]);

        let account_one_state = contract.accounts.get(&alice).unwrap();
        let grant_one = account_one_state.grants.get(&DEFAULT_CLIFF).unwrap();
        assert_eq!(grant_one.claimed_amount.0, 100);
        assert_eq!(grant_one.order_amount.0, 0);

        let account_two_state = contract.accounts.get(&bob).unwrap();
        let grant_two = account_two_state.grants.get(&DEFAULT_CLIFF).unwrap();
        assert_eq!(grant_two.claimed_amount.0, 0);
        assert_eq!(grant_two.order_amount.0, 200);

        assert!(contract.pending_transfers.is_empty());
    }

    #[rstest]
    fn terminate_respects_cliff(
        mut context: TestContext,
        mut contract: Contract,
        alice: AccountId,
    ) {
        contract.create_grant_internal(&alice, DEFAULT_CLIFF, U128::from(10_000), None);

        {
            let account = contract.accounts.get_mut(&alice).unwrap();
            let grant = account.grants.get_mut(&1_000).unwrap();
            grant.claimed_amount = U128::from(2_000);
            grant.order_amount = U128::from(3_000);
        }

        context.switch_to_executor();
        contract.terminate(alice.clone(), 1_500);

        let account = contract.accounts.get(&alice).unwrap();
        let grant = account.grants.get(&1_000).unwrap();
        assert_eq!(grant.order_amount.0, 0);
        assert_eq!(grant.total_amount.0, 2_000);
    }

    #[rstest]
    fn buy_requires_executor_role(
        mut context: TestContext,
        mut contract: Contract,
        alice: AccountId,
    ) {
        contract.create_grant_internal(&alice, DEFAULT_CLIFF, U128::from(10_000), None);

        {
            let account = contract.accounts.get_mut(&alice).unwrap();
            let grant = account.grants.get_mut(&1_000).unwrap();
            grant.order_amount = U128::from(4_000);
        }

        context.switch_account(&alice);
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            contract.buy(vec![alice.clone()], 5_000);
        }));

        assert!(result.is_err());
        let grant = contract
            .accounts
            .get(&alice)
            .unwrap()
            .grants
            .get(&1_000)
            .unwrap();
        assert_eq!(grant.order_amount.0, 4_000);
        assert_eq!(grant.claimed_amount.0, 0);
    }

    #[rstest]
    fn terminate_requires_executor_role(
        mut context: TestContext,
        mut contract: Contract,
        alice: AccountId,
        bob: AccountId,
    ) {
        contract.create_grant_internal(&alice, DEFAULT_CLIFF, U128::from(10_000), None);

        context.switch_account(&bob);
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            contract.terminate(alice.clone(), 1_500);
        }));

        assert!(result.is_err());
        let grant = contract
            .accounts
            .get(&alice)
            .unwrap()
            .grants
            .get(&1_000)
            .unwrap();
        assert_eq!(grant.total_amount.0, 10_000);
    }

    const GRANT_CLIFF_DURATION: u32 = ONE_YEAR_IN_SECONDS; // 1 year in seconds
    const GRANT_VESTING_DURATION: u32 = 3 * ONE_YEAR_IN_SECONDS; // 3 years in seconds

    #[rstest]
    fn test_terminate_before_cliff_cancels_order(
        mut context: TestContext,
        #[with(GRANT_CLIFF_DURATION, GRANT_VESTING_DURATION)] mut contract: Contract,
        alice: AccountId,
    ) {
        let grant_amount = 94_670_856u128.to_otto(); // 94670856 tokens
        let issue_at = 1_000;
        let cliff_end = issue_at + GRANT_CLIFF_DURATION;
        let terminate_at = cliff_end - ONE_DAY_IN_SECONDS;

        contract.create_grant_internal(&alice, issue_at, grant_amount.into(), None);

        // Claim at 1000 seconds after cliff end
        context.set_block_timestamp_in_seconds(cliff_end + 1_000);
        context.switch_account(&alice);
        contract.claim();

        let account = contract.accounts.get(&alice).unwrap();
        let grant = account.grants.get(&issue_at).unwrap();
        assert_eq!(grant.order_amount.0, 1_000u128.to_otto());

        // Terminate at cliff_end - one day (set block timestamp to termination time)
        context.switch_to_executor();
        context.set_block_timestamp_in_seconds(terminate_at);
        contract.terminate(alice.clone(), terminate_at);

        let account = contract.accounts.get(&alice).unwrap();
        let grant = account.grants.get(&issue_at).unwrap();
        assert_eq!(grant.order_amount.0, 0);
        assert_eq!(grant.total_amount.0, 0);
        assert_eq!(grant.claimed_amount.0, 0);
    }

    #[rstest]
    fn test_terminate_after_buy_sets_total_to_claimed(
        mut context: TestContext,
        #[with(GRANT_CLIFF_DURATION, GRANT_VESTING_DURATION)] mut contract: Contract,
        alice: AccountId,
    ) {
        let grant_amount = 94_670_856u128.to_otto(); // 94670856 tokens
        let issue_at = 1_000;
        let cliff_end = issue_at + GRANT_CLIFF_DURATION;

        contract.create_grant_internal(&alice, issue_at, grant_amount.into(), None);

        // Claim at 1000 seconds after cliff end
        context.set_block_timestamp_in_seconds(cliff_end + 1_000);
        context.switch_account(&alice);
        contract.claim();

        let account = contract.accounts.get(&alice).unwrap();
        let grant = account.grants.get(&issue_at).unwrap();
        assert_eq!(grant.order_amount.0, 1_000u128.to_otto());

        // Executor buys 100% of the order
        context.switch_to_executor();
        contract.buy(vec![alice.clone()], 10_000); // 100%

        let account = contract.accounts.get(&alice).unwrap();
        let grant = account.grants.get(&issue_at).unwrap();
        assert_eq!(grant.claimed_amount.0, 1_000u128.to_otto());
        assert_eq!(grant.order_amount.0, 0);

        // 1000 seconds later, terminate the grant at the timestamp when vested equals claimed
        // (terminate at cliff_end + 1000 to get total = claimed = 1000)
        let terminate_at = cliff_end + 1_000;
        context.set_block_timestamp_in_seconds(terminate_at);
        contract.terminate(alice.clone(), terminate_at);

        let account = contract.accounts.get(&alice).unwrap();
        let grant = account.grants.get(&issue_at).unwrap();
        assert_eq!(grant.total_amount.0, 1_000u128.to_otto());
        assert_eq!(grant.claimed_amount.0, 1_000u128.to_otto());
        assert_eq!(grant.order_amount.0, 0);
    }

    #[rstest]
    fn test_terminate_cuts_order_to_vested_amount(
        mut context: TestContext,
        #[with(GRANT_CLIFF_DURATION, GRANT_VESTING_DURATION)] mut contract: Contract,
        alice: AccountId,
    ) {
        let grant_amount = 94_670_856u128.to_otto(); // 94670856 tokens
        let issue_at = 1_000;
        let cliff_end = issue_at + GRANT_CLIFF_DURATION;
        let terminate_at = cliff_end + 500;

        contract.create_grant_internal(&alice, issue_at, grant_amount.into(), None);

        // Claim at 1000 seconds after cliff end
        context.set_block_timestamp_in_seconds(cliff_end + 1_000);
        context.switch_account(&alice);
        contract.claim();

        let account = contract.accounts.get(&alice).unwrap();
        let grant = account.grants.get(&issue_at).unwrap();
        assert_eq!(grant.order_amount.0, 1_000u128.to_otto());

        // Terminate at 500 seconds after cliff end (cutting the order)
        // Set block timestamp to termination time so vested calculation uses that
        context.switch_to_executor();
        context.set_block_timestamp_in_seconds(terminate_at);
        contract.terminate(alice.clone(), terminate_at);

        let account = contract.accounts.get(&alice).unwrap();
        let grant = account.grants.get(&issue_at).unwrap();
        assert_eq!(grant.order_amount.0, 500u128.to_otto());
        assert_eq!(grant.total_amount.0, 500u128.to_otto());
    }

    #[rstest]
    fn test_terminate_after_buy_preserves_claimed_amount(
        mut context: TestContext,
        #[with(GRANT_CLIFF_DURATION, GRANT_VESTING_DURATION)] mut contract: Contract,
        alice: AccountId,
    ) {
        let grant_amount = 94_670_856u128.to_otto(); // 94670856 tokens
        let issue_at = 1_000;
        let cliff_end = issue_at + GRANT_CLIFF_DURATION;
        let terminate_at = cliff_end + 500;

        contract.create_grant_internal(&alice, issue_at, grant_amount.into(), None);

        // Claim at 1000 seconds after cliff end
        context.set_block_timestamp_in_seconds(cliff_end + 1_000);
        context.switch_account(&alice);
        contract.claim();

        let account = contract.accounts.get(&alice).unwrap();
        let grant = account.grants.get(&issue_at).unwrap();
        assert_eq!(grant.order_amount.0, 1_000u128.to_otto());

        // Executor buys 100% of the order
        context.switch_to_executor();
        contract.buy(vec![alice.clone()], 10_000); // 100%

        let account = contract.accounts.get(&alice).unwrap();
        let grant = account.grants.get(&issue_at).unwrap();
        assert_eq!(grant.claimed_amount.0, 1_000u128.to_otto());

        // Terminate at 500 seconds after cliff end
        // Set block timestamp to termination time
        context.set_block_timestamp_in_seconds(terminate_at);
        contract.terminate(alice.clone(), terminate_at);

        let account = contract.accounts.get(&alice).unwrap();
        let grant = account.grants.get(&issue_at).unwrap();
        assert_eq!(grant.total_amount.0, 1_000u128.to_otto());
    }

    #[rstest]
    fn test_terminate_before_cliff_sets_total_to_zero(
        mut context: TestContext,
        #[with(GRANT_CLIFF_DURATION, GRANT_VESTING_DURATION)] mut contract: Contract,
        alice: AccountId,
    ) {
        let grant_amount = 94_670_856u128.to_otto(); // 94670856 tokens
        let issue_at = 1_000;
        let cliff_end = issue_at + GRANT_CLIFF_DURATION;
        let terminate_at = cliff_end - 1_000;

        contract.create_grant_internal(&alice, issue_at, grant_amount.into(), None);

        // Terminate 1000 seconds before cliff end
        context.switch_to_executor();
        context.set_block_timestamp_in_seconds(terminate_at);
        contract.terminate(alice.clone(), terminate_at);

        let account = contract.accounts.get(&alice).unwrap();
        let grant = account.grants.get(&issue_at).unwrap();
        assert_eq!(grant.total_amount.0, 0);
    }

    #[rstest]
    fn test_terminate_twice_fails(
        mut context: TestContext,
        #[with(GRANT_CLIFF_DURATION, GRANT_VESTING_DURATION)] mut contract: Contract,
        alice: AccountId,
    ) {
        let grant_amount = 94_670_856u128.to_otto(); // 94670856 tokens
        let issue_at = 1_000;
        let cliff_end = issue_at + GRANT_CLIFF_DURATION;

        contract.create_grant_internal(&alice, issue_at, grant_amount.into(), None);

        // Terminate 5000 seconds after cliff end
        context.switch_to_executor();
        let first_terminate_at = cliff_end + 5_000;
        context.set_block_timestamp_in_seconds(first_terminate_at);
        contract.terminate(alice.clone(), first_terminate_at);

        let account = contract.accounts.get(&alice).unwrap();
        let grant = account.grants.get(&issue_at).unwrap();
        assert_eq!(grant.total_amount.0, 5_000u128.to_otto());

        // Try to terminate again at 1000 seconds after cliff (should fail/no-op)
        // The terminate function returns early if already terminated, so it doesn't panic
        // but the state shouldn't change
        let second_terminate_at = cliff_end + 1_000;
        context.set_block_timestamp_in_seconds(second_terminate_at);
        contract.terminate(alice.clone(), second_terminate_at);

        let account = contract.accounts.get(&alice).unwrap();
        let grant = account.grants.get(&issue_at).unwrap();
        // Should remain unchanged (still at 5000)
        assert_eq!(grant.total_amount.0, 5_000u128.to_otto());
    }
}
