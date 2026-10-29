use super::*;
use crate::codegen_abi::LayoutWord;

unsafe fn atom(name: &str) -> (u64, u64) {
    let key = crate::string::js_string_pool_atom(
        name.as_ptr(),
        name.len() as u32,
        crate::object::key_bytes_hash(name.as_ptr(), name.len()),
        0,
    );
    (
        STRING_TAG | key as u64,
        crate::string::property_atom_hash(key as *const crate::StringHeader),
    )
}

/// An object of nine atom keys (an indexed list) whose key `target` is moved
/// to the bucket after its own, while its own first bucket holds the TEXT
/// entry of `decoy` with text hash `decoy_hash`. Returns the object, the
/// target's key word and hash, and the target's value.
unsafe fn rigged(tag: &str, decoy_hash: impl Fn(u64) -> u64) -> (*mut ObjectHeader, u64, u64, f64) {
    let obj = crate::object::js_object_alloc(0, 16);
    let mut keys = Vec::new();
    for i in 0..9 {
        let (bits, hash) = atom(&format!("brg_text_first_{tag}_{i}"));
        crate::object::js_object_set_field_by_name(
            obj,
            (bits & POINTER_MASK) as *const crate::StringHeader,
            (i + 1) as f64,
        );
        keys.push((bits, hash));
    }
    let dir = crate::object::shapes::ordinary_dir_addr();
    let biased = (obj as usize).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR) as i64;
    // The first read derives the record's index.
    assert_eq!(perry_probe_get(dir, biased, keys[0].0, keys[0].1), 1.0);
    let r = record_of(dir, LayoutWord((*obj).parent_class_id));
    let table = ((*r).probe & !PROBE_FLAGS) as *mut KeyIndex;
    assert!(!table.is_null(), "nine keys exceed SCAN_KEYS: the list is indexed");
    let mask = (*table).mask as usize;
    let slots = KeyIndex::buckets(table) as *mut u32;
    let at = |pos: u32| (0..=mask).find(|&b| *slots.add(b) & POS_BITS == pos && *slots.add(b) != u32::MAX);
    // The target whose entry sits in its own first bucket, followed by a
    // bucket that is free or whose entry is not the target's.
    let (t, bt) = (0..9u32)
        .find_map(|t| {
            let b = at(t)?;
            (b == (keys[t as usize].1 as usize & mask)).then_some((t, b))
        })
        .expect("a key in its first bucket");
    let next = (bt + 1) & mask;
    let decoy = (0..9u32).find(|&d| d != t && at(d) != Some(next)).unwrap();
    // The target moves to the next bucket (displacing what is there, which
    // no read here uses); the first bucket holds the decoy as a TEXT entry
    // of another text hash.
    *slots.add(next) = *slots.add(bt);
    *slots.add(bt) = decoy | TEXT_ENTRY;
    *(*table).words.as_mut_ptr().add(decoy as usize) = decoy_hash(keys[t as usize].1);
    (obj, keys[t as usize].0, keys[t as usize].1, (t + 1) as f64)
}

fn underived_entries() -> usize {
    unloaded::UNDERIVED_ENTRIES.load(std::sync::atomic::Ordering::Relaxed)
}

#[test]
fn first_bucket_text_entry_of_another_hash_continues_the_index() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let (obj, key, hash, want) = rigged("miss", |h| h ^ 1);
        let dir = crate::object::shapes::ordinary_dir_addr();
        let biased = (obj as usize).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR) as i64;
        let before = underived_entries();
        assert_eq!(perry_probe_get(dir, biased, key, hash), want);
        assert_eq!(
            underived_entries(),
            before,
            "a TEXT entry of another hash must not send the read to the complete lookup"
        );
    }
}

#[test]
fn first_bucket_text_entry_of_the_same_hash_takes_the_complete_lookup() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let (obj, key, hash, want) = rigged("same", |h| h);
        let dir = crate::object::shapes::ordinary_dir_addr();
        let biased = (obj as usize).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR) as i64;
        let before = underived_entries();
        assert_eq!(perry_probe_get(dir, biased, key, hash), want);
        assert_eq!(
            underived_entries(),
            before + 1,
            "a TEXT entry of the key's hash is compared by the complete lookup"
        );
    }
}
