/// Table-free primary weight: ASCII punctuation/symbols precede digits and
/// letters. Preserve their existing within-group order, control ordering,
/// and every non-ASCII weight relative to the entire ASCII range. Inputs
/// are already lowercase. This approximates classes, not ICU punctuation order.
#[inline]
pub(super) fn locale_primary_weight(c: u32) -> u32 {
    if (b'0' as u32..=b'9' as u32).contains(&c)
        || (b'a' as u32..=b'z' as u32).contains(&c)
        || c >= 0x7f
    {
        c + 0x80
    } else {
        c
    }
}

/// Independent allocating primary oracle for the streaming differential tests.
#[cfg(test)]
pub(super) fn reference_primary_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let weights = |text: &str| {
        text.chars()
            .map(|c| {
                let cp = c as u32;
                if c.is_ascii_digit() || c.is_ascii_lowercase() || cp >= 127 {
                    cp + 128
                } else {
                    cp
                }
            })
            .collect::<Vec<_>>()
    };
    weights(a).cmp(&weights(b))
}

#[cfg(test)]
mod tests {
    fn compare(a: &str, b: &str) -> f64 {
        super::super::locale_compare_default(
            crate::string::wtf8::Wtf8Str::from_str(a),
            crate::string::wtf8::Wtf8Str::from_str(b),
        )
    }

    #[test]
    fn ascii_punctuation_precedes_digits_and_letters() {
        for punctuation in 0x21u8..=0x7e {
            if !punctuation.is_ascii_punctuation() {
                continue;
            }
            for letter_or_digit in b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
            {
                let a = format!("items/{}", punctuation as char);
                let b = format!("items/{}", *letter_or_digit as char);
                assert_eq!(compare(&a, &b), -1.0, "{a:?} vs {b:?}");
                assert_eq!(compare(&b, &a), 1.0);
            }
        }
        // The contextual lowercase fallback and scalar walk share the weights.
        assert_eq!(compare("ΟΣ/{id}", "ος/bulk"), -1.0);
        assert_eq!(compare("café/{id}", "café/bulk"), -1.0);
        // Within-group and non-ASCII comparisons retain the approximation.
        assert_eq!(compare("{", "~"), -1.0);
        assert_eq!(compare("ä", "z"), 1.0);
    }
}
