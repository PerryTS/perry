//! #10873: resizable ArrayBuffer — storage model, view relength, and the
//! "costs nothing when unused" latch.

use super::*;

fn boxed(buf: *const BufferHeader) -> f64 {
    f64::from_bits(crate::value::JSValue::pointer(buf as *const u8).bits())
}

fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn unbox(value: f64) -> *mut BufferHeader {
    crate::value::JSValue::from_bits(value.to_bits()).as_pointer::<BufferHeader>()
        as *mut BufferHeader
}

fn resize(buf: *mut BufferHeader, len: u32) {
    resizable::array_buffer_resize(buf as usize, &[len as f64]);
}

#[test]
fn resize_never_moves_the_payload_and_reserves_max_once() {
    let buf = resizable::alloc_resizable_array_buffer(4, 4096);
    let data = buffer_data(buf);
    unsafe {
        assert_eq!((*buf).length, 4);
        assert_eq!((*buf).capacity, 4096, "capacity IS the reservation");
    }
    assert_eq!(resizable_max_byte_length(buf as usize), Some(4096));
    assert!(is_resizable_buffer(buf as usize));
    assert!(is_array_buffer(buf as usize));
    for len in [4096u32, 0, 17, 4096, 1] {
        resize(buf, len);
        unsafe {
            assert_eq!((*buf).length, len);
            assert_eq!((*buf).capacity, 4096);
        }
        assert_eq!(buffer_data(buf), data, "views alias this address raw");
    }
}

#[test]
fn a_grow_zero_fills_exactly_what_it_exposes() {
    let buf = resizable::alloc_resizable_array_buffer(8, 64);
    let data = buffer_data_mut(buf);
    unsafe {
        // Dirty the whole reservation behind the API's back: a regrow must not
        // let any of it through.
        std::ptr::write_bytes(data, 0xAB, 64);
    }
    resize(buf, 4);
    resize(buf, 32);
    let bytes = unsafe { std::slice::from_raw_parts(data, 32) };
    assert_eq!(&bytes[..4], &[0xAB; 4], "surviving prefix is untouched");
    assert!(
        bytes[4..].iter().all(|&b| b == 0),
        "exposed range is zeroed"
    );
}

#[test]
fn a_large_shrink_and_regrow_never_leaks_old_bytes() {
    // Large enough to take the decommit path (and, on Linux, the "skip the
    // clear, the OS zero-fills" path): dirty every byte, drop it all, regrow.
    const MAX: u32 = 4 * 1024 * 1024;
    let buf = resizable::alloc_resizable_array_buffer(0, MAX as i32);
    let data = buffer_data_mut(buf);
    for round in 0..3u8 {
        resize(buf, MAX);
        let bytes = unsafe { std::slice::from_raw_parts_mut(data, MAX as usize) };
        assert!(
            bytes.iter().all(|&b| b == 0),
            "round {round}: regrown bytes"
        );
        bytes.fill(0xC0 + round);
        // An unaligned, partial shrink: the edge pages are cleared by hand.
        resize(buf, 4097);
        resize(buf, MAX);
        let bytes = unsafe { std::slice::from_raw_parts_mut(data, MAX as usize) };
        assert!(bytes[..4097].iter().all(|&b| b == 0xC0 + round));
        assert!(bytes[4097..].iter().all(|&b| b == 0), "round {round}: tail");
        bytes.fill(0xEE);
        resize(buf, 0);
    }
    // A small shrink (below the decommit threshold) keeps the dirty boundary:
    // the regrow must clear by hand.
    resize(buf, 1024);
    unsafe { std::slice::from_raw_parts_mut(data, 1024) }.fill(0x55);
    resize(buf, 16);
    resize(buf, 1024);
    let bytes = unsafe { std::slice::from_raw_parts(data, 1024) };
    assert!(bytes[..16].iter().all(|&b| b == 0x55));
    assert!(bytes[16..].iter().all(|&b| b == 0));
}

