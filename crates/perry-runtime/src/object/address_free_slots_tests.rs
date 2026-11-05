//! Slot placement depends on key content; identity remains a separate check.
use super::*;
use crate::codegen_abi::LayoutWord;

/// Two valid headers separated by an exact old slot period. Raw-pointer
/// hashing maps them to the same transition slot regardless of ASLR.
pub(super) fn relocated_keys(a: &[u8], b: &[u8]) -> (Vec<u64>, usize, usize) {
    const GAP: usize = TRANSITION_CACHE_SIZE * 8;
    let mut storage = vec![0u64; (GAP + 128) / 8];
    let first = storage.as_mut_ptr() as usize;
    let second = first + GAP;
    for (addr, bytes) in [(first, a), (second, b)] {
        unsafe {
            let key = addr as *mut crate::StringHeader;
            // GC_STORE_AUDIT(POINTER_FREE): Writes only five scalar StringHeader fields into test-owned Vec<u64> backing. No fields contain GC child pointers.
            key.write(crate::StringHeader {
                utf16_len: bytes.len() as u32,
                byte_len: bytes.len() as u32,
                capacity: bytes.len() as u32,
                refcount: 0,
                flags: 0,
            });
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                crate::string::string_data(key) as *mut u8,
                bytes.len(),
            );
        }
    }
    (storage, first, second)
}

#[test]
fn transition_slots_distinguish_content_at_old_pointer_collision() {
    let (_storage, a, b) = relocated_keys(b"first-property", b"other-property");
    assert_ne!(
        transition_cache_slot(LayoutWord(0x8000_0042), a, 0),
        transition_cache_slot(LayoutWord(0x8000_0042), b, 0),
        "an exact pointer-table period must not force different names to collide"
    );
}

#[test]
fn slots_follow_content_when_a_key_moves() {
    let (_storage, a, b) = relocated_keys(b"same-property", b"same-property");
    assert_eq!(interned_key_slot_hash(a), interned_key_slot_hash(b));
    assert_eq!(
        transition_cache_slot(LayoutWord(0x8000_0042), a, 0),
        transition_cache_slot(LayoutWord(0x8000_0042), b, 0),
    );
    // These are deliberately noncanonical strings. Equal content chooses a
    // slot but must not let one pointer borrow the other's store verdict.
    prop_plan::store_plan_record(LayoutWord(0x8000_0042), a);
    assert!(prop_plan::store_plan_check(LayoutWord(0x8000_0042), a));
    assert!(!prop_plan::store_plan_check(LayoutWord(0x8000_0042), b));
    prop_plan::read_plan_record(LayoutWord(0x8000_0042), 0x1234, a, 2);
    assert_eq!(
        prop_plan::read_plan_lookup(LayoutWord(0x8000_0042), 0x1234, a, 3),
        Some(2)
    );
    assert_eq!(
        prop_plan::read_plan_lookup(LayoutWord(0x8000_0042), 0x1234, b, 3),
        None
    );
    assert_eq!(
        prop_plan::read_plan_lookup(LayoutWord(0x8000_0042), 0x5678, a, 3),
        None
    );
    // The synthetic headers are Rust-owned rather than GC-owned; invalidate
    // their weak plan metadata before releasing that storage.
    prop_plan::prop_plan_epoch_bump();
}

#[test]
fn stored_atom_slot_uses_its_existing_content_hash() {
    let atom =
        unsafe { crate::string::property_atom(crate::string::canonical_key(b"stable-property")) };
    let hash = unsafe { crate::string::property_atom_hash(atom) };
    assert_eq!(interned_key_slot_hash(atom as usize), hash ^ (hash >> 32));
}

#[test]
fn legacy_slot_hash_reads_every_word_and_tail_byte() {
    let original = *b"long-property-with-a-tail!";
    for i in 0..original.len() {
        let mut changed = original;
        changed[i] ^= 1;
        let (_storage, a, b) = relocated_keys(&original, &changed);
        assert_ne!(interned_key_slot_hash(a), interned_key_slot_hash(b));
    }
}
