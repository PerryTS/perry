//! Runtime read sites: a builtin's spec `Get(O, key)` on the one-shape path.
//!
//! A builtin that reads a fixed key off a receiver it was handed (the promise
//! resolution's `Get(resolution, "then")`; later the RegExp builtins'
//! `Get(R, "exec")` and flag reads) holds a [`RuntimeReadSite`]. The site is
//! exactly the pair of words an emitted `o.key` site holds — the compact MRU
//! word and the full-cache slot — and it is read through exactly the entries
//! an emitted site's tower calls:
//!
//! 1. the compact word: the receiver's ShapeId compared with its low half,
//!    then one load of the inline slot its high half names (an own key);
//! 2. on a ShapeId miss, the GC-leaf front
//!    ([`super::js_object_get_field_ic_front`]): the polymorphic ways, a spill
//!    entry, and the holder entry (`method_site::read_holder::entry_answer`,
//!    then the declared-class entry) for a key that is NOT own;
//! 3. on `TAG_HOLE`, the collecting slow entry
//!    ([`super::js_object_get_field_ic_slow`]), whose miss handler primes the
//!    words (`packed_get::prime_get` for an own key,
//!    `read_holder::prime_read_holder` for an inherited or absent one).
//!
//! So "the builtin reads `then`" costs what `o.then` costs in compiled code:
//! one shape compare and one load for an own key, the receiver's and the
//! holder's ShapeId compares and one load for an inherited one. There is no
//! second path: every fact the site answers from is a shape fact the emitted
//! site would answer from, invalidated the same way (a key add, delete,
//! descriptor change or `setPrototypeOf` moves a ShapeId; a value store to the
//! holder is seen because the hit loads the slot).
//!
//! # Per agent
//!
//! A site's words are declared with [`crate::perry_thread_local`], so every
//! agent (the main thread and each `perry/thread` worker) has its own pair and
//! no agent ever reads another's words. The full cache the slot points to is a
//! PIC-arena allocation, never freed, exactly as an emitted site's is, so the
//! holder entry's root registration (`read_holder`'s site list) names memory
//! that outlives the thread. Holder entries keep their existing rule: they are
//! primed and answered only by the primary agent until a worker starts; a
//! worker's site answers its own keys from its own words and takes the slow
//! entry for the rest.
//!
//! # The key
//!
//! The front confirms nothing by key position here: the site passes the empty
//! shape directory, as a `length` site does, so it never needs the key on the
//! leaf path. The slow entry gets the key's ATOM (`string::js_string_pool_atom`),
//! the one string per key text the agent holds strongly for its lifetime. A
//! site's key is a fixed builtin name, so the runtime's sites add a bounded
//! set of atoms, as the program's pools do. The atom is looked up per slow
//! call, never cached in the site: the atom table is what the collector
//! rewrites.

use super::{js_object_get_field_ic_front, js_object_get_field_ic_slow};
use crate::object::{ObjectHeader, PicCache, PicCacheSlot};
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};

/// The value an emitted site's compact word is born holding
/// (`ic_miss::PACKED_GET_EMPTY`): above every ShapeId, so no receiver's
/// `+4` word can match an unprimed site.
const PACKED_EMPTY: u64 = 0xFFFF_FFFF;

/// One builtin's read site for one key: an emitted read site's two words.
/// Declare one per (builtin, key) with [`crate::perry_thread_local`].
pub(crate) struct RuntimeReadSite {
    /// The compact MRU word (`@perry_ic_N_packed_get`): ShapeId low, slot high.
    packed: AtomicU64,
    /// The full-cache slot (`@perry_ic_N`): null until the first prime.
    slot: AtomicPtr<PicCache>,
}

impl RuntimeReadSite {
    pub(crate) const fn new() -> Self {
        RuntimeReadSite {
            packed: AtomicU64::new(PACKED_EMPTY),
            slot: AtomicPtr::new(std::ptr::null_mut()),
        }
    }

    #[inline(always)]
    fn slot_ptr(&self) -> *mut PicCacheSlot {
        self.slot.as_ptr() as *mut PicCacheSlot
    }

