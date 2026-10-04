//! Method-site entries for `fn.bind(…)` / `fn.call(…)` / `fn.apply(…)`: a
//! function receiver whose key is inherited from `%Function.prototype%`.
//!
//! The receiver's word (`capture_count | ShapeId`) names a Function shape, and
//! a Function shape pins its own keys and its prototype. So "the receiver
//! lacks `bind` and inherits it from `%Function.prototype%`" is a fact of the
//! word, and "`%Function.prototype%` holds `bind` as a data property at slot
//! `s`" is a fact of the holder's word. The entry records both words, the
//! slot, and the `JsFunctionInfo` of the intrinsic the slot held when primed.
//!
//! The emitted site never answers this kind: its tag
//! ([`METHOD_SITE_FUNCTION_INTRINSIC`]) is none of the ones the emitted
//! dispatch decodes, so a word match falls to `js_method_site_miss`, which
//! answers from the entry before anything is spelled: compare the holder's
//! word, load the slot, compare the loaded closure's info with the entry's,
//! and run the intrinsic. A patched slot (`Function.prototype.bind = f`), an
//! own `bind` on the receiver (a new ShapeId) or a redefined holder key (a new
//! holder ShapeId) fails one of those compares, and the call takes the full
//! dispatch. The method NAME is read only when the entry is primed.

use super::{
    publish, site_is_megamorphic, MethodEntry, MethodSiteSlot, METHOD_SITE_EMPTY,
    METHOD_SITE_INDEX_MASK, WORKER_AGENTS_EXIST,
};
use std::sync::atomic::Ordering;

/// The `slot` tag of a function-intrinsic entry: bits 60 and 59, a pattern the
/// emitted dispatch routes to its miss (only `0`, bit 59 alone, bits 63+59,
/// bit 63, bit 62 and bit 61 are hits there).
pub const METHOD_SITE_FUNCTION_INTRINSIC: u64 = (1 << 60) | (1 << 59);
const KIND_MASK: u64 = !METHOD_SITE_INDEX_MASK;

per_test_global! {
    static PRIMES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static NEGATIVE_PRIMES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static HITS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
}

/// Diagnostic counters: (entries primed, negative entries primed, calls
/// answered). `PERRY_METHOD_SITE_STATS` prints them.
pub(crate) fn function_intrinsic_stats() -> (u64, u64, u64) {
    (
        PRIMES.load(Ordering::Relaxed),
        NEGATIVE_PRIMES.load(Ordering::Relaxed),
        HITS.load(Ordering::Relaxed),
    )
}

const _: () = assert!(METHOD_SITE_FUNCTION_INTRINSIC & METHOD_SITE_INDEX_MASK == 0);

/// The receiver's word when it is a heap function object, else `None`.
#[inline]
unsafe fn function_receiver_word(recv: f64) -> Option<u64> {
    let bits = recv.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_above_handle_band(addr) {
        return None;
    }
    match crate::value::addr_class::try_read_gc_header(addr) {
        Some(h) if h.obj_type == crate::gc::GC_TYPE_CLOSURE => {
            Some(std::ptr::read(addr as *const u64))
        }
        _ => None,
    }
}

/// What a site's function-intrinsic entries say about a call.
pub(super) enum Lookup {
    /// The entry answered the call: its result.
    Answered(f64),
    /// The site already knows this receiver word: either a NEGATIVE entry
    /// (the key is not an inherited intrinsic, primed once so the name is
    /// not looked up again) or a positive one whose compares failed.
    Known,
    /// No entry for this receiver word: the miss may prime one.
    Unknown,
}

/// Answer `recv.<name>(args)` from a function-intrinsic entry of `slot`'s
/// site (module docs). `word` is the receiver's word
/// ([`served_receiver_word`]).
///
/// # Safety
/// `slot` is null or a live method-site slot; `args_ptr` holds `argc` values.
#[inline]
unsafe fn lookup(
    slot: *mut MethodSiteSlot,
    word: u64,
    recv: f64,
    args_ptr: *const f64,
    argc: usize,
) -> Lookup {
    if WORKER_AGENTS_EXIST.load(Ordering::Relaxed) != 0 {
        return Lookup::Known;
    }
    // A site that never primed has no entry to consult.
    let site = crate::object::pic_slot_peek(slot);
    if site.is_null() {
        return Lookup::Unknown;
    }
    let Some(entry) = (*site)
        .entries
        .iter()
        .find(|e| e.word == word && e.slot & KIND_MASK == METHOD_SITE_FUNCTION_INTRINSIC)
    else {
        return Lookup::Unknown;
    };
    let holder = entry.closure;
    if entry.info == 0 || holder == 0 || std::ptr::read(holder as *const u64) != entry.gen {
        return Lookup::Known;
    }
    // The holder's word pins its key list, so the index names the key; the
    // read follows the shape's placement (inline or spill).
    let index = (entry.slot & METHOD_SITE_INDEX_MASK) as u32;
    let value =
        crate::object::js_object_get_field(holder as *const crate::object::ObjectHeader, index)
            .bits();
    if value & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return Lookup::Known;
    }
    let method = (value & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
    if !crate::closure::is_closure_ptr(method as usize) || (*method).info as u64 != entry.info {
        return Lookup::Known;
    }
    match crate::object::native_call_method::call_function_intrinsic(recv, method, args_ptr, argc) {
        Some(result) => {
            HITS.fetch_add(1, Ordering::Relaxed);
            Lookup::Answered(result)
        }
        None => Lookup::Known,
    }
}

