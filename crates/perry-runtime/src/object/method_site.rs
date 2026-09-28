//! The method-call site memo: `recv.m(args)` on the One Path.
//!
//! A method call is a property read followed by a call. The emitted site
//! (`perry-codegen/src/expr/method_site.rs`) compares the receiver's
//! `(class_id | ShapeId)` word against each entry's [`MethodEntry::word`] and
//! then, with no runtime call:
//!
//! * **own entry** — loads the receiver's inline slot [`MethodEntry::slot`],
//!   proves the value is a closure whose body info equals
//!   [`MethodEntry::info`], and calls [`MethodEntry::code`] directly with the
//!   receiver as `this`;
//! * **inherited entry** ([`METHOD_SITE_INHERITED`] in `slot`) — compares
//!   [`MethodEntry::gen`] against `PERRY_PROTO_VALIDITY` and calls
//!   [`MethodEntry::code`] on the memoized closure [`MethodEntry::closure`].
//!
//! Everything else calls [`js_method_site_miss`], which primes the entry when
//! the facts below hold and then performs the ordinary dispatch.
//!
//! # What an entry claims, and why it cannot go stale
//!
//! The memo holds facts of ONE shape, validated on every use:
//!
//! * A ShapeId names one immutable key list, one descriptor state and one
//!   [[Prototype]] (#11342). So "`m` is an own inline data property at slot
//!   `s`" and "`m` is absent from the receiver" are facts of the ShapeId: a
//!   shadowing own property, a descriptor, a delete or a prototype change
//!   re-stamps the receiver and the word stops matching.
//! * An own entry re-loads the slot on every call and compares the value's
//!   code pointer, so a reassigned method (`o.m = other`, no shape change) is
//!   seen at once. The body info, not the closure, is compared: a factory
//!   that returns fresh closures per object shares one body, and the call
//!   passes the LOADED closure, so each object's captures are its own.
//! * An inherited entry holds the method closure itself. The chain it was
//!   found through is made of MARKED prototypes only, and
//!   `PERRY_PROTO_VALIDITY` moves on every structural change of a marked
//!   object AND on every write to an existing slot of one
//!   (`proto_validity::note_marked_value_write`, owner decision D3(b)), so an
//!   unchanged word proves the closure is still the value `m` resolves to.
//!
//! What the prime refuses (they keep the ordinary dispatch): non-ordinary
//! receivers (class objects, native-module namespaces, dictionaries,
//! `Object.prototype`, typed-array prototypes, exotic read receivers),
//! accessors, spill slots, class instances for the inherited entry (their
//! methods live in the vtable until class prototypes carry real slots, D4),
//! and any value that is not a plain closure the call can enter directly for
//! this site's argument count (bound functions, rest / `arguments` bodies,
//! runtime thunks, class constructors, closures that capture `this`).
//!
//! # The one site-memo module (shared)
//!
//! This is THE per-site memo for "the receiver's shape answers this key":
//! method calls use it today, and class accessors (step 3, S4) add their
//! entry kind here rather than growing a second table. The contract every
//! entry kind keeps:
//!
//! * an entry is facts of ONE receiver word (`class_id | ShapeId`), compared
//!   by the emitted code on every use; nothing is keyed on a class id, an
//!   address or a name alone;
//! * the kind lives in the top bits of [`MethodEntry::slot`]: `0` own inline,
//!   bit 63 inherited ([`METHOD_SITE_INHERITED`]), bit 62 own spill
//!   ([`METHOD_SITE_SPILL`]), bit 61 own function-bag
//!   ([`METHOD_SITE_FUNCTION_BAG`]: a function-object receiver whose method is
//!   an inline slot of its own-property object); bit 60 is RESERVED for the
//!   accessor kind. A new kind extends the emitted `msite.other` dispatch and
//!   [`publish`], nothing else;
//! * an entry that holds a heap reference stores it in [`MethodEntry::closure`]
//!   and is registered by [`publish`], so [`scan_method_site_roots_mut`] marks
//!   and rewrites it;
//! * anything the ShapeId does not pin is validated by `PERRY_PROTO_VALIDITY`
//!   ([`MethodEntry::gen`]), which moves on every structural change of a
//!   marked prototype and every write to an existing slot of one
//!   (`proto_validity::note_marked_value_write`,
//!   `proto_validity::store_cache_may_learn`);
//! * primes run after the ordinary dispatch, with collection suppressed.
//!
//! # GC
//!
//! [`MethodEntry::closure`] is a STRONG root: marked, and rewritten when the
//! closure moves (`scan_method_site_roots_mut`). Every site that ever primed an
//! inherited entry is registered once for the scan.
//!
//! # Agents
//!
//! Entries are primed only on the primary agent, like the chain-store
//! verdicts. A program with workers emits thread-local site slots (#10399), so
//! a worker never reads a primary-heap closure through a site.

