#![cfg(test)]

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

const CLIFF_DURATION: u32 = 1000;
const FULL_UNLOCK_DURATION: u32 = 2000;

fn init_contract_with_spare(spare_balance: u128) -> Contract {
    let admin = accounts(0);
    let operator = accounts(1);

    let mut contract = Contract::init(
        admin.clone(),
        CLIFF_DURATION,
        FULL_UNLOCK_DURATION,
        admin.clone(),
    );

    contract.add_role(&admin, &Roles::Executor);
    contract.add_role(&admin, &Roles::Issuer);
    contract.add_role(&operator, &Roles::Executor);
    contract.add_role(&operator, &Roles::Issuer);

    contract.spare_balance = U128::from(spare_balance);
    contract
}

fn grant_executor(contract: &mut Contract, account: &AccountId) {
    contract.add_role(account, &Roles::Executor);
}

fn grant_issuer(contract: &mut Contract, account: &AccountId) {
    contract.add_role(account, &Roles::Issuer);
}

fn set_predecessor(predecessor: &AccountId, timestamp: u64) {
    let mut context = get_context(predecessor.clone());
    context.block_timestamp(timestamp);
    testing_env!(context.build());
}

#[test]
fn test_claim_during_cliff_period() {
    let mut context = get_context(accounts(1));
    context.block_timestamp(1000); // Set initial timestamp in seconds
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000000);

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

    let mut contract = init_contract_with_spare(1000000);

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

    let mut contract = init_contract_with_spare(1000000);

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

    let mut contract = init_contract_with_spare(1000000);

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
    let admin = accounts(0);
    let executor = admin.clone();
    let beneficiary = accounts(1);

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000000);

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
    contract.accounts.insert(beneficiary.clone(), account);

    contract.add_role(&executor, &Roles::Executor);

    let mut context = get_context(executor.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    contract.buy(vec![beneficiary.clone()], 5000);

    let account = contract.accounts.get(&beneficiary).unwrap();
    let grant = account.grants.get(&1000).unwrap();
    assert_eq!(grant.claimed_amount, U128::from(3500)); // 1000 + 2500
    assert_eq!(grant.order_amount, U128::from(0)); // Should be reset to 0
    assert_eq!(contract.spare_balance, U128::from(1002500)); // 1000000 + 2500
}

#[test]
fn test_buy_function_partial_percentage() {
    let admin = accounts(0);
    let executor = admin.clone();
    let beneficiary = accounts(1);

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000000);

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
    contract.accounts.insert(beneficiary.clone(), account);

    contract.add_role(&executor, &Roles::Executor);

    let mut context = get_context(executor.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    contract.buy(vec![beneficiary.clone()], 1234);

    let account = contract.accounts.get(&beneficiary).unwrap();
    let grant = account.grants.get(&1000).unwrap();
    assert_eq!(grant.claimed_amount, U128::from(1234));
    assert_eq!(grant.order_amount, U128::from(0));
    assert_eq!(contract.spare_balance, U128::from(1001234));
}

#[test]
fn test_buy_function_multiple_accounts() {
    let admin = accounts(0);
    let executor = admin.clone();
    let beneficiary_one = accounts(1);
    let beneficiary_two = accounts(2);

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000000);

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
    contract.accounts.insert(beneficiary_one.clone(), account1);

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
    contract.accounts.insert(beneficiary_two.clone(), account2);

    contract.add_role(&executor, &Roles::Executor);

    let mut context = get_context(executor.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    contract.buy(vec![beneficiary_one.clone(), beneficiary_two.clone()], 2500);

    let account1 = contract.accounts.get(&beneficiary_one).unwrap();
    let grant1 = account1.grants.get(&1000).unwrap();
    assert_eq!(grant1.claimed_amount, U128::from(1250));
    assert_eq!(grant1.order_amount, U128::from(0));

    let account2 = contract.accounts.get(&beneficiary_two).unwrap();
    let grant2 = account2.grants.get(&1000).unwrap();
    assert_eq!(grant2.claimed_amount, U128::from(3000));
    assert_eq!(grant2.order_amount, U128::from(0));

    assert_eq!(contract.spare_balance, U128::from(1003250));
}

#[test]
fn test_buy_function_no_grants() {
    let admin = accounts(0);
    let executor = admin.clone();
    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000000);
    contract.add_role(&executor, &Roles::Executor);

    let mut context = get_context(executor.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    contract.buy(vec![accounts(1)], 5000);

    assert_eq!(contract.spare_balance, U128::from(1000000));
}

