//! A sparse Array (`length > capacity`): every reader that copies, moves or
//! searches by raw slots must stop at the store. Each test would read (and
//! `splice` would move) megabytes past a 16-slot store before the fix.
use super::*;

const N: u32 = 3_000_000;
const UNDEFINED: u64 = crate::value::TAG_UNDEFINED;
const HOLE: u64 = crate::value::TAG_HOLE;

/// `const a = [1, 2, 3]; a[N] = 9`: three dense elements, one far element.
fn sparse() -> *mut ArrayHeader {
    let mut arr = js_array_alloc(0);
    for value in [1.0, 2.0, 3.0] {
        arr = js_array_push_f64(arr, value);
    }
    arr = js_array_set_f64_extend(arr, N, 9.0);
    unsafe {
        assert_eq!((*arr).length, N + 1);
        assert!(
            (*arr).capacity < (*arr).length,
            "the premise: a far store leaves the Array sparse"
        );
    }
    arr
}

/// How many slots of a dense result hold a Number, and their sum.
unsafe fn numbers(arr: *const ArrayHeader) -> (usize, f64) {
    assert!((*arr).length <= (*arr).capacity, "the result must be dense");
    let slots = array_elements_ptr(arr) as *const u64;
    let (mut count, mut sum) = (0, 0.0);
    for i in 0..(*arr).length as usize {
        let bits = *slots.add(i);
        if bits != UNDEFINED && bits != HOLE {
            count += 1;
            sum += f64::from_bits(bits);
        }
    }
    (count, sum)
}

#[test]
fn clone_copies_the_present_elements_only() {
    let _serialized = crate::array::test_serialize();
    let copy = js_array_clone(sparse());
    unsafe {
        assert_eq!((*copy).length, N + 1);
        assert_eq!(numbers(copy), (4, 15.0));
    }
    assert_eq!(js_array_get_f64(copy, N), 9.0);
}

#[test]
fn to_reversed_with_and_to_sorted_read_by_get() {
    let _serialized = crate::array::test_serialize();
    let reversed = js_array_to_reversed(sparse());
    unsafe {
        assert_eq!((*reversed).length, N + 1);
        assert_eq!(numbers(reversed), (4, 15.0));
    }
    assert_eq!(js_array_get_f64(reversed, 0), 9.0);
    assert_eq!(js_array_get_f64(reversed, N), 1.0);

    let replaced = js_array_with(sparse(), 1.0, 20.0);
    unsafe {
        assert_eq!((*replaced).length, N + 1);
        assert_eq!(numbers(replaced), (4, 33.0));
    }

    let sorted = js_array_to_sorted_default(sparse());
    unsafe {
        assert_eq!((*sorted).length, N + 1);
        assert_eq!(numbers(sorted), (4, 15.0));
    }
    assert_eq!(js_array_get_f64(sorted, 3), 9.0);
}

#[test]
fn to_spliced_keeps_the_far_element() {
    let _serialized = crate::array::test_serialize();
    let items = [7.0, 8.0];
    let spliced = js_array_to_spliced(sparse(), 1.0, 1.0, items.as_ptr(), 2);
    unsafe {
        assert_eq!((*spliced).length, N + 2);
        assert_eq!(numbers(spliced), (5, 28.0));
    }
    assert_eq!(js_array_get_f64(spliced, N + 1), 9.0);
}

#[test]
fn at_and_the_searches_stop_at_the_store() {
    let _serialized = crate::array::test_serialize();
    let arr = sparse();
    assert_eq!(js_array_at(arr, -1.0), 9.0);
    assert_eq!(js_array_at(arr, -2.0).to_bits(), UNDEFINED);
    assert_eq!(js_array_at(arr, 1000.0).to_bits(), UNDEFINED);
    assert_eq!(js_array_indexOf_f64(arr, 9.0), N as i32);
    assert_eq!(js_array_indexOf_f64(arr, 12345.5), -1);
    assert_eq!(js_array_includes_jsvalue(arr, 9.0, 0.0, 0), 1);
    assert_eq!(js_array_includes_jsvalue(arr, 12345.5, 0.0, 0), 0);
    assert_eq!(
        js_array_includes_jsvalue(arr, f64::from_bits(UNDEFINED), 0.0, 0),
        1
    );
}

#[test]
fn splice_moves_the_far_element_and_nothing_else() {
    let _serialized = crate::array::test_serialize();
    let arr = sparse();
    let mut live = arr;
    let removed = js_array_splice(arr, 0, 2, std::ptr::null(), 0, &mut live);
    unsafe {
        assert_eq!((*removed).length, 2);
        assert_eq!(numbers(removed), (2, 3.0));
        assert_eq!((*live).length, N - 1);
        assert!((*live).length <= (*live).capacity);
        assert_eq!(numbers(live), (2, 12.0));
    }
    assert_eq!(js_array_get_f64(live, 0), 3.0);
    assert_eq!(js_array_get_f64(live, N - 2), 9.0);
}

#[test]
fn a_growth_over_a_far_element_takes_it_into_its_slot() {
    let _serialized = crate::array::test_serialize();
    let arr = sparse();
    // `push` grows the store over index N.
    let grown = js_array_push_f64(arr, 4.0);
    unsafe {
        assert_eq!((*grown).length, N + 2);
    }
    assert_eq!(js_array_get_f64(grown, N), 9.0);
    assert_eq!(js_array_get_f64(grown, N + 1), 4.0);
    assert_eq!(js_array_get_f64(grown, 5).to_bits(), UNDEFINED);
}

#[test]
fn flat_keeps_the_far_element() {
    let _serialized = crate::array::test_serialize();
    let flat = js_array_flat(sparse());
    unsafe {
        assert_eq!((*flat).length, 4);
        assert_eq!(numbers(flat), (4, 15.0));
    }
}

#[test]
fn own_element_walk_stops_at_the_store_and_visits_the_far_element() {
    let _serialized = crate::array::test_serialize();
    let arr = sparse();
    let mut seen = Vec::new();
    unsafe {
        crate::array::array_for_each_own_element(arr, |index, value| {
            seen.push((index, value));
            true
        });
    }
    assert_eq!(seen, [(0, 1.0), (1, 2.0), (2, 3.0), (N, 9.0)]);
    // A visit that answers `false` stops the walk.
    let mut first = Vec::new();
    unsafe {
        crate::array::array_for_each_own_element(arr, |index, _| {
            first.push(index);
            false
        });
    }
    assert_eq!(first, [0]);
}
