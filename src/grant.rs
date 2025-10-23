use std::collections::{HashMap, HashSet};

use crate::{
    event::{LtipEvent, OrderUpdateData},
    Account, Contract, ContractExt, Grant, Role,
};
use near_sdk::{
    env::{self, log_str},
    json_types::U128,
    near, require, serde_json, AccountId, NearToken, Promise, PromiseResult,
};
use near_sdk_contract_tools::{pause::Pause, rbac::Rbac, standard::nep297::Event};

const GAS_PER_TRANSFER: near_sdk::Gas = near_sdk::Gas::from_tgas(10);
const GAS_FOR_CALLBACK: near_sdk::Gas = near_sdk::Gas::from_tgas(5);

#[derive(Clone, Debug, PartialEq, Eq)]
#[near(serializers = [json])]
pub struct TransferKey {
    pub account_id: AccountId,
    pub issue_at: u32,
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
    fn get_account(&self, account_id: &AccountId) -> Option<Account>;

    /// Returns the contract's spare balance.
    fn get_spare_balance(&self) -> U128;

    /// Returns a copy of the pending transfers accumulated during authorization flow.
    fn get_pending_transfers(&self) -> HashMap<AccountId, Vec<(u32, U128)>>;

    /// Terminates an account's grants at the provided timestamp, adjusting totals to reflect vested amounts.
    fn terminate(&mut self, account_id: AccountId, timestamp: u64);
}

