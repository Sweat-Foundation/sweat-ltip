use crate::{Config, Contract, ContractExt, StorageKey};
use near_sdk::{near, store::IterableMap, AccountId};
use near_sdk_contract_tools::owner::Owner;

/// InitApi covers the contract bootstrapping entry points.
pub trait InitApi {
    /// Creates a fresh `Contract` configured with the supplied parameters.
    fn new(
        token_id: AccountId,
        cliff_duration: u32,
        full_unlock_duration: u32,
        owner_id: AccountId,
    ) -> Contract;
}

#[near]
impl InitApi for Contract {
    #[init]
    #[private]
    fn new(
        token_id: AccountId,
        cliff_duration: u32,
        full_unlock_duration: u32,
        owner_id: AccountId,
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

        Owner::init(&mut contract, &owner_id);

        contract
    }
}

#[cfg(test)]
mod tests {
    use near_sdk::test_utils::accounts;
    use near_sdk_contract_tools::owner::OwnerExternal;

    use crate::{
        init::InitApi,
        testing_api::{init_contract_with_spare, set_predecessor},
        Contract,
    };

    #[test]
    fn init_assigns_owner() {
        set_predecessor(&accounts(0), 0);
        let contract = Contract::new(accounts(0), 10, 20, accounts(1));

        assert_eq!(contract.own_get_owner().unwrap(), accounts(1));
    }

    #[test]
    fn init_helper_sets_spare_balance() {
        set_predecessor(&accounts(0), 0);
        let contract = init_contract_with_spare(1_000);
        assert_eq!(contract.spare_balance.0, 1_000);
    }
}
