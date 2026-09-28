//! Typed class layouts under link-time ShapeIds (design step 4): the driver's
//! static id carries the masks, is adopted in whichever order the defining
//! and an importing module initialize, and a refusal — impossible by
//! construction since decision 16 — aborts. No registry is consulted.
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
/// cycle). Its structural view of the class has exactly the definer's facts,
/// so it is the same content and the driver hands it the definer's (typed) id:
/// its mint adopts that id with no layout, the definer's typed install then
/// accepts the same facts under it and installs the descriptor, and every
/// later birth of those facts reaches it. Init order never matters.
#[test]
fn an_importer_first_adopts_the_definers_id_and_the_typed_install_accepts_it() {
    let class_id = 0x0B1_2002;
    let s = SHAPE_ID_BASE + 0x5103;
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
    assert_eq!(
        crate::object::static_shapes::js_object_shape_id_for_class_keys_static(
            keys, 2, 2, class_id, s,
        ),
        s,
        "a later birth of those facts reaches the one id"
    );
}

/// Run `name` in a child test process with `SABOTAGE_ENV` set and require it
/// to abort with the refusal message.
fn child_aborts_with_refusal(name: &str) {
    let out = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .arg(name)
        .arg("--exact")
        .arg("--nocapture")
        .arg("--test-threads=1")
        .env(SABOTAGE_ENV, "1")
        .output()
        .expect("launch the sabotaged child");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "the sabotaged install did not abort");
    assert!(
        stderr.contains("was refused by its typed layout install"),
        "the child died without the refusal message: {stderr}"
    );
}

const SABOTAGE_ENV: &str = "PERRY_TEST_TYPED_STATIC_REFUSAL_CHILD";

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

/// Sabotage: a hot-table entry that disagrees with the typed layout (learned
/// from an object, or poisoned) under the static id. The install must ABORT,
/// never fall back to a fresh id the definer's immediates do not name.
#[test]
fn a_conflicting_hot_entry_aborts_the_typed_install() {
    if std::env::var_os(SABOTAGE_ENV).is_none() {
        child_aborts_with_refusal(
            "gc::layout::typed_shape::static_id_tests::a_conflicting_hot_entry_aborts_the_typed_install",
        );
        return;
    }
    let class_id = 0x0B1_2004;
    let s = SHAPE_ID_BASE + 0x5104;
    let keys = keys_for(class_id, b"lt4w_next\0lt4w_value\0");
    hot_shape_layouts().borrow_mut().insert(s, None);
    typed(class_id, keys, &RAW, &POINTERS, s);
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

/// Sabotage: a static id already naming OTHER facts in this agent (another
/// class's mint took it) makes the typed install abort.
#[test]
fn a_static_id_naming_other_facts_aborts_the_typed_install() {
    if std::env::var_os(SABOTAGE_ENV).is_none() {
        child_aborts_with_refusal(
            "gc::layout::typed_shape::static_id_tests::a_static_id_naming_other_facts_aborts_the_typed_install",
        );
        return;
    }
    let class_id = 0x0B1_2006;
    let s = SHAPE_ID_BASE + 0x5107;
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
    typed(class_id, keys, &RAW, &POINTERS, s);
}
