//! The probe answers an accessor from the receiver's shape (lane D): the
//! binding of a `return <captured binding>` getter, `undefined` for a pair
//! without a getter, and a decline (the call) for every other getter; for the
//! key list and the overflow table alike.
use super::*;

unsafe fn atom(name: &str) -> (u64, u64) {
    let hash = crate::object::key_bytes_hash(name.as_ptr(), name.len());
    let key = crate::string::js_string_pool_atom(name.as_ptr(), name.len() as u32, hash, 0);
    (crate::JSValue::string_ptr(key).bits(), hash)
}

extern "C" fn getter_body(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// An info for a getter body; with `cell`, it carries `FN_RETURNS_CAPTURE`
/// for capture 0 (`cell` as the info encodes it).
fn getter_info(cell: Option<u64>) -> *const crate::closure::JsFunctionInfo {
    let mut info = crate::closure::JsFunctionInfo::of(
        getter_body as crate::codegen_abi::JsBody0<crate::closure::ClosureHeader>,
    );
    if let Some(cell) = cell {
        info.flags |= crate::codegen_abi::FN_RETURNS_CAPTURE;
        info.trusted_captures = 0;
        info.trusted_boxed_mask = cell;
    }
    Box::leak(Box::new(info))
}

unsafe fn getter(cell: Option<u64>, capture: u64) -> f64 {
    let closure = crate::closure::js_closure_alloc(getter_info(cell), 1);
    crate::closure::js_closure_set_capture_bits(closure, 0, capture);
    f64::from_bits(crate::value::js_nanbox_pointer(closure as i64).to_bits())
}

unsafe fn define_getter(obj: *mut ObjectHeader, key: u64, get: f64) {
    crate::object::object_ops::js_object_define_get_accessor(
        f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits()),
        f64::from_bits(key),
        get,
    );
}

fn on_big_stack(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn a_returned_capture_getter_reads_as_its_binding() {
    on_big_stack(|| unsafe {
        crate::gc::initialize_runtime_fixture();
        let _no_move = crate::gc::GcSuppressScope::new();
        let dir = test_dir();
        let object = crate::object::js_object_alloc(0, 2);
        crate::object::mark_object_plain_ordinary(object);
        let biased = (object as usize).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR) as i64;

        // A boxed binding: the value lives in the box (offset 0, encoded 1).
        let cell = crate::r#box::js_box_alloc(5.0);
        let (boxed, bh) = atom("lane_d_boxed");
        define_getter(
            object,
            boxed,
            getter(
                Some(1),
                crate::value::js_nanbox_pointer(cell as i64).to_bits(),
            ),
        );
        assert_eq!(perry_probe_get(dir, biased, boxed, bh), 5.0);
        (*cell).value = 6.0f64.to_bits();
        assert_eq!(
            perry_probe_get(dir, biased, boxed, bh),
            6.0,
            "the binding is live"
        );
        (*cell).value = crate::value::TAG_TDZ;
        assert_eq!(
            perry_probe_get(dir, biased, boxed, bh).to_bits(),
            TAG_HOLE,
            "a binding in its dead zone declines: the call throws"
        );

        // An unboxed capture: the capture word is the value.
        let (direct, dh) = atom("lane_d_direct");
        define_getter(object, direct, getter(Some(0), 11.0f64.to_bits()));
        assert_eq!(perry_probe_get(dir, biased, direct, dh), 11.0);

        // A getter without the fact is called by the decline edge.
        let (plain, ph) = atom("lane_d_plain");
        define_getter(object, plain, getter(None, 12.0f64.to_bits()));
        assert_eq!(perry_probe_get(dir, biased, plain, ph).to_bits(), TAG_HOLE);
        assert!(probe_accessor_word(object, plain, ph).is_some_and(|acc| acc.get != 0));

        // A pair without a getter reads `undefined`.
        let (setonly, sh) = atom("lane_d_setonly");
        define_getter(object, setonly, f64::from_bits(crate::value::TAG_UNDEFINED));
        assert_eq!(
            perry_probe_get(dir, biased, setonly, sh).to_bits(),
            crate::value::TAG_UNDEFINED
        );
    });
}

