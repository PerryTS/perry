//! WRT step 2: a static literal seed whose canonical key list is a cell of
//! the realm image (`PerryStaticSeedV1`, `static_shapes::install_image_seed`).
//!
//! The image holds one key list cell, `{value, done}`, whose slots name the
//! runtime's own atoms' strings (the cell the compiler emits for a seed). The
//! tests install a seed on it and show: the record's keys ARE the cell, the
//! trie answers the cell to a by-names walk of the same keys, both survive a
//! synchronous full collection, and a seed the image cannot serve (no list,
//! or a list that does not hold the atoms' strings) is minted by names.
#![cfg(target_pointer_width = "64")]
use super::support::*;
use crate::array::ArrayHeader;
use crate::codegen_abi::{LayoutWord, PerryStaticLayoutRequestV1, PerryStaticSeedV1};
use crate::gc::*;
use crate::object::layout_desc::PerryLayoutKey;
use crate::object::static_shapes::fixtures::fixture_word;
use crate::object::{shapes, static_shapes};

#[repr(C, align(8))]
struct ListCell {
    gc: GcHeader,
    arr: ArrayHeader,
    slots: [u64; 2],
}

const LIST_CELL: u32 = (GC_HEADER_SIZE + std::mem::size_of::<ArrayHeader>() + 16) as u32;
const _: () = assert!(std::mem::size_of::<ArrayHeader>() == 24);

const fn list_cell() -> ListCell {
    ListCell {
        gc: GcHeader {
            obj_type: GC_TYPE_ARRAY,
            gc_flags: GC_FLAG_MARKED | GC_FLAG_PINNED | GC_FLAG_SHAPE_SHARED,
            _reserved: 0,
            size: LIST_CELL,
        },
        arr: ArrayHeader {
            length: 2,
            capacity: 2,
            named: std::ptr::null_mut(),
            ek_prefix: 0,
            ek_kind: 0,
        },
        // Written by `image_list` (a Rust initializer cannot tag an address);
        // codegen emits the tagged words at link time.
        slots: [0, 0],
    }
}

#[used]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_realm")]
#[cfg_attr(target_os = "windows", link_section = ".perryrealm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_realm"
)]
static mut LIST: ListCell = list_cell();

/// The cell is one process-wide fixture; a test holds it while it reads
/// or rewrites its slots (the image_cells tests hold theirs the same way).
static LIST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn hold_list() -> std::sync::MutexGuard<'static, ()> {
    LIST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// The runtime's `{value, done}` keys (ITERATOR_RESULT's atoms).
fn keys() -> &'static [PerryLayoutKey] {
    unsafe {
        let desc = std::ptr::addr_of!(crate::object::runtime_layouts::ITERATOR_RESULT);
        std::slice::from_raw_parts((*desc).keys, 2)
    }
}

fn string_word(key: &PerryLayoutKey) -> u64 {
    unsafe { crate::JSValue::string_ptr((*key.atom).string.cast_mut()).bits() }
}

/// The image list, holding the keys' strings in `order`.
fn image_list(order: [usize; 2]) -> *mut ArrayHeader {
    let keys = keys();
    unsafe {
        let cell = std::ptr::addr_of_mut!(LIST);
        (*cell).slots = [string_word(&keys[order[0]]), string_word(&keys[order[1]])];
        std::ptr::addr_of_mut!((*cell).arr)
    }
}

struct Seed {
    request: PerryStaticLayoutRequestV1,
    packed: &'static [u8],
}

/// Install the `{value, done}` seed on the image list under fixture content
/// `content`, for an image object to carry: its layout word.
pub(super) fn install_value_done_seed(content: u32) -> LayoutWord {
    let _list = hold_list();
    let list = image_list([0, 1]);
    let mut seed = Seed::new(content, b"value\0done\0");
    static_shapes::install_image_seed(&seed.row(Some(keys()), list));
    assert_eq!(seed.record_keys(), list as u64);
    seed.word()
}

impl Seed {
    fn new(content: u32, packed: &'static [u8]) -> Self {
        let word = fixture_word(content);
        Seed {
            request: PerryStaticLayoutRequestV1 {
                shape_id: word.wrapping_add(0x10000),
                layout_word: word,
            },
            packed,
        }
    }