#[near]
impl GrantApi for Contract {
    fn claim(&mut self) {
        Self::require_unpaused();

        let caller = env::predecessor_account_id();
        let current_timestamp = env::block_timestamp();

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

            let cliff_end = (*issue_at as u64) + (self.config.cliff_duration as u64);
            let full_unlock = cliff_end + (self.config.full_unlock_duration as u64);

            if current_timestamp < cliff_end {
                continue;
            }

            let total_unlocked_amount = if current_timestamp >= full_unlock {
                grant.total_amount.0
            } else {
                let unlock_period_duration = full_unlock - cliff_end;
                let elapsed_since_cliff = current_timestamp - cliff_end;

                (grant.total_amount.0 * elapsed_since_cliff as u128)
                    / unlock_period_duration as u128
            };

            let outstanding = grant.claimed_amount.0 + grant.order_amount.0;
            if total_unlocked_amount > outstanding {
                grant.order_amount.0 += total_unlocked_amount - outstanding;
            }

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
                GAS_PER_TRANSFER,
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

    fn get_account(&self, account_id: &AccountId) -> Option<Account> {
        self.accounts.get(account_id).cloned()
    }

    fn get_spare_balance(&self) -> U128 {
        self.spare_balance
    }

    fn get_pending_transfers(&self) -> HashMap<AccountId, Vec<(u32, U128)>> {
        self.pending_transfers.clone()
    }

    fn terminate(&mut self, account_id: AccountId, timestamp: u64) {
        Self::require_role(&Role::Executor);
        Self::require_unpaused();

        self.decline_orders(vec![account_id.clone()]);

        if let Some(account) = self.accounts.get_mut(&account_id) {
            for (issue_at, grant) in account.grants.iter_mut() {
                let cliff_end = (*issue_at as u64) + (self.config.cliff_duration as u64);
                let full_unlock = cliff_end + (self.config.full_unlock_duration as u64);

                let unlocked_amount = if timestamp < cliff_end {
                    0
                } else if timestamp >= full_unlock {
                    grant.total_amount.0
                } else {
                    let unlock_period_duration = full_unlock - cliff_end;
                    let elapsed_since_cliff = timestamp - cliff_end;

                    (grant.total_amount.0 * elapsed_since_cliff as u128)
                        / unlock_period_duration as u128
                };

                let updated_amount = unlocked_amount.max(grant.claimed_amount.0);

                self.spare_balance.0 += grant.total_amount.0 - updated_amount;
                grant.total_amount = updated_amount.into();
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

#[cfg(test)]
mod tests {
    use std::panic::{self, AssertUnwindSafe};

    use near_sdk::{json_types::U128, test_utils::accounts, PromiseResult};
    use near_sdk_contract_tools::pause::Pause;

    use crate::{
        auth::{AuthApi, Role},
        grant::{GrantApi, TransferKey},
        testing_api::{
            get_context, init_contract_with_grant, init_contract_with_spare, set_predecessor,
            DEFAULT_CLIFF,
        },
    };

    #[test]
    fn claim_during_cliff_does_nothing() {
        let mut contract = init_contract_with_grant(U128::from(10_000));
        set_predecessor(&accounts(1), 1_500);

        contract.claim();

        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1_000).unwrap();
        assert_eq!(grant.order_amount.0, 0);
    }

    #[test]
    fn claim_after_unlock_accumulates() {
        let mut contract = init_contract_with_grant(U128::from(10_000));
        set_predecessor(&accounts(1), 4_000);

        contract.claim();

        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1_000).unwrap();
        assert_eq!(grant.order_amount.0, 10_000);
    }

    #[test]
    fn buy_updates_claimed_and_spare_balance() {
        let mut contract = init_contract_with_grant(U128::from(10_000));
        let user = accounts(1);
        let initial_spare = contract.spare_balance.0;

        {
            let account = contract.accounts.get_mut(&user).unwrap();
            let grant = account.grants.get_mut(&1_000).unwrap();
            grant.order_amount = U128::from(5_000);
        }

        set_predecessor(&accounts(0), 0);
        contract.buy(vec![user.clone()], 5_000);

        let account = contract.accounts.get(&user).unwrap();
        let grant = account.grants.get(&1_000).unwrap();
        assert_eq!(grant.claimed_amount.0, 2_500);
        assert_eq!(grant.order_amount.0, 0);
        assert_eq!(contract.spare_balance.0, initial_spare + 2_500);
    }

    #[test]
    fn issue_reduces_spare_balance() {
        let mut contract = init_contract_with_spare(10_000);
        set_predecessor(&accounts(0), 0);

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

    #[test]
    fn issue_requires_issuer_role() {
        let mut contract = init_contract_with_spare(10_000);

        set_predecessor(&accounts(1), 0);
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            contract.issue(1_000, vec![(accounts(2), U128::from(1_000))]);
        }));

        assert!(result.is_err());
        assert!(contract.accounts.get(&accounts(2)).is_none());
        assert_eq!(contract.spare_balance.0, 10_000);
    }

    #[test]
    fn authorize_moves_order_into_pending_transfers() {
        let mut contract = init_contract_with_grant(U128::from(10_000));
        let user = accounts(1);

        {
            let account = contract.accounts.get_mut(&user).unwrap();
            let grant = account.grants.get_mut(&1_000).unwrap();
            grant.order_amount = U128::from(4_000);
        }

        set_predecessor(&accounts(0), 0);
        contract.grant_role(&accounts(0), Role::Executor);

        contract.authorize(vec![user.clone()], Some(5_000));

        let pending = contract.get_pending_transfers();
        assert!(pending.contains_key(&user));
        let transfers = pending.get(&user).unwrap();
        assert_eq!(transfers[0].1 .0, 2_000);
    }

    #[test]
    fn authorize_requires_executor_role() {
        let mut contract = init_contract_with_grant(U128::from(10_000));

        set_predecessor(&accounts(1), 0);
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            contract.authorize(vec![accounts(1)], Some(10_000));
        }));

        assert!(result.is_err());
        assert!(contract.get_pending_transfers().is_empty());
    }

    #[test]
    fn on_authorize_complete_reverts_failed_transfers_using_keys() {
        let mut contract = init_contract_with_spare(0);
        let account_one = accounts(1);
        let account_two = accounts(2);

        contract.create_grant_internal(&account_one, DEFAULT_CLIFF, U128::from(1_000), None);
        contract.create_grant_internal(&account_two, DEFAULT_CLIFF, U128::from(1_000), None);

        {
            let grant = contract
                .accounts
                .get_mut(&account_one)
                .unwrap()
                .grants
                .get_mut(&DEFAULT_CLIFF)
                .unwrap();
            grant.claimed_amount = U128::from(100);
        }

        {
            let grant = contract
                .accounts
                .get_mut(&account_two)
                .unwrap()
                .grants
                .get_mut(&DEFAULT_CLIFF)
                .unwrap();
            grant.claimed_amount = U128::from(200);
        }

        contract
            .pending_transfers
            .insert(account_one.clone(), vec![(DEFAULT_CLIFF, U128::from(100))]);
        contract
            .pending_transfers
            .insert(account_two.clone(), vec![(DEFAULT_CLIFF, U128::from(200))]);

        let context = get_context(accounts(0)).build();
        near_sdk::testing_env!(
            context,
            near_sdk::test_vm_config(),
            near_sdk::RuntimeFeesConfig::test(),
            Default::default(),
            vec![PromiseResult::Successful(vec![]), PromiseResult::Failed]
        );

        contract.pause();
        contract.on_authorize_complete(vec![
            TransferKey {
                account_id: account_two.clone(),
                issue_at: DEFAULT_CLIFF,
            },
            TransferKey {
                account_id: account_one.clone(),
                issue_at: DEFAULT_CLIFF,
            },
        ]);

        let account_one_state = contract.accounts.get(&account_one).unwrap();
        let grant_one = account_one_state.grants.get(&DEFAULT_CLIFF).unwrap();
        assert_eq!(grant_one.claimed_amount.0, 0);
        assert_eq!(grant_one.order_amount.0, 100);

        let account_two_state = contract.accounts.get(&account_two).unwrap();
        let grant_two = account_two_state.grants.get(&DEFAULT_CLIFF).unwrap();
        assert_eq!(grant_two.claimed_amount.0, 200);
        assert_eq!(grant_two.order_amount.0, 0);

        assert!(contract.pending_transfers.is_empty());
    }

    #[test]
    fn terminate_respects_cliff() {
        let mut contract = init_contract_with_grant(U128::from(10_000));
        let user = accounts(1);

        {
            let account = contract.accounts.get_mut(&user).unwrap();
            let grant = account.grants.get_mut(&1_000).unwrap();
            grant.claimed_amount = U128::from(2_000);
            grant.order_amount = U128::from(3_000);
        }

        set_predecessor(&accounts(0), 0);
        contract.terminate(user.clone(), 1_500);

        let account = contract.accounts.get(&user).unwrap();
        let grant = account.grants.get(&1_000).unwrap();
        assert_eq!(grant.order_amount.0, 0);
        assert_eq!(grant.total_amount.0, 2_000);
    }

    #[test]
    fn buy_requires_executor_role() {
        let mut contract = init_contract_with_grant(U128::from(10_000));
        let user = accounts(1);

        {
            let account = contract.accounts.get_mut(&user).unwrap();
            let grant = account.grants.get_mut(&1_000).unwrap();
            grant.order_amount = U128::from(4_000);
        }

        set_predecessor(&accounts(2), 0);
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            contract.buy(vec![user.clone()], 5_000);
        }));

        assert!(result.is_err());
        let grant = contract
            .accounts
            .get(&user)
            .unwrap()
            .grants
            .get(&1_000)
            .unwrap();
        assert_eq!(grant.order_amount.0, 4_000);
        assert_eq!(grant.claimed_amount.0, 0);
    }

    #[test]
    fn terminate_requires_executor_role() {
        let mut contract = init_contract_with_grant(U128::from(10_000));
        let user = accounts(1);

        set_predecessor(&accounts(2), 0);
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            contract.terminate(user.clone(), 1_500);
        }));

        assert!(result.is_err());
        let grant = contract
            .accounts
            .get(&user)
            .unwrap()
            .grants
            .get(&1_000)
            .unwrap();
        assert_eq!(grant.total_amount.0, 10_000);
    }
}
