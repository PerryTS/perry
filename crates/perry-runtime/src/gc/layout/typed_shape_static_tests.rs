//! Typed class layouts under link-time ShapeIds (design step 4): the driver's
//! static id carries the masks, is adopted in whichever order the defining
//! and an importing module initialize, and a refusal falls back to a fresh id
//! that still describes the layout. No registry is consulted.
use super::*;
use crate::object::shapes::{is_shape_id, is_static_shape_id, SHAPE_ID_BASE};

const RAW: [u64; 1] = [0b10];
const POINTERS: [u64; 1] = [0b01];

fn keys_for(class_id: u32, packed: &[u8]) -> u64 {
    crate::object::js_build_class_keys_array(class_id, 2, packed.as_ptr(), packed.len() as u32)
        as usize as u64
}

fn typed(class_id: u32, keys: u64, raw: &[u64], pointers: &[u64], requested: u32) -> u32 {
    js_gc_typed_shape_id_for_keys(
        class_id,
        keys,
        2,
        raw.as_ptr(),
        raw.len() as u32,
        pointers.as_ptr(),
        pointers.len() as u32,
        requested,
    )
}

fn hot(shape_id: u32) -> Option<Option<TypedLayoutDescriptor>> {
    hot_shape_layouts().borrow().get(&shape_id).cloned()
}

fn descriptor(raw: &[u64], pointers: &[u64]) -> TypedLayoutDescriptor {
    TypedLayoutDescriptor {
        slot_count: 2,
        raw_f64_mask: LayoutSlotMask::from_words(raw),
        pointer_mask: LayoutSlotMask::from_words(pointers),
    }
}

/// The defining module initializes first: its typed install adopts the static
/// id and installs the descriptor; an importer's structural mint of the same
/// facts, handed the same id by the driver, resolves to it.
#[test]
fn the_definer_adopts_the_static_id_and_an_importer_resolves_to_it() {
    let class_id = 0x0B1_2001;
    let s = SHAPE_ID_BASE + 0x5101;
    let keys = keys_for(class_id, b"lt4t_next\0lt4t_value\0");
    assert_eq!(typed(class_id, keys, &RAW, &POINTERS, s), s);
    assert!(hot(s) == Some(Some(descriptor(&RAW, &POINTERS))));
    let importer = crate::object::static_shapes::js_object_shape_id_for_class_keys_static(
        keys, 2, 2, class_id, s,
    );
    assert_eq!(
        importer, s,
        "the importer's structural mint must find the typed id"
    );
}

/// An importing module initializes first (the entry module, or an import
/// cycle): its structural mint adopts the static id as a plain record, and the
/// definer's typed install then finds matching facts and adds the layout.
#[test]
fn an_importer_first_leaves_the_static_id_for_the_definer_to_type() {
    let class_id = 0x0B1_2002;
    let s = SHAPE_ID_BASE + 0x5102;
    let keys = keys_for(class_id, b"lt4u_next\0lt4u_value\0");
    let importer = crate::object::static_shapes::js_object_shape_id_for_class_keys_static(
        keys, 2, 2, class_id, s,
    );
    assert_eq!(importer, s);
    assert!(
        hot(s).is_none(),
        "a structural mint installs no typed layout"
    );
    assert_eq!(typed(class_id, keys, &RAW, &POINTERS, s), s);
    assert!(hot(s) == Some(Some(descriptor(&RAW, &POINTERS))));
}

/// Without a static id (a build the driver did not assign) the layout still
/// gets its own counter id and descriptor.
#[test]
fn no_static_id_mints_a_fresh_typed_id() {
    let class_id = 0x0B1_2003;
    let keys = keys_for(class_id, b"lt4v_next\0lt4v_value\0");
    let id = typed(class_id, keys, &RAW, &POINTERS, 0);
    assert!(is_shape_id(id) && !is_static_shape_id(id));
    assert!(hot(id) == Some(Some(descriptor(&RAW, &POINTERS))));
}

/// A hot-table entry that disagrees with the typed layout (learned from an
/// object, or poisoned) keeps the static id: the typed layout takes a fresh
/// id, so no image-born object is ever described by the wrong masks.
#[test]
fn a_conflicting_hot_entry_sends_the_typed_layout_to_a_fresh_id() {
    let class_id = 0x0B1_2004;
    let s = SHAPE_ID_BASE + 0x5104;
    let keys = keys_for(class_id, b"lt4w_next\0lt4w_value\0");
    hot_shape_layouts().borrow_mut().insert(s, None);
    let id = typed(class_id, keys, &RAW, &POINTERS, s);
    assert_ne!(
        id, s,
        "a poisoned static id must not carry the typed layout"
    );
    assert!(!is_static_shape_id(id));
    assert!(hot(id) == Some(Some(descriptor(&RAW, &POINTERS))));
    assert!(
        matches!(hot(s), Some(None)),
        "the poisoned entry is untouched"
    );
}

/// Two typed layouts with identical facts (colliding class ids, same keys) but
/// different masks have different contents, so the driver gives them two ids,
/// and each keeps its own descriptor.
#[test]
fn equal_facts_with_different_masks_keep_two_ids() {
    let class_id = 0x0B1_2005;
    let (s1, s2) = (SHAPE_ID_BASE + 0x5105, SHAPE_ID_BASE + 0x5106);
    let keys = keys_for(class_id, b"lt4x_next\0lt4x_value\0");
    assert_eq!(typed(class_id, keys, &RAW, &POINTERS, s1), s1);
    assert_eq!(typed(class_id, keys, &[], &[0b11], s2), s2);
    assert!(hot(s1) == Some(Some(descriptor(&RAW, &POINTERS))));
    assert!(hot(s2) == Some(Some(descriptor(&[], &[0b11]))));
}

/// A static id already naming OTHER facts in this agent is declined.
#[test]
fn a_static_id_naming_other_facts_is_declined() {
    let class_id = 0x0B1_2006;
    let s = SHAPE_ID_BASE + 0x5107;
    // Another class (its own id: a class keys array is built once per class
    // id) whose mint took the static id first.
    let other_class = class_id + 0x100;
    let other = keys_for(other_class, b"lt4y_a\0lt4y_b\0");
    assert_eq!(
        crate::object::static_shapes::js_object_shape_id_for_class_keys_static(
            other,
            2,
            2,
            other_class,
            s
        ),
        s
    );
    let keys = keys_for(class_id, b"lt4y_next\0lt4y_value\0");
    let id = typed(class_id, keys, &RAW, &POINTERS, s);
    assert_ne!(id, s);
    assert!(hot(s).is_none(), "the other facts' id gets no typed layout");
}