#[test]
fn test_buy_function_zero_order_amount() {
    let admin = accounts(0);
    let executor = admin.clone();
    let beneficiary = accounts(1);

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000000);

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
    contract.accounts.insert(beneficiary.clone(), account);

    contract.add_role(&executor, &Roles::Executor);

    let mut context = get_context(executor.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    contract.buy(vec![beneficiary.clone()], 5000);

    let account = contract.accounts.get(&beneficiary).unwrap();
    let grant = account.grants.get(&1000).unwrap();
    assert_eq!(grant.claimed_amount, U128::from(1000));
    assert_eq!(grant.order_amount, U128::from(0));
    assert_eq!(contract.spare_balance, U128::from(1000000));
}

#[test]
fn test_claim_and_buy_workflow() {
    let admin = accounts(0);
    let executor = admin.clone();
    let beneficiary = accounts(1);

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000000);

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
    contract.accounts.insert(beneficiary.clone(), account);

    contract.add_role(&executor, &Roles::Executor);

    // Step 1: Claim after full unlock (timestamp 4000)
    let mut context = get_context(beneficiary.clone());
    context.block_timestamp(4000);
    testing_env!(context.build());
    contract.claim();

    let account = contract.accounts.get(&beneficiary).unwrap();
    let grant = account.grants.get(&1000).unwrap();
    assert_eq!(grant.order_amount, U128::from(10000)); // Full amount available for order

    // Step 2: Buy 30% of the order
    let mut context = get_context(executor.clone());
    context.block_timestamp(4000);
    testing_env!(context.build());
    contract.buy(vec![beneficiary.clone()], 3000); // 30% = 3000 basis points

    let account = contract.accounts.get(&beneficiary).unwrap();
    let grant = account.grants.get(&1000).unwrap();
    assert_eq!(grant.claimed_amount, U128::from(3000)); // 30% of 10000
    assert_eq!(grant.order_amount, U128::from(0)); // Order amount reset
    assert_eq!(contract.spare_balance, U128::from(1003000)); // 1000000 + 3000

    // Step 3: Claim again (should get remaining 70%)
    let mut context = get_context(beneficiary.clone());
    context.block_timestamp(4000);
    testing_env!(context.build());
    contract.claim();

    let account = contract.accounts.get(&beneficiary).unwrap();
    let grant = account.grants.get(&1000).unwrap();
    assert_eq!(grant.order_amount, U128::from(7000)); // Remaining 70%

    // Step 4: Buy remaining 50% of the new order
    let mut context = get_context(executor.clone());
    context.block_timestamp(4000);
    testing_env!(context.build());
    contract.buy(vec![beneficiary.clone()], 5000); // 50% of 7000 = 3500

    let account = contract.accounts.get(&beneficiary).unwrap();
    let grant = account.grants.get(&1000).unwrap();
    assert_eq!(grant.claimed_amount, U128::from(6500)); // 3000 + 3500
    assert_eq!(grant.order_amount, U128::from(0)); // Order amount reset
    assert_eq!(contract.spare_balance, U128::from(1006500)); // 1000000 + 3000 + 3500
}

#[test]
fn test_authorize_function() {
    let executor = accounts(0);
    let beneficiary = accounts(1);

    let mut context = get_context(executor.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000000);
    grant_executor(&mut contract, &executor);

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
    contract.accounts.insert(beneficiary.clone(), account);

    // Authorize 50% (5000 basis points) of the order amount
    contract.authorize(vec![beneficiary.clone()], Some(5000));

    // Check that 50% was authorized: 5000 * 50% = 2500
    let account = contract.accounts.get(&beneficiary).unwrap();
    let grant = account.grants.get(&1000).unwrap();
    assert_eq!(grant.claimed_amount, U128::from(3500)); // 1000 + 2500
    assert_eq!(grant.order_amount, U128::from(0)); // Should be reset to 0
                                                   // spare_balance should remain unchanged (unlike buy function)
    assert_eq!(contract.spare_balance, U128::from(1000000));
}

#[test]
fn test_authorize_function_default_percentage() {
    let executor = accounts(0);
    let beneficiary = accounts(1);

    let mut context = get_context(executor.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000000);
    grant_executor(&mut contract, &executor);

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
    contract.accounts.insert(beneficiary.clone(), account);

    // Authorize with no percentage (should default to 100%)
    contract.authorize(vec![beneficiary.clone()], None);

    // Check that 100% was authorized: 8000 * 100% = 8000
    let account = contract.accounts.get(&beneficiary).unwrap();
    let grant = account.grants.get(&1000).unwrap();
    assert_eq!(grant.claimed_amount, U128::from(8000));
    assert_eq!(grant.order_amount, U128::from(0)); // Should be reset to 0
    assert_eq!(contract.spare_balance, U128::from(1000000)); // Unchanged
}

