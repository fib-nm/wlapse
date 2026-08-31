use std::time::Duration;

pub fn format_elapsed(elapsed: Duration) -> String {
    const MAX_TENTHS: u128 = ((100 * 60 * 60) - 1) * 10 + 9;

    let total_tenths = (elapsed.as_millis() / 100).min(MAX_TENTHS);
    let tenths = total_tenths % 10;
    let total_seconds = total_tenths / 10;
    let seconds = total_seconds % 60;
    let total_minutes = total_seconds / 60;
    let minutes = total_minutes % 60;
    let hours = total_minutes / 60;

    format!("{hours:02}:{minutes:02}:{seconds:02}.{tenths}")
}

#[cfg(test)]
mod tests {
    use super::format_elapsed;
    use std::time::Duration;

    #[test]
    fn formats_zero() {
        assert_eq!(format_elapsed(Duration::ZERO), "00:00:00.0");
    }

    #[test]
    fn formats_tenths_and_carries_time_units() {
        let elapsed = Duration::from_millis(3_723_456);
        assert_eq!(format_elapsed(elapsed), "01:02:03.4");
    }

    #[test]
    fn clamps_values_above_two_digit_hours() {
        assert_eq!(
            format_elapsed(Duration::from_secs(100 * 60 * 60)),
            "99:59:59.9"
        );
    }
}
