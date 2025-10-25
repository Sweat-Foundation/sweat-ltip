use near_sdk::near;

use crate::{Config, Contract, ContractExt};

pub trait ConfigApi {
    fn get_config(&self) -> Config;
}

#[near]
impl ConfigApi for Contract {
    fn get_config(&self) -> Config {
        self.config.clone()
    }
}