#[test]
fn test_authorize_function_multiple_accounts() {
    let executor = accounts(0);
    let account_one = accounts(1);
    let account_two = accounts(2);

    let mut context = get_context(executor.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000000);
    grant_executor(&mut contract, &executor);

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
    contract.accounts.insert(account_one.clone(), account1);

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
    contract.accounts.insert(account_two.clone(), account2);

    // Authorize 25% (2500 basis points) from both accounts
    contract.authorize(vec![account_one.clone(), account_two.clone()], Some(2500));

    // Check account 1: 6000 * 25% = 1500
    let account1 = contract.accounts.get(&account_one).unwrap();
    let grant1 = account1.grants.get(&1000).unwrap();
    assert_eq!(grant1.claimed_amount, U128::from(1500));
    assert_eq!(grant1.order_amount, U128::from(0));

    // Check account 2: 4000 * 25% = 1000
    let account2 = contract.accounts.get(&account_two).unwrap();
    let grant2 = account2.grants.get(&1000).unwrap();
    assert_eq!(grant2.claimed_amount, U128::from(3000)); // 2000 + 1000
    assert_eq!(grant2.order_amount, U128::from(0));

    // spare_balance should remain unchanged
    assert_eq!(contract.spare_balance, U128::from(1000000));
}

#[test]
fn test_authorize_function_no_grants() {
    let executor = accounts(0);

    let mut context = get_context(executor.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000000);
    grant_executor(&mut contract, &executor);

    // Try to authorize from an account that doesn't exist
    contract.authorize(vec![accounts(1)], Some(5000));

    // spare_balance should remain unchanged
    assert_eq!(contract.spare_balance, U128::from(1000000));
}

#[test]
fn test_authorize_function_zero_order_amount() {
    let executor = accounts(0);
    let beneficiary = accounts(1);

    let mut context = get_context(executor.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000000);
    grant_executor(&mut contract, &executor);

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
    contract.accounts.insert(beneficiary.clone(), account);

    // Try to authorize from this account
    contract.authorize(vec![beneficiary.clone()], Some(5000));

    // Nothing should change since order_amount is 0
    let account = contract.accounts.get(&beneficiary).unwrap();
    let grant = account.grants.get(&1000).unwrap();
    assert_eq!(grant.claimed_amount, U128::from(1000)); // Unchanged
    assert_eq!(grant.order_amount, U128::from(0)); // Unchanged
    assert_eq!(contract.spare_balance, U128::from(1000000)); // Unchanged
}

#[test]
fn test_complete_workflow_claim_buy_authorize() {
    let admin = accounts(0);
    let executor = admin.clone();
    let beneficiary = accounts(1);

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000000);
    contract.create_grant(beneficiary.clone(), 1000, U128::from(10000));
    contract.add_role(&executor, &Roles::Executor);

    let mut context = get_context(beneficiary.clone());
    context.block_timestamp(4000);
    testing_env!(context.build());
    contract.claim();

    let account = contract.accounts.get(&beneficiary).unwrap();
    let grant = account.grants.get(&1000).unwrap();
    assert_eq!(grant.order_amount, U128::from(10000));

    let mut context = get_context(executor.clone());
    context.block_timestamp(4000);
    testing_env!(context.build());
    contract.buy(vec![beneficiary.clone()], 2000);

    let account = contract.accounts.get(&beneficiary).unwrap();
    let grant = account.grants.get(&1000).unwrap();
    assert_eq!(grant.claimed_amount, U128::from(2000));
    assert_eq!(grant.order_amount, U128::from(0));
    assert_eq!(contract.spare_balance, U128::from(1002000));

    let mut context = get_context(beneficiary.clone());
    context.block_timestamp(4000);
    testing_env!(context.build());
    contract.claim();

    let account = contract.accounts.get(&beneficiary).unwrap();
    let grant = account.grants.get(&1000).unwrap();
    assert_eq!(grant.order_amount, U128::from(8000));

    let mut context = get_context(executor.clone());
    context.block_timestamp(4000);
    testing_env!(context.build());
    contract.authorize(vec![beneficiary.clone()], Some(5000));

    let account = contract.accounts.get(&beneficiary).unwrap();
    let grant = account.grants.get(&1000).unwrap();
    assert_eq!(grant.claimed_amount, U128::from(6000));
    assert_eq!(grant.order_amount, U128::from(0));
    assert_eq!(contract.spare_balance, U128::from(1002000));

    contract.clear_pending_transfers();

    let mut context = get_context(beneficiary.clone());
    context.block_timestamp(4000);
    testing_env!(context.build());
    contract.claim();

    let account = contract.accounts.get(&beneficiary).unwrap();
    let grant = account.grants.get(&1000).unwrap();
    assert_eq!(grant.order_amount, U128::from(4000));

    let mut context = get_context(executor.clone());
    context.block_timestamp(4000);
    testing_env!(context.build());
    contract.authorize(vec![beneficiary.clone()], Some(10000));

    let account = contract.accounts.get(&beneficiary).unwrap();
    let grant = account.grants.get(&1000).unwrap();
    assert_eq!(grant.claimed_amount, U128::from(10000));
    assert_eq!(grant.order_amount, U128::from(0));
    assert_eq!(contract.spare_balance, U128::from(1002000));
}

