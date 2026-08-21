use near_sdk::AccountId;
use near_sdk_contract_tools::rbac::Rbac;
use rstest::fixture;

use crate::{init::InitApi, Contract, Role};

use super::context::TestContext;

pub const DEFAULT_CLIFF_DURATION: u32 = 1_000;
pub const DEFAULT_VESTING_DURATION: u32 = 2_000;

#[fixture]
pub fn context() -> TestContext {
    TestContext::new()
}

#[fixture]
pub fn alice() -> AccountId {
    "alice.test.near".parse().unwrap()
}

#[fixture]
pub fn bob() -> AccountId {
    "bob.test.near".parse().unwrap()
}

#[fixture]
pub fn issuer() -> AccountId {
    "issuer.test.near".parse().unwrap()
}

#[fixture]
pub fn executor() -> AccountId {
    "exevutor.test.near".parse().unwrap()
}

#[fixture]
pub fn owner() -> AccountId {
    "owner.test.near".parse().unwrap()
}

#[fixture]
pub fn token() -> AccountId {
    "token.test.near".parse().unwrap()
}

#[fixture]
pub fn contract(
    #[default(DEFAULT_CLIFF_DURATION)] cliff_duration: u32,
    #[default(DEFAULT_VESTING_DURATION)] vesting_duration: u32,
    token: AccountId,
    owner: AccountId,
    issuer: AccountId,
    executor: AccountId,
) -> Contract {
    let mut contract = Contract::new(token, cliff_duration, vesting_duration, owner);

    contract.add_role(&executor, &Role::Executor);
    contract.add_role(&issuer, &Role::Issuer);

    contract
}