    fn row(
        &mut self,
        keys: Option<&[PerryLayoutKey]>,
        list: *mut ArrayHeader,
    ) -> PerryStaticSeedV1 {
        PerryStaticSeedV1 {
            rep: 0,
            request: &mut self.request,
            packed: self.packed.as_ptr(),
            keys: keys.map_or(std::ptr::null(), |keys| keys.as_ptr().cast()),
            list: list.cast(),
            constfn: std::ptr::null(),
            packed_len: self.packed.len() as u32,
            key_count: 2,
            live: 2,
            constfn_count: 0,
        }
    }

    fn word(&self) -> LayoutWord {
        LayoutWord(self.request.layout_word)
    }

    fn record_keys(&self) -> u64 {
        shapes::shape_record_for_word(self.word())
            .expect("the seed published its record")
            .keys()
    }
}

fn trigger() -> GcTriggerSnapshot {
    GcTriggerSnapshot::capture(GcTriggerKind::Direct)
}

fn by_names(names: &[&[u8]]) -> u64 {
    unsafe { static_shapes::canonical_keys_for_names(names) }.arr() as u64
}

#[test]
fn an_image_seed_links_its_cell_as_the_canonical_list_and_publishes_without_a_heap_list() {
    let _list = hold_list();
    crate::test_support::bootstrap_test_thread();
    let list = image_list([0, 1]);
    let mut seed = Seed::new(0x301, b"value\0done\0");
    static_shapes::install_image_seed(&seed.row(Some(keys()), list));
    assert_eq!(
        unsafe { shapes::published_static_request(&seed.request, 2) },
        Some(seed.request.layout_word),
        "the seed is published under its request"
    );
    assert_eq!(
        seed.record_keys(),
        list as u64,
        "the record's keys ARE the image cell"
    );
    assert!(crate::value::addr_class::realm::is_image_object(
        list as usize
    ));
    // The invariant: a later walk of the same keys reaches the seeded list.
    assert_eq!(by_names(&[b"value", b"done"]), list as u64);
    // A repeated request answers from the row.
    static_shapes::install_image_seed(&seed.row(Some(keys()), list));
    assert_eq!(seed.record_keys(), list as u64);
}

#[test]
fn an_image_seed_survives_a_synchronous_full_collection_with_no_carrier() {
    let _list = hold_list();
    crate::test_support::bootstrap_test_thread();
    let list = image_list([0, 1]);
    let mut seed = Seed::new(0x302, b"value\0done\0");
    static_shapes::install_image_seed(&seed.row(Some(keys()), list));
    gc_collect_full_mark_sweep_with_trigger(trigger());
    gc_collect_full_mark_sweep_with_trigger(trigger());
    assert_eq!(
        seed.record_keys(),
        list as u64,
        "a static record and its image keys outlive every trace"
    );
    assert_eq!(by_names(&[b"value", b"done"]), list as u64);
}

#[test]
fn a_seed_without_an_image_list_is_minted_by_names() {
    let _list = hold_list();
    crate::test_support::bootstrap_test_thread();
    let mut seed = Seed::new(0x303, b"alpha\0beta\0");
    static_shapes::install_image_seed(&seed.row(None, std::ptr::null_mut()));
    let keys = seed.record_keys();
    assert_ne!(keys, 0);
    assert!(!crate::value::addr_class::realm::is_image_object(
        keys as usize
    ));
    assert_eq!(by_names(&[b"alpha", b"beta"]), keys);
}

#[test]
fn a_cell_that_does_not_hold_the_atoms_strings_is_refused_and_the_seed_minted_by_names() {
    let _list = hold_list();
    crate::test_support::bootstrap_test_thread();
    // The cell holds {done, value}: not the list the keys name.
    let list = image_list([1, 0]);
    let mut seed = Seed::new(0x304, b"value\0done\0");
    static_shapes::install_image_seed(&seed.row(Some(keys()), list));
    let keys = seed.record_keys();
    assert_ne!(
        keys, list as u64,
        "a list the keys do not describe is never linked"
    );
    assert!(!crate::value::addr_class::realm::is_image_object(
        keys as usize
    ));
    image_list([0, 1]);
}