use crate::object::ObjectHeader;
use std::sync::atomic::{AtomicU64, Ordering};

/// `word` of a site no prime has touched: no receiver word is all-ones.
pub const METHOD_SITE_EMPTY: u64 = u64::MAX;
/// The `slot` bit that marks an inherited entry.
pub const METHOD_SITE_INHERITED: u64 = 1 << 63;
/// The `slot` bit that marks an own entry whose key lives in the receiver's
/// spill buffer (`ObjectMeta::spill`) at the index in the low bits.
pub const METHOD_SITE_SPILL: u64 = 1 << 62;
/// The `slot` bit that marks an own entry of a FUNCTION receiver: the key is
/// inline slot (low bits) of the function's own-property object
/// (`ClosureHeader::props`, `closure/props.rs`). A keyed Function ShapeId is
/// canonical per that object's key list, so the receiver word pins the slot.
pub const METHOD_SITE_FUNCTION_BAG: u64 = crate::codegen_abi::METHOD_SITE_FUNCTION_BAG;
/// The index bits of an entry's `slot` word.
pub const METHOD_SITE_INDEX_MASK: u64 = crate::codegen_abi::METHOD_SITE_INDEX_MASK;

/// One entry of a site's memo. **Field offsets are baked into emitted code**
/// (`perry_abi::METHOD_SITE_*_OFFSET`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MethodEntry {
    /// The receiver's `(class_id | ShapeId << 32)` word.
    pub word: u64,
    /// Own entry: the inline slot. Inherited entry: [`METHOD_SITE_INHERITED`].
    pub slot: u64,
    /// The method body's `JsFunctionInfo` (the identity an own hit compares
    /// the slot closure's info word with).
    pub info: u64,
    /// Inherited entry: the method closure's address (a STRONG GC root).
    pub closure: usize,
    /// Inherited entry: `PERRY_PROTO_VALIDITY` when the entry was primed.
    pub gen: u64,
    /// The method body's code address, the hit's call target.
    pub code: u64,
}

const EMPTY_ENTRY: MethodEntry = MethodEntry {
    word: METHOD_SITE_EMPTY,
    slot: 0,
    info: 0,
    closure: 0,
    gen: 0,
    code: 0,
};

/// Entries per site, all compared by the emitted code (the census: 97.5% of
/// tsc's executed method calls are at one-shape sites, the rest at two).
pub const METHOD_SITE_WAYS: usize = crate::codegen_abi::METHOD_SITE_WAYS;

/// One site's memo: [`METHOD_SITE_WAYS`] entries the emitted code compares in
/// order, then bookkeeping it never reads.
#[repr(C)]
pub struct MethodSite {
    pub entries: [MethodEntry; METHOD_SITE_WAYS],
    /// The entry the next prime replaces when every entry is taken.
    next: u64,
    /// Registered with the root scan.
    registered: u64,
}

// The emitted site runs on 64-bit targets only (`method_site_enabled`).
#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(
        std::mem::offset_of!(crate::closure::ClosureHeader, info)
            == crate::codegen_abi::CLOSURE_INFO_OFFSET
    );
    assert!(
        std::mem::offset_of!(crate::closure::ClosureHeader, props)
            == crate::codegen_abi::CLOSURE_PROPS_OFFSET
    );
    assert!(std::mem::offset_of!(MethodEntry, word) == crate::codegen_abi::METHOD_SITE_WORD_OFFSET);
    assert!(std::mem::offset_of!(MethodEntry, slot) == crate::codegen_abi::METHOD_SITE_SLOT_OFFSET);
    assert!(std::mem::offset_of!(MethodEntry, info) == crate::codegen_abi::METHOD_SITE_INFO_OFFSET);
    assert!(std::mem::offset_of!(MethodEntry, code) == crate::codegen_abi::METHOD_SITE_CODE_OFFSET);
    assert!(
        std::mem::offset_of!(MethodEntry, closure)
            == crate::codegen_abi::METHOD_SITE_CLOSURE_OFFSET
    );
    assert!(std::mem::offset_of!(MethodEntry, gen) == crate::codegen_abi::METHOD_SITE_GEN_OFFSET);
    assert!(std::mem::size_of::<MethodEntry>() == crate::codegen_abi::METHOD_SITE_ENTRY_SIZE);
    assert!(std::mem::offset_of!(MethodSite, entries) == 0);
    assert!(
        std::mem::offset_of!(crate::object::ObjectMeta, spill)
            == crate::codegen_abi::OBJECT_META_SPILL_OFFSET
    );
    assert!(METHOD_SITE_SPILL == crate::codegen_abi::METHOD_SITE_SPILL);
    assert!(
        std::mem::size_of::<crate::array::ArrayHeader>() == crate::codegen_abi::ARRAY_HEADER_SIZE
    );
};

