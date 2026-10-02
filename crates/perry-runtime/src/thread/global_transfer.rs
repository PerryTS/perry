//! Immutable module-global leaves shared between perry/thread agents.
//!
//! perry/thread agents share user-module globals and module-once
//! initialization, but each agent owns a separate moving heap. A String or
//! BigInt allocated by the initializing agent lives in that agent's arena; its
//! collector rewrites the canonical `@perry_global_*` slot, never a copy another
//! agent loaded. So another agent must never hold the initializer's object.
//!
//! The owning initializer instead publishes a pointer-free transport record
//! (String bytes + semantic flags + UTF-16 length, or BigInt limbs) exactly
//! once. Every agent reads the binding through a compiler-generated,
//! thread-local cache (`AgentGlobalCache`) whose value word is registered as
//! that agent's own GC root. A cold read materializes an ordinary movable
//! String/BigInt in the reading agent's heap from the record.
//!
//! The record is not a JS value and holds no heap address. The publication cell
//! and cache are generated per binding by the compiler (no runtime registry, no
//! lookup by address). Pending (initializer not yet run, or it threw) keeps the
//! uninitialized `undefined` and retries on the next read: no waiting, no
//! replaying module initialization, no caching of `undefined`.
//!
//! Lifetime: records live for the executable. An image that can be unloaded
//! (a dylib plugin) is not handled yet: its records are never freed.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crate::bigint::{self, BigIntHeader, BIGINT_LIMBS};
use crate::string::StringHeader;

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const BIGINT_TAG: u64 = 0x7FFA_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// Cache states. The compiler emits `STATE_LOCAL_LEAF_READY` and
/// `STATE_CANONICAL_NONLEAF` as immediates in the inline hot path
/// (`perry-codegen/src/codegen/global_transfer.rs`); keep both in sync.
pub(crate) const STATE_UNSEEN: u64 = 0;
pub(crate) const STATE_REGISTERED_PENDING: u64 = 1;
pub(crate) const STATE_LOCAL_LEAF_READY: u64 = 2;
pub(crate) const STATE_CANONICAL_NONLEAF: u64 = 3;

/// Publication-cell value for an initialized binding whose value is not a heap
/// String/BigInt (an immediate, or another heap kind outside this claim).
/// Readers then use the canonical slot, exactly as before this transfer.
const NONLEAF_RECORD: usize = 1;

/// The compiler-owned, per-binding, per-agent cache: `{ double, i64 }` with the
/// value at offset 0 and the state at offset 8.
#[repr(C)]
pub struct AgentGlobalCache {
    value: u64,
    state: u64,
}

const _: () = {
    assert!(std::mem::size_of::<AgentGlobalCache>() == 16);
    assert!(std::mem::offset_of!(AgentGlobalCache, value) == 0);
    assert!(std::mem::offset_of!(AgentGlobalCache, state) == 8);
};

/// Owned, immutable, pointer-free transport data. Never modified after
/// publication.
enum LeafRecord {
    String {
        utf16_len: u32,
        /// Semantic flags only (lone surrogates / WTF-8). Capacity, refcount
        /// and cache/validation flags are representation details.
        flags: u32,
        bytes: Box<[u8]>,
    },
    BigInt {
        limbs: [u64; BIGINT_LIMBS],
    },
}

/// Build the record from the owner's value. Reads only; never allocates on a
/// GC heap and never collects.
unsafe fn leaf_record_from_owned(bits: u64) -> Option<LeafRecord> {
    let payload = bits & POINTER_MASK;
    match bits & !POINTER_MASK {
        STRING_TAG => {
            let header = payload as *mut StringHeader;
            if !crate::string::is_valid_string_ptr(header) {
                return Some(LeafRecord::String {
                    utf16_len: 0,
                    flags: 0,
                    bytes: Box::new([]),
                });
            }
            let len = (*header).byte_len as usize;
            let data = crate::string::string_data(header);
            let bytes = std::slice::from_raw_parts(data, len)
                .to_vec()
                .into_boxed_slice();
            // The binding is immutable and now visible to other agents: an
            // in-place append must never mutate the owner's copy.
            (*header).refcount = 0;
            Some(LeafRecord::String {
                utf16_len: (*header).utf16_len,
                flags: (*header).flags & crate::string::STRING_FLAG_HAS_LONE_SURROGATES,
                bytes,
            })
        }
        BIGINT_TAG => {
            let ptr = bigint::clean_bigint_ptr(payload as *const BigIntHeader);
            let limbs = if ptr.is_null() {
                [0u64; BIGINT_LIMBS]
            } else {
                (*ptr).limbs
            };
            Some(LeafRecord::BigInt { limbs })
        }
        // A short string (SHORT_STRING_TAG) is an immediate with no
        // StringHeader. It publishes as NONLEAF, and every agent reads the
        // canonical slot, which holds no heap pointer.
        crate::value::SHORT_STRING_TAG => None,
        _ => None,
    }
}