#[test]
fn fixed_length_buffers_are_not_resizable() {
    let plain = js_array_buffer_new(8);
    assert!(!is_resizable_buffer(plain as usize));
    assert_eq!(resizable_max_byte_length(plain as usize), None);
    // An options bag with no usable `maxByteLength` is the fixed-length path.
    let via_options = js_array_buffer_new_with_options(8.0, undefined());
    assert!(!is_resizable_buffer(via_options as usize));
    unsafe {
        assert_eq!((*via_options).length, 8);
    }
}

#[test]
fn byte_views_track_go_out_of_bounds_and_come_back() {
    let buf = resizable::alloc_resizable_array_buffer(8, 32);
    let whole = js_uint8array_new(boxed(buf));
    let fixed = js_uint8array_view(boxed(buf), 4.0, 4.0);
    let tail = js_uint8array_view(boxed(buf), 6.0, undefined());
    let len = |v: *mut BufferHeader| unsafe { (*v).length };
    assert_eq!((len(whole), len(fixed), len(tail)), (8, 4, 2));
    assert!(view::is_length_tracking(whole as usize));
    assert!(!view::is_length_tracking(fixed as usize));
    assert!(view::is_length_tracking(tail as usize));

    resize(buf, 6);
    assert_eq!((len(whole), len(fixed), len(tail)), (6, 0, 0));
    assert!(view::is_out_of_bounds_view(fixed as usize));
    assert!(
        !view::is_out_of_bounds_view(tail as usize),
        "offset == byteLength is an empty in-bounds view"
    );
    assert_eq!(view::byte_offset_of(fixed as usize), 0);
    assert_eq!(view::byte_offset_of(tail as usize), 6);

    resize(buf, 5);
    assert!(view::is_out_of_bounds_view(tail as usize));
    assert_eq!(view::byte_offset_of(tail as usize), 0);

    resize(buf, 32);
    assert_eq!((len(whole), len(fixed), len(tail)), (32, 4, 26));
    assert!(!view::is_out_of_bounds_view(fixed as usize));
    assert_eq!(view::byte_offset_of(fixed as usize), 4);
    assert_eq!(view::byte_offset_of(tail as usize), 6);
}

#[test]
fn a_view_over_a_fixed_buffer_is_never_marked_tracking() {
    // Arm the latch first so the probes below are really answered by the
    // tables, not by the idle fast path.
    let _armed = resizable::alloc_resizable_array_buffer(0, 8);
    let plain = js_array_buffer_new(16);
    let view = js_uint8array_new(boxed(plain));
    assert!(!view::is_length_tracking(view as usize));
    assert!(!view::is_out_of_bounds_view(view as usize));
    // Relength on a non-resizable backing is never requested; if it were it
    // would treat the view as fixed-length and keep it.
    view::relength_views_of_resized_backing(plain as usize, 16);
    assert_eq!(unsafe { (*view).length }, 16);
}

#[test]
fn typed_array_views_floor_track_and_survive_growth_past_their_birth_length() {
    use crate::typedarray::{js_typed_array_get, js_typed_array_length, js_typed_array_set};
    let buf = resizable::alloc_resizable_array_buffer(8, 64);
    let tracking = crate::typedarray_view::js_typed_array_view(
        crate::typedarray::KIND_INT32 as i32,
        boxed(buf),
        undefined(),
        undefined(),
    );
    let fixed = crate::typedarray_view::js_typed_array_view(
        crate::typedarray::KIND_FLOAT64 as i32,
        boxed(buf),
        0.0,
        1.0,
    );
    assert_eq!(js_typed_array_length(tracking), 2);
    assert_eq!(js_typed_array_length(fixed), 1);

    // 18 bytes = 4 whole int32 + 2 stray bytes: a tracking view floors.
    resize(buf, 18);
    assert_eq!(js_typed_array_length(tracking), 4);
    // Element 3 lies beyond the 2 elements the header was allocated with; the
    // write must land in the BACKING (which reserves max), not the header.
    js_typed_array_set(tracking, 3, 123456.0);
    assert_eq!(js_typed_array_get(tracking, 3), 123456.0);
    let raw = unsafe { std::slice::from_raw_parts(buffer_data(buf).add(12), 4) };
    assert_eq!(i32::from_le_bytes(raw.try_into().unwrap()), 123456);

    resize(buf, 4);
    assert_eq!(js_typed_array_length(tracking), 1);
    assert_eq!(js_typed_array_length(fixed), 0, "8-byte view over 4 bytes");
    assert_eq!(crate::typedarray_view::js_typed_array_byte_offset(fixed), 0);

    resize(buf, 16);
    assert_eq!(js_typed_array_length(tracking), 4);
    assert_eq!(js_typed_array_length(fixed), 1);
    assert_eq!(
        js_typed_array_get(tracking, 3),
        0.0,
        "regrown bytes are zero"
    );
}