/// The emitted `@perry_ic_N = private global ptr null` for a method site.
pub type MethodSiteSlot = *mut MethodSite;

crate::perry_thread_local! {
    /// Every site that holds (or held) an inherited entry, for the root scan.
    static METHOD_SITES: std::cell::UnsafeCell<Vec<*mut MethodSite>> =
        const { std::cell::UnsafeCell::new(Vec::new()) };
}

/// Set when the first `perry/thread` worker agent starts. Site memos are
/// process-global and an inherited entry holds a primary-heap closure, so from
/// then on no inherited entry is primed and every existing one is dead: the
/// same call bumps `PERRY_PROTO_VALIDITY`, which no entry primed earlier can
/// match again. Own entries hold no heap reference (ShapeIds are
/// process-unique; the call passes the receiver's own closure).
static WORKER_AGENTS_EXIST: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Called by `agent::enter_worker_agent` before the worker runs any code.
pub fn note_worker_agent() {
    if !WORKER_AGENTS_EXIST.swap(true, Ordering::SeqCst) {
        super::proto_validity::bump_proto_validity();
    }
}

/// Why a miss did not prime (diagnostic; `PERRY_METHOD_SITE_STATS` prints it).
const REFUSALS: [&str; 19] = [
    "not_object_pointer",
    "not_ordinary",
    "dictionary",
    "own_spill_slot",
    "own_accessor",
    "own_not_direct_callable",
    "inh_class_instance",
    "inh_proto_not_in_shape",
    "inh_hop_refused",
    "inh_not_found",
    "inh_not_direct_callable",
    "inh_workers",
    "dc_not_closure",
    "dc_special",
    "dc_rest",
    "dc_captures_this",
    "dc_arity_pad",
    "dc_bound",
    "site_megamorphic",
];
per_test_global! {
    static SITE_REFUSED: [AtomicU64; 19] = [const { AtomicU64::new(0) }; 19];
}
#[inline]
fn refuse(reason: usize) {
    SITE_REFUSED[reason].fetch_add(1, Ordering::Relaxed);
}

per_test_global! {
    static PRIMES_OWN: AtomicU64 = AtomicU64::new(0);
    static PRIMES_INHERITED: AtomicU64 = AtomicU64::new(0);
    static MISSES: AtomicU64 = AtomicU64::new(0);
    static PRIMES_FUNCTION: AtomicU64 = AtomicU64::new(0);
}

/// Function-bag entries primed ([`METHOD_SITE_FUNCTION_BAG`]).
pub fn method_site_function_primes() -> u64 {
    PRIMES_FUNCTION.load(Ordering::Relaxed)
}

/// Test/diagnostic counters: (own primes, inherited primes, misses).
pub fn method_site_stats() -> (u64, u64, u64) {
    (
        PRIMES_OWN.load(Ordering::Relaxed),
        PRIMES_INHERITED.load(Ordering::Relaxed),
        MISSES.load(Ordering::Relaxed),
    )
}

/// `js_method_site_stats(which)`: 0 own primes, 1 inherited primes, 2 misses,
/// 3 function-bag primes.
/// Exposed so gap tests can prove a path ran.
#[no_mangle]
pub extern "C" fn js_method_site_stats(which: i32) -> f64 {
    let (a, b, c) = method_site_stats();
    (match which {
        0 => a,
        1 => b,
        3 => method_site_function_primes(),
        _ => c,
    }) as f64
}

fn stats_report_enabled() -> bool {
    per_test_global! {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    }
    *ON.get_or_init(|| {
        let on = std::env::var_os("PERRY_METHOD_SITE_STATS").is_some();
        if on {
            extern "C" fn report() {
                let (a, b, c) = method_site_stats();
                let mut refused = String::new();
                for (i, n) in SITE_REFUSED.iter().enumerate() {
                    let n = n.load(Ordering::Relaxed);
                    if n != 0 {
                        refused.push_str(&format!(" refused.{}={n}", REFUSALS[i]));
                    }
                }
                eprintln!(
                    "[method-site] primes_own={a} primes_inherited={b} primes_function={} misses={c} marked_value_write_bumps={}{refused}",
                    method_site_function_primes(),
                    crate::object::proto_validity::marked_value_write_bumps()
                );
            }
            unsafe { libc::atexit(report) };
        }
        on
    })
}

