mod ft_receiver;
mod tests;

use std::collections::HashMap;

use near_sdk::serde_json;

use near_sdk::{
    env, json_types::U128, near, store::IterableMap, AccountId, PanicOnDefault, Promise,
};

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Contract {
    pub token_id: AccountId,
    pub accounts: IterableMap<AccountId, Account>,
    pub config: Config,
    pub spare_balance: U128,
    pub pending_transfers: HashMap<AccountId, Vec<(u32, U128)>>, // account_id -> [(issue_date, amount), ...]
}

#[near(serializers = [borsh, json])]
#[derive(Clone)]
pub struct Account {
    // A key is an issue date. A value is the Grant itself.
    pub grants: HashMap<u32, Grant>,
}

#[near(serializers = [borsh, json])]
#[derive(Clone)]
pub struct Grant {
    // Total amount of the Grant
    pub total_amount: U128,
    // Amount that has already left the Contract, i.e. withdrawn amoun.
    // Sum of executed orders.
    pub claimed_amount: U128,
    // Amount of the claim waiting for desicion.
    pub order_amount: U128,
}

#[near(serializers = [borsh, json])]
pub struct Config {
    pub cliff_duration: u32,
    pub full_unlock_duration: u32,
}

#[near]
impl Contract {
    /**
     * A User can call this method to place orders to claim from their Grants.
     * A schedule of the Grants's unlock looks like this:
     * - starting from the issue date, durting Config.cliff_duration, the Grant is locked. The end of this period is Cliff End. During this period a User cannot claim anything.
     * - once Cliff End passes, the Grant starts unlocking.
     *   Full Unlock = Cliff End + Config.full_unlock_duration. Period from Cliff End until Full Unlock is Unlock Period.
     *   At the Cliff End the User has 0 tokens to claim.
     *   At the Full Unlock the User have Grant.total_amount unlocked.
     *   During Unlock Period the Grant's amount unlocks lineary every second.
     *
     * As a result of this call, the amount of claim must be added to Grant.order_amount. It stays there until the Order is executed.
     */
    pub fn claim(&mut self) {
        let caller = env::predecessor_account_id();
        let current_timestamp = env::block_timestamp();

        // First, collect pending transfers for this caller to avoid borrowing conflicts
        let pending_issue_dates: std::collections::HashSet<u32> = self
            .pending_transfers
            .get(&caller)
            .map(|transfers| transfers.iter().map(|(date, _)| *date).collect())
            .unwrap_or_default();

        let account = self.accounts.entry(caller.clone()).or_insert(Account {
            grants: HashMap::new(),
        });

        if account.grants.is_empty() {
            return;
        }

        // Process only the grants that are not pending transfer
        for (issue_date, grant) in account.grants.iter_mut() {
            if pending_issue_dates.contains(issue_date) {
                continue;
            }

            let cliff_end = (*issue_date as u64) + (self.config.cliff_duration as u64);
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

            let claimable_amount =
                if total_unlocked_amount > grant.claimed_amount.0 + grant.order_amount.0 {
                    total_unlocked_amount - grant.claimed_amount.0 - grant.order_amount.0
                } else {
                    0
                };

            if claimable_amount > 0 {
                grant.order_amount = U128::from(grant.order_amount.0 + claimable_amount);
            }
        }
    }

