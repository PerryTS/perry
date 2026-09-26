//! `split_string_by_string`, the fast entry for `str.split(string, limit)`
//! (#10519): it must build the same parts as the general byte split for every
//! input it accepts, and decline every input it cannot answer exactly.

use super::split::split_string_by_string;
use super::*;
use crate::value::{JSValue, TAG_UNDEFINED};

const UNDEFINED: f64 = f64::from_bits(TAG_UNDEFINED);

fn heap(text: &str) -> f64 {
    crate::value::js_nanbox_string(js_string_from_bytes(text.as_ptr(), text.len() as u32) as i64)
}

fn inline(text: &str) -> f64 {
    f64::from_bits(JSValue::short_string_unchecked(text.as_bytes()).bits())
}

fn parts(array: *const crate::array::ArrayHeader) -> Vec<String> {
    (0..crate::array::js_array_length(array))
        .map(|i| {
            let bits = crate::array::js_array_get_f64(array, i).to_bits();
            let part = (bits & crate::value::POINTER_MASK) as *const StringHeader;
            string_as_str(part).to_owned()
        })
        .collect()
}

fn fast(receiver: &str, separator: f64, limit: f64) -> Option<Vec<String>> {
    split_string_by_string(heap(receiver), separator, limit)
        .map(|value| parts(crate::value::js_nanbox_get_pointer(value) as *const _))
}

/// The oracle. For valid UTF-8 and a separator with no lone surrogate, a split
/// over UTF-16 units and one over UTF-8 bytes find the same occurrences.
/// (`js_string_split_n` is no oracle: with the engine linked, it reaches the
/// fast entry itself.)
fn reference(receiver: &str, separator: &str, limit: usize) -> Vec<String> {
    receiver
        .split(separator)
        .take(limit)
        .map(str::to_owned)
        .collect()
}

#[test]
fn accepted_inputs_match_the_general_split() {
    let many = (0..40)
        .map(|i| format!("p{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let cases: &[(&str, &str)] = &[
        ("a.b.c", "."),
        (".a..b.", "."),
        ("", "."),
        ("abc", ","),
        ("ab", "abc"),
        ("aaaa", "aa"),
        ("one, two, three", ", "),
        ("x--sep--y--sep--z", "--sep--"),
        ("café,naïve", ","),
        ("aébéc", "é"),
        ("a😀b😀c", "😀"),
        (&many, ","),
    ];
    for &(receiver, separator) in cases {
        let separator_value = if separator.len() <= crate::value::SHORT_STRING_MAX_LEN {
            inline(separator)
        } else {
            heap(separator)
        };
        for (limit_value, limit) in [(UNDEFINED, usize::MAX), (1.0, 1), (2.0, 2), (17.0, 17)] {
            assert_eq!(
                fast(receiver, separator_value, limit_value),
                Some(reference(receiver, separator, limit)),
                "{receiver:?}.split({separator:?}, {limit})"
            );
        }
        // A heap separator takes the same path as an inline one.
        assert_eq!(
            fast(receiver, heap(separator), UNDEFINED),
            fast(receiver, separator_value, UNDEFINED)
        );
    }
}

#[test]
fn more_parts_than_the_inline_buffer_holds_keep_their_order() {
    let many = (0..40).map(|i| i.to_string()).collect::<Vec<_>>().join(",");
    let expected: Vec<String> = (0..40).map(|i| i.to_string()).collect();
    assert_eq!(fast(&many, inline(","), UNDEFINED), Some(expected.clone()));
    assert_eq!(
        fast(&many, inline(","), 16.0),
        Some(expected[..16].to_vec())
    );
    assert_eq!(
        fast(&many, inline(","), 17.0),
        Some(expected[..17].to_vec())
    );
}

#[test]
fn a_numeric_limit_is_to_uint32() {
    let all = Some(vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]);
    assert_eq!(fast("a,b,c", inline(","), 0.0), Some(vec![]));
    assert_eq!(fast("a,b,c", inline(","), f64::NAN), Some(vec![]));
    assert_eq!(fast("a,b,c", inline(","), 4_294_967_296.0), Some(vec![]));
    assert_eq!(
        fast("a,b,c", inline(","), 4_294_967_297.0),
        Some(vec!["a".to_owned()])
    );
    assert_eq!(fast("a,b,c", inline(","), 1.9), Some(vec!["a".to_owned()]));
    assert_eq!(fast("a,b,c", inline(","), -1.0), all);
    assert_eq!(fast("a,b,c", inline(","), f64::INFINITY), Some(vec![]));
}

#[test]
fn inputs_it_cannot_answer_exactly_are_declined() {
    // An empty separator splits by UTF-16 code unit.
    assert_eq!(fast("abc", inline(""), UNDEFINED), None);
    // A lone-surrogate separator can match half of a pair in the receiver.
    let low_half = "\u{1F600}".encode_utf16().nth(1).unwrap();
    let low = crate::value::js_nanbox_string(crate::string::string_from_code_unit(low_half) as i64);
    assert_eq!(fast("\u{1F600}", low, UNDEFINED), None);
    // A limit whose coercion can run user code, or a separator that is not a
    // string at all.
    assert_eq!(fast("a,b", inline(","), inline("1")), None);
    assert_eq!(fast("a1b", 1.0, UNDEFINED), None);
    assert_eq!(fast("a,b", UNDEFINED, UNDEFINED), None);
    // A receiver that is not a heap string.
    assert_eq!(
        split_string_by_string(inline("a,b"), inline(","), UNDEFINED),
        None
    );
    assert_eq!(split_string_by_string(1.0, inline(","), UNDEFINED), None);
}