/// The miss entry: prime the site when the facts hold, then dispatch as the
/// universal method dispatcher always has.
///
/// # Safety
/// `slot` is null or a live method-site slot; `args_ptr` holds `argc` values.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_method_site_miss(
    slot: *mut MethodSiteSlot,
    site_id: u64,
    recv: f64,
    method_id: i64,
    args_ptr: *const f64,
    argc: usize,
) -> f64 {
    let _ = stats_report_enabled();
    MISSES.fetch_add(1, Ordering::Relaxed);
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(name_ref) = crate::string::perry_string_ref_from_dispatch_id(method_id, &mut scratch)
    else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    // Only an ordinary heap object can prime. Everything else (primitives,
    // handles, functions, arrays) dispatches with no extra work at all.
    let megamorphic = site_is_megamorphic(slot);
    if megamorphic || !prime_candidate(recv, std::slice::from_raw_parts(name_ref.ptr, name_ref.len))
    {
        refuse(if megamorphic { 18 } else { 1 });
        return crate::typed_feedback::js_typed_feedback_native_call_method(
            site_id,
            recv,
            name_ref.ptr as *const i8,
            name_ref.len,
            args_ptr,
            argc,
        );
    }
    // Dispatch first, then prime: the prime may allocate (marking a
    // prototype hop, the borrowed-builtin classifier's key), which can move
    // the receiver and the arguments the dispatcher still needs. The
    // receiver and the result are rooted across the prime.
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_h = scope.root_nanbox_f64(recv);
    let result = crate::typed_feedback::js_typed_feedback_native_call_method(
        site_id,
        recv,
        name_ref.ptr as *const i8,
        name_ref.len,
        args_ptr,
        argc,
    );
    let result_h = scope.root_nanbox_f64(result);
    let name = std::slice::from_raw_parts(name_ref.ptr, name_ref.len);
    {
        // The prime reads the receiver, its holder chain and the method value
        // as raw addresses and may allocate (a prototype mark, the
        // borrowed-builtin classifier key, a lazily built intrinsic), so no
        // collection may move anything until it has published its entry.
        let _no_move = crate::gc::GcSuppressScope::new();
        prime(slot, recv_h.get_nanbox_f64(), name, argc);
    }
    result_h.get_nanbox_f64()
}

/// A cheap first cut of [`ordinary_receiver`] / [`prime_function`]: a heap
/// pointer whose GcHeader says ordinary object, or a function object whose
/// SHAPE lists `name` as an own key. Most calls on functions (`fn.bind`,
/// `fn.call`) name an inherited builtin a site never memoizes; they leave
/// here on the shape's key list, before the miss roots anything.
#[inline]
fn prime_candidate(recv: f64, name: &[u8]) -> bool {
    let bits = recv.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return false;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_above_handle_band(addr) {
        return false;
    }
    match unsafe { crate::value::addr_class::try_read_gc_header(addr) } {
        Some(h) if h.obj_type == crate::gc::GC_TYPE_OBJECT => true,
        Some(h) if h.obj_type == crate::gc::GC_TYPE_CLOSURE => unsafe {
            function_shape_lists_key(addr, name)
        },
        _ => false,
    }
}

/// Does the (claimed) function object at `addr` sit on a KEYED Function shape
/// whose key list holds `name`? Reads the ShapeId word and the shape's key
/// list only; [`prime_function`] re-proves ownership before trusting it.
#[inline]
unsafe fn function_shape_lists_key(addr: usize, name: &[u8]) -> bool {
    let id = *((addr as *const u8).add(crate::closure::CLOSURE_SHAPE_OFFSET) as *const u32);
    if !super::shapes::is_exotic_shape_id(id)
        || id == crate::closure::shape::function_dictionary_shape()
    {
        return false;
    }
    // The record in place (no descriptor copy): its key list and count.
    let Some(record) = super::shapes::shape_record_by_id(id) else {
        return false;
    };
    let keys = record.keys() as usize as *const crate::array::ArrayHeader;
    !keys.is_null()
        && super::keys_find_slot_by_bytes_resolved(keys, record.logical_key_count(), name).is_some()
}

unsafe fn site_of(slot: *mut MethodSiteSlot) -> *mut MethodSite {
    crate::object::pic_slot_resolve_init(slot, |fresh| {
        // GC_STORE_AUDIT(INIT): a fresh site record in the IC arena; its only
        // heap references (inherited closures) are written by `publish` and
        // scanned as strong roots.
        std::ptr::write(
            fresh,
            MethodSite {
                entries: [EMPTY_ENTRY; METHOD_SITE_WAYS],
                next: 0,
                registered: 0,
            },
        );
    })
}

/// Evictions after which a site stops priming: it has more (shape, body)
/// pairs than ways, and re-priming on every miss would cost more than the
/// dispatcher alone.
const METHOD_SITE_MAX_EVICTIONS: u64 = 16;

/// Has `slot`'s site given up priming ([`METHOD_SITE_MAX_EVICTIONS`])?
#[inline]
unsafe fn site_is_megamorphic(slot: *mut MethodSiteSlot) -> bool {
    let site = crate::object::pic_slot_peek(slot);
    !site.is_null() && (*site).next >= METHOD_SITE_MAX_EVICTIONS
}

