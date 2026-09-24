use super::*;
/// perry#4898: a structurally-plausible pointer that `js_box_alloc`
/// never minted (here, a `&'static` read-only constant that is ≥0x1000,
/// untagged, and 8-byte aligned — exactly the shape of the leaked
/// `..._guard` string) must NOT be dereferenced by `js_box_set`. Before
/// the registry check this stored into read-only memory → SIGBUS.
#[test]
fn box_set_skips_unregistered_plausible_pointer() {
    // 8-byte aligned static — passes every structural check, is not a box.
    static RODATA: [u64; 2] = [0xDEAD_BEEF, 0xFEED_FACE];
    let fake = (&RODATA[0] as *const u64) as *mut Box;
    assert!(!is_registered_box_ptr(fake), "fake must not be registered");
    // Must be a silent no-op, not a write/crash.
    js_box_set(fake, 1.0);
    js_box_set_bits(
        fake,
        crate::value::JSValue::try_short_string(b"bad")
            .unwrap()
            .bits() as i64,
    );
    assert_eq!(RODATA[0], 0xDEAD_BEEF, "rodata must be untouched");
    // Reads from an unregistered pointer return `undefined` (perry#4926:
    // the read-before-initialization value of a boxed variable), never
    // deref. TAG_UNDEFINED is a NaN bit pattern, so this also preserves
    // the older "returns NaN" numeric behavior.
    assert_eq!(
        js_box_get_bits(fake) as u64,
        crate::value::TAG_UNDEFINED,
        "unregistered bits box read must yield undefined"
    );
    assert_eq!(
        js_box_get(fake).to_bits(),
        crate::value::TAG_UNDEFINED,
        "unregistered box read must yield undefined"
    );
}

/// A real `js_box_alloc` box still round-trips through set/get after the
/// registry gate (no false negatives on genuine boxes).
#[test]
fn box_set_get_roundtrips_for_real_box() {
    let b = js_box_alloc(3.5);
    assert!(is_registered_box_ptr(b));
    assert_eq!(js_box_get(b), 3.5);
    js_box_set(b, 42.0);
    assert_eq!(js_box_get(b), 42.0);
}

/// The bits ABI is the canonical boxed-local storage path for dynamic
/// JSValues. It must not turn Perry's NaN-boxed non-number values into a
/// numeric NaN payload.
#[test]
fn box_bits_roundtrips_non_number_tags_exactly() {
    let cases = [
        crate::value::JSValue::int32(-17).bits(),
        crate::value::JSValue::try_short_string(b"ok")
            .unwrap()
            .bits(),
        crate::value::TAG_UNDEFINED,
    ];

    for bits in cases {
        let b = js_box_alloc_bits(bits as i64);
        assert!(is_registered_box_ptr(b));
        assert_eq!(js_box_get_bits(b) as u64, bits);
        assert_eq!(js_box_get(b).to_bits(), bits);

        let replacement = crate::value::JSValue::try_short_string(b"next")
            .unwrap()
            .bits();
        js_box_set_bits(b, replacement as i64);
        assert_eq!(js_box_get_bits(b) as u64, replacement);
        assert_eq!(js_box_get(b).to_bits(), replacement);
    }
}

#[test]
fn trusted_box_access_matches_valid_public_access_and_tdz_suppression() {
    let initial = crate::value::JSValue::int32(17).bits();
    let replacement = crate::value::JSValue::try_short_string(b"next")
        .unwrap()
        .bits();
    let b = js_box_alloc_bits(initial as i64);

    assert_eq!(unsafe { js_box_get_bits_trusted(b) } as u64, initial);
    unsafe {
        js_box_set_bits_trusted_no_barrier(b, replacement as i64);
    }
    assert_eq!(js_box_get_bits(b) as u64, replacement);

    js_box_set_bits(b, crate::value::TAG_TDZ as i64);
    js_tdz_suppress_begin();
    let trusted_tdz = unsafe { js_box_get_bits_trusted(b) } as u64;
    let public_tdz = js_box_get_bits(b) as u64;
    js_tdz_suppress_end();
    assert_eq!(trusted_tdz, crate::value::TAG_UNDEFINED);
    assert_eq!(public_tdz, crate::value::TAG_UNDEFINED);
}

#[test]
fn primitive_control_boxes_round_trip_and_reject_foreign_pointers() {
    let i32_box = js_i32_box_alloc(7);
    assert!(is_registered_i32_box_ptr(i32_box));
    assert_eq!(js_i32_box_get(i32_box), 7);
    js_i32_box_set(i32_box, -3);
    assert_eq!(js_i32_box_get(i32_box), -3);

    let bool_box = js_bool_box_alloc(0);
    assert!(is_registered_bool_box_ptr(bool_box));
    assert_eq!(js_bool_box_get(bool_box), 0);
    js_bool_box_set(bool_box, 1);
    assert_eq!(js_bool_box_get(bool_box), 1);

    let ordinary_box = js_box_alloc(1.0);
    assert_eq!(js_i32_box_get(ordinary_box.cast::<I32Box>()), 0);
    js_i32_box_set(ordinary_box.cast::<I32Box>(), 99);
    assert_eq!(js_box_get(ordinary_box), 1.0);
}

/// Pending-await thunks carry a raw malloc-token pointer so the moving GC
/// cannot invalidate it. Reusing that token must not make an old thunk
/// name a new activation; the captured generation is the discriminator.
#[test]
fn recycled_activation_token_rejects_a_stale_generation() {
    let first = new_async_box_activation();
    let first_id = async_box_activation_id(first);
    assert_eq!(find_async_box_activation(first, first_id), first);
    finish_async_box_activation(first);
    assert!(find_async_box_activation(first, first_id).is_null());

    let second = new_async_box_activation();
    let second_id = async_box_activation_id(second);
    assert_eq!(second, first, "the test must exercise token recycling");
    assert_ne!(second_id, first_id);
    assert!(find_async_box_activation(second, first_id).is_null());
    assert_eq!(find_async_box_activation(second, second_id), second);
    finish_async_box_activation(second);
}
