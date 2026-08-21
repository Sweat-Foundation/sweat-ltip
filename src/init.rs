use crate::{Config, Contract, ContractExt, StorageKey};
use near_sdk::{near, store::IterableMap, AccountId};
use near_sdk_contract_tools::owner::Owner;

/// InitApi covers the contract bootstrapping entry points.
pub trait InitApi {
    /// Creates a fresh `Contract` configured with the supplied parameters.
    fn new(
        token_id: AccountId,
        cliff_duration: u32,
        vestin_duration: u32,
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
        vesting_duration: u32,
        owner_id: AccountId,
    ) -> Contract {
        let mut contract = Contract {
            token_id,
            accounts: IterableMap::new(StorageKey::Accounts),
            config: Config {
                cliff_duration,
                vesting_duration,
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
    use near_sdk::AccountId;
    use near_sdk_contract_tools::owner::OwnerExternal;
    use rstest::rstest;

    use crate::{init::InitApi, tests::fixtures::*, Contract};

    #[rstest]
    fn init_assigns_owner(owner: AccountId, token: AccountId) {
        let contract = Contract::new(token, 10, 20, owner.clone());

        assert_eq!(contract.own_get_owner().unwrap(), owner);
    }
}