/// Publish `entry` into `slot`'s site: over the entry that already names the
/// receiver word, else into an empty one, else over the next in turn. The
/// word is written LAST, so a half-written entry never matches.
unsafe fn publish(slot: *mut MethodSiteSlot, entry: MethodEntry) -> bool {
    let site = site_of(slot);
    if site.is_null() {
        return false;
    }
    let site = &mut *site;
    // An inherited entry replaces the one for its word (a newer generation);
    // an own entry replaces only the one naming the same slot AND body, so
    // objects of one shape holding different bodies each get an entry (the
    // emitted own hit falls through to the next way on a body mismatch).
    let inherited = entry.slot == METHOD_SITE_INHERITED;
    let idx = site
        .entries
        .iter()
        .position(|e| {
            e.word == entry.word
                && if inherited {
                    e.slot == METHOD_SITE_INHERITED
                } else {
                    e.slot == entry.slot && e.info == entry.info
                }
        })
        .or_else(|| {
            site.entries
                .iter()
                .position(|e| e.word == METHOD_SITE_EMPTY)
        })
        .unwrap_or_else(|| {
            let i = (site.next as usize) % METHOD_SITE_WAYS;
            site.next = site.next.wrapping_add(1);
            i
        });
    if entry.closure != 0 && site.registered == 0 {
        site.registered = 1;
        let ptr = site as *mut MethodSite;
        METHOD_SITES.with(|cell| (*cell.get()).push(ptr));
    }
    let e = &mut site.entries[idx];
    e.word = METHOD_SITE_EMPTY;
    e.slot = entry.slot;
    e.info = entry.info;
    e.closure = entry.closure;
    e.gen = entry.gen;
    e.code = entry.code;
    e.word = entry.word;
    true
}

fn name_refused(name: &[u8]) -> bool {
    name.is_empty()
        || name[0] == b'#'
        || name.starts_with(b"__perry_")
        || name.starts_with(b"@@")
        || name == b"constructor"
}

/// Prime `slot` for `recv.name(...)` with `argc` arguments, or leave it.
unsafe fn prime(slot: *mut MethodSiteSlot, recv: f64, name: &[u8], argc: usize) {
    if slot.is_null()
        || name_refused(name)
        || crate::agent::current_agent() != crate::agent::PRIMARY_AGENT
    {
        return;
    }
    let bits = recv.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        refuse(0);
        return;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    if crate::value::addr_class::is_above_handle_band(addr) && crate::closure::is_closure_ptr(addr)
    {
        prime_function(slot, addr, name, argc);
        return;
    }
    let Some(obj) = ordinary_receiver(addr) else {
        let dict = crate::value::addr_class::try_read_gc_header(addr)
            .is_some_and(|h| h.obj_type == crate::gc::GC_TYPE_OBJECT)
            && !crate::closure::is_closure_ptr(addr)
            && super::dictionary::is_dictionary(addr as *const ObjectHeader);
        refuse(if dict { 2 } else { 1 });
        return;
    };
    let Some(shape) = super::shapes::object_shape_descriptor(obj) else {
        refuse(1);
        return;
    };
    let word = std::ptr::read(addr as *const u64);
    let keys = shape.keys as usize as *const crate::array::ArrayHeader;
    let own = if keys.is_null() {
        None
    } else {
        super::keys_find_slot_by_bytes_resolved(keys, shape.logical_key_count, name)
    };
    if let Some(s) = own {
        // An own key: an inline DATA property holding a directly callable
        // closure. A tombstone (`TAG_HOLE`) is not a closure and refuses.
        if key_may_be_accessor(obj, name) {
            refuse(4);
            return;
        }
        // Where the value lives follows the by-name read's rule: below
        // `max(live slots, INLINE_SLOT_FLOOR)` it is inline, above it in the
        // spill buffer. The gap between the shape's live inline count and
        // the floor is refused rather than guessed.
        let spill_from = shape
            .live_inline_slot_count
            .max(super::INLINE_SLOT_FLOOR as u32);
        let (value, slot_word) = if s < shape.live_inline_slot_count {
            (field_bits(addr, s), s as u64)
        } else if s < spill_from {
            refuse(3);
            return;
        } else {
            // A spill-located key: the emitted hit reads `meta.spill[s]`,
            // bounds-checked against the buffer's length. Only the object-owned
            // spill buffer is addressable; the legacy side table is not.
            match spill_bits(obj, s) {
                Some(bits) => (bits, s as u64 | METHOD_SITE_SPILL),
                None => {
                    refuse(3);
                    return;
                }
            }
        };
        let Some(info) = direct_callable(value, argc) else {
            refuse(5);
            return;
        };
        if !is_user_method(value, name) {
            refuse(13);
            return;
        }
        let entry = MethodEntry {
            word,
            slot: slot_word,
            info: info as *const crate::closure::JsFunctionInfo as u64,
            code: info.code as u64,
            closure: 0,
            gen: 0,
        };
        if publish(slot, entry) {
            PRIMES_OWN.fetch_add(1, Ordering::Relaxed);
        }
        return;
    }
    prime_inherited(slot, obj, word, name, argc);
}