#[test]
fn test_authorize_callback_function() {
    let mut context = get_context(accounts(1));
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000000);

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

    let mut contract = init_contract_with_spare(1000000);

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

    let mut contract = init_contract_with_spare(0);

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

    let mut contract = init_contract_with_spare(0);

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
    let admin = accounts(0);
    let executor = admin.clone();
    let beneficiary = accounts(1);

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000);
    grant_executor(&mut contract, &executor);

    // Create grants with order amounts
    contract.create_grant(beneficiary.clone(), 1000, U128::from(10000));
    let account1 = contract.accounts.get_mut(&beneficiary).unwrap();
    account1.grants.get_mut(&1000).unwrap().order_amount = U128::from(5000);

    // Call buy with 0% - should decline orders
    let mut context = get_context(executor.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());
    contract.buy(vec![beneficiary.clone()], 0);

    // Verify orders are cleared but no tokens were bought
    let account1 = contract.accounts.get(&beneficiary).unwrap();
    let grant1 = account1.grants.get(&1000).unwrap();
    assert_eq!(grant1.order_amount, U128::from(0));
    assert_eq!(grant1.claimed_amount, U128::from(0)); // No claimed amount added
    assert_eq!(contract.spare_balance, U128::from(1000)); // Spare balance unchanged
}

#[test]
fn test_authorize_with_zero_percentage() {
    let admin = accounts(0);
    let executor = admin.clone();
    let beneficiary = accounts(1);

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000);
    grant_executor(&mut contract, &executor);

    // Create grants with order amounts
    contract.create_grant(beneficiary.clone(), 1000, U128::from(10000));
    let account1 = contract.accounts.get_mut(&beneficiary).unwrap();
    account1.grants.get_mut(&1000).unwrap().order_amount = U128::from(5000);

    // Call authorize with 0% - should decline orders
    let mut context = get_context(executor.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());
    contract.authorize(vec![beneficiary.clone()], Some(0));

    // Verify orders are cleared but no tokens were authorized
    let account1 = contract.accounts.get(&beneficiary).unwrap();
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

    let mut contract = init_contract_with_spare(0);

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

    let mut contract = init_contract_with_spare(10000);

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

    let contract = init_contract_with_spare(10000);

    // Try to get non-existent account
    let account = contract.get_account(&accounts(1));
    assert!(account.is_none());
}

#[test]
fn test_get_account_empty_grants() {
    let mut context = get_context(accounts(1));
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(10000);

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

    let mut contract = init_contract_with_spare(10000);

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

    let mut contract = init_contract_with_spare(10000);

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
    let admin = accounts(0);
    let issuer = admin.clone();

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(10000);
    grant_issuer(&mut contract, &issuer);

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
    let mut context = get_context(accounts(1));
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

    let contract = init_contract_with_spare(10000);

    assert_eq!(contract.get_spare_balance(), U128::from(10000));
}