    /// The emitted own-inline hit. The honest +4 word of any live heap cell
    /// can be compared here: only a primed ordinary ShapeId admits the load.
    #[inline(always)]
    pub(crate) unsafe fn read_own_inline(&self, obj: *const ObjectHeader) -> Option<f64> {
        let word = self.packed.load(Ordering::Relaxed);
        if (*obj).parent_class_id != word as u32 {
            return None;
        }
        let bits = *((obj as *const u8)
            .add(std::mem::size_of::<ObjectHeader>() + (word >> 32) as usize * 8)
            as *const u64);
        (bits != crate::value::TAG_HOLE).then_some(f64::from_bits(bits))
    }

    /// Publish an own data slot established by a namespace-aware lookup.
    /// The caller has proved that this shape owns the key as a data entry.
    /// Dictionaries and overflow slots stay on that lookup's slow path.
    pub(crate) fn prime_own_inline(&self, shape: u32, index: u32, live: u32) {
        if crate::object::shapes::is_site_matchable_shape_id(shape) && index < live {
            self.packed.store(
                (u64::from(index) << 32) | u64::from(shape),
                Ordering::Relaxed,
            );
        }
    }

    /// The site's answer for `obj` when the shapes give it, without
    /// collecting: the compact word, then the GC-leaf front. `None` when the
    /// site must take [`Self::read_slow`].
    ///
    /// # Safety
    /// `obj` is a live `GC_TYPE_OBJECT` above the handle band (see
    /// [`object_receiver`]).
    #[inline]
    pub(crate) unsafe fn read_leaf(&self, obj: *const ObjectHeader) -> Option<f64> {
        if let Some(value) = self.read_own_inline(obj) {
            return Some(value);
        }
        let dir = std::ptr::addr_of!(crate::object::shapes::PERRY_EMPTY_SHAPE_DIR) as *const u8;
        let biased = (obj as usize).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR) as i64;
        let v = js_object_get_field_ic_front(dir, biased, 0, self.slot_ptr(), &self.packed);
        (v.to_bits() != crate::value::TAG_HOLE).then_some(v)
    }

    /// The native entry of a getter, after the ordinary accessor lane checks.
    /// An inherited immutable pair owns the memo; own accessors are inspected
    /// on every call because equal receiver shapes do not imply equal pairs.
    #[inline]
    #[cfg(any(test, feature = "regex-engine"))]
    pub(crate) unsafe fn probe_getter_code(
        &self,
        obj: *const ObjectHeader,
        key: crate::object::method_site::read_holder::probe::Key<'_>,
    ) -> Option<usize> {
        use crate::object::method_site::read_holder::probe::{self, Answer};
        let cache = self.slot.load(Ordering::Relaxed);
        if !cache.is_null() {
            if let Some(code) = probe::probe_accessor_code(&*cache, obj) {
                return Some(code);
            }
        }
        match self.probe(obj, key)? {
            Answer::Getter(bits) => Some(probe::getter_code(bits)),
            Answer::Data(_) => None,
        }
    }

    /// Inspect the already primed entry without resolving its key. Symbol
    /// identities, like name atoms, are needed only when priming a miss.
    /// # Safety
    /// As Self::read_leaf.
    #[inline]
    #[cfg(any(test, feature = "regex-engine"))]
    pub(crate) unsafe fn probe_leaf(
        &self,
        obj: *const ObjectHeader,
    ) -> Option<crate::object::method_site::read_holder::probe::Answer> {
        use crate::object::method_site::read_holder::{self, probe::Answer};
        if let Some(value) = self.read_own_inline(obj) {
            return Some(Answer::Data(value.to_bits()));
        }
        let cache = self.slot.load(Ordering::Relaxed);
        if !cache.is_null() {
            let stamp = crate::object::shapes::object_shape_stamp(obj);
            let token = (u64::from(stamp) | crate::object::shapes::PIC_ID_TOKEN_BIT) as i64;
            if let Some(bits) = read_holder::entry_answer(&*cache, token) {
                return Some(Answer::Data(bits));
            }
            if let Some(answer) = read_holder::probe::probe_accessor_entry(&*cache, obj) {
                return Some(answer);
            }
        }
        None
    }

    /// Inspect a data property or accessor without running user code.
    /// Uses the emitted read's PIC and accessor lane validation.
    #[cfg(any(test, feature = "regex-engine"))]
    pub(crate) unsafe fn probe(
        &self,
        obj: *const ObjectHeader,
        key: crate::object::method_site::read_holder::probe::Key<'_>,
    ) -> Option<crate::object::method_site::read_holder::probe::Answer> {
        use crate::object::method_site::read_holder::{
            self,
            probe::{Answer, Key},
        };
        if let Some(answer) = self.probe_leaf(obj) {
            return Some(answer);
        }
        let answer = read_holder::probe::prime(obj, key, self.slot_ptr())?;
        if matches!(answer, Answer::Data(_)) {
            let keys = crate::object::object_keys(obj);
            let index = match key {
                Key::Name(name) => {
                    crate::object::keys_find_property_slot_by_bytes(keys.arr(), keys.count(), name)
                }
                Key::Symbol(symbol) => crate::object::shaped_symbols::position(obj, symbol),
            };
            if let Some(index) = index {
                self.prime_own_inline(
                    crate::object::shapes::object_shape_stamp(obj),
                    index,
                    crate::object::object_live_slot_count(obj),
                );
            }
        }
        Some(answer)
    }

    /// The collecting read: the slow entry an emitted site calls on a front
    /// miss, which also primes the words. It can run a getter, collect and
    /// throw. Returns the value and the receiver's address after the call.
    ///
    /// # Safety
    /// As [`Self::read_leaf`]; `key` is the site's fixed key text.
    #[cold]
    #[inline(never)]
    pub(crate) unsafe fn read_slow(
        &self,
        obj: *mut ObjectHeader,
        key: &'static [u8],
    ) -> (f64, *mut ObjectHeader) {
        let hash = crate::object::key_bytes_hash(key.as_ptr(), key.len());
        let (atom, obj) = match crate::string::atom_lookup(key, hash) {
            Some(atom) => (atom, obj),
            None => {
                // The first read per agent mints the atom: an allocation, so
                // the receiver is rooted across it.
                let scope = crate::gc::RuntimeHandleScope::new();
                let handle = scope.root_raw_mut_ptr(obj);
                let (atom, obj) = handle.across_mut::<ObjectHeader, _>(|| {
                    crate::string::js_string_pool_atom(key.as_ptr(), key.len() as u32, hash, 0)
                        as *const crate::StringHeader
                });
                (atom, obj)
            }
        };
        let scope = crate::gc::RuntimeHandleScope::new();
        let handle = scope.root_raw_mut_ptr(obj);
        handle.across_mut::<ObjectHeader, _>(|| {
            js_object_get_field_ic_slow(obj as i64, atom, self.slot_ptr(), &self.packed)
        })
    }

    /// `Get(obj, key)`: [`Self::read_leaf`], else [`Self::read_slow`].
    ///
    /// # Safety
    /// As [`Self::read_slow`].
    #[cfg(any(test, feature = "regex-engine"))]
    pub(crate) unsafe fn read(&self, obj: *mut ObjectHeader, key: &'static [u8]) -> f64 {
        match self.read_leaf(obj) {
            Some(v) => v,
            None => self.read_slow(obj, key).0,
        }
    }
}

/// The ordinary-object receiver a site reads from: `value` is a pointer above
/// the handle band whose validated GC header says `GC_TYPE_OBJECT`. Every
/// other value (primitives, proxies and handles, arrays, closures, promises,
/// exotic cells) keeps its caller's generic `Get`.
#[inline]
pub(crate) fn object_receiver(value: f64) -> Option<*mut ObjectHeader> {
    let v = crate::value::JSValue::from_bits(value.to_bits());
    if !v.is_pointer() {
        return None;
    }
    let addr = (value.to_bits() & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_above_handle_band(addr) {
        return None;
    }
    let header = unsafe { crate::value::addr_class::try_read_gc_header(addr)? };
    (header.obj_type == crate::gc::GC_TYPE_OBJECT
        && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0)
        .then_some(addr as *mut ObjectHeader)
}

#[cfg(test)]
#[path = "runtime_read_site_tests.rs"]
mod tests;