/// Prime a function-bag entry: `recv` is a function object on a KEYED
/// Function shape (its own non-intrinsic properties are exactly the key list
/// of its own-property object, `closure/props.rs`; no accessor, no delete, no
/// recorded prototype — any of those makes it FunctionDictionary, which is
/// refused), and `name` is an inline data slot of that object holding a plain
/// closure. The receiver word (`capture_count | ShapeId`) pins the slot; the
/// emitted hit re-loads the object and the value on every call and compares
/// the value's kind and code pointer, exactly as for an ordinary own entry.
unsafe fn prime_function(slot: *mut MethodSiteSlot, addr: usize, name: &[u8], argc: usize) {
    if !address_is_prime_stable(addr) {
        refuse(1);
        return;
    }
    let closure = addr as *const crate::closure::ClosureHeader;
    let id = (*closure).shape_id;
    if id == crate::closure::shape::function_dictionary_shape()
        || super::shapes::shape_object_kind_by_id(id)
            != Some(super::shapes::ShapeObjectKind::Function)
    {
        refuse(2);
        return;
    }
    let Some(shape) = super::shapes::shape_descriptor_by_id(id) else {
        refuse(1);
        return;
    };
    let keys = shape.keys as usize as *const crate::array::ArrayHeader;
    let bag = crate::closure::props::bag_of(addr);
    if keys.is_null() || bag.is_null() {
        // A base Function shape: no own non-intrinsic key to serve.
        refuse(1);
        return;
    }
    let Some(s) = super::keys_find_slot_by_bytes_resolved(keys, shape.logical_key_count, name)
    else {
        refuse(1);
        return;
    };
    if s >= shape.live_inline_slot_count {
        refuse(3);
        return;
    }
    let value = field_bits(bag as usize, s);
    let Some(info) = direct_callable(value, argc) else {
        refuse(5);
        return;
    };
    if !is_user_method(value, name) {
        refuse(13);
        return;
    }
    let entry = MethodEntry {
        word: std::ptr::read(addr as *const u64),
        slot: s as u64 | METHOD_SITE_FUNCTION_BAG,
        info: info as *const crate::closure::JsFunctionInfo as u64,
        code: info.code as u64,
        closure: 0,
        gen: 0,
    };
    if publish(slot, entry) {
        PRIMES_FUNCTION.fetch_add(1, Ordering::Relaxed);
    }
}

/// The receiver, if it is an ordinary object a site may learn.
unsafe fn ordinary_receiver(addr: usize) -> Option<*const ObjectHeader> {
    if !crate::value::addr_class::is_above_handle_band(addr) {
        return None;
    }
    let header = crate::value::addr_class::try_read_gc_header(addr)?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || crate::closure::is_closure_ptr(addr)
        || !address_is_prime_stable(addr)
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
    {
        return None;
    }
    let obj = addr as *const ObjectHeader;
    if !super::object_is_regular(obj)
        || super::dictionary::is_dictionary(obj)
        || (*obj).class_id == super::native_module::NATIVE_MODULE_CLASS_ID
        || super::class_registry::is_class_object_ptr(obj.cast())
        || crate::array::object_prototype_addr_matches(addr)
        || ((*obj).class_id == 0 && crate::url::is_url_object_shape(obj as *mut ObjectHeader))
    {
        return None;
    }
    let stamp = super::shapes::object_shape_stamp(obj);
    if !super::shapes::is_shape_id(stamp) {
        return None;
    }
    let meta = (*obj).meta;
    if !meta.is_null()
        && ((*meta).elements != 0
            || (*meta).flags & super::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER != 0)
    {
        return None;
    }
    Some(obj)
}

fn address_is_prime_stable(addr: usize) -> bool {
    crate::value::addr_class::is_plausible_heap_addr(addr)
        && crate::arena::classify_heap_generation(addr) != crate::arena::HeapGeneration::Unknown
}

/// Could `name` be an accessor (or a customized descriptor) on `obj`? The
/// authoritative answer is the object's descriptor state, consulted the way
/// the by-name read does; a clear Bloom bit in the meta record short-cuts it.
/// Descriptor installs re-stamp the ShapeId (#10824), so a prime-time answer
/// holds for every carrier of the shape.
unsafe fn key_may_be_accessor(obj: *const ObjectHeader, name: &[u8]) -> bool {
    let meta = (*obj).meta;
    if !meta.is_null() {
        let bit = 1u64 << (super::key_bytes_hash(name.as_ptr(), name.len()) & 63);
        if (*meta).accessor_key_bits & bit != 0 || (*meta).attr_key_bits & bit != 0 {
            return true;
        }
    }
    if super::descriptor_state::object_has_descriptors(obj as usize) {
        let Ok(name) = std::str::from_utf8(name) else {
            return true;
        };
        if super::descriptor_state::get_accessor_descriptor(obj as usize, name).is_some() {
            return true;
        }
    }
    false
}