/// Is a miss on `recv.<name>(…)` worth the rooted dispatch-then-prime: the
/// receiver (`word`, a [`served_receiver_word`]) does not own `name`, so it
/// inherits it from `%Function.prototype%`. The shape's cached verdict only;
/// a receiver this refuses is never primed.
#[inline]
fn worth_priming(word: u64, name: &[u8]) -> bool {
    crate::closure::shape::function_shape_inherits_from_function_prototype(
        (word >> 32) as u32,
        name,
    )
}

/// Prime a function-intrinsic entry for `recv.<name>(…)` when the shape facts
/// hold (`function_intrinsic_facts`). Runs after the dispatch, with
/// collection suppressed by the caller.
///
/// # Safety
/// `slot` is null or a live method-site slot; `recv` is live.
pub(super) unsafe fn prime(slot: *mut MethodSiteSlot, recv: f64, name: &[u8]) {
    if slot.is_null()
        || site_is_megamorphic(slot)
        || crate::agent::current_agent() != crate::agent::PRIMARY_AGENT
        || WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0
    {
        return;
    }
    let Some(word) = function_receiver_word(recv) else {
        return;
    };
    if word == METHOD_SITE_EMPTY {
        return;
    }
    // A key that is not an inherited intrinsic primes a NEGATIVE entry (no
    // holder, info 0): the site then dispatches this word without asking
    // the name again; so does a receiver word the facts refuse. Before the
    // realm has built `%Function.prototype%` nothing is primed.
    use crate::object::native_call_method::FunctionIntrinsicFacts;
    let entry = match crate::object::native_call_method::function_intrinsic_facts(recv, name) {
        FunctionIntrinsicFacts::Intrinsic(holder, index, info) if index as u64 & KIND_MASK == 0 => {
            MethodEntry {
                word,
                slot: index as u64 | METHOD_SITE_FUNCTION_INTRINSIC,
                info,
                code: 0,
                closure: holder,
                gen: std::ptr::read(holder as *const u64),
            }
        }
        FunctionIntrinsicFacts::NoPrototype => return,
        _ => MethodEntry {
            word,
            slot: METHOD_SITE_FUNCTION_INTRINSIC,
            info: 0,
            code: 0,
            closure: 0,
            gen: 0,
        },
    };
    let negative = entry.info == 0;
    if publish(slot, entry) {
        if negative {
            NEGATIVE_PRIMES.fetch_add(1, Ordering::Relaxed);
        } else {
            PRIMES.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// The miss handler's function-intrinsic step for `recv.<name>(args)` on a
/// function receiver whose ShapeId says it inherits from
/// `%Function.prototype%` (`word` is its word), in a realm that has built it:
/// answer from the site's entry, or dispatch and prime one. `None` leaves the
/// call to the ordinary miss path unchanged.
///
/// # Safety
/// As `js_method_site_miss`.
#[inline]
pub(super) unsafe fn on_miss(
    slot: *mut MethodSiteSlot,
    site_id: u64,
    recv: f64,
    word: u64,
    name: &[u8],
    args_ptr: *const f64,
    argc: usize,
) -> Option<f64> {
    match lookup(slot, word, recv, args_ptr, argc) {
        Lookup::Answered(result) => Some(result),
        Lookup::Known => None,
        Lookup::Unknown => dispatch_and_prime(slot, site_id, recv, word, name, args_ptr, argc),
    }
}

#[inline(never)]
unsafe fn dispatch_and_prime(
    slot: *mut MethodSiteSlot,
    site_id: u64,
    recv: f64,
    word: u64,
    name: &[u8],
    args_ptr: *const f64,
    argc: usize,
) -> Option<f64> {
    if site_is_megamorphic(slot) || !worth_priming(word, name) {
        return None;
    }
    // Dispatch first, then prime, as the ordinary miss does: the receiver
    // and the result are rooted across the prime, which runs with
    // collection suppressed.
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_h = scope.root_nanbox_f64(recv);
    let result = crate::typed_feedback::js_typed_feedback_native_call_method(
        site_id,
        recv,
        name.as_ptr() as *const i8,
        name.len(),
        args_ptr,
        argc,
    );
    let result_h = scope.root_nanbox_f64(result);
    {
        let _no_move = crate::gc::GcSuppressScope::new();
        prime(slot, recv_h.get_nanbox_f64(), name);
    }
    Some(result_h.get_nanbox_f64())
}
