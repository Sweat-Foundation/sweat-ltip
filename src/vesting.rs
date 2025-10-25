use near_sdk::env::{self, panic_str};

use crate::{Config, Grant};

// let now = u32::try_from(env::block_timestamp_ms() / 1_000)
//     .unwrap_or_else(|_| panic_str("Failed to convert timestamp."));

pub(crate) fn calculate_vested_amount(
    now: u32,
    config: &Config,
    issue_at: u32,
    amount: u128,
) -> u128 {
    let cliff_end = issue_at + config.cliff_duration;
    if now < cliff_end {
        return 0;
    }

    let vesting_end = cliff_end + config.vesting_duration;
    if now >= vesting_end as _ {
        return amount;
    }

    let vest_per_second = amount / config.vesting_duration as u128;
    let seconds_ellapsed = (now - cliff_end) as u128;

    return vest_per_second
        .checked_mul(seconds_ellapsed)
        .unwrap_or_else(|| panic_str("Failed to multiply."))
        .into();
}

#[cfg(test)]
mod tests {
    use crate::{vesting::calculate_vested_amount, Config};

    #[test]
    fn test_vesting_calculation() {
        let amount = 946_080_000_000_000_000_000_000_000;
        let issue_at = 1704067200;
        let config = &Config {
            cliff_duration: 31536000,
            vesting_duration: 94608000,
        };

        assert_eq!(
            0,
            calculate_vested_amount(
                issue_at + config.cliff_duration - 1_000,
                config,
                issue_at,
                amount
            )
        );

        assert_eq!(
            amount,
            calculate_vested_amount(
                issue_at + config.cliff_duration + config.vesting_duration + 1_000,
                config,
                issue_at,
                amount
            )
        );

        assert_eq!(
            10_000_000_000_000_000_000,
            calculate_vested_amount(
                issue_at + config.cliff_duration + 1,
                config,
                issue_at,
                amount
            )
        );

        assert_eq!(
            864_000_000_000_000_000_000_000,
            calculate_vested_amount(
                issue_at + config.cliff_duration + 60 * 60 * 24,
                config,
                issue_at,
                amount
            )
        );

        assert_eq!(
            999_990_000_000_000_000_000_000,
            calculate_vested_amount(
                issue_at + config.cliff_duration + 99_999,
                config,
                issue_at,
                amount
            )
        );
    }
}