/// The value of spill-located key `index` as the emitted hit will read it:
/// through `ObjectMeta::spill`, a dense buffer the runtime never shifts.
unsafe fn spill_bits(obj: *const ObjectHeader, index: u32) -> Option<u64> {
    if !super::spill::object_spill_enabled() {
        return None;
    }
    let meta = (*obj).meta;
    if meta.is_null() || (*meta).spill == 0 {
        return None;
    }
    let spill = (*meta).spill as usize as *const crate::array::ArrayHeader;
    if index >= (*spill).length || crate::array::array_front_offset(spill) != 0 {
        return None;
    }
    Some(std::ptr::read(
        (spill as *const u8).add(crate::codegen_abi::ARRAY_HEADER_SIZE + index as usize * 8)
            as *const u64,
    ))
}

#[inline]
unsafe fn field_bits(addr: usize, slot: u32) -> u64 {
    std::ptr::read(
        (addr as *const u8).add(std::mem::size_of::<ObjectHeader>() + slot as usize * 8)
            as *const u64,
    )
}

/// A borrowed builtin (`o.get = Map.prototype.get`) keeps the dispatcher's
/// native arm, exactly as `own_override::resolve_own_user_method` decides.
fn is_user_method(value_bits: u64, name: &[u8]) -> bool {
    match std::str::from_utf8(name) {
        Ok(name) => crate::array::value_is_own_user_method(f64::from_bits(value_bits), name),
        Err(_) => false,
    }
}

/// The body info whose code a site may call for `value` with `argc`
/// arguments, when the call `js_native_call_value(value, args)` would reach
/// `code(closure, this, args...)` with nothing in between.
unsafe fn direct_callable(
    value_bits: u64,
    argc: usize,
) -> Option<&'static crate::closure::JsFunctionInfo> {
    if value_bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        refuse(12);
        return None;
    }
    let addr = (value_bits & crate::value::POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(addr) || !address_is_prime_stable(addr) {
        refuse(12);
        return None;
    }
    let value = f64::from_bits(value_bits);
    let header = addr as *const crate::closure::ClosureHeader;
    // The cell is proven a live function object above; a bodiless one has a
    // null info.
    let Some(info) = (*header).info.as_ref() else {
        refuse(17);
        return None;
    };
    let func = info.code;
    if func.is_null()
        || func == crate::closure::BOUND_METHOD_FUNC_PTR
        || func == crate::closure::BOUND_FUNCTION_FUNC_PTR
    {
        refuse(17);
        return None;
    }
    if func == super::global_this::global_this_builtin_noop_thunk as *const u8
        || func == super::global_this::global_this_array_thunk as *const u8
        || super::class_registry::is_class_object_value(value)
        || super::global_this::is_function_prototype_object_value(value)
        || super::native_module::bound_native_callable_module_and_method(value).is_some()
    {
        refuse(13);
        return None;
    }
    if crate::closure::info_rest(info).is_some() {
        refuse(14);
        return None;
    }
    // A body that keeps `this` in its last capture is re-bound by cloning
    // (`clone_closure_rebind_this`); the direct call cannot do that.
    let raw_count = (*header).capture_count;
    if raw_count & crate::closure::CAPTURES_THIS_FLAG != 0
        && raw_count & crate::closure::NO_THIS_REBIND_FLAG == 0
        && !crate::closure::closure_is_arrow(header)
    {
        refuse(15);
        return None;
    }
    match crate::closure::resolve_strategy(info).kind() {
        crate::closure::DispatchKind::Arity(declared)
            if declared as usize <= crate::codegen_abi::method_site_padded_argc(argc) =>
        {
            Some(info)
        }
        crate::closure::DispatchKind::Arity(_) => {
            refuse(16);
            None
        }
        crate::closure::DispatchKind::Rest(..) => {
            refuse(14);
            None
        }
        _ => {
            refuse(17);
            None
        }
    }
}