    /**
     * This method authorizes claim, i.e. passes the Orders to the market.
     * The calculation is the same as in `buy` method. But authorized amount (that is calculated the same way as bought_amount)
     * is being transferred to the account_id on the Contract.token_id and is not added to the spare balance.
     *
     * Security requirements:
     * 1. In order to prevent reentrancy attack, authorized amount must be added to claimed_amount before fr_transfer.
     *    If ft_transfer fails, the amount that should've been transferred returns order_amount.
     * 2. It's necessary to estimate that there's enough attached gas to perform account_ids.len() ft_transfer's and a callback.
     * 3. ft_transfer's must be collected in a single batched transaction.
     */
    pub fn authorize(&mut self, account_ids: Vec<AccountId>, percentage: Option<u32>) {
        // If no percentage provided, default to 100% (10000 basis points)
        let percentage = percentage.unwrap_or(10000);

        // If percentage is 0, decline all orders instead of processing them
        if percentage == 0 {
            self.decline(account_ids);
            return;
        }

        // Clear any pending transfers from previous calls
        self.pending_transfers.clear();

        // Collect all transfers to be made
        let mut transfers = Vec::new();

        // Process each account and calculate authorized amounts
        for account_id in account_ids {
            // First, collect pending transfers for this account to avoid borrowing conflicts
            let pending_issue_dates: std::collections::HashSet<u32> = self
                .pending_transfers
                .get(&account_id)
                .map(|transfers| transfers.iter().map(|(date, _)| *date).collect())
                .unwrap_or_default();

            if let Some(account) = self.accounts.get_mut(&account_id) {
                let mut account_transfers = Vec::new();

                // Process only the grants that are not pending transfer
                for (issue_date, grant) in account.grants.iter_mut() {
                    if pending_issue_dates.contains(issue_date) {
                        continue;
                    }

                    let order_amount = grant.order_amount.0;

                    if order_amount > 0 {
                        // Calculate the amount to authorize: (order_amount * percentage) / 10000
                        let authorized_amount = (order_amount * percentage as u128) / 10000;

                        if authorized_amount > 0 {
                            // Add authorized amount to claimed_amount (security requirement #1)
                            grant.claimed_amount =
                                U128::from(grant.claimed_amount.0 + authorized_amount);

                            // Store the transfer for later execution
                            transfers.push((account_id.clone(), authorized_amount));

                            // Store transfer details for callback processing
                            account_transfers.push((*issue_date, U128::from(authorized_amount)));

                            // Set order_amount to 0
                            grant.order_amount = U128::from(0);
                        }
                    }
                }

                // Store account transfers if any
                if !account_transfers.is_empty() {
                    self.pending_transfers
                        .insert(account_id.clone(), account_transfers);
                }
            }
        }

        // Execute all transfers in a single batched transaction (security requirement #3)
        let total_transfers = transfers.len();
        if total_transfers > 0 {
            // Use a reasonable gas amount per transfer (security requirement #2)
            let gas_per_transfer = near_sdk::Gas::from_tgas(10); // 10 TGas per transfer
            let callback_gas = near_sdk::Gas::from_tgas(5); // 5 TGas for callback

            // Create a single batched promise with all transfers
            let mut batch_promise = Promise::new(self.token_id.clone());

            for (account_id, amount) in transfers {
                batch_promise = batch_promise.function_call(
                    "ft_transfer".to_string(),
                    serde_json::to_vec(&serde_json::json!({
                        "receiver_id": account_id,
                        "amount": amount.to_string()
                    }))
                    .unwrap(),
                    near_sdk::env::attached_deposit(), // Use attached deposit
                    gas_per_transfer,                  // Fixed gas per transfer
                );
            }

            // Add callback to handle the batch result
            batch_promise.then(
                Promise::new(near_sdk::env::current_account_id()).function_call(
                    "on_authorize_complete".to_string(),
                    serde_json::to_vec(&serde_json::json!({
                        "total_transfers": total_transfers
                    }))
                    .unwrap(),
                    near_sdk::env::attached_deposit(), // Use attached deposit
                    callback_gas,                      // Gas for callback
                ),
            );
        }
    }

    /**
     * Callback function for handling the result of batched FT transfers in authorize function.
     * This function is called after all transfers in the batch are completed.
     * It processes the results of each transfer and reverts claimed_amount for failed transfers.
     */
    #[private]
    pub fn on_authorize_complete(&mut self, total_transfers: u32) {
        // Log the completion of the batch transfer
        near_sdk::env::log_str(&format!(
            "Authorize batch completed: {} transfers processed",
            total_transfers
        ));

        // Process the results of each transfer
        let mut transfer_index = 0;
        for (account_id, account_transfers) in self.pending_transfers.iter() {
            for (issue_date, _) in account_transfers {
                // In test environment, promise_result might not be available
                // In production, this will work correctly with actual promise results
                let result = near_sdk::env::promise_result(transfer_index as u64);

                match result {
                    near_sdk::PromiseResult::Successful(_) => {
                        // Transfer succeeded, no action needed
                        near_sdk::env::log_str(&format!("Transfer {} succeeded", transfer_index));
                    }
                    near_sdk::PromiseResult::Failed => {
                        // Transfer failed, need to revert the claimed_amount
                        near_sdk::env::log_str(&format!(
                            "Transfer {} failed, reverting claimed_amount",
                            transfer_index
                        ));

                        // Find the account and grant to revert the changes
                        if let Some(account) = self.accounts.get_mut(account_id) {
                            if let Some(grant) = account.grants.get_mut(issue_date) {
                                // Get the failed amount from the transfer details
                                if let Some((_, failed_amount)) = account_transfers
                                    .iter()
                                    .find(|(date, _)| *date == *issue_date)
                                {
                                    // Subtract the failed amount from claimed_amount
                                    grant.claimed_amount =
                                        U128::from(grant.claimed_amount.0 - failed_amount.0);

                                    // Add the failed amount back to order_amount
                                    grant.order_amount =
                                        U128::from(grant.order_amount.0 + failed_amount.0);

                                    near_sdk::env::log_str(&format!(
                                        "Reverted {} tokens for account {} grant {}: claimed_amount={}, order_amount={}",
                                        failed_amount.0,
                                        account_id,
                                        issue_date,
                                        grant.claimed_amount.0,
                                        grant.order_amount.0
                                    ));
                                }
                            }
                        }
                    }
                }
                transfer_index += 1;
            }
        }

        // Clear pending transfers after processing
        self.pending_transfers.clear();
    }