#[test]
fn an_overflow_accessor_is_answered_and_walked() {
    on_big_stack(|| unsafe {
        crate::gc::initialize_runtime_fixture();
        let _no_move = crate::gc::GcSuppressScope::new();
        let dir = test_dir();
        let object = crate::object::js_object_alloc(0, 2);
        crate::object::mark_object_plain_ordinary(object);
        let (fixed, _) = atom("lane_d_fixed");
        crate::object::js_object_set_field_by_name(
            object,
            (fixed & POINTER_MASK) as *const crate::StringHeader,
            1.0,
        );
        crate::object::overflow::ensure(object);
        let biased = (object as usize).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR) as i64;

        let (bound, bh) = atom("lane_d_overflow_bound");
        define_getter(object, bound, getter(Some(0), 21.0f64.to_bits()));
        let (called, ch) = atom("lane_d_overflow_called");
        define_getter(object, called, getter(None, 22.0f64.to_bits()));
        let table = crate::object::overflow::table(object);
        assert!(!table.is_null(), "the accessors live in the overflow table");
        assert!(crate::object::overflow::find(table, bound).is_some());

        assert_eq!(perry_probe_get(dir, biased, bound, bh), 21.0);
        assert_eq!(perry_probe_get(dir, biased, called, ch).to_bits(), TAG_HOLE);
        // The decline edge's walk finds the overflow pair (it used to give up
        // and take the by-name read).
        let acc = probe_accessor_word(object, called, ch).expect("overflow accessor walked");
        assert!(acc.get != 0);
        // The slow read decodes the getter half of the walked slot word only:
        // the same getter as the whole pair, nothing of the setter half.
        let (paired, ph) = atom("lane_d_overflow_paired");
        crate::object::js_object_define_accessor(
            f64::from_bits(crate::value::js_nanbox_pointer(object as i64).to_bits()),
            f64::from_bits(paired),
            getter(None, 23.0f64.to_bits()),
            getter(None, 24.0f64.to_bits()),
        );
        let slot = probe_accessor_slot(object, paired, ph).expect("overflow slot walked");
        let acc = crate::object::accessor_pair::pair_of_value(slot).expect("a pair");
        assert!(acc.get != 0 && acc.set != 0, "the pair holds both halves");
        let half = crate::object::accessor_pair::getter_half_of_value(slot);
        assert_eq!(half.get, acc.get);
        assert_eq!(
            (half.raw_get, half.static_get),
            (acc.raw_get, acc.static_get)
        );
        assert_eq!((half.set, half.raw_set, half.static_set), (0, 0, 0));
        // A word that is no pair reads as the accessor with neither half.
        assert_eq!(
            crate::object::accessor_pair::getter_half_of_value(1.5f64.to_bits()),
            crate::object::accessor_pair::Accessor::default()
        );
    });
}

/// The key index names a spilled accessor's pair too (`PAIR_SPILLED`): the
/// probe and the slow walk (`own_entry` -> `accessor_in`) read it with one
/// spill load, the same word `accessor_slot_bits` finds by the attributes.
#[test]
fn the_index_names_a_spilled_accessor_pair() {
    on_big_stack(|| unsafe {
        crate::gc::initialize_runtime_fixture();
        let _no_move = crate::gc::GcSuppressScope::new();
        let dir = test_dir();
        let object = crate::object::js_object_alloc(0, 2);
        crate::object::mark_object_plain_ordinary(object);
        let biased = (object as usize).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR) as i64;
        let names = [
            "nxpa2e_a", "nxpa2e_b", "nxpa2e_c", "nxpa2e_d", "nxpa2e_e", "nxpa2e_f",
        ];
        let keys: Vec<(u64, u64)> = names.iter().map(|n| atom(n)).collect();
        for (k, (key, _)) in keys.iter().enumerate() {
            // The last one returns its capture: the probe answers it.
            let cell = (k + 1 == keys.len()).then_some(0);
            define_getter(object, *key, getter(cell, (k as f64 + 20.0).to_bits()));
        }
        let r = record_of(dir, LayoutWord((*object).parent_class_id));
        assert!(!r.is_null() && (*r).present());
        let mut spilled = 0;
        for (key, hash) in &keys {
            let (pos, pair) = own_entry(r, *key, *hash).expect("own accessor");
            assert_ne!(
                pair, NO_PAIR,
                "the index names every public accessor's slot"
            );
            let bits = accessor_slot_bits(object, r, pos).expect("the pair by its attributes");
            assert_eq!(index_pair_bits(object, pair), Some(bits));
            assert_eq!(accessor_in(object, r, pos, pair), Some(bits));
            if pair & PAIR_SPILLED != 0 {
                assert!(pair as u32 >= (*r).live_inline_slot_count);
                spilled += 1;
            } else {
                assert!((pair as u32) < (*r).live_inline_slot_count);
            }
        }
        assert!(spilled > 0, "the fixture spills accessor pairs");
        let (last, lh) = *keys.last().unwrap();
        assert_eq!(
            perry_probe_get(dir, biased, last, lh),
            25.0,
            "a spilled capture getter answers"
        );
        let (first, fh) = keys[keys.len() - 2];
        assert_eq!(
            perry_probe_get(dir, biased, first, fh).to_bits(),
            TAG_HOLE,
            "the call declines"
        );
    });
}
