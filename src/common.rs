use near_sdk::env::{self, panic_str};

pub(crate) fn now() -> u32 {
    u32::try_from(env::block_timestamp_ms() / 1_000)
        .unwrap_or_else(|_| panic_str("Failed to convert current timestamp to seconds"))
}