#[test]
fn data_views_track_and_report_out_of_bounds() {
    let buf = resizable::alloc_resizable_array_buffer(8, 16);
    let tracking = unbox(js_data_view_new(boxed(buf), undefined(), undefined()));
    let fixed = unbox(js_data_view_new(boxed(buf), 4.0, 4.0));
    resize(buf, 16);
    unsafe {
        assert_eq!(((*tracking).length, (*fixed).length), (16, 4));
    }
    resize(buf, 6);
    unsafe {
        assert_eq!(((*tracking).length, (*fixed).length), (6, 0));
    }
    assert!(is_out_of_bounds_data_view(fixed as usize));
    assert!(!is_out_of_bounds_data_view(tracking as usize));
    resize(buf, 8);
    assert!(!is_out_of_bounds_data_view(fixed as usize));
    unsafe {
        assert_eq!((*fixed).length, 4);
    }
}

#[test]
fn transfer_preserves_resizability_and_to_fixed_length_drops_it() {
    let buf = resizable::alloc_resizable_array_buffer(4, 8);
    unsafe {
        std::ptr::copy_nonoverlapping([9u8, 8, 7, 6].as_ptr(), buffer_data_mut(buf), 4);
    }
    let moved = unbox(array_buffer_transfer(buf as usize, &[], true));
    assert!(is_detached_buffer(buf as usize));
    assert_eq!(resizable_max_byte_length(moved as usize), Some(8));
    unsafe {
        assert_eq!((*moved).length, 4);
        assert_eq!(
            std::slice::from_raw_parts(buffer_data(moved), 4),
            &[9, 8, 7, 6]
        );
    }
    let pinned = unbox(array_buffer_transfer(moved as usize, &[], false));
    assert!(!is_resizable_buffer(pinned as usize));
    unsafe {
        assert_eq!((*pinned).length, 4);
    }
}

#[test]
fn a_dead_buffers_resizable_mark_is_pruned() {
    let buf = resizable::alloc_resizable_array_buffer(1, 2);
    let before = header::test_resizable_registry_len();
    assert!(is_resizable_buffer(buf as usize));
    header::finalize_collected_dead_buffer(buf as usize);
    assert!(
        !is_resizable_buffer(buf as usize),
        "a recycled address must not inherit resizability (#6080 ABA class)"
    );
    assert_eq!(header::test_resizable_registry_len(), before - 1);
}

#[test]
fn view_length_after_resize_matches_the_spec_table() {
    let f = resizable::view_length_after_resize;
    // length-tracking
    assert_eq!(f(16, 0, 1, true, 0), Some(16));
    assert_eq!(
        f(16, 16, 1, true, 0),
        Some(0),
        "offset == length is in bounds"
    );
    assert_eq!(f(16, 17, 1, true, 0), None);
    assert_eq!(f(18, 0, 4, true, 0), Some(4), "floors to whole elements");
    assert_eq!(f(18, 4, 8, true, 0), Some(1));
    // fixed-length
    assert_eq!(f(16, 4, 4, false, 3), Some(3));
    assert_eq!(f(15, 4, 4, false, 3), None, "one byte short");
    assert_eq!(f(0, 0, 1, false, 0), Some(0));
    // no u32 overflow at the top of the range
    assert_eq!(f(u32::MAX, u32::MAX - 8, 8, false, 2), None);
}
