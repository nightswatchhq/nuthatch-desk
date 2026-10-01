//! How numbers are worded, kept the same as the terminal client so the two read alike.

use std::time::Duration;

/// `value` with a comma every three digits.
pub fn group_digits(value: u64) -> String {
    let digits = value.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// `count` blocks, in words.
pub fn count_blocks(count: u64) -> String {
    match count {
        1 => "1 block".into(),
        count => format!("{} blocks", group_digits(count)),
    }
}

/// A span to the nearest unit worth reading.
pub fn format_span(span: Duration) -> String {
    match span.as_secs() {
        secs if secs < 120 => format!("{secs}s"),
        secs if secs < 7200 => format!("{}m", secs / 60),
        secs => format!("{}h", secs / 3600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_are_grouped_in_threes() {
        assert_eq!(group_digits(0), "0");
        assert_eq!(group_digits(999), "999");
        assert_eq!(group_digits(1_000), "1,000");
        assert_eq!(group_digits(26_096_543), "26,096,543");
    }

    #[test]
    fn one_block_is_singular() {
        assert_eq!(count_blocks(1), "1 block");
        assert_eq!(count_blocks(1_300), "1,300 blocks");
    }

    #[test]
    fn spans_change_unit_where_the_terminal_client_does() {
        assert_eq!(format_span(Duration::from_secs(119)), "119s");
        assert_eq!(format_span(Duration::from_secs(120)), "2m");
        assert_eq!(format_span(Duration::from_secs(7200)), "2h");
    }
}
