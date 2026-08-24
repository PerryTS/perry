use super::*;

pub(super) struct CollatorCompareOptions {
    pub(super) locale: String,
    pub(super) usage: String,
    pub(super) collation: String,
    pub(super) sensitivity: String,
    pub(super) numeric: bool,
    pub(super) case_first: String,
    pub(super) ignore_punctuation: bool,
}

fn compare_digit_runs(left: &str, right: &str) -> std::cmp::Ordering {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    let (mut li, mut ri) = (0usize, 0usize);
    while li < left.len() && ri < right.len() {
        if left[li].is_ascii_digit() && right[ri].is_ascii_digit() {
            let left_end = (li..left.len())
                .find(|&i| !left[i].is_ascii_digit())
                .unwrap_or(left.len());
            let right_end = (ri..right.len())
                .find(|&i| !right[i].is_ascii_digit())
                .unwrap_or(right.len());
            let left_significant = (li..left_end)
                .find(|&i| left[i] != b'0')
                .unwrap_or(left_end);
            let right_significant = (ri..right_end)
                .find(|&i| right[i] != b'0')
                .unwrap_or(right_end);
            let length_order = (left_end - left_significant).cmp(&(right_end - right_significant));
            if length_order != std::cmp::Ordering::Equal {
                return length_order;
            }
            let value_order =
                left[left_significant..left_end].cmp(&right[right_significant..right_end]);
            if value_order != std::cmp::Ordering::Equal {
                return value_order;
            }
            li = left_end;
            ri = right_end;
            continue;
        }

        let left_char = std::str::from_utf8(&left[li..])
            .expect("collation key is UTF-8")
            .chars()
            .next()
            .expect("left key is not exhausted");
        let right_char = std::str::from_utf8(&right[ri..])
            .expect("collation key is UTF-8")
            .chars()
            .next()
            .expect("right key is not exhausted");
        let order = left_char.cmp(&right_char);
        if order != std::cmp::Ordering::Equal {
            return order;
        }
        li += left_char.len_utf8();
        ri += right_char.len_utf8();
    }
    (left.len() - li).cmp(&(right.len() - ri))
}

fn compare_collation_keys(left: &str, right: &str, numeric: bool) -> std::cmp::Ordering {
    if numeric {
        compare_digit_runs(left, right)
    } else {
        left.cmp(right)
    }
}

fn case_first_order(left: &str, right: &str, case_first: &str) -> std::cmp::Ordering {
    if !matches!(case_first, "upper" | "lower") {
        return std::cmp::Ordering::Equal;
    }
    for (left, right) in left.chars().zip(right.chars()) {
        if left == right || left.to_lowercase().to_string() != right.to_lowercase().to_string() {
            continue;
        }
        let left_upper = left.is_uppercase();
        let right_upper = right.is_uppercase();
        if left_upper != right_upper {
            let upper_first = case_first == "upper";
            return if left_upper == upper_first {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }
    }
    std::cmp::Ordering::Equal
}

pub(super) fn collator_compare_order(
    options: &CollatorCompareOptions,
    left: &str,
    right: &str,
) -> std::cmp::Ordering {
    let swedish = options.locale == "sv" || options.locale.starts_with("sv-");
    let phonebook = options.collation == "phonebk"
        || (options.usage == "search"
            && (options.locale == "de" || options.locale.starts_with("de-")));
    let primary_left = if swedish {
        swedish_collation_key(left)
            .into_iter()
            .filter_map(char::from_u32)
            .collect()
    } else if phonebook {
        german_phonebook_key(left)
    } else {
        base_collation_key(left, false)
    };
    let primary_right = if swedish {
        swedish_collation_key(right)
            .into_iter()
            .filter_map(char::from_u32)
            .collect()
    } else if phonebook {
        german_phonebook_key(right)
    } else {
        base_collation_key(right, false)
    };
    let primary = compare_collation_keys(&primary_left, &primary_right, options.numeric);
    if primary != std::cmp::Ordering::Equal || options.sensitivity == "base" {
        return primary;
    }
    let normalized_left = collation_normalize(left);
    let normalized_right = collation_normalize(right);
    if options.sensitivity == "accent" {
        return compare_collation_keys(
            &normalized_left.to_lowercase(),
            &normalized_right.to_lowercase(),
            options.numeric,
        );
    }
    let case_order = case_first_order(&normalized_left, &normalized_right, &options.case_first);
    if case_order != std::cmp::Ordering::Equal {
        return case_order;
    }
    if options.sensitivity == "case" {
        return compare_collation_keys(
            &base_collation_key(&normalized_left, true),
            &base_collation_key(&normalized_right, true),
            options.numeric,
        );
    }
    compare_collation_keys(&normalized_left, &normalized_right, options.numeric)
}
