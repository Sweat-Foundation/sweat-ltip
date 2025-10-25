use near_sdk::{near, AccountId, BorshStorageKey};
use near_sdk_contract_tools::owner::Owner;
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
}

#[cfg(test)]
mod tests {
    use std::panic::{self, AssertUnwindSafe};

    use near_sdk::test_utils::accounts;

    use crate::{
        auth::{AuthApi, Role},
        testing_api::{init_contract_with_spare, set_predecessor},
    };

    #[test]
    fn grant_and_revoke_role() {
        set_predecessor(&accounts(0), 0);
        let mut contract = init_contract_with_spare(0);

        contract.grant_role(&accounts(1), Role::Executor);
        assert!(contract.has_role(&accounts(1), Role::Executor));

        contract.revoke_role(&accounts(1), Role::Executor);
        assert!(!contract.has_role(&accounts(1), Role::Executor));
    }

    #[test]
    #[should_panic]
    fn grant_role_requires_owner() {
        set_predecessor(&accounts(0), 0);
        let mut contract = init_contract_with_spare(0);

        set_predecessor(&accounts(1), 0);
        let err = panic::catch_unwind(AssertUnwindSafe(|| {
            contract.grant_role(&accounts(2), Role::Issuer);
        }))
        .err()
        .expect("grant_role should panic for non-owners");

        assert!(!contract.has_role(&accounts(2), Role::Issuer));
        panic::resume_unwind(err);
    }

    #[test]
    #[should_panic]
    fn revoke_role_requires_owner() {
        set_predecessor(&accounts(0), 0);
        let mut contract = init_contract_with_spare(0);
        contract.grant_role(&accounts(1), Role::Executor);
        assert!(contract.has_role(&accounts(1), Role::Executor));

        set_predecessor(&accounts(2), 0);
        let err = panic::catch_unwind(AssertUnwindSafe(|| {
            contract.revoke_role(&accounts(1), Role::Executor);
        }))
        .err()
        .expect("revoke_role should panic for non-owners");

        assert!(contract.has_role(&accounts(1), Role::Executor));
        panic::resume_unwind(err);
    }

    #[test]
    fn members_returns_all_role_holders() {
        set_predecessor(&accounts(0), 0);
        let mut contract = init_contract_with_spare(0);

        contract.grant_role(&accounts(1), Role::Issuer);
        contract.grant_role(&accounts(2), Role::Issuer);

        let mut members = contract.members(Role::Issuer);
        members.sort();

        assert_eq!(members, vec![accounts(0), accounts(1), accounts(2)]);
    }

    #[test]
    #[should_panic]
    fn non_owner_with_role_cannot_grant_roles() {
        set_predecessor(&accounts(0), 0);
        let mut contract = init_contract_with_spare(0);
        contract.grant_role(&accounts(1), Role::Executor);
        assert!(contract.has_role(&accounts(1), Role::Executor));

        set_predecessor(&accounts(1), 0);
        let err = panic::catch_unwind(AssertUnwindSafe(|| {
            contract.grant_role(&accounts(2), Role::Issuer);
        }))
        .err()
        .expect("grant_role should panic for non-owners");

        assert!(!contract.has_role(&accounts(2), Role::Issuer));
        panic::resume_unwind(err);
    }

    #[test]
    #[should_panic]
    fn non_owner_with_role_cannot_revoke_roles() {
        set_predecessor(&accounts(0), 0);
        let mut contract = init_contract_with_spare(0);
        contract.grant_role(&accounts(1), Role::Executor);
        assert!(contract.has_role(&accounts(1), Role::Executor));

        set_predecessor(&accounts(1), 0);
        let err = panic::catch_unwind(AssertUnwindSafe(|| {
            contract.revoke_role(&accounts(0), Role::Issuer);
        }))
        .err()
        .expect("revoke_role should panic for non-owners");

        assert!(contract.has_role(&accounts(0), Role::Issuer));
        panic::resume_unwind(err);
    }
}