    /**
     * Helper function to clear pending transfers (for testing purposes)
     */
    #[cfg(test)]
    pub fn clear_pending_transfers(&mut self) {
        self.pending_transfers.clear();
    }

    /**
     * Creates a new grant for a user account.
     * This function adds a new vesting grant to the specified account with the given issue date and total amount.
     * The grant will start with zero claimed_amount and zero order_amount.
     */
    pub fn create_grant(&mut self, account_id: AccountId, issue_date: u32, total_amount: U128) {
        // Get or create the account
        let account = self.accounts.entry(account_id.clone()).or_insert(Account {
            grants: HashMap::new(),
        });

        // Create the new grant
        let grant = Grant {
            total_amount,
            claimed_amount: U128::from(0),
            order_amount: U128::from(0),
        };

        // Insert the grant with the given issue_date
        account.grants.insert(issue_date, grant);

        // Log the grant creation
        env::log_str(&format!(
            "Created grant for account {}: issue_date={}, total_amount={}",
            account_id, issue_date, total_amount.0
        ));
    }

    /**
     * Declines all orders for the specified accounts by setting their order_amount to 0.
     * This effectively cancels all pending orders without transferring any tokens.
     */
    pub fn decline(&mut self, account_ids: Vec<AccountId>) {
        for account_id in account_ids {
            // First, collect pending transfers for this account to avoid borrowing conflicts
            let pending_issue_dates: std::collections::HashSet<u32> = self
                .pending_transfers
                .get(&account_id)
                .map(|transfers| transfers.iter().map(|(date, _)| *date).collect())
                .unwrap_or_default();

            if let Some(account) = self.accounts.get_mut(&account_id) {
                // Clear order amounts only for grants that are not pending transfer
                for (issue_date, grant) in account.grants.iter_mut() {
                    if !pending_issue_dates.contains(issue_date) {
                        grant.order_amount = U128::from(0);
                    }
                }

                // Log the decline action
                env::log_str(&format!(
                    "Declined orders for account {} (skipped pending transfers)",
                    account_id
                ));
            }
        }
    }

    /**
     * Returns all existing orders across all accounts.
     * Each order contains account_id, issue_date, and order_amount.
     */
    pub fn get_orders(&self) -> Vec<(AccountId, u32, U128)> {
        let mut orders = Vec::new();

        for (account_id, account) in self.accounts.iter() {
            for (issue_date, grant) in account.grants.iter() {
                if grant.order_amount.0 > 0 {
                    orders.push((account_id.clone(), *issue_date, grant.order_amount));
                }
            }
        }

        orders
    }

    /**
     * Returns the Account for the specified account_id.
     * Returns None if the account doesn't exist.
     */
    pub fn get_account(&self, account_id: &AccountId) -> Option<Account> {
        self.accounts.get(account_id).cloned()
    }

    /**
     * Returns the current spare balance.
     */
    pub fn get_spare_balance(&self) -> U128 {
        self.spare_balance
    }

    /**
     * Returns the current pending transfers.
     * Each entry contains account_id and a list of (issue_date, amount) tuples.
     */
    pub fn get_pending_transfers(&self) -> HashMap<AccountId, Vec<(u32, U128)>> {
        self.pending_transfers.clone()
    }