/// Allocate the record's value in the CURRENT agent's heap. May collect.
unsafe fn materialize(record: &LeafRecord) -> u64 {
    match record {
        LeafRecord::String {
            utf16_len,
            flags,
            bytes,
        } => {
            let len = bytes.len() as u32;
            let (header, data) = crate::string::string_storage_alloc(len);
            // Exact UTF-16 length and semantic flags from the owner, shared
            // ownership (refcount 0): never an in-place append target.
            crate::string::init_string_header(header, *utf16_len, len, len, 0, *flags);
            if len > 0 {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, len as usize);
            }
            STRING_TAG | (header as u64 & POINTER_MASK)
        }
        LeafRecord::BigInt { limbs } => {
            let ptr = bigint::bigint_alloc_with_limbs(*limbs);
            BIGINT_TAG | (ptr as u64 & POINTER_MASK)
        }
    }
}

/// Register this agent's cache value word as one of its GC roots, once,
/// before anything can allocate on its behalf.
unsafe fn ensure_registered(cache: *mut AgentGlobalCache) {
    if (*cache).state == STATE_UNSEEN {
        (*cache).value = TAG_UNDEFINED;
        crate::gc::js_gc_register_global_root(std::ptr::addr_of_mut!((*cache).value) as i64);
        (*cache).state = STATE_REGISTERED_PENDING;
    }
}

unsafe fn store_cache_value(cache: *mut AgentGlobalCache, bits: u64) {
    crate::gc::runtime_store_root_nanbox_f64_raw_slot(
        std::ptr::addr_of_mut!((*cache).value).cast::<f64>(),
        f64::from_bits(bits),
    );
}

#[inline]
unsafe fn load_canonical(canonical: i64) -> f64 {
    f64::from_bits((*(canonical as *const AtomicU64)).load(Ordering::Relaxed))
}

/// Called by the owning module initializer right after it stored the
/// binding's value into its registered canonical slot. Does not allocate on
/// the GC heap and never collects; reads the value from the canonical root.
///
/// `cell`: the binding's process-wide publication cell (0 = pending).
/// `cache`: the publishing agent's `AgentGlobalCache` for the binding.
/// `canonical`: the canonical `@perry_global_*` slot.
#[no_mangle]
pub unsafe extern "C" fn js_thread_global_publish(cell: i64, cache: i64, canonical: i64) {
    let cell = &*(cell as *const AtomicUsize);
    let cache = cache as *mut AgentGlobalCache;
    let bits = *(canonical as *const u64);
    let record = match leaf_record_from_owned(bits) {
        Some(record) => Box::into_raw(Box::new(record)) as usize,
        None => NONLEAF_RECORD,
    };
    ensure_registered(cache);
    if record == NONLEAF_RECORD {
        (*cache).state = STATE_CANONICAL_NONLEAF;
    } else {
        // The owner's own replica IS its canonical object: both are this
        // agent's roots and its collector rewrites both.
        store_cache_value(cache, bits);
        (*cache).state = STATE_LOCAL_LEAF_READY;
    }
    if cell
        .compare_exchange(0, record, Ordering::Release, Ordering::Relaxed)
        .is_err()
    {
        // The compiler admits only single-initializer, never-reassigned
        // bindings evaluated under the module-once guard. A second publication
        // means that proof failed; never silently replace a published value.
        eprintln!(
            "perry/thread: immutable module global published twice (compiler proof violated)"
        );
        std::process::abort();
    }
}

/// Cold read of an eligible binding on the current agent. The generated hot
/// path handles `STATE_LOCAL_LEAF_READY` / `STATE_CANONICAL_NONLEAF` inline and
/// calls this otherwise. May collect (it allocates the local replica).
#[no_mangle]
pub unsafe extern "C" fn js_thread_global_materialize(
    cell: i64,
    cache: i64,
    canonical: i64,
) -> f64 {
    let cache = cache as *mut AgentGlobalCache;
    match (*cache).state {
        STATE_LOCAL_LEAF_READY => return f64::from_bits((*cache).value),
        STATE_CANONICAL_NONLEAF => return load_canonical(canonical),
        _ => {}
    }
    ensure_registered(cache);
    let record = (*(cell as *const AtomicUsize)).load(Ordering::Acquire);
    if record == 0 {
        // Pending: the binding's uninitialized value. Not cached; the next
        // read retries and observes a later publication.
        return f64::from_bits(TAG_UNDEFINED);
    }
    if record == NONLEAF_RECORD {
        (*cache).state = STATE_CANONICAL_NONLEAF;
        return load_canonical(canonical);
    }
    let bits = materialize(&*(record as *const LeafRecord));
    // No collection point between the allocation above and this store.
    store_cache_value(cache, bits);
    (*cache).state = STATE_LOCAL_LEAF_READY;
    f64::from_bits((*cache).value)
}

#[cfg(test)]
#[path = "global_transfer_tests.rs"]
mod tests;
