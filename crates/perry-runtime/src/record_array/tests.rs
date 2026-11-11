use super::*;
use crate::value::{TAG_FALSE, TAG_NULL};

const UNDEFINED: u64 = TAG_UNDEFINED;
const TRUE: u64 = TAG_TRUE;

/// Record `i` of `words` words: doubles (with -0.0 and a NaN) and every
/// immediate, by position.
fn record(i: u32, words: usize) -> Vec<u64> {
    (0..words)
        .map(|w| match (i as usize + w) % 8 {
            0 => (i as f64 * 1.5 + w as f64).to_bits(),
            1 => TAG_UNDEFINED,
            2 => TAG_TRUE,
            3 => TAG_FALSE,
            4 => TAG_NULL,
            5 => (-0.0f64).to_bits(),
            6 => f64::NAN.to_bits(),
            _ => (-(i as f64) - 0.25).to_bits(),
        })
        .collect()
}

fn get(arr: f64, index: f64, words: usize) -> Option<Vec<u64>> {
    let mut out = vec![0xDEAD_BEEF_u64; words];
    let found = js_record_array_get(arr, index, out.as_mut_ptr()).to_bits();
    if found == UNDEFINED {
        assert!(out.iter().all(|&w| w == 0xDEAD_BEEF), "dst was written");
        return None;
    }
    assert_eq!(found, TRUE);
    Some(out)
}

fn pop(arr: f64, words: usize) -> Option<Vec<u64>> {
    let mut out = vec![0xDEAD_BEEF_u64; words];
    let found = js_record_array_pop(arr, out.as_mut_ptr()).to_bits();
    if found == UNDEFINED {
        assert!(out.iter().all(|&w| w == 0xDEAD_BEEF), "dst was written");
        return None;
    }
    assert_eq!(found, TRUE);
    Some(out)
}

fn round_trip(words: usize) {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let arr = js_record_array_new(words as u32, 77);
    let header = header_of(arr);
    unsafe {
        assert_eq!((*header).words, words as u32);
        assert_eq!((*header).shape, 77);
        assert_eq!((*header).capacity, 0);
        assert!((*header).elems.is_null());
    }
    assert_eq!(
        arr.to_bits() & crate::value::TAG_MASK,
        crate::value::POINTER_TAG
    );
    assert_eq!(js_record_array_length(arr), 0.0);
    assert_eq!(pop(arr, words), None);
    assert_eq!(get(arr, 0.0, words), None);

    const COUNT: u32 = 37;
    for i in 0..COUNT {
        let rec = record(i, words);
        assert_eq!(js_record_array_push(arr, rec.as_ptr()), (i + 1) as f64);
        assert_eq!(js_record_array_length(arr), (i + 1) as f64);
    }
    unsafe {
        assert_eq!((*header).capacity, 64, "4, 8, 16, 32, 64");
    }
    for i in 0..COUNT {
        assert_eq!(get(arr, i as f64, words), Some(record(i, words)), "{i}");
    }
    // -0 is index 0, as in `a[-0]`.
    assert_eq!(get(arr, -0.0, words), Some(record(0, words)));
    for index in [
        COUNT as f64,
        -1.0,
        0.5,
        1.000_000_1,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        4_294_967_296.0,
        1e300,
    ] {
        assert_eq!(get(arr, index, words), None, "{index}");
    }

    for i in (20..COUNT).rev() {
        assert_eq!(pop(arr, words), Some(record(i, words)), "{i}");
        assert_eq!(js_record_array_length(arr), i as f64);
    }
    assert_eq!(get(arr, 20.0, words), None);
    // A push after a pop reuses the slot.
    let rec = record(1000, words);
    assert_eq!(js_record_array_push(arr, rec.as_ptr()), 21.0);
    assert_eq!(get(arr, 20.0, words), Some(rec));

    let buffer = unsafe { (*header).elems };
    js_record_array_clear(arr);
    assert_eq!(js_record_array_length(arr), 0.0);
    assert_eq!(pop(arr, words), None);
    assert_eq!(get(arr, 0.0, words), None);
    unsafe {
        assert_eq!((*header).elems, buffer, "clear keeps the buffer");
        assert_eq!((*header).capacity, 64);
    }
    let rec = record(5, words);
    assert_eq!(js_record_array_push(arr, rec.as_ptr()), 1.0);
    assert_eq!(get(arr, 0.0, words), Some(rec));
}

#[test]
fn record_array_round_trips_one_word_records() {
    round_trip(1);
}

#[test]
fn record_array_round_trips_two_word_records() {
    round_trip(2);
}

#[test]
fn record_array_round_trips_fourteen_word_records() {
    round_trip(14);
}

#[test]
fn record_array_of_no_words_counts_without_a_buffer() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let arr = js_record_array_new(0, 1);
    for i in 0..9 {
        assert_eq!(js_record_array_push(arr, std::ptr::null()), (i + 1) as f64);
    }
    assert!(unsafe { (*header_of(arr)).elems.is_null() });
    assert_eq!(
        js_record_array_get(arr, 8.0, std::ptr::null_mut()).to_bits(),
        TRUE
    );
    assert_eq!(
        js_record_array_get(arr, 9.0, std::ptr::null_mut()).to_bits(),
        UNDEFINED
    );
    assert_eq!(
        js_record_array_pop(arr, std::ptr::null_mut()).to_bits(),
        TRUE
    );
    assert_eq!(js_record_array_length(arr), 8.0);
}

#[test]
fn record_array_capacity_is_bounded_by_the_buffer_size_and_the_shape_range() {
    // One-word records: the count is what payload +4 can hold.
    assert_eq!(
        max_capacity(1).min(crate::object::shape_rule3::MAX_PLUS_FOUR_WORD),
        max_capacity(1)
    );
    for words in [1u32, 2, 14, 1000, u32::MAX] {
        let max = max_capacity(words) as u64;
        assert!(
            max * words as u64 * 8 + crate::gc::GC_HEADER_SIZE as u64 + 8 <= u32::MAX as u64,
            "{words}"
        );
        assert!(max <= crate::object::shape_rule3::MAX_PLUS_FOUR_WORD as u64);
    }
    assert_eq!(grown_capacity_or_throw(0, 14), 4);
    assert_eq!(grown_capacity_or_throw(4, 14), 8);
    let max = max_capacity(14);
    assert_eq!(grown_capacity_or_throw(max - 1, 14), max);
    assert_eq!(grown_capacity_or_throw(max / 2 + 1, 14), max);
}