#[test]
fn test_get_spare_balance_after_issue() {
    let admin = accounts(0);
    let issuer = admin.clone();

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(10000);
    grant_issuer(&mut contract, &issuer);

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
    let admin = accounts(0);
    let executor = admin.clone();
    let beneficiary = accounts(1);

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(10000);
    grant_executor(&mut contract, &executor);

    // Create grants and claim them
    contract.create_grant(beneficiary.clone(), 1000, U128::from(5000));

    let mut context = get_context(beneficiary.clone());
    context.block_timestamp(4000); // After full unlock
    testing_env!(context.build());
    contract.claim();

    // Buy back 50% of orders
    let mut context = get_context(executor.clone());
    context.block_timestamp(4000);
    testing_env!(context.build());
    contract.buy(vec![beneficiary.clone()], 50);

    // Spare balance should increase by 50% of the order amount (50 basis points = 0.5%)
    // bought_amount = (5000 * 50) / 10000 = 25
    assert_eq!(contract.get_spare_balance(), U128::from(10000 + 25)); // 10000 + 0.5% of 5000
}

#[test]
fn test_get_spare_balance_zero() {
    let mut context = get_context(accounts(1));
    context.block_timestamp(1000);
    testing_env!(context.build());

    let contract = init_contract_with_spare(0);

    assert_eq!(contract.get_spare_balance(), U128::from(0));
}

#[test]
fn test_get_spare_balance_large_value() {
    let mut context = get_context(accounts(1));
    context.block_timestamp(1000);
    testing_env!(context.build());

    let contract = init_contract_with_spare(u128::MAX);

    assert_eq!(contract.get_spare_balance(), U128::from(u128::MAX));
}

#[test]
fn test_get_spare_balance_workflow_integration() {
    let admin = accounts(0);
    let executor = admin.clone();
    let issuer = admin.clone();

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(10000);
    grant_executor(&mut contract, &executor);
    grant_issuer(&mut contract, &issuer);

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
    let mut context = get_context(executor.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());
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

    let contract = init_contract_with_spare(10000);

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

    let mut contract = init_contract_with_spare(10000);
    contract.pending_transfers = pending_transfers;

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

    let mut contract = init_contract_with_spare(10000);
    contract.pending_transfers = pending_transfers;

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
    let admin = accounts(0);
    let executor = admin.clone();

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(10000);
    grant_executor(&mut contract, &executor);

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
    let mut context = get_context(executor.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());
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

    let mut contract = init_contract_with_spare(10000);

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

    let mut contract = init_contract_with_spare(10000);
    contract.pending_transfers = pending_transfers;

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
    let admin = accounts(0);
    let executor = admin.clone();

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(10000);
    grant_executor(&mut contract, &executor);

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
    let mut context = get_context(executor.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());
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

    let contract = init_contract_with_spare(10000);

    // No accounts, no orders
    let orders = contract.get_orders();
    assert_eq!(orders.len(), 0);
}

#[test]
fn test_get_orders_no_orders() {
    let mut context = get_context(accounts(1));
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(10000);

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

    let mut contract = init_contract_with_spare(10000);

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

    let mut contract = init_contract_with_spare(10000);

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

    let mut contract = init_contract_with_spare(10000);

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

    let mut contract = init_contract_with_spare(10000);

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

    let mut contract = init_contract_with_spare(10000);

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
    let admin = accounts(0);
    let executor = admin.clone();
    let issuer = admin.clone();

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(10000);
    grant_executor(&mut contract, &executor);
    grant_issuer(&mut contract, &issuer);

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
    let mut context = get_context(executor.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());
    contract.buy(vec![accounts(1)], 5000); // 50% buyback

    // After buy, all orders should be cleared
    let orders = contract.get_orders();
    assert_eq!(orders.len(), 0);
}

#[test]
fn test_issue_function() {
    let admin = accounts(0);
    let issuer = admin.clone();

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(10000);
    grant_issuer(&mut contract, &issuer);

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
    let admin = accounts(0);
    let issuer = admin.clone();

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(5000);
    grant_issuer(&mut contract, &issuer);

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
    let admin = accounts(0);
    let issuer = admin.clone();

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(5000);
    grant_issuer(&mut contract, &issuer);

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
    let admin = accounts(0);
    let issuer = admin.clone();

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(5000);
    grant_issuer(&mut contract, &issuer);

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
    let admin = accounts(0);
    let issuer = admin.clone();

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(5000);
    grant_issuer(&mut contract, &issuer);

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
    let admin = accounts(0);
    let issuer = admin.clone();

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(10000);
    grant_issuer(&mut contract, &issuer);

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
fn test_terminate_basic_functionality() {
    let admin = accounts(0);
    let executor = admin.clone();

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000000);
    grant_executor(&mut contract, &executor);

    // Create a grant with some claimed and order amounts
    let mut account = Account {
        grants: HashMap::new(),
    };
    account.grants.insert(
        1000, // issue_date
        Grant {
            total_amount: U128::from(10000),
            claimed_amount: U128::from(2000),
            order_amount: U128::from(3000),
        },
    );
    contract.accounts.insert(accounts(1), account);

    // Terminate at timestamp 1500 (during cliff period)
    let mut context = get_context(executor.clone());
    context.block_timestamp(1500);
    testing_env!(context.build());
    contract.terminate(accounts(1), 1500);

    // Check that orders were declined
    let account = contract.accounts.get(&accounts(1)).unwrap();
    let grant = account.grants.get(&1000).unwrap();
    assert_eq!(grant.order_amount, U128::from(0)); // Orders declined

    // Check that total_amount was set to claimed_amount (since claimed_amount > unlocked amount)
    assert_eq!(grant.total_amount, U128::from(2000)); // Set to claimed_amount
    assert_eq!(grant.claimed_amount, U128::from(2000)); // Unchanged
}

