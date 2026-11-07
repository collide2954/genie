pub fn parse_bases(text: &str) -> Option<u64> {
    let text: String = text.trim().chars().filter(|&c| c != ',').collect();
    let (number, scale) = match text.as_bytes().last()? {
        b'k' | b'K' => (&text[..text.len() - 1], 1_000),
        b'm' | b'M' => (&text[..text.len() - 1], 1_000_000),
        b'g' | b'G' => (&text[..text.len() - 1], 1_000_000_000),
        _ => (text.as_str(), 1),
    };
    let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
    if whole.is_empty() && fraction.is_empty() {
        return None;
    }
    let whole = digits(whole)?.checked_mul(scale)?;
    let divisor = 10_u64.checked_pow(u32::try_from(fraction.len()).ok()?)?;
    let fraction = digits(fraction)?.checked_mul(scale)?;
    if fraction % divisor != 0 {
        return None;
    }
    whole.checked_add(fraction / divisor)
}

pub fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut text = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

fn digits(text: &str) -> Option<u64> {
    if text.is_empty() {
        return Some(0);
    }
    if !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::{grouped, parse_bases};

    #[test]
    fn groups_digits_in_thousands() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1,000");
        assert_eq!(grouped(1_234_567), "1,234,567");
        assert_eq!(grouped(u64::MAX), "18,446,744,073,709,551,615");
    }

    #[test]
    fn grouped_text_parses_back() {
        assert_eq!(parse_bases(&grouped(248_956_422)), Some(248_956_422));
    }

    #[test]
    fn parses_plain_and_grouped_digits() {
        assert_eq!(parse_bases("12345"), Some(12_345));
        assert_eq!(parse_bases("12,345"), Some(12_345));
        assert_eq!(parse_bases(" 42 "), Some(42));
    }

    #[test]
    fn parses_size_suffixes() {
        assert_eq!(parse_bases("1k"), Some(1_000));
        assert_eq!(parse_bases("2.5M"), Some(2_500_000));
        assert_eq!(parse_bases("1g"), Some(1_000_000_000));
    }

    #[test]
    fn rejects_fractional_bases() {
        assert_eq!(parse_bases("1.5"), None);
        assert_eq!(parse_bases("1.2345k"), None);
    }

    #[test]
    fn rejects_malformed_text() {
        for text in [
            "",
            "k",
            ".",
            "abc",
            "-5",
            "+5",
            "1e6",
            "18446744073709551616",
        ] {
            assert_eq!(parse_bases(text), None, "{text}");
        }
    }
}
