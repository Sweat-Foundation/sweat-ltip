use std::fmt::Display;

use near_sdk::env::{self, panic_str};

pub const ONE_DAY_IN_SECONDS: u32 = 86_400;
pub const ONE_YEAR_IN_SECONDS: u32 = 31_556_952;

const TOKEN_UNITS: u128 = 1_000_000_000_000_000_000; // 1 token = 1e18 token units

pub trait ToOtto {
    fn to_otto(self) -> u128;
}

impl ToOtto for u128 {
    fn to_otto(self) -> u128 {
        self * TOKEN_UNITS
    }
}

impl ToOtto for u64 {
    fn to_otto(self) -> u128 {
        u128::from(self) * TOKEN_UNITS
    }
}

pub(crate) fn now() -> u32 {
    u32::try_from(env::block_timestamp_ms() / 1_000)
        .unwrap_or_else(|_| panic_str("Failed to convert current timestamp to seconds"))
}

pub(crate) fn assert_gas<Message: Display>(gas_needed: u64, error: impl FnOnce() -> Message) {
    let gas_left = env::prepaid_gas().as_gas() - env::used_gas().as_gas();

    if gas_left < gas_needed {
        let error = error();

        env::panic_str(&format!(
            r"Not enough gas left. Consider attaching more gas to the transaction.
               {error}
               Gas left: {gas_left} Needed: {gas_needed}. Need additional {} gas",
            gas_needed - gas_left
        ));
    }
}