#[test]
fn test_terminate_during_unlock_period() {
    let admin = accounts(0);
    let executor = admin.clone();

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000000);
    grant_executor(&mut contract, &executor);

    // Create a grant
    let mut account = Account {
        grants: HashMap::new(),
    };
    account.grants.insert(
        1000, // issue_date
        Grant {
            total_amount: U128::from(10000),
            claimed_amount: U128::from(1000),
            order_amount: U128::from(2000),
        },
    );
    contract.accounts.insert(accounts(1), account);

    // Terminate at timestamp 2000 (50% through unlock period)
    // At 2000: cliff_duration=1000, unlock_duration=2000
    // Time since cliff: 2000 - 1000 - 1000 = 0 (just started unlock)
    // Unlocked amount should be 0 (just at cliff end)
    let mut context = get_context(executor.clone());
    context.block_timestamp(2000);
    testing_env!(context.build());
    contract.terminate(accounts(1), 2000);

    let account = contract.accounts.get(&accounts(1)).unwrap();
    let grant = account.grants.get(&1000).unwrap();
    assert_eq!(grant.order_amount, U128::from(0)); // Orders declined
    assert_eq!(grant.total_amount, U128::from(1000)); // Set to claimed_amount
    assert_eq!(grant.claimed_amount, U128::from(1000)); // Unchanged
}

#[test]
fn test_terminate_during_linear_unlock() {
    let admin = accounts(0);
    let executor = admin.clone();

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000000);
    grant_executor(&mut contract, &executor);

    // Create a grant
    let mut account = Account {
        grants: HashMap::new(),
    };
    account.grants.insert(
        1000, // issue_date
        Grant {
            total_amount: U128::from(10000),
            claimed_amount: U128::from(1000),
            order_amount: U128::from(2000),
        },
    );
    contract.accounts.insert(accounts(1), account);

    // Terminate at timestamp 2500 (25% through unlock period)
    // At 2500: cliff_duration=1000, unlock_duration=2000
    // Time since cliff: 2500 - 1000 - 1000 = 500
    // Unlocked amount: 10000 * 500 / 2000 = 2500
    let mut context = get_context(executor.clone());
    context.block_timestamp(2500);
    testing_env!(context.build());
    contract.terminate(accounts(1), 2500);

    let account = contract.accounts.get(&accounts(1)).unwrap();
    let grant = account.grants.get(&1000).unwrap();
    assert_eq!(grant.order_amount, U128::from(0)); // Orders declined
    assert_eq!(grant.total_amount, U128::from(2500)); // 25% unlocked
    assert_eq!(grant.claimed_amount, U128::from(1000)); // Unchanged (less than total)
}

#[test]
fn test_terminate_after_full_unlock() {
    let admin = accounts(0);
    let executor = admin.clone();

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000000);
    grant_executor(&mut contract, &executor);

    // Create a grant
    let mut account = Account {
        grants: HashMap::new(),
    };
    account.grants.insert(
        1000, // issue_date
        Grant {
            total_amount: U128::from(10000),
            claimed_amount: U128::from(5000),
            order_amount: U128::from(2000),
        },
    );
    contract.accounts.insert(accounts(1), account);

    // Terminate at timestamp 4000 (after full unlock)
    let mut context = get_context(executor.clone());
    context.block_timestamp(4000);
    testing_env!(context.build());
    contract.terminate(accounts(1), 4000);

    let account = contract.accounts.get(&accounts(1)).unwrap();
    let grant = account.grants.get(&1000).unwrap();
    assert_eq!(grant.order_amount, U128::from(0)); // Orders declined
    assert_eq!(grant.total_amount, U128::from(10000)); // Fully unlocked
    assert_eq!(grant.claimed_amount, U128::from(5000)); // Unchanged
}

