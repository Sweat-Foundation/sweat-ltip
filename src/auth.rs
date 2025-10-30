use near_sdk::{near, AccountId, BorshStorageKey};
use near_sdk_contract_tools::owner::Owner;
use near_sdk_contract_tools::pause::Pause;
use near_sdk_contract_tools::rbac::Rbac;

use crate::{Contract, ContractExt};

#[derive(BorshStorageKey)]
#[near(serializers = [json, borsh])]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Issuer,
    Executor,
    Predecessor,
}

/// AuthApi exposes helper methods for managing role assignments on the contract.
pub trait AuthApi {
    /// Grants the specified `role` to `account_id`.
    fn grant_role(&mut self, account_id: &AccountId, role: Role);
    /// Revokes the specified `role` from `account_id`.
    fn revoke_role(&mut self, account_id: &AccountId, role: Role);
    /// Returns true when `account_id` currently holds `role`.
    fn has_role(&self, account_id: &AccountId, role: Role) -> bool;
    /// Returns list of accounts for the provided `Role`.
    fn members(&self, role: Role) -> Vec<AccountId>;
    fn force_unpause(&mut self);
}

#[near]
impl AuthApi for Contract {
    fn grant_role(&mut self, account_id: &AccountId, role: Role) {
        Self::require_owner();

        self.add_role(account_id, &role);
    }

    fn revoke_role(&mut self, account_id: &AccountId, role: Role) {
        Self::require_owner();

        self.remove_role(account_id, &role);
    }

    fn has_role(&self, account_id: &AccountId, role: Role) -> bool {
        <Self as Rbac>::has_role(account_id, &role)
    }

    fn members(&self, role: Role) -> Vec<AccountId> {
        Self::iter_members_of(&role).collect()
    }

    fn force_unpause(&mut self) {
        Self::require_owner();

        self.unpause();
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{self, AssertUnwindSafe};

    use near_sdk::AccountId;
    use rstest::*;

    use crate::{
        auth::{AuthApi, Role},
        tests::context::TestContext,
        tests::fixtures::*,
        Contract,
    };

    #[rstest]
    fn grant_and_revoke_role(
        mut context: TestContext,
        mut contract: Contract,
        owner: AccountId,
        alice: AccountId,
    ) {
        context.switch_account(&owner);
        contract.grant_role(&alice, Role::Executor);
        assert!(contract.has_role(&alice, Role::Executor));

        contract.revoke_role(&alice, Role::Executor);
        assert!(!contract.has_role(&alice, Role::Executor));
    }

    #[rstest]
    fn grant_role_requires_owner(
        mut context: TestContext,
        mut contract: Contract,
        alice: AccountId,
        bob: AccountId,
    ) {
        context.switch_account(&alice);
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            contract.grant_role(&bob, Role::Issuer);
        }));

        assert!(result.is_err());
        assert!(!contract.has_role(&bob, Role::Issuer));
    }

    #[rstest]
    fn revoke_role_requires_owner(
        mut context: TestContext,
        mut contract: Contract,
        owner: AccountId,
        alice: AccountId,
        bob: AccountId,
    ) {
        context.switch_account(&owner);
        contract.grant_role(&alice, Role::Executor);
        assert!(contract.has_role(&alice, Role::Executor));

        context.switch_account(&bob);
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            contract.revoke_role(&alice, Role::Executor);
        }));

        assert!(result.is_err());
        assert!(contract.has_role(&alice, Role::Executor));
    }

    #[rstest]
    fn members_returns_all_role_holders(
        mut context: TestContext,
        mut contract: Contract,
        owner: AccountId,
        issuer: AccountId,
        alice: AccountId,
        bob: AccountId,
    ) {
        context.switch_account(&owner);
        contract.grant_role(&alice, Role::Issuer);
        contract.grant_role(&bob, Role::Issuer);

        let mut members = contract.members(Role::Issuer);
        members.sort();

        // Contract fixture grants Issuer role to issuer, and we grant it to alice and bob
        assert!(members.contains(&issuer));
        assert!(members.contains(&alice));
        assert!(members.contains(&bob));
        assert_eq!(members.len(), 3);
    }

    #[rstest]
    fn non_owner_with_role_cannot_grant_roles(
        mut context: TestContext,
        mut contract: Contract,
        owner: AccountId,
        alice: AccountId,
        bob: AccountId,
    ) {
        context.switch_account(&owner);
        contract.grant_role(&alice, Role::Executor);
        assert!(contract.has_role(&alice, Role::Executor));

        context.switch_account(&alice);
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            contract.grant_role(&bob, Role::Issuer);
        }));

        assert!(result.is_err());
        assert!(!contract.has_role(&bob, Role::Issuer));
    }

    #[rstest]
    fn non_owner_with_role_cannot_revoke_roles(
        mut context: TestContext,
        mut contract: Contract,
        owner: AccountId,
        issuer: AccountId,
        alice: AccountId,
    ) {
        context.switch_account(&owner);
        contract.grant_role(&alice, Role::Executor);
        assert!(contract.has_role(&alice, Role::Executor));

        context.switch_account(&alice);
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            contract.revoke_role(&issuer, Role::Issuer);
        }));

        assert!(result.is_err());
        assert!(contract.has_role(&issuer, Role::Issuer));
    }
}
