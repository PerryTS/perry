//! DEC 243(3): a symbol store on an ordinary receiver is the probe store.
use super::*;

unsafe fn plain(field: f64) -> *mut ObjectHeader {
    let object = crate::object::js_object_alloc(0, 1);
    crate::object::mark_object_plain_ordinary(object);
    let name = "symbol_store_x";
    let hash = crate::object::key_bytes_hash(name.as_ptr(), name.len());
    let key = crate::string::js_string_pool_atom(name.as_ptr(), name.len() as u32, hash, 0);
    crate::object::js_object_set_field_by_name(object, key, field);
    object
}

#[test]
fn a_symbol_store_on_an_ordinary_receiver_takes_the_probe_store() {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(|| unsafe {
            crate::gc::initialize_runtime_fixture();
            let _no_move = crate::gc::GcSuppressScope::new();
            let a = plain(1.0);
            let b = plain(2.0);
            let a_bits = crate::value::POINTER_TAG | a as u64;
            let b_bits = crate::value::POINTER_TAG | b as u64;
            let symbol = crate::symbol::js_symbol_new_empty();
            let get =
                |o: u64| crate::symbol::js_object_get_symbol_property(f64::from_bits(o), symbol);
            // The append: the probe proves the key absent and the add admitted,
            // and the canonical key-add transition takes it (no [[Set]], so no
            // HolderEdit, no owner or entry lookup).
            assert!(
                symbol_store(a_bits, symbol.to_bits(), 5.0),
                "append must take the probe store"
            );
            assert_eq!(get(a_bits), 5.0);
            // A second receiver of the same layout meets the same transition.
            assert!(
                symbol_store(b_bits, symbol.to_bits(), 6.0),
                "append must take the probe store"
            );
            assert_eq!(get(b_bits), 6.0);
            assert_eq!(
                (*a).parent_class_id,
                (*b).parent_class_id,
                "the first symbol store on a shape is one canonical transition"
            );
            // The existing key: an own plain entry takes the value in place.
            let word = (*a).parent_class_id;
            assert!(
                symbol_store(a_bits, symbol.to_bits(), 7.0),
                "own store must take the probe store"
            );
            assert_eq!(get(a_bits), 7.0);
            assert_eq!((*a).parent_class_id, word, "an own store keeps the layout");
            // A frozen receiver declines, having changed nothing: [[Set]] answers.
            crate::object::js_object_freeze(f64::from_bits(b_bits));
            assert!(!symbol_store(b_bits, symbol.to_bits(), 8.0));
            assert_eq!(get(b_bits), 6.0);
        })
        .unwrap()
        .join()
        .unwrap();
}

/// The same rule seen from the two runtime entries (`obj[sym] = v` through
/// `js_object_set_symbol_property`, and PutValue): the append is the canonical
/// key-add transition, so the key joins the receiver key list and two
/// receivers of one layout meet one successor layout, one publish each.
#[test]
fn a_symbol_append_through_either_entry_is_one_canonical_transition() {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(|| unsafe {
            crate::gc::initialize_runtime_fixture();
            let _no_move = crate::gc::GcSuppressScope::new();
            let a = plain(1.0);
            let b = plain(2.0);
            let a_f = f64::from_bits(crate::value::POINTER_TAG | a as u64);
            let b_f = f64::from_bits(crate::value::POINTER_TAG | b as u64);
            let before = (*a).parent_class_id;
            // As a one-key literal is born: one live slot, room inline for
            // the next, so the append also raises the live bound.
            assert_eq!(crate::object::object_live_slot_count(a), 1);
            assert!(crate::object::object_inline_capacity(a) > 1);
            assert_eq!(before, (*b).parent_class_id);
            let symbol = crate::symbol::js_symbol_new_empty();
            crate::symbol::js_object_set_symbol_property(a_f, symbol, 5.0);
            crate::object::property_ops::js_put_value_set(b_f, symbol, 6.0, b_f, 1);
            assert_eq!(
                crate::symbol::js_object_get_symbol_property(a_f, symbol),
                5.0
            );
            assert_eq!(
                crate::symbol::js_object_get_symbol_property(b_f, symbol),
                6.0
            );
            assert_eq!(
                crate::object::object_keys(a).count(),
                2,
                "the symbol joins the key list"
            );
            assert_eq!(
                crate::object::object_keys(b).count(),
                2,
                "the symbol joins the key list"
            );
            assert_ne!(
                (*a).parent_class_id,
                before,
                "the append is a layout transition"
            );
            assert_eq!(
                (*a).parent_class_id,
                (*b).parent_class_id,
                "both entries meet one canonical successor"
            );
            // One transition per append: the successor list holds the one
            // descriptor both receivers carry, not an intermediate minted with
            // the predecessor live bound.
            let node = crate::object::object_keys(a).node();
            let family = crate::object::canonical_keys::with_list_shapes(node, |l| l.family.len());
            assert_eq!(family, Some(1), "one publish per symbol append");
        })
        .unwrap()
        .join()
        .unwrap();
}
