use crate::{auth::Role, Config, Contract, ContractExt, StorageKey};
use near_sdk::{near, store::IterableMap, AccountId};
use near_sdk_contract_tools::rbac::Rbac;

/// InitApi covers the contract bootstrapping entry points.
pub trait InitApi {
    /// Creates a fresh `Contract` configured with the supplied parameters.
    fn init(
        token_id: AccountId,
        cliff_duration: u32,
        full_unlock_duration: u32,
        admin: AccountId,
    ) -> Contract;
}

#[near]
impl InitApi for Contract {
    #[init]
    #[private]
    fn init(
        token_id: AccountId,
        cliff_duration: u32,
        full_unlock_duration: u32,
        admin: AccountId,
    ) -> Contract {
        let mut contract = Contract {
            token_id,
            accounts: IterableMap::new(StorageKey::Accounts),
            config: Config {
                cliff_duration,
                full_unlock_duration,
            },
            spare_balance: 0.into(),
            pending_transfers: Default::default(),
        };

        contract.add_role(&admin, &Role::Admin);

        contract
    }
}

#[cfg(test)]
mod tests {
    use near_sdk::test_utils::accounts;

    use crate::{
        auth::{AuthApi, Role},
        init::InitApi,
        testing_api::{init_contract_with_spare, set_predecessor},
    };

    #[test]
    fn init_assigns_admin_role() {
        set_predecessor(&accounts(0), 0);
        let contract = <crate::Contract as InitApi>::init(accounts(0), 10, 20, accounts(1));

        assert!(contract.has_role(&accounts(1), Role::Admin));
    }

    #[test]
    fn init_helper_sets_spare_balance() {
        set_predecessor(&accounts(0), 0);
        let contract = init_contract_with_spare(1_000);
        assert_eq!(contract.spare_balance.0, 1_000);
    }
}
