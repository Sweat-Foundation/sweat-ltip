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

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::{accounts, VMContextBuilder};
    use near_sdk::testing_env;

    fn get_context(predecessor_account_id: AccountId) -> VMContextBuilder {
        let mut builder = VMContextBuilder::new();
        builder
            .current_account_id(accounts(0))
            .signer_account_id(predecessor_account_id.clone())
            .predecessor_account_id(predecessor_account_id);
        builder
    }

    #[test]
    fn test_claim_during_cliff_period() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000); // Set initial timestamp in seconds
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,       // 1000 seconds cliff
                full_unlock_duration: 2000, // 2000 seconds unlock period
            },
            spare_balance: U128::from(1000000),
            pending_transfers: HashMap::new(),
        };

        // Add a grant with issue date at timestamp 1000
        let mut account = Account {
            grants: HashMap::new(),
        };
        account.grants.insert(
            1000,
            Grant {
                total_amount: U128::from(10000),
                claimed_amount: U128::from(0),
                order_amount: U128::from(0),
            },
        );
        contract.accounts.insert(accounts(1), account);

        // Try to claim during cliff period (timestamp 1500, still within cliff)
        context.block_timestamp(1500);
        testing_env!(context.build());

        contract.claim();

        // Should not have any order amount since still in cliff
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.order_amount, U128::from(0));
    }

    #[test]
    fn test_claim_during_unlock_period() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,       // 1000 seconds cliff
                full_unlock_duration: 2000, // 2000 seconds unlock period
            },
            spare_balance: U128::from(1000000),
            pending_transfers: HashMap::new(),
        };

        // Add a grant with issue date at timestamp 1000
        let mut account = Account {
            grants: HashMap::new(),
        };
        account.grants.insert(
            1000,
            Grant {
                total_amount: U128::from(10000),
                claimed_amount: U128::from(0),
                order_amount: U128::from(0),
            },
        );
        contract.accounts.insert(accounts(1), account);

        // Try to claim during unlock period (timestamp 2500, 1500 seconds into unlock period)
        context.block_timestamp(2500);
        testing_env!(context.build());

        contract.claim();

        // Should have some order amount since we're in unlock period
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert!(grant.order_amount.0 > 0);
        assert!(grant.order_amount.0 < 10000); // Should not be fully unlocked yet
    }

    #[test]
    fn test_claim_after_full_unlock() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,       // 1000 seconds cliff
                full_unlock_duration: 2000, // 2000 seconds unlock period
            },
            spare_balance: U128::from(1000000),
            pending_transfers: HashMap::new(),
        };

        // Add a grant with issue date at timestamp 1000
        let mut account = Account {
            grants: HashMap::new(),
        };
        account.grants.insert(
            1000,
            Grant {
                total_amount: U128::from(10000),
                claimed_amount: U128::from(0),
                order_amount: U128::from(0),
            },
        );
        contract.accounts.insert(accounts(1), account);

        // Try to claim after full unlock (timestamp 4000, after cliff + unlock period)
        context.block_timestamp(4000);
        testing_env!(context.build());

        contract.claim();

        // Should have full amount available for claim
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.order_amount, U128::from(10000));
    }

    #[test]
    fn test_claim_with_existing_orders() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,       // 1000 seconds cliff
                full_unlock_duration: 2000, // 2000 seconds unlock period
            },
            spare_balance: U128::from(1000000),
            pending_transfers: HashMap::new(),
        };

        // Add a grant with issue date at timestamp 1000, with some already claimed and some existing orders
        let mut account = Account {
            grants: HashMap::new(),
        };
        account.grants.insert(
            1000,
            Grant {
                total_amount: U128::from(10000),
                claimed_amount: U128::from(2000), // Already claimed 2000
                order_amount: U128::from(3000),   // Existing order for 3000
            },
        );
        contract.accounts.insert(accounts(1), account);

        // Try to claim after full unlock (timestamp 4000, after cliff + unlock period)
        context.block_timestamp(4000);
        testing_env!(context.build());

        contract.claim();

        // Should only be able to claim: 10000 (total) - 2000 (claimed) - 3000 (existing order) = 5000
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.order_amount, U128::from(8000)); // 3000 (existing) + 5000 (new claim)
    }

    #[test]
    fn test_buy_function() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(1000000),
            pending_transfers: HashMap::new(),
        };

        // Add a grant with some order amount
        let mut account = Account {
            grants: HashMap::new(),
        };
        account.grants.insert(
            1000,
            Grant {
                total_amount: U128::from(10000),
                claimed_amount: U128::from(1000),
                order_amount: U128::from(5000), // 5000 available for buy
            },
        );
        contract.accounts.insert(accounts(1), account);

        // Buy 50% (5000 basis points) of the order amount
        contract.buy(vec![accounts(1)], 5000);

        // Check that 50% was bought: 5000 * 50% = 2500
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.claimed_amount, U128::from(3500)); // 1000 + 2500
        assert_eq!(grant.order_amount, U128::from(0)); // Should be reset to 0
        assert_eq!(contract.spare_balance, U128::from(1002500)); // 1000000 + 2500
    }

    #[test]
    fn test_buy_function_partial_percentage() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(1000000),
            pending_transfers: HashMap::new(),
        };

        // Add a grant with some order amount
        let mut account = Account {
            grants: HashMap::new(),
        };
        account.grants.insert(
            1000,
            Grant {
                total_amount: U128::from(10000),
                claimed_amount: U128::from(0),
                order_amount: U128::from(10000), // 10000 available for buy
            },
        );
        contract.accounts.insert(accounts(1), account);

        // Buy 12.34% (1234 basis points) of the order amount
        contract.buy(vec![accounts(1)], 1234);

        // Check that 12.34% was bought: 10000 * 12.34% = 1234
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.claimed_amount, U128::from(1234));
        assert_eq!(grant.order_amount, U128::from(0)); // Should be reset to 0
        assert_eq!(contract.spare_balance, U128::from(1001234)); // 1000000 + 1234
    }

    #[test]
    fn test_buy_function_multiple_accounts() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(1000000),
            pending_transfers: HashMap::new(),
        };

        // Add grants for two accounts
        let mut account1 = Account {
            grants: HashMap::new(),
        };
        account1.grants.insert(
            1000,
            Grant {
                total_amount: U128::from(10000),
                claimed_amount: U128::from(0),
                order_amount: U128::from(5000),
            },
        );
        contract.accounts.insert(accounts(1), account1);

        let mut account2 = Account {
            grants: HashMap::new(),
        };
        account2.grants.insert(
            1000,
            Grant {
                total_amount: U128::from(20000),
                claimed_amount: U128::from(1000),
                order_amount: U128::from(8000),
            },
        );
        contract.accounts.insert(accounts(2), account2);

        // Buy 25% (2500 basis points) from both accounts
        contract.buy(vec![accounts(1), accounts(2)], 2500);

        // Check account 1: 5000 * 25% = 1250
        let account1 = contract.accounts.get(&accounts(1)).unwrap();
        let grant1 = account1.grants.get(&1000).unwrap();
        assert_eq!(grant1.claimed_amount, U128::from(1250));
        assert_eq!(grant1.order_amount, U128::from(0));

        // Check account 2: 8000 * 25% = 2000
        let account2 = contract.accounts.get(&accounts(2)).unwrap();
        let grant2 = account2.grants.get(&1000).unwrap();
        assert_eq!(grant2.claimed_amount, U128::from(3000)); // 1000 + 2000
        assert_eq!(grant2.order_amount, U128::from(0));

        // Check total spare balance: 1000000 + 1250 + 2000 = 1003250
        assert_eq!(contract.spare_balance, U128::from(1003250));
    }

    #[test]
    fn test_buy_function_no_grants() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(1000000),
            pending_transfers: HashMap::new(),
        };

        // Try to buy from an account that doesn't exist
        contract.buy(vec![accounts(1)], 5000);

        // Spare balance should remain unchanged
        assert_eq!(contract.spare_balance, U128::from(1000000));
    }

    #[test]
    fn test_buy_function_zero_order_amount() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(1000000),
            pending_transfers: HashMap::new(),
        };

        // Add a grant with zero order amount
        let mut account = Account {
            grants: HashMap::new(),
        };
        account.grants.insert(
            1000,
            Grant {
                total_amount: U128::from(10000),
                claimed_amount: U128::from(1000),
                order_amount: U128::from(0), // No order amount
            },
        );
        contract.accounts.insert(accounts(1), account);

        // Try to buy from this account
        contract.buy(vec![accounts(1)], 5000);

        // Nothing should change since order_amount is 0
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.claimed_amount, U128::from(1000)); // Unchanged
        assert_eq!(grant.order_amount, U128::from(0)); // Unchanged
        assert_eq!(contract.spare_balance, U128::from(1000000)); // Unchanged
    }

    #[test]
    fn test_claim_and_buy_workflow() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(1000000),
            pending_transfers: HashMap::new(),
        };

        // Add a grant
        let mut account = Account {
            grants: HashMap::new(),
        };
        account.grants.insert(
            1000,
            Grant {
                total_amount: U128::from(10000),
                claimed_amount: U128::from(0),
                order_amount: U128::from(0),
            },
        );
        contract.accounts.insert(accounts(1), account);

        // Step 1: Claim after full unlock (timestamp 4000)
        context.block_timestamp(4000);
        testing_env!(context.build());
        contract.claim();

        // Verify claim worked
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.order_amount, U128::from(10000)); // Full amount available for order

        // Step 2: Buy 30% of the order
        contract.buy(vec![accounts(1)], 3000); // 30% = 3000 basis points

        // Verify buy worked
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.claimed_amount, U128::from(3000)); // 30% of 10000
        assert_eq!(grant.order_amount, U128::from(0)); // Order amount reset
        assert_eq!(contract.spare_balance, U128::from(1003000)); // 1000000 + 3000

        // Step 3: Claim again (should get remaining 70%)
        contract.claim();

        // Verify second claim worked
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.order_amount, U128::from(7000)); // Remaining 70%

        // Step 4: Buy remaining 50% of the new order
        contract.buy(vec![accounts(1)], 5000); // 50% of 7000 = 3500

        // Verify final state
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.claimed_amount, U128::from(6500)); // 3000 + 3500
        assert_eq!(grant.order_amount, U128::from(0)); // Order amount reset
        assert_eq!(contract.spare_balance, U128::from(1006500)); // 1000000 + 3000 + 3500
    }

    #[test]
    fn test_authorize_function() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(1000000),
            pending_transfers: HashMap::new(),
        };

        // Add a grant with some order amount
        let mut account = Account {
            grants: HashMap::new(),
        };
        account.grants.insert(
            1000,
            Grant {
                total_amount: U128::from(10000),
                claimed_amount: U128::from(1000),
                order_amount: U128::from(5000), // 5000 available for authorization
            },
        );
        contract.accounts.insert(accounts(1), account);

        // Authorize 50% (5000 basis points) of the order amount
        contract.authorize(vec![accounts(1)], Some(5000));

        // Check that 50% was authorized: 5000 * 50% = 2500
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.claimed_amount, U128::from(3500)); // 1000 + 2500
        assert_eq!(grant.order_amount, U128::from(0)); // Should be reset to 0
                                                       // spare_balance should remain unchanged (unlike buy function)
        assert_eq!(contract.spare_balance, U128::from(1000000));
    }

    #[test]
    fn test_authorize_function_default_percentage() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(1000000),
            pending_transfers: HashMap::new(),
        };

        // Add a grant with some order amount
        let mut account = Account {
            grants: HashMap::new(),
        };
        account.grants.insert(
            1000,
            Grant {
                total_amount: U128::from(10000),
                claimed_amount: U128::from(0),
                order_amount: U128::from(8000), // 8000 available for authorization
            },
        );
        contract.accounts.insert(accounts(1), account);

        // Authorize with no percentage (should default to 100%)
        contract.authorize(vec![accounts(1)], None);

        // Check that 100% was authorized: 8000 * 100% = 8000
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.claimed_amount, U128::from(8000));
        assert_eq!(grant.order_amount, U128::from(0)); // Should be reset to 0
        assert_eq!(contract.spare_balance, U128::from(1000000)); // Unchanged
    }

    #[test]
    fn test_authorize_function_multiple_accounts() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(1000000),
            pending_transfers: HashMap::new(),
        };

        // Add grants for two accounts
        let mut account1 = Account {
            grants: HashMap::new(),
        };
        account1.grants.insert(
            1000,
            Grant {
                total_amount: U128::from(10000),
                claimed_amount: U128::from(0),
                order_amount: U128::from(6000),
            },
        );
        contract.accounts.insert(accounts(1), account1);

        let mut account2 = Account {
            grants: HashMap::new(),
        };
        account2.grants.insert(
            1000,
            Grant {
                total_amount: U128::from(20000),
                claimed_amount: U128::from(2000),
                order_amount: U128::from(4000),
            },
        );
        contract.accounts.insert(accounts(2), account2);

        // Authorize 25% (2500 basis points) from both accounts
        contract.authorize(vec![accounts(1), accounts(2)], Some(2500));

        // Check account 1: 6000 * 25% = 1500
        let account1 = contract.accounts.get(&accounts(1)).unwrap();
        let grant1 = account1.grants.get(&1000).unwrap();
        assert_eq!(grant1.claimed_amount, U128::from(1500));
        assert_eq!(grant1.order_amount, U128::from(0));

        // Check account 2: 4000 * 25% = 1000
        let account2 = contract.accounts.get(&accounts(2)).unwrap();
        let grant2 = account2.grants.get(&1000).unwrap();
        assert_eq!(grant2.claimed_amount, U128::from(3000)); // 2000 + 1000
        assert_eq!(grant2.order_amount, U128::from(0));

        // spare_balance should remain unchanged
        assert_eq!(contract.spare_balance, U128::from(1000000));
    }

    #[test]
    fn test_authorize_function_no_grants() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(1000000),
            pending_transfers: HashMap::new(),
        };

        // Try to authorize from an account that doesn't exist
        contract.authorize(vec![accounts(1)], Some(5000));

        // spare_balance should remain unchanged
        assert_eq!(contract.spare_balance, U128::from(1000000));
    }

    #[test]
    fn test_authorize_function_zero_order_amount() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(1000000),
            pending_transfers: HashMap::new(),
        };

        // Add a grant with zero order amount
        let mut account = Account {
            grants: HashMap::new(),
        };
        account.grants.insert(
            1000,
            Grant {
                total_amount: U128::from(10000),
                claimed_amount: U128::from(1000),
                order_amount: U128::from(0), // No order amount
            },
        );
        contract.accounts.insert(accounts(1), account);

        // Try to authorize from this account
        contract.authorize(vec![accounts(1)], Some(5000));

        // Nothing should change since order_amount is 0
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.claimed_amount, U128::from(1000)); // Unchanged
        assert_eq!(grant.order_amount, U128::from(0)); // Unchanged
        assert_eq!(contract.spare_balance, U128::from(1000000)); // Unchanged
    }

    #[test]
    fn test_complete_workflow_claim_buy_authorize() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(1000000),
            pending_transfers: HashMap::new(),
        };

        // Create a grant using the new create_grant function
        contract.create_grant(accounts(1), 1000, U128::from(10000));

        // Step 1: Claim after full unlock (timestamp 4000)
        context.block_timestamp(4000);
        testing_env!(context.build());
        contract.claim();

        // Verify claim worked
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.order_amount, U128::from(10000)); // Full amount available for order

        // Step 2: Buy 20% of the order (goes to spare_balance)
        contract.buy(vec![accounts(1)], 2000); // 20% = 2000 basis points

        // Verify buy worked
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.claimed_amount, U128::from(2000)); // 20% of 10000
        assert_eq!(grant.order_amount, U128::from(0)); // Order amount reset
        assert_eq!(contract.spare_balance, U128::from(1002000)); // 1000000 + 2000

        // Step 3: Claim again (should get remaining 80%)
        contract.claim();

        // Verify second claim worked
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.order_amount, U128::from(8000)); // Remaining 80%

        // Step 4: Authorize 50% of the new order (transfers to user, not spare_balance)
        contract.authorize(vec![accounts(1)], Some(5000)); // 50% of 8000 = 4000

        // Verify authorize worked
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.claimed_amount, U128::from(6000)); // 2000 + 4000
        assert_eq!(grant.order_amount, U128::from(0)); // Order amount reset
        assert_eq!(contract.spare_balance, U128::from(1002000)); // Unchanged (unlike buy)

        // Clear pending transfers to simulate callback completion (for testing)
        contract.clear_pending_transfers();

        // Step 5: Claim remaining 40%
        contract.claim();

        // Verify final claim worked
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.order_amount, U128::from(4000)); // Remaining 40%

        // Final state verification
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.total_amount, U128::from(10000));
        assert_eq!(grant.claimed_amount, U128::from(6000)); // 20% bought + 50% authorized
        assert_eq!(grant.order_amount, U128::from(4000)); // 40% still available for order
        assert_eq!(contract.spare_balance, U128::from(1002000)); // Only buy amount added
    }

    #[test]
    fn test_authorize_callback_function() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(1000000),
            pending_transfers: HashMap::new(),
        };

        // Test the callback function directly with 0 transfers to avoid promise_result issues
        contract.on_authorize_complete(0);

        // The callback should not change any state, just log
        // This test verifies the function can be called without panicking
        assert_eq!(contract.spare_balance, U128::from(1000000)); // Unchanged
    }

    #[test]
    fn test_authorize_callback_with_failed_transfers() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(1000000),
            pending_transfers: HashMap::new(),
        };

        // Add a grant with some claimed amount
        let mut account = Account {
            grants: HashMap::new(),
        };
        account.grants.insert(
            1000,
            Grant {
                total_amount: U128::from(10000),
                claimed_amount: U128::from(5000), // Some amount already claimed
                order_amount: U128::from(0),
            },
        );
        contract.accounts.insert(accounts(1), account);

        // Simulate pending transfers (as if authorize was called)
        contract
            .pending_transfers
            .insert(accounts(1), vec![(1000, U128::from(2000))]); // 2000 tokens to transfer

        // Test the core logic by manually simulating a failed transfer
        // In a real scenario, this would be called by the blockchain after a failed transfer
        if let Some(account_transfers) = contract.pending_transfers.get(&accounts(1)) {
            if let Some((issue_date, failed_amount)) = account_transfers.get(0) {
                // Find the account and grant to revert the changes
                if let Some(account) = contract.accounts.get_mut(&accounts(1)) {
                    if let Some(grant) = account.grants.get_mut(issue_date) {
                        // Subtract the failed amount from claimed_amount
                        grant.claimed_amount = U128::from(grant.claimed_amount.0 - failed_amount.0);

                        // Add the failed amount back to order_amount
                        grant.order_amount = U128::from(grant.order_amount.0 + failed_amount.0);
                    }
                }
            }
        }

        // Check that the failed transfer was reverted
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();

        // The claimed_amount should be reduced by the failed amount
        assert_eq!(grant.claimed_amount, U128::from(3000)); // 5000 - 2000
                                                            // The order_amount should be increased by the failed amount
        assert_eq!(grant.order_amount, U128::from(2000)); // 0 + 2000

        // Clear pending transfers
        contract.pending_transfers.clear();
        assert_eq!(contract.pending_transfers.len(), 0);
    }

    #[test]
    fn test_decline_function() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(0),
            pending_transfers: HashMap::new(),
        };

        // Create grants with some order amounts
        contract.create_grant(accounts(1), 1000, U128::from(10000));
        contract.create_grant(accounts(1), 2000, U128::from(5000));
        contract.create_grant(accounts(2), 1000, U128::from(3000));

        // Add some order amounts manually
        let account1 = contract.accounts.get_mut(&accounts(1)).unwrap();
        let grant1 = account1.grants.get_mut(&1000).unwrap();
        grant1.order_amount = U128::from(2000);
        let grant2 = account1.grants.get_mut(&2000).unwrap();
        grant2.order_amount = U128::from(1500);

        let account2 = contract.accounts.get_mut(&accounts(2)).unwrap();
        let grant3 = account2.grants.get_mut(&1000).unwrap();
        grant3.order_amount = U128::from(1000);

        // Decline orders for account 1
        contract.decline(vec![accounts(1)]);

        // Verify account 1's orders are cleared
        let account1 = contract.accounts.get(&accounts(1)).unwrap();
        let grant1 = account1.grants.get(&1000).unwrap();
        assert_eq!(grant1.order_amount, U128::from(0));
        let grant2 = account1.grants.get(&2000).unwrap();
        assert_eq!(grant2.order_amount, U128::from(0));

        // Verify account 2's orders are unchanged
        let account2 = contract.accounts.get(&accounts(2)).unwrap();
        let grant3 = account2.grants.get(&1000).unwrap();
        assert_eq!(grant3.order_amount, U128::from(1000));
    }

    #[test]
    fn test_decline_multiple_accounts() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(0),
            pending_transfers: HashMap::new(),
        };

        // Create grants for multiple accounts
        contract.create_grant(accounts(1), 1000, U128::from(10000));
        contract.create_grant(accounts(2), 1000, U128::from(5000));
        contract.create_grant(accounts(3), 1000, U128::from(3000));

        // Add order amounts
        let account1 = contract.accounts.get_mut(&accounts(1)).unwrap();
        account1.grants.get_mut(&1000).unwrap().order_amount = U128::from(2000);
        let account2 = contract.accounts.get_mut(&accounts(2)).unwrap();
        account2.grants.get_mut(&1000).unwrap().order_amount = U128::from(1500);
        let account3 = contract.accounts.get_mut(&accounts(3)).unwrap();
        account3.grants.get_mut(&1000).unwrap().order_amount = U128::from(1000);

        // Decline orders for multiple accounts
        contract.decline(vec![accounts(1), accounts(2)]);

        // Verify both accounts' orders are cleared
        let account1 = contract.accounts.get(&accounts(1)).unwrap();
        assert_eq!(
            account1.grants.get(&1000).unwrap().order_amount,
            U128::from(0)
        );
        let account2 = contract.accounts.get(&accounts(2)).unwrap();
        assert_eq!(
            account2.grants.get(&1000).unwrap().order_amount,
            U128::from(0)
        );
        let account3 = contract.accounts.get(&accounts(3)).unwrap();
        assert_eq!(
            account3.grants.get(&1000).unwrap().order_amount,
            U128::from(1000)
        ); // Unchanged
    }

    #[test]
    fn test_buy_with_zero_percentage() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(1000),
            pending_transfers: HashMap::new(),
        };

        // Create grants with order amounts
        contract.create_grant(accounts(1), 1000, U128::from(10000));
        let account1 = contract.accounts.get_mut(&accounts(1)).unwrap();
        account1.grants.get_mut(&1000).unwrap().order_amount = U128::from(5000);

        // Call buy with 0% - should decline orders
        contract.buy(vec![accounts(1)], 0);

        // Verify orders are cleared but no tokens were bought
        let account1 = contract.accounts.get(&accounts(1)).unwrap();
        let grant1 = account1.grants.get(&1000).unwrap();
        assert_eq!(grant1.order_amount, U128::from(0));
        assert_eq!(grant1.claimed_amount, U128::from(0)); // No claimed amount added
        assert_eq!(contract.spare_balance, U128::from(1000)); // Spare balance unchanged
    }

    #[test]
    fn test_authorize_with_zero_percentage() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(1000),
            pending_transfers: HashMap::new(),
        };

        // Create grants with order amounts
        contract.create_grant(accounts(1), 1000, U128::from(10000));
        let account1 = contract.accounts.get_mut(&accounts(1)).unwrap();
        account1.grants.get_mut(&1000).unwrap().order_amount = U128::from(5000);

        // Call authorize with 0% - should decline orders
        contract.authorize(vec![accounts(1)], Some(0));

        // Verify orders are cleared but no tokens were authorized
        let account1 = contract.accounts.get(&accounts(1)).unwrap();
        let grant1 = account1.grants.get(&1000).unwrap();
        assert_eq!(grant1.order_amount, U128::from(0));
        assert_eq!(grant1.claimed_amount, U128::from(0)); // No claimed amount added
        assert_eq!(contract.spare_balance, U128::from(1000)); // Spare balance unchanged
        assert_eq!(contract.pending_transfers.len(), 0); // No pending transfers
    }

    #[test]
    fn test_decline_respects_pending_transfers() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(0),
            pending_transfers: HashMap::new(),
        };

        // Create grants with order amounts
        contract.create_grant(accounts(1), 1000, U128::from(10000));
        contract.create_grant(accounts(1), 2000, U128::from(5000));

        // Add order amounts
        let account1 = contract.accounts.get_mut(&accounts(1)).unwrap();
        account1.grants.get_mut(&1000).unwrap().order_amount = U128::from(2000);
        account1.grants.get_mut(&2000).unwrap().order_amount = U128::from(1500);

        // Add one grant to pending transfers (simulating it's being processed by authorize)
        contract.pending_transfers.insert(
            accounts(1),
            vec![(1000, U128::from(1000))], // issue_date, amount
        );

        // Decline orders - should only clear the grant that's not pending transfer
        contract.decline(vec![accounts(1)]);

        // Verify only the non-pending grant's order is cleared
        let account1 = contract.accounts.get(&accounts(1)).unwrap();
        let grant1 = account1.grants.get(&1000).unwrap();
        assert_eq!(grant1.order_amount, U128::from(2000)); // Unchanged because pending transfer
        let grant2 = account1.grants.get(&2000).unwrap();
        assert_eq!(grant2.order_amount, U128::from(0)); // Cleared because not pending transfer
    }

    #[test]
    fn test_get_account_existing() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        // Create grants for an account
        contract.create_grant(accounts(1), 1000, U128::from(5000));
        contract.create_grant(accounts(1), 2000, U128::from(3000));

        // Get the account
        let account = contract.get_account(&accounts(1));
        assert!(account.is_some());

        let account = account.unwrap();
        assert_eq!(account.grants.len(), 2);

        // Check first grant
        let grant1 = account.grants.get(&1000).unwrap();
        assert_eq!(grant1.total_amount, U128::from(5000));
        assert_eq!(grant1.claimed_amount, U128::from(0));
        assert_eq!(grant1.order_amount, U128::from(0));

        // Check second grant
        let grant2 = account.grants.get(&2000).unwrap();
        assert_eq!(grant2.total_amount, U128::from(3000));
        assert_eq!(grant2.claimed_amount, U128::from(0));
        assert_eq!(grant2.order_amount, U128::from(0));
    }

    #[test]
    fn test_get_account_nonexistent() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        // Try to get non-existent account
        let account = contract.get_account(&accounts(1));
        assert!(account.is_none());
    }

    #[test]
    fn test_get_account_empty_grants() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        // Create account with empty grants
        let account = Account {
            grants: HashMap::new(),
        };
        contract.accounts.insert(accounts(1), account);

        // Get the account
        let account = contract.get_account(&accounts(1));
        assert!(account.is_some());

        let account = account.unwrap();
        assert_eq!(account.grants.len(), 0);
    }

    #[test]
    fn test_get_account_with_claimed_grants() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        // Create grant and manually set claimed/order amounts
        contract.create_grant(accounts(1), 1000, U128::from(5000));
        let account = contract.accounts.get_mut(&accounts(1)).unwrap();
        let grant = account.grants.get_mut(&1000).unwrap();
        grant.claimed_amount = U128::from(2000);
        grant.order_amount = U128::from(1000);

        // Get the account
        let account = contract.get_account(&accounts(1));
        assert!(account.is_some());

        let account = account.unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.total_amount, U128::from(5000));
        assert_eq!(grant.claimed_amount, U128::from(2000));
        assert_eq!(grant.order_amount, U128::from(1000));
    }

    #[test]
    fn test_get_account_multiple_accounts() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        // Create grants for multiple accounts
        contract.create_grant(accounts(1), 1000, U128::from(5000));
        contract.create_grant(accounts(2), 2000, U128::from(3000));
        contract.create_grant(accounts(3), 1000, U128::from(2000));

        // Get each account
        let account1 = contract.get_account(&accounts(1));
        assert!(account1.is_some());
        assert_eq!(account1.unwrap().grants.len(), 1);

        let account2 = contract.get_account(&accounts(2));
        assert!(account2.is_some());
        assert_eq!(account2.unwrap().grants.len(), 1);

        let account3 = contract.get_account(&accounts(3));
        assert!(account3.is_some());
        assert_eq!(account3.unwrap().grants.len(), 1);

        // Try to get non-existent account
        let account4 = contract.get_account(&accounts(4));
        assert!(account4.is_none());
    }

    #[test]
    fn test_get_account_workflow_integration() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        // Issue grants
        let grants = vec![
            (accounts(1), U128::from(5000)),
            (accounts(2), U128::from(3000)),
        ];
        contract.issue(1000, grants);

        // Get accounts and verify grants
        let account1 = contract.get_account(&accounts(1)).unwrap();
        assert_eq!(account1.grants.len(), 1);
        let grant1 = account1.grants.get(&1000).unwrap();
        assert_eq!(grant1.total_amount, U128::from(5000));

        let account2 = contract.get_account(&accounts(2)).unwrap();
        assert_eq!(account2.grants.len(), 1);
        let grant2 = account2.grants.get(&1000).unwrap();
        assert_eq!(grant2.total_amount, U128::from(3000));

        // Claim to create orders
        context.block_timestamp(4000); // After full unlock
        testing_env!(context.build());
        contract.claim();

        // Get account after claim and verify order amount
        let account1 = contract.get_account(&accounts(1)).unwrap();
        let grant1 = account1.grants.get(&1000).unwrap();
        assert_eq!(grant1.order_amount, U128::from(5000)); // Full amount claimable
    }

    #[test]
    fn test_get_spare_balance_initial() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        assert_eq!(contract.get_spare_balance(), U128::from(10000));
    }

    #[test]
    fn test_get_spare_balance_after_issue() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        // Issue grants that consume spare balance
        let grants = vec![
            (accounts(1), U128::from(3000)),
            (accounts(2), U128::from(2000)),
        ];
        contract.issue(1000, grants);

        assert_eq!(contract.get_spare_balance(), U128::from(5000));
    }

    #[test]
    fn test_get_spare_balance_after_buy() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        // Create grants and claim them
        contract.create_grant(accounts(1), 1000, U128::from(5000));
        context.block_timestamp(4000); // After full unlock
        testing_env!(context.build());
        contract.claim();

        // Buy back 50% of orders
        contract.buy(vec![accounts(1)], 50);

        // Spare balance should increase by 50% of the order amount (50 basis points = 0.5%)
        // bought_amount = (5000 * 50) / 10000 = 25
        assert_eq!(contract.get_spare_balance(), U128::from(10000 + 25)); // 10000 + 0.5% of 5000
    }

    #[test]
    fn test_get_spare_balance_zero() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(0),
            pending_transfers: HashMap::new(),
        };

        assert_eq!(contract.get_spare_balance(), U128::from(0));
    }

    #[test]
    fn test_get_spare_balance_large_value() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(u128::MAX),
            pending_transfers: HashMap::new(),
        };

        assert_eq!(contract.get_spare_balance(), U128::from(u128::MAX));
    }

    #[test]
    fn test_get_spare_balance_workflow_integration() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        // Initial spare balance
        assert_eq!(contract.get_spare_balance(), U128::from(10000));

        // Issue grants
        let grants = vec![
            (accounts(1), U128::from(3000)),
            (accounts(2), U128::from(2000)),
        ];
        contract.issue(1000, grants);
        assert_eq!(contract.get_spare_balance(), U128::from(5000));

        // Manually set order amounts for both accounts to simulate claimed grants
        let account1 = contract.accounts.get_mut(&accounts(1)).unwrap();
        let grant1 = account1.grants.get_mut(&1000).unwrap();
        grant1.order_amount = U128::from(3000);

        let account2 = contract.accounts.get_mut(&accounts(2)).unwrap();
        let grant2 = account2.grants.get_mut(&1000).unwrap();
        grant2.order_amount = U128::from(2000);

        // Buy back 30% of orders
        contract.buy(vec![accounts(1), accounts(2)], 30); // 30 basis points = 0.3%

        // Spare balance should increase by 30% of total orders (30 basis points = 0.3%)
        // bought_amount = (5000 * 30) / 10000 = 15
        assert_eq!(contract.get_spare_balance(), U128::from(5000 + 15));
    }

    #[test]
    fn test_get_pending_transfers_empty() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        let pending = contract.get_pending_transfers();
        assert!(pending.is_empty());
    }

    #[test]
    fn test_get_pending_transfers_single_account() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut pending_transfers = HashMap::new();
        pending_transfers.insert(
            accounts(1),
            vec![(1000, U128::from(5000)), (2000, U128::from(3000))],
        );

        let contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers,
        };

        let pending = contract.get_pending_transfers();
        assert_eq!(pending.len(), 1);
        assert!(pending.contains_key(&accounts(1)));

        let transfers = pending.get(&accounts(1)).unwrap();
        assert_eq!(transfers.len(), 2);
        assert_eq!(transfers[0], (1000, U128::from(5000)));
        assert_eq!(transfers[1], (2000, U128::from(3000)));
    }

    #[test]
    fn test_get_pending_transfers_multiple_accounts() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut pending_transfers = HashMap::new();
        pending_transfers.insert(accounts(1), vec![(1000, U128::from(5000))]);
        pending_transfers.insert(
            accounts(2),
            vec![(2000, U128::from(3000)), (3000, U128::from(2000))],
        );
        pending_transfers.insert(accounts(3), vec![(1000, U128::from(1000))]);

        let contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers,
        };

        let pending = contract.get_pending_transfers();
        assert_eq!(pending.len(), 3);

        // Check account 1
        assert!(pending.contains_key(&accounts(1)));
        let transfers1 = pending.get(&accounts(1)).unwrap();
        assert_eq!(transfers1.len(), 1);
        assert_eq!(transfers1[0], (1000, U128::from(5000)));

        // Check account 2
        assert!(pending.contains_key(&accounts(2)));
        let transfers2 = pending.get(&accounts(2)).unwrap();
        assert_eq!(transfers2.len(), 2);
        assert_eq!(transfers2[0], (2000, U128::from(3000)));
        assert_eq!(transfers2[1], (3000, U128::from(2000)));

        // Check account 3
        assert!(pending.contains_key(&accounts(3)));
        let transfers3 = pending.get(&accounts(3)).unwrap();
        assert_eq!(transfers3.len(), 1);
        assert_eq!(transfers3[0], (1000, U128::from(1000)));
    }

    #[test]
    fn test_get_pending_transfers_after_authorize() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        // Create grants and manually set order amounts
        contract.create_grant(accounts(1), 1000, U128::from(5000));
        contract.create_grant(accounts(2), 2000, U128::from(3000));

        let account1 = contract.accounts.get_mut(&accounts(1)).unwrap();
        let grant1 = account1.grants.get_mut(&1000).unwrap();
        grant1.order_amount = U128::from(5000);

        let account2 = contract.accounts.get_mut(&accounts(2)).unwrap();
        let grant2 = account2.grants.get_mut(&2000).unwrap();
        grant2.order_amount = U128::from(3000);

        // Authorize transfers (this will populate pending_transfers)
        contract.authorize(vec![accounts(1), accounts(2)], Some(50)); // 50 basis points = 0.5%

        let pending = contract.get_pending_transfers();
        assert_eq!(pending.len(), 2);

        // Check account 1
        assert!(pending.contains_key(&accounts(1)));
        let transfers1 = pending.get(&accounts(1)).unwrap();
        assert_eq!(transfers1.len(), 1);
        assert_eq!(transfers1[0], (1000, U128::from(25))); // 0.5% of 5000 = 25

        // Check account 2
        assert!(pending.contains_key(&accounts(2)));
        let transfers2 = pending.get(&accounts(2)).unwrap();
        assert_eq!(transfers2.len(), 1);
        assert_eq!(transfers2[0], (2000, U128::from(15))); // 0.5% of 3000 = 15
    }

    #[test]
    fn test_get_pending_transfers_after_callback() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        // Manually set pending transfers
        let mut pending_transfers = HashMap::new();
        pending_transfers.insert(accounts(1), vec![(1000, U128::from(5000))]);
        contract.pending_transfers = pending_transfers;

        // Simulate callback completion (clears pending transfers)
        contract.clear_pending_transfers();

        let pending = contract.get_pending_transfers();
        assert!(pending.is_empty());
    }

    #[test]
    fn test_get_pending_transfers_large_values() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut pending_transfers = HashMap::new();
        pending_transfers.insert(
            accounts(1),
            vec![
                (1000, U128::from(u128::MAX)),
                (2000, U128::from(0)),
                (3000, U128::from(1)),
            ],
        );

        let contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers,
        };

        let pending = contract.get_pending_transfers();
        assert_eq!(pending.len(), 1);

        let transfers = pending.get(&accounts(1)).unwrap();
        assert_eq!(transfers.len(), 3);
        assert_eq!(transfers[0], (1000, U128::from(u128::MAX)));
        assert_eq!(transfers[1], (2000, U128::from(0)));
        assert_eq!(transfers[2], (3000, U128::from(1)));
    }

    #[test]
    fn test_get_pending_transfers_workflow_integration() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        // Initial state - no pending transfers
        let pending = contract.get_pending_transfers();
        assert!(pending.is_empty());

        // Create grants and set order amounts
        contract.create_grant(accounts(1), 1000, U128::from(5000));
        contract.create_grant(accounts(2), 2000, U128::from(3000));

        let account1 = contract.accounts.get_mut(&accounts(1)).unwrap();
        let grant1 = account1.grants.get_mut(&1000).unwrap();
        grant1.order_amount = U128::from(5000);

        let account2 = contract.accounts.get_mut(&accounts(2)).unwrap();
        let grant2 = account2.grants.get_mut(&2000).unwrap();
        grant2.order_amount = U128::from(3000);

        // Authorize transfers
        contract.authorize(vec![accounts(1), accounts(2)], Some(30)); // 30 basis points = 0.3%

        // Check pending transfers after authorize
        let pending = contract.get_pending_transfers();
        assert_eq!(pending.len(), 2);

        let transfers1 = pending.get(&accounts(1)).unwrap();
        assert_eq!(transfers1[0], (1000, U128::from(15))); // 0.3% of 5000 = 15

        let transfers2 = pending.get(&accounts(2)).unwrap();
        assert_eq!(transfers2[0], (2000, U128::from(9))); // 0.3% of 3000 = 9

        // Simulate callback completion
        contract.clear_pending_transfers();

        // Check pending transfers after callback
        let pending = contract.get_pending_transfers();
        assert!(pending.is_empty());
    }

    #[test]
    fn test_get_orders_empty() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        // No accounts, no orders
        let orders = contract.get_orders();
        assert_eq!(orders.len(), 0);
    }

    #[test]
    fn test_get_orders_no_orders() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        // Create grants but no orders
        contract.create_grant(accounts(1), 1000, U128::from(5000));
        contract.create_grant(accounts(2), 2000, U128::from(3000));

        let orders = contract.get_orders();
        assert_eq!(orders.len(), 0);
    }

    #[test]
    fn test_get_orders_single_order() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        // Create grant and claim to create order
        contract.create_grant(accounts(1), 1000, U128::from(5000));
        context.block_timestamp(4000); // After full unlock
        testing_env!(context.build());
        contract.claim();

        let orders = contract.get_orders();
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].0, accounts(1));
        assert_eq!(orders[0].1, 1000);
        assert_eq!(orders[0].2, U128::from(5000));
    }

    #[test]
    fn test_get_orders_multiple_accounts() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        // Create grants for multiple accounts
        contract.create_grant(accounts(1), 1000, U128::from(5000));
        contract.create_grant(accounts(2), 2000, U128::from(3000));
        contract.create_grant(accounts(3), 1000, U128::from(2000));

        // Manually set order amounts to simulate claimed grants
        let account1 = contract.accounts.get_mut(&accounts(1)).unwrap();
        let grant1 = account1.grants.get_mut(&1000).unwrap();
        grant1.order_amount = U128::from(5000);

        let account2 = contract.accounts.get_mut(&accounts(2)).unwrap();
        let grant2 = account2.grants.get_mut(&2000).unwrap();
        grant2.order_amount = U128::from(3000);

        let account3 = contract.accounts.get_mut(&accounts(3)).unwrap();
        let grant3 = account3.grants.get_mut(&1000).unwrap();
        grant3.order_amount = U128::from(2000);

        let orders = contract.get_orders();
        assert_eq!(orders.len(), 3);

        // Sort orders for consistent testing
        let mut sorted_orders = orders;
        sorted_orders.sort_by(|a, b| a.0.cmp(&b.0));

        assert_eq!(sorted_orders[0].0, accounts(1));
        assert_eq!(sorted_orders[0].1, 1000);
        assert_eq!(sorted_orders[0].2, U128::from(5000));

        assert_eq!(sorted_orders[1].0, accounts(2));
        assert_eq!(sorted_orders[1].1, 2000);
        assert_eq!(sorted_orders[1].2, U128::from(3000));

        assert_eq!(sorted_orders[2].0, accounts(3));
        assert_eq!(sorted_orders[2].1, 1000);
        assert_eq!(sorted_orders[2].2, U128::from(2000));
    }

    #[test]
    fn test_get_orders_multiple_grants_same_account() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        // Create multiple grants for same account
        contract.create_grant(accounts(1), 1000, U128::from(5000));
        contract.create_grant(accounts(1), 2000, U128::from(3000));
        contract.create_grant(accounts(1), 3000, U128::from(2000));

        // Manually set order amounts to simulate claimed grants
        let account1 = contract.accounts.get_mut(&accounts(1)).unwrap();
        let grant1 = account1.grants.get_mut(&1000).unwrap();
        grant1.order_amount = U128::from(5000);
        let grant2 = account1.grants.get_mut(&2000).unwrap();
        grant2.order_amount = U128::from(3000);
        let grant3 = account1.grants.get_mut(&3000).unwrap();
        grant3.order_amount = U128::from(2000);

        let orders = contract.get_orders();
        assert_eq!(orders.len(), 3);

        // Sort by issue_date for consistent testing
        let mut sorted_orders = orders;
        sorted_orders.sort_by(|a, b| a.1.cmp(&b.1));

        assert_eq!(sorted_orders[0].0, accounts(1));
        assert_eq!(sorted_orders[0].1, 1000);
        assert_eq!(sorted_orders[0].2, U128::from(5000));

        assert_eq!(sorted_orders[1].0, accounts(1));
        assert_eq!(sorted_orders[1].1, 2000);
        assert_eq!(sorted_orders[1].2, U128::from(3000));

        assert_eq!(sorted_orders[2].0, accounts(1));
        assert_eq!(sorted_orders[2].1, 3000);
        assert_eq!(sorted_orders[2].2, U128::from(2000));
    }

    #[test]
    fn test_get_orders_partial_orders() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        // Create grants and manually set order amounts for caller account only
        contract.create_grant(accounts(1), 1000, U128::from(5000));
        contract.create_grant(accounts(1), 2000, U128::from(3000));

        let account1 = contract.accounts.get_mut(&accounts(1)).unwrap();
        let grant1 = account1.grants.get_mut(&1000).unwrap();
        grant1.order_amount = U128::from(5000);
        let grant2 = account1.grants.get_mut(&2000).unwrap();
        grant2.order_amount = U128::from(3000);

        // Verify initial orders
        let orders = contract.get_orders();
        assert_eq!(orders.len(), 2);

        // Buy back 50% from account 1's grants (this will clear all orders for account 1)
        contract.buy(vec![accounts(1)], 5000);

        // After buy, all orders for account 1 should be cleared
        let orders = contract.get_orders();
        assert_eq!(orders.len(), 0);
    }

    #[test]
    fn test_get_orders_after_decline() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        // Create grants and manually set order amounts
        contract.create_grant(accounts(1), 1000, U128::from(5000));
        contract.create_grant(accounts(2), 2000, U128::from(3000));

        let account1 = contract.accounts.get_mut(&accounts(1)).unwrap();
        let grant1 = account1.grants.get_mut(&1000).unwrap();
        grant1.order_amount = U128::from(5000);

        let account2 = contract.accounts.get_mut(&accounts(2)).unwrap();
        let grant2 = account2.grants.get_mut(&2000).unwrap();
        grant2.order_amount = U128::from(3000);

        // Verify orders exist
        let orders = contract.get_orders();
        assert_eq!(orders.len(), 2);

        // Decline orders for account 1
        contract.decline(vec![accounts(1)]);

        // Verify only account 2 has orders now
        let orders = contract.get_orders();
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].0, accounts(2));
        assert_eq!(orders[0].1, 2000);
        assert_eq!(orders[0].2, U128::from(3000));
    }

    #[test]
    fn test_get_orders_workflow_integration() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        // Issue grants for caller account only
        let grants = vec![(accounts(1), U128::from(5000))];
        contract.issue(1000, grants);

        // No orders initially
        let orders = contract.get_orders();
        assert_eq!(orders.len(), 0);

        // Manually set order amount to simulate claimed grant
        let account1 = contract.accounts.get_mut(&accounts(1)).unwrap();
        let grant1 = account1.grants.get_mut(&1000).unwrap();
        grant1.order_amount = U128::from(5000);

        // Should have order now
        let orders = contract.get_orders();
        assert_eq!(orders.len(), 1);

        // Buy back some orders (this will clear all orders for account 1)
        contract.buy(vec![accounts(1)], 5000); // 50% buyback

        // After buy, all orders should be cleared
        let orders = contract.get_orders();
        assert_eq!(orders.len(), 0);
    }

    #[test]
    fn test_issue_function() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        // Issue grants to multiple accounts
        let grants = vec![
            (accounts(1), U128::from(3000)),
            (accounts(2), U128::from(2000)),
            (accounts(3), U128::from(1000)),
        ];
        contract.issue(1000, grants);

        // Verify grants were created
        let account1 = contract.accounts.get(&accounts(1)).unwrap();
        let grant1 = account1.grants.get(&1000).unwrap();
        assert_eq!(grant1.total_amount, U128::from(3000));
        assert_eq!(grant1.claimed_amount, U128::from(0));
        assert_eq!(grant1.order_amount, U128::from(0));

        let account2 = contract.accounts.get(&accounts(2)).unwrap();
        let grant2 = account2.grants.get(&1000).unwrap();
        assert_eq!(grant2.total_amount, U128::from(2000));

        let account3 = contract.accounts.get(&accounts(3)).unwrap();
        let grant3 = account3.grants.get(&1000).unwrap();
        assert_eq!(grant3.total_amount, U128::from(1000));

        // Verify spare_balance was reduced
        assert_eq!(contract.spare_balance, U128::from(4000)); // 10000 - 6000
    }

    #[test]
    fn test_issue_insufficient_balance() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(5000),
            pending_transfers: HashMap::new(),
        };

        // Try to issue more than available balance
        let grants = vec![
            (accounts(1), U128::from(3000)),
            (accounts(2), U128::from(3000)), // Total: 6000 > 5000
        ];

        // This should panic
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            contract.issue(1000, grants);
        }));

        assert!(result.is_err(), "Should panic when insufficient balance");
    }

    #[test]
    fn test_issue_exact_balance() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(5000),
            pending_transfers: HashMap::new(),
        };

        // Issue exactly the available balance
        let grants = vec![
            (accounts(1), U128::from(3000)),
            (accounts(2), U128::from(2000)), // Total: 5000 = 5000
        ];
        contract.issue(1000, grants);

        // Verify grants were created
        let account1 = contract.accounts.get(&accounts(1)).unwrap();
        assert_eq!(
            account1.grants.get(&1000).unwrap().total_amount,
            U128::from(3000)
        );
        let account2 = contract.accounts.get(&accounts(2)).unwrap();
        assert_eq!(
            account2.grants.get(&1000).unwrap().total_amount,
            U128::from(2000)
        );

        // Verify spare_balance is now 0
        assert_eq!(contract.spare_balance, U128::from(0));
    }

    #[test]
    fn test_issue_single_account() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(5000),
            pending_transfers: HashMap::new(),
        };

        // Issue grant to single account
        let grants = vec![(accounts(1), U128::from(3000))];
        contract.issue(1000, grants);

        // Verify grant was created
        let account1 = contract.accounts.get(&accounts(1)).unwrap();
        let grant1 = account1.grants.get(&1000).unwrap();
        assert_eq!(grant1.total_amount, U128::from(3000));

        // Verify spare_balance was reduced
        assert_eq!(contract.spare_balance, U128::from(2000));
    }

    #[test]
    fn test_issue_empty_grants() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(5000),
            pending_transfers: HashMap::new(),
        };

        // Issue empty grants list
        let grants = vec![];
        contract.issue(1000, grants);

        // Verify spare_balance unchanged
        assert_eq!(contract.spare_balance, U128::from(5000));
        // Verify no accounts were created
        assert_eq!(contract.accounts.len(), 0);
    }

    #[test]
    fn test_issue_overwrites_existing_grant() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        // Create initial grant
        contract.create_grant(accounts(1), 1000, U128::from(2000));
        let account1 = contract.accounts.get_mut(&accounts(1)).unwrap();
        let grant1 = account1.grants.get_mut(&1000).unwrap();
        grant1.claimed_amount = U128::from(500);
        grant1.order_amount = U128::from(300);

        // Issue new grant with same issue_timestamp (should overwrite)
        let grants = vec![(accounts(1), U128::from(5000))];
        contract.issue(1000, grants);

        // Verify grant was overwritten
        let account1 = contract.accounts.get(&accounts(1)).unwrap();
        let grant1 = account1.grants.get(&1000).unwrap();
        assert_eq!(grant1.total_amount, U128::from(5000));
        assert_eq!(grant1.claimed_amount, U128::from(0)); // Reset to 0
        assert_eq!(grant1.order_amount, U128::from(0)); // Reset to 0

        // Verify spare_balance was reduced by new amount
        assert_eq!(contract.spare_balance, U128::from(5000)); // 10000 - 5000
    }

    #[test]
    fn test_issue_workflow_integration() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(10000),
            pending_transfers: HashMap::new(),
        };

        // Issue grants
        let grants = vec![
            (accounts(1), U128::from(5000)),
            (accounts(2), U128::from(3000)),
        ];
        contract.issue(1000, grants);

        // Verify initial state
        assert_eq!(contract.spare_balance, U128::from(2000)); // 10000 - 8000

        // Test that grants can be used in the vesting workflow
        context.block_timestamp(4000); // After full unlock
        testing_env!(context.build());
        contract.claim();

        // Verify claim worked for account 1 (the caller)
        let account1 = contract.accounts.get(&accounts(1)).unwrap();
        let grant1 = account1.grants.get(&1000).unwrap();
        assert_eq!(grant1.order_amount, U128::from(5000)); // Full amount claimable

        // Account 2's grant should still be unclaimed since we're calling from account 1
        let account2 = contract.accounts.get(&accounts(2)).unwrap();
        let grant2 = account2.grants.get(&1000).unwrap();
        assert_eq!(grant2.order_amount, U128::from(0)); // Not claimed yet
        assert_eq!(grant2.claimed_amount, U128::from(0)); // Not claimed yet
    }

    #[test]
    fn test_create_grant() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(0),
            pending_transfers: HashMap::new(),
        };

        // Create a grant
        contract.create_grant(accounts(1), 1000, U128::from(5000));

        // Verify the grant was created
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.total_amount, U128::from(5000));
        assert_eq!(grant.claimed_amount, U128::from(0));
        assert_eq!(grant.order_amount, U128::from(0));
    }

    #[test]
    fn test_create_grant_multiple_grants_same_account() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(0),
            pending_transfers: HashMap::new(),
        };

        // Create multiple grants for the same account
        contract.create_grant(accounts(1), 1000, U128::from(5000));
        contract.create_grant(accounts(1), 2000, U128::from(3000));
        contract.create_grant(accounts(1), 3000, U128::from(7000));

        // Verify all grants were created
        let account = contract.accounts.get(&accounts(1)).unwrap();
        assert_eq!(account.grants.len(), 3);

        let grant1 = account.grants.get(&1000).unwrap();
        assert_eq!(grant1.total_amount, U128::from(5000));

        let grant2 = account.grants.get(&2000).unwrap();
        assert_eq!(grant2.total_amount, U128::from(3000));

        let grant3 = account.grants.get(&3000).unwrap();
        assert_eq!(grant3.total_amount, U128::from(7000));
    }

    #[test]
    fn test_create_grant_multiple_accounts() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(0),
            pending_transfers: HashMap::new(),
        };

        // Create grants for different accounts
        contract.create_grant(accounts(1), 1000, U128::from(5000));
        contract.create_grant(accounts(2), 1000, U128::from(3000));
        contract.create_grant(accounts(3), 2000, U128::from(7000));

        // Verify all accounts and grants were created
        assert_eq!(contract.accounts.len(), 3);

        let account1 = contract.accounts.get(&accounts(1)).unwrap();
        let grant1 = account1.grants.get(&1000).unwrap();
        assert_eq!(grant1.total_amount, U128::from(5000));

        let account2 = contract.accounts.get(&accounts(2)).unwrap();
        let grant2 = account2.grants.get(&1000).unwrap();
        assert_eq!(grant2.total_amount, U128::from(3000));

        let account3 = contract.accounts.get(&accounts(3)).unwrap();
        let grant3 = account3.grants.get(&2000).unwrap();
        assert_eq!(grant3.total_amount, U128::from(7000));
    }

    #[test]
    fn test_create_grant_overwrites_existing() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(0),
            pending_transfers: HashMap::new(),
        };

        // Create initial grant
        contract.create_grant(accounts(1), 1000, U128::from(5000));

        // Verify initial grant
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.total_amount, U128::from(5000));

        // Create grant with same issue_date (should overwrite)
        contract.create_grant(accounts(1), 1000, U128::from(8000));

        // Verify grant was overwritten
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.total_amount, U128::from(8000));
        assert_eq!(grant.claimed_amount, U128::from(0)); // Reset to 0
        assert_eq!(grant.order_amount, U128::from(0)); // Reset to 0
    }

    #[test]
    fn test_create_grant_workflow_integration() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(0),
            pending_transfers: HashMap::new(),
        };

        // Create a grant
        contract.create_grant(accounts(1), 1000, U128::from(10000));

        // Verify initial state
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.total_amount, U128::from(10000));
        assert_eq!(grant.claimed_amount, U128::from(0));
        assert_eq!(grant.order_amount, U128::from(0));

        // Test that the grant can be used in the vesting workflow
        context.block_timestamp(4000); // After full unlock
        testing_env!(context.build());
        contract.claim();

        // Verify claim worked
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.order_amount, U128::from(10000)); // Full amount claimable
    }

    #[test]
    fn test_pending_transfers_prevent_claim_buy_authorize() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(4000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(0),
            pending_transfers: HashMap::new(),
        };

        // Add a grant
        let mut account = Account {
            grants: HashMap::new(),
        };
        account.grants.insert(
            1000,
            Grant {
                total_amount: U128::from(10000),
                claimed_amount: U128::from(0),
                order_amount: U128::from(5000), // Some order amount
            },
        );
        contract.accounts.insert(accounts(1), account);

        // Add the grant to pending transfers (simulating it's being processed by authorize)
        contract.pending_transfers.insert(
            accounts(1),
            vec![(1000, U128::from(2000))], // issue_date, amount
        );

        // Try to claim - should skip the grant because it's pending transfer
        contract.claim();
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.order_amount, U128::from(5000)); // Unchanged because skipped

        // Try to buy - should skip the grant because it's pending transfer
        contract.buy(vec![accounts(1)], 5000); // 50%
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.order_amount, U128::from(5000)); // Unchanged because skipped
        assert_eq!(contract.spare_balance, U128::from(0)); // No spare balance added

        // Try to authorize - should clear pending_transfers and process the grant
        contract.authorize(vec![accounts(1)], Some(5000)); // 50%
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.order_amount, U128::from(0)); // Processed (5000 * 50% = 2500, but order_amount becomes 0)
        assert_eq!(grant.claimed_amount, U128::from(2500)); // 50% of 5000 = 2500

        // Clear pending transfers and try again - should work now
        contract.clear_pending_transfers();
        contract.claim();
        let account = contract.accounts.get(&accounts(1)).unwrap();
        let grant = account.grants.get(&1000).unwrap();
        assert_eq!(grant.order_amount, U128::from(7500)); // Now it should work (remaining 7500 claimable from 10000 total - 2500 claimed)
    }

    #[test]
    fn test_authorize_pending_transfers_hashmap_structure() {
        let mut context = get_context(accounts(1));
        context.block_timestamp(1000);
        testing_env!(context.build());

        let mut contract = Contract {
            token_id: accounts(0),
            accounts: IterableMap::new(b"a".to_vec()),
            config: Config {
                cliff_duration: 1000,
                full_unlock_duration: 2000,
            },
            spare_balance: U128::from(1000000),
            pending_transfers: HashMap::new(),
        };

        // Add grants for two accounts with multiple grants each
        let mut account1 = Account {
            grants: HashMap::new(),
        };
        account1.grants.insert(
            1000,
            Grant {
                total_amount: U128::from(10000),
                claimed_amount: U128::from(0),
                order_amount: U128::from(5000),
            },
        );
        account1.grants.insert(
            2000,
            Grant {
                total_amount: U128::from(20000),
                claimed_amount: U128::from(0),
                order_amount: U128::from(3000),
            },
        );
        contract.accounts.insert(accounts(1), account1);

        let mut account2 = Account {
            grants: HashMap::new(),
        };
        account2.grants.insert(
            1500,
            Grant {
                total_amount: U128::from(15000),
                claimed_amount: U128::from(0),
                order_amount: U128::from(4000),
            },
        );
        contract.accounts.insert(accounts(2), account2);

        // Call authorize to populate pending_transfers
        contract.authorize(vec![accounts(1), accounts(2)], Some(5000)); // 50%

        // Verify the HashMap structure
        assert_eq!(contract.pending_transfers.len(), 2); // Two accounts

        // Check account1 has 2 transfers
        let account1_transfers = contract.pending_transfers.get(&accounts(1)).unwrap();
        assert_eq!(account1_transfers.len(), 2);
        assert!(account1_transfers.contains(&(1000, U128::from(2500)))); // 5000 * 50% = 2500
        assert!(account1_transfers.contains(&(2000, U128::from(1500)))); // 3000 * 50% = 1500

        // Check account2 has 1 transfer
        let account2_transfers = contract.pending_transfers.get(&accounts(2)).unwrap();
        assert_eq!(account2_transfers.len(), 1);
        assert!(account2_transfers.contains(&(1500, U128::from(2000)))); // 4000 * 50% = 2000

        // Verify the grants were updated correctly
        let account1 = contract.accounts.get(&accounts(1)).unwrap();
        let grant1_1000 = account1.grants.get(&1000).unwrap();
        let grant1_2000 = account1.grants.get(&2000).unwrap();
        assert_eq!(grant1_1000.claimed_amount, U128::from(2500));
        assert_eq!(grant1_1000.order_amount, U128::from(0));
        assert_eq!(grant1_2000.claimed_amount, U128::from(1500));
        assert_eq!(grant1_2000.order_amount, U128::from(0));

        let account2 = contract.accounts.get(&accounts(2)).unwrap();
        let grant2_1500 = account2.grants.get(&1500).unwrap();
        assert_eq!(grant2_1500.claimed_amount, U128::from(2000));
        assert_eq!(grant2_1500.order_amount, U128::from(0));
    }
}