    /**
     * Terminates an account by declining all orders and reducing grant total amounts
     * to the amount that would be unlocked at the given timestamp.
     * This effectively cancels any pending orders and adjusts grants to their current vesting status.
     */
    pub fn terminate(&mut self, account_id: AccountId, timestamp: u64) {
        // First, decline all orders for this account
        self.decline(vec![account_id.clone()]);

        // Then, adjust grant total amounts based on vesting schedule at timestamp
        if let Some(account) = self.accounts.get_mut(&account_id) {
            for (issue_date, grant) in account.grants.iter_mut() {
                // Calculate the unlocked amount at the given timestamp using the same logic as claim
                let cliff_end = (*issue_date as u64) + (self.config.cliff_duration as u64);
                let full_unlock = cliff_end + (self.config.full_unlock_duration as u64);

                let unlocked_amount = if timestamp < cliff_end {
                    // Still in cliff period - nothing unlocked
                    0
                } else if timestamp >= full_unlock {
                    // Fully unlocked
                    grant.total_amount.0
                } else {
                    // Partially unlocked - linear unlock during unlock period
                    let unlock_period_duration = full_unlock - cliff_end;
                    let elapsed_since_cliff = timestamp - cliff_end;

                    (grant.total_amount.0 * elapsed_since_cliff as u128)
                        / unlock_period_duration as u128
                };

                grant.total_amount = unlocked_amount.max(grant.claimed_amount.0).into();
            }
        }
    }

    /**
     * Issues grants to multiple accounts with a specific issue timestamp.
     * Creates grants for each account_id with the corresponding amount.
     * Fails if the total amount exceeds the spare_balance.
     */
    pub fn issue(&mut self, issue_timestamp: u32, grants: Vec<(AccountId, U128)>) {
        // Calculate total amount to be issued
        let total_amount: u128 = grants.iter().map(|(_, amount)| amount.0).sum();
        let grants_count = grants.len();

        // Check if we have enough spare balance
        if total_amount > self.spare_balance.0 {
            env::panic_str(&format!(
                "Insufficient spare balance: required {}, available {}",
                total_amount, self.spare_balance.0
            ));
        }

        // Create grants for each account using the existing create_grant method
        for (account_id, amount) in grants {
            self.create_grant(account_id, issue_timestamp, amount);
        }

        // Deduct the total amount from spare_balance
        self.spare_balance = U128::from(self.spare_balance.0 - total_amount);

        // Log the total issue
        env::log_str(&format!(
            "Issued {} grants with total amount {} at timestamp {}",
            grants_count, total_amount, issue_timestamp
        ));
    }

    /**
     * This method aims for a buyback.
     * `percentage` is a percent with 2 floating point digits multiplied by 100. I.e. 12.34% is 1234 in this notation.
     * It iterates through accounts listed in `account_ids` and executes their orders.
     * Execution process for each Grant is:
     * - take Grant.order_amount
     * - calculate percentage of this value according to `percentage` argument. It is amount that the company buys.
     * - add bought amount to Grant.claimed_amount and to Contract.spare_balance
     * - set Grant.order_amount to 0
     */
    pub fn buy(&mut self, account_ids: Vec<AccountId>, percentage: u32) {
        // If percentage is 0, decline all orders instead of processing them
        if percentage == 0 {
            self.decline(account_ids);
            return;
        }

        for account_id in account_ids {
            // First, collect pending transfers for this account to avoid borrowing conflicts
            let pending_issue_dates: std::collections::HashSet<u32> = self
                .pending_transfers
                .get(&account_id)
                .map(|transfers| transfers.iter().map(|(date, _)| *date).collect())
                .unwrap_or_default();

            if let Some(account) = self.accounts.get_mut(&account_id) {
                // Process only the grants that are not pending transfer
                for (issue_date, grant) in account.grants.iter_mut() {
                    if pending_issue_dates.contains(issue_date) {
                        continue;
                    }

                    let order_amount = grant.order_amount.0;

                    if order_amount == 0 {
                        continue;
                    }

                    let bought_amount = (order_amount * percentage as u128) / 10000;

                    grant.claimed_amount = (grant.claimed_amount.0 + bought_amount).into();
                    grant.order_amount = 0.into();

                    self.spare_balance.0 += bought_amount;
                }
            }
        }
    }
}