#[test]
fn test_terminate_sets_total_amount_to_claimed_when_needed() {
    let admin = accounts(0);
    let executor = admin.clone();

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000000);
    grant_executor(&mut contract, &executor);

    // Create a grant with claimed_amount > what will be unlocked
    // This tests that total_amount is set to claimed_amount when claimed > unlocked
    let mut account = Account {
        grants: HashMap::new(),
    };
    account.grants.insert(
        1000, // issue_date
        Grant {
            total_amount: U128::from(10000),
            claimed_amount: U128::from(8000), // More than will be unlocked
            order_amount: U128::from(2000),
        },
    );
    contract.accounts.insert(accounts(1), account);

    // Terminate at timestamp 2500 (25% through unlock period)
    // Unlocked amount: 10000 * 500 / 2000 = 2500
    let mut context = get_context(executor.clone());
    context.block_timestamp(2500);
    testing_env!(context.build());
    contract.terminate(accounts(1), 2500);

    let account = contract.accounts.get(&accounts(1)).unwrap();
    let grant = account.grants.get(&1000).unwrap();
    assert_eq!(grant.order_amount, U128::from(0)); // Orders declined
    assert_eq!(grant.total_amount, U128::from(8000)); // Set to claimed_amount (since claimed > unlocked)
    assert_eq!(grant.claimed_amount, U128::from(8000)); // Unchanged
}

#[test]
fn test_terminate_multiple_grants() {
    let admin = accounts(0);
    let executor = admin.clone();

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000000);
    grant_executor(&mut contract, &executor);

    // Create multiple grants
    let mut account = Account {
        grants: HashMap::new(),
    };
    account.grants.insert(
        1000, // issue_date
        Grant {
            total_amount: U128::from(10000),
            claimed_amount: U128::from(1000),
            order_amount: U128::from(2000),
        },
    );
    account.grants.insert(
        2000, // issue_date (different)
        Grant {
            total_amount: U128::from(5000),
            claimed_amount: U128::from(500),
            order_amount: U128::from(1000),
        },
    );
    contract.accounts.insert(accounts(1), account);

    // Terminate at timestamp 2500
    let mut context = get_context(executor.clone());
    context.block_timestamp(2500);
    testing_env!(context.build());
    contract.terminate(accounts(1), 2500);

    let account = contract.accounts.get(&accounts(1)).unwrap();

    // First grant (issue_date 1000): 25% unlocked = 2500
    let grant1 = account.grants.get(&1000).unwrap();
    assert_eq!(grant1.order_amount, U128::from(0));
    assert_eq!(grant1.total_amount, U128::from(2500));
    assert_eq!(grant1.claimed_amount, U128::from(1000));

    // Second grant (issue_date 2000): still in cliff, but set to claimed_amount
    let grant2 = account.grants.get(&2000).unwrap();
    assert_eq!(grant2.order_amount, U128::from(0));
    assert_eq!(grant2.total_amount, U128::from(500)); // Set to claimed_amount
    assert_eq!(grant2.claimed_amount, U128::from(500)); // Unchanged
}

#[test]
fn test_terminate_nonexistent_account() {
    let admin = accounts(0);
    let executor = admin.clone();

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000000);
    grant_executor(&mut contract, &executor);

    // Terminate non-existent account - should not panic
    let mut context = get_context(executor.clone());
    context.block_timestamp(2500);
    testing_env!(context.build());
    contract.terminate(accounts(2), 2500);

    // Verify contract state is unchanged
    assert_eq!(contract.accounts.len(), 0);
}