/// Prime an inherited entry: `name` is absent from the receiver and found as
/// a plain inline data slot on a chain of MARKED prototypes.
unsafe fn prime_inherited(
    slot: *mut MethodSiteSlot,
    obj: *const ObjectHeader,
    word: u64,
    name: &[u8],
    argc: usize,
) {
    if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) {
        refuse(11);
        return;
    }
    // Class instances resolve methods through their vtable (D4: until class
    // prototypes carry real slots).
    let class_id = (*obj).class_id;
    if class_id != 0
        && class_id < super::class_registry::prototype_objects::SYNTHETIC_CLASS_ID_BASE
        && !super::is_anon_shape_class_id(class_id)
    {
        refuse(6);
        return;
    }
    if key_may_be_accessor(obj, name) {
        refuse(8);
        return;
    }
    // The ShapeId must name the prototype this walk follows (#11342).
    let stamp = super::shapes::object_shape_stamp(obj);
    if super::shapes::shape_proto_id(stamp) != Some(super::shapes::object_proto_id(obj)) {
        refuse(7);
        return;
    }
    // Read BEFORE the walk: a change during it can only make the entry older.
    let gen = super::proto_validity::proto_validity();
    let mut current = obj;
    for _hop in 0..4 {
        let next = next_prototype(current);
        if next.is_null() || next == current || next == obj {
            refuse(9);
            return;
        }
        let next_addr = next as usize;
        if !crate::value::addr_class::is_above_handle_band(next_addr)
            || !address_is_prime_stable(next_addr)
        {
            refuse(8);
            return;
        }
        let Some(header) = crate::value::addr_class::try_read_gc_header(next_addr) else {
            refuse(8);
            return;
        };
        if header.obj_type != crate::gc::GC_TYPE_OBJECT
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
            || super::dictionary::is_dictionary(next)
        {
            refuse(8);
            return;
        }
        let Some(shape) = super::shapes::object_shape_descriptor(next) else {
            refuse(8);
            return;
        };
        if shape.object_kind != super::shapes::ShapeObjectKind::Ordinary
            || super::shapes::object_shape_stamp(next) == 0
        {
            refuse(8);
            return;
        }
        let meta = (*next).meta;
        if meta.is_null() || (*meta).flags & super::OBJECT_META_FLAG_IS_PROTOTYPE == 0 {
            // Only a marked hop invalidates an entry recorded through it.
            // Marking allocates (the meta record), so nothing here may be
            // touched afterwards: mark and abandon; the next miss primes.
            let _ = super::proto_validity::mark_object_as_prototype(next_addr);
            refuse(8);
            return;
        }
        if (*meta).elements != 0
            || (*meta).flags & super::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER != 0
            || key_may_be_accessor(next, name)
        {
            refuse(8);
            return;
        }
        let keys = shape.keys as usize as *const crate::array::ArrayHeader;
        if !keys.is_null() {
            if let Some(s) =
                super::keys_find_slot_by_bytes_resolved(keys, shape.logical_key_count, name)
            {
                if s >= shape.live_inline_slot_count {
                    refuse(8);
                    return;
                }
                let value = field_bits(next_addr, s);
                let Some(info) = direct_callable(value, argc) else {
                    refuse(10);
                    return;
                };
                if !is_user_method(value, name) {
                    refuse(13);
                    return;
                }
                let entry = MethodEntry {
                    word,
                    slot: METHOD_SITE_INHERITED,
                    info: info as *const crate::closure::JsFunctionInfo as u64,
                    code: info.code as u64,
                    closure: (value & crate::value::POINTER_MASK) as usize,
                    gen,
                };
                if publish(slot, entry) {
                    PRIMES_INHERITED.fetch_add(1, Ordering::Relaxed);
                }
                return;
            }
        }
        current = next;
    }
    refuse(9);
}

/// The next prototype the way the inherited-read walk resolves it: the meta
/// record's `[[Prototype]]`, else a synthetic class's (`Object.create`, an ES5
/// constructor) registered prototype. Null for a default builtin prototype.
unsafe fn next_prototype(obj: *const ObjectHeader) -> *const ObjectHeader {
    let meta = (*obj).meta;
    if !meta.is_null() && (*meta).prototype != 0 {
        let p = crate::value::JSValue::from_bits((*meta).prototype);
        if !p.is_pointer() {
            return std::ptr::null();
        }
        return p.as_pointer();
    }
    let class_id = (*obj).class_id;
    let synthetic = class_id >= 0x8000_0000
        && class_id < super::NEXT_SYNTHETIC_CLASS_ID.load(std::sync::atomic::Ordering::Relaxed);
    if !synthetic || !super::class_decl_prototype_object(class_id).is_null() {
        return std::ptr::null();
    }
    super::class_prototype_object(class_id)
}

/// Root scan: every inherited entry's closure is marked and rewritten.
pub(crate) fn scan_method_site_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    METHOD_SITES.with(|cell| unsafe {
        for &site in (*cell.get()).iter() {
            for e in (*site).entries.iter_mut() {
                if e.closure != 0 {
                    visitor.visit_tagged_usize_slot(&mut e.closure, crate::value::POINTER_TAG);
                }
            }
        }
    });
}
