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
}