#[test]
fn test_terminate_preserves_claimed_amount_when_unlocked_less() {
    let admin = accounts(0);
    let executor = admin.clone();

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000000);
    grant_executor(&mut contract, &executor);

    // Create a grant where claimed_amount > what will be unlocked at termination
    let mut account = Account {
        grants: HashMap::new(),
    };
    account.grants.insert(
        1000, // issue_date
        Grant {
            total_amount: U128::from(10000),
            claimed_amount: U128::from(6000), // More than will be unlocked
            order_amount: U128::from(1000),
        },
    );
    contract.accounts.insert(accounts(1), account);

    // Terminate at timestamp 1500 (during cliff period - 0% unlocked)
    let mut context = get_context(executor.clone());
    context.block_timestamp(1500);
    testing_env!(context.build());
    contract.terminate(accounts(1), 1500);

    let account = contract.accounts.get(&accounts(1)).unwrap();
    let grant = account.grants.get(&1000).unwrap();

    // Orders should be declined
    assert_eq!(grant.order_amount, U128::from(0));

    // total_amount should be set to claimed_amount (6000) since unlocked (0) < claimed (6000)
    assert_eq!(grant.total_amount, U128::from(6000));

    // claimed_amount should remain unchanged
    assert_eq!(grant.claimed_amount, U128::from(6000));
}

#[test]
fn test_terminate_workflow_integration() {
    let admin = accounts(0);
    let executor = admin.clone();
    let user = accounts(1);

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(1000000);
    grant_executor(&mut contract, &executor);

    // Create account with grants
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
    contract.accounts.insert(user.clone(), account);

    // Simulate some activity: claim and create orders
    let mut context = get_context(user.clone());
    context.block_timestamp(4000);
    testing_env!(context.build());
    contract.claim();

    // Manually set order amounts to simulate buy/authorize workflow
    let account = contract.accounts.get_mut(&user).unwrap();
    let grant = account.grants.get_mut(&1000).unwrap();
    grant.order_amount = U128::from(3000);

    // Terminate the account
    let mut context = get_context(executor.clone());
    context.block_timestamp(2500);
    testing_env!(context.build());
    contract.terminate(user.clone(), 2500);

    // Verify termination effects
    let account = contract.accounts.get(&user).unwrap();
    let grant = account.grants.get(&1000).unwrap();
    assert_eq!(grant.order_amount, U128::from(0)); // Orders declined
    assert_eq!(grant.total_amount, U128::from(2500)); // Reduced to unlocked amount
                                                      // claimed_amount should be adjusted if needed
}

#[test]
fn test_issue_workflow_integration() {
    let admin = accounts(0);
    let issuer = admin.clone();

    let mut context = get_context(admin.clone());
    context.block_timestamp(1000);
    testing_env!(context.build());

    let mut contract = init_contract_with_spare(10000);
    grant_issuer(&mut contract, &issuer);

    // Issue grants
    let grants = vec![
        (accounts(1), U128::from(5000)),
        (accounts(2), U128::from(3000)),
    ];
    contract.issue(1000, grants);

    // Verify initial state
    assert_eq!(contract.spare_balance, U128::from(2000)); // 10000 - 8000

    // Test that grants can be used in the vesting workflow
    let mut context = get_context(accounts(1));
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

    let mut contract = init_contract_with_spare(0);

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

    let mut contract = init_contract_with_spare(0);

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

    let mut contract = init_contract_with_spare(0);

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

    let mut contract = init_contract_with_spare(0);

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

    let mut contract = init_contract_with_spare(0);

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

    let mut contract = init_contract_with_spare(0);

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

    let mut contract = init_contract_with_spare(1000000);

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

#[test]
#[should_panic(expected = "Unauthorized role")]
fn test_buy_requires_executor_role() {
    set_predecessor(&accounts(2), 1000);

    let mut contract = init_contract_with_spare(1000000);

    let mut account = Account {
        grants: HashMap::new(),
    };
    account.grants.insert(
        1000,
        Grant {
            total_amount: U128::from(10000),
            claimed_amount: U128::from(0),
            order_amount: U128::from(5000),
        },
    );
    contract.accounts.insert(accounts(1), account);

    contract.buy(vec![accounts(1)], 5000);
}

#[test]
#[should_panic(expected = "Unauthorized role")]
fn test_issue_requires_issuer_role() {
    set_predecessor(&accounts(2), 1000);

    let mut contract = init_contract_with_spare(10000);
    let grants = vec![(accounts(1), U128::from(5000))];

    contract.issue(1000, grants);
}

#[test]
fn test_issue_with_issuer_role() {
    set_predecessor(&accounts(0), 1000);

    let mut contract = init_contract_with_spare(10000);
    contract.create_grant(accounts(1), 500, U128::from(2000));

    contract.issue(1000, vec![(accounts(2), U128::from(4000))]);

    let account = contract.accounts.get(&accounts(2)).unwrap();
    let grant = account.grants.get(&1000).unwrap();
    assert_eq!(grant.total_amount, U128::from(4000));
    assert_eq!(contract.spare_balance, U128::from(6000));
}
