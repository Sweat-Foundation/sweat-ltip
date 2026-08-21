use near_sdk::env::panic_str;

pub(crate) fn calculate_vested_amount(
    now: u32,
    cliff_end: u32,
    vesting_end: u32,
    amount: u128,
) -> u128 {
    if now < cliff_end {
        return 0;
    }

    if now >= vesting_end {
        return amount;
    }

    let vest_per_second = amount / u128::from(vesting_end - cliff_end);
    let seconds_ellapsed = (now - cliff_end) as u128;

    return vest_per_second
        .checked_mul(seconds_ellapsed)
        .unwrap_or_else(|| panic_str("Failed to multiply."))
        .into();
}

#[cfg(test)]
mod tests {
    use crate::vesting::calculate_vested_amount;

    #[test]
    fn test_vesting_calculation() {
        let amount = 946_080_000_000_000_000_000_000_000;
        let cliff_duration = 31536000;
        let vesting_duration = 94608000;
        let issue_at = 1704067200;
        let cliff_end = issue_at + cliff_duration;
        let vesting_end = cliff_end + vesting_duration;

        assert_eq!(
            0,
            calculate_vested_amount(cliff_end - 1_000, cliff_end, vesting_end, amount)
        );

        assert_eq!(
            amount,
            calculate_vested_amount(vesting_end + 1_000, cliff_end, vesting_end, amount)
        );

        assert_eq!(
            10_000_000_000_000_000_000,
            calculate_vested_amount(cliff_end + 1, cliff_end, vesting_end, amount)
        );

        assert_eq!(
            864_000_000_000_000_000_000_000,
            calculate_vested_amount(cliff_end + 60 * 60 * 24, cliff_end, vesting_end, amount)
        );

        assert_eq!(
            999_990_000_000_000_000_000_000,
            calculate_vested_amount(cliff_end + 99_999, cliff_end, vesting_end, amount)
        );
    }
}
