//! Method-site entries for `fn.bind(…)` / `fn.call(…)` / `fn.apply(…)`: a
//! function receiver whose key is inherited from `%Function.prototype%`.
//!
//! The receiver's ShapeId names a Function shape, and a Function shape pins
//! its own keys and its prototype. So "the receiver lacks `call` and inherits
//! it from `%Function.prototype%`" is a fact of the ShapeId, and
//! "`%Function.prototype%` holds `call` as a data property at slot `s`" is a
//! fact of the holder's word. An entry records:
//!
//! * `word`: the receiver's ShapeId half, with the capture-count half zero,
//!   so every closure on one Function shape shares one entry;
//! * `closure` / `gen`: the holder and its word; `slot`: the key's index;
//! * `info`: the `JsFunctionInfo` of the intrinsic the slot held when primed;
//! * `code`: which intrinsic that is ([`FN_CALL`], [`FN_APPLY`], [`FN_BIND`])
//!   when it is the one the site's name names, else 0
//!   (`Function.prototype.call = Function.prototype.apply` primes 0).
//!
//! The emitted site never answers this kind: its tag
//! ([`METHOD_SITE_FUNCTION_INTRINSIC`]) is none of the ones the emitted
//! dispatch decodes, so a match falls to `js_method_site_miss`. The miss's
//! FIRST step ([`fast_miss`]), before it spells the method name or classifies
//! anything else, finds the receiver's entry by its ShapeId half and checks
//! it against the current state ([`entry_holds`]): the receiver is a closure,
//! the holder's word is still `gen`, and the slot's CURRENT value is a
//! closure of `info`. A patched slot (`Function.prototype.call = f`, whether
//! or not the compiler saw the write: #11886), an own `call` on the receiver
//! (a new ShapeId) or a redefined holder key (a new holder ShapeId) fails one
//! of them, and the call takes the full dispatch, which runs the patch. When
//! all hold it runs the intrinsic `code` names ([`run_intrinsic`]): for a
//! compiled JavaScript target whose explicit receiver needs no boxing, `call`
//! and `apply` are the ordinary compiled-body call with no probe at all.
//!
//! # Before the realm builds `%Function.prototype%`
//!
//! Nothing can replace an intrinsic on an object that does not exist yet. A
//! miss on a base-shaped receiver's `call` / `apply` / `bind` then primes an
//! entry with no holder whose `code` names the intrinsic: it holds exactly
//! while the realm's `%Function.prototype%` pointer is null. Once the realm
//! builds the prototype the entry stops holding, and the next miss primes the
//! holder-validated entry over it.
//!
//! The method NAME is read only when an entry is primed.

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

/// The half of a receiver's word an entry keeps: the ShapeId.
const SHAPE_HALF: u64 = 0xFFFF_FFFF_0000_0000;

/// An entry's `code`: the intrinsic the slot held, which the site names.
const FN_CALL: u64 = 1;
const FN_APPLY: u64 = 2;
const FN_BIND: u64 = 3;

per_test_global! {
    static PRIMES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static NEGATIVE_PRIMES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static UNBUILT_PRIMES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static HITS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static FAST_HITS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static COMPILED_CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
}

/// Diagnostic counters: entries primed, negative entries primed, entries
/// primed before the realm built `%Function.prototype%`, calls the entry
/// lookup answered, calls the miss's first step answered, and of those the
/// compiled-body calls with no probe. `PERRY_METHOD_SITE_STATS` prints them.
pub(crate) fn function_intrinsic_stats() -> [u64; 6] {
    [
        PRIMES.load(Ordering::Relaxed),
        NEGATIVE_PRIMES.load(Ordering::Relaxed),
        UNBUILT_PRIMES.load(Ordering::Relaxed),
        HITS.load(Ordering::Relaxed),
        FAST_HITS.load(Ordering::Relaxed),
        COMPILED_CALLS.load(Ordering::Relaxed),
    ]
}

const _: () = assert!(METHOD_SITE_FUNCTION_INTRINSIC & METHOD_SITE_INDEX_MASK == 0);

/// The intrinsic a method name names, as an entry's `code`.
fn fn_kind(name: &[u8]) -> Option<u64> {
    match name {
        b"call" => Some(FN_CALL),
        b"apply" => Some(FN_APPLY),
        b"bind" => Some(FN_BIND),
        _ => None,
    }
}

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

/// The miss's first step (module docs): a site whose function-intrinsic
/// entry names its own intrinsic and still holds runs that intrinsic at once,
/// before the miss spells the method name or reads anything else. `None`
/// leaves the call to the ordinary miss, unchanged.
///
/// # Safety
/// As `js_method_site_miss`: `slot` is null or a live method-site slot,
/// `recv` is a live closure whose word is `word`, `args_ptr` holds `argc`
/// values.
#[inline]
pub(super) unsafe fn fast_miss(
    slot: *mut MethodSiteSlot,
    recv: f64,
    word: u64,
    args_ptr: *const f64,
    argc: usize,
) -> Option<f64> {
    if WORKER_AGENTS_EXIST.load(Ordering::Relaxed) != 0 {
        return None;
    }
    let site = crate::object::pic_slot_peek(slot);
    if site.is_null() {
        return None;
    }
    let word = word & SHAPE_HALF;
    let entry = (*site).entries.iter().find(|e| {
        e.word == word && e.slot & KIND_MASK == METHOD_SITE_FUNCTION_INTRINSIC && e.code != 0
    })?;
    if !entry_holds(entry) {
        return None;
    }
    FAST_HITS.fetch_add(1, Ordering::Relaxed);
    Some(run_intrinsic(entry.code, recv, args_ptr, argc))
}

/// Does `e` (an entry the receiver's ShapeId matched) still hold? With a
/// holder: the holder's word is `gen` (its shape pins the key's slot) and the
/// slot's CURRENT value is a closure of `info`, so a patched slot fails here.
/// With none: the realm has not built `%Function.prototype%`.
///
/// # Safety
/// `e` is an entry of a live site.
unsafe fn entry_holds(e: &MethodEntry) -> bool {
    if e.closure == 0 {
        return !crate::object::native_call_method::function_prototype_built();
    }
    let holder = e.closure as *const crate::object::ObjectHeader;
    if std::ptr::read(holder as *const u64) != e.gen {
        return false;
    }
    // The holder's word pins its key list, so the index names the key; the
    // read follows the shape's placement (inline or spill).
    let index = (e.slot & METHOD_SITE_INDEX_MASK) as u32;
    let value = crate::object::js_object_get_field(holder, index).bits();
    if value & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return false;
    }
    let method = (value & crate::value::POINTER_MASK) as usize;
    crate::value::addr_class::is_above_handle_band(method)
        && is_live_closure(method)
        && (*(method as *const crate::closure::ClosureHeader)).info as u64 == e.info
}

/// Is the heap cell at `addr` a closure that has not been forwarded? Its
/// GcHeader type byte and flags, read in place (the emitted method site reads
/// the same half-word to prove a slot value a closure).
#[inline(always)]
unsafe fn is_live_closure(addr: usize) -> bool {
    let header = &*((addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader);
    header.obj_type == crate::gc::GC_TYPE_CLOSURE
        && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
}

/// Run the intrinsic an entry's `code` names on the function `recv`: what
/// the full dispatch would run once it had found that intrinsic in the slot,
/// with no name, site or slot lookup.
///
/// # Safety
/// `recv` is a live closure; `args_ptr` holds `argc` values.
unsafe fn run_intrinsic(kind: u64, recv: f64, args_ptr: *const f64, argc: usize) -> f64 {
    if kind == FN_BIND {
        // The `bind` arm on a function receiver (`value_is_callable` holds
        // for every closure): the bound function, built from `thisArg` and
        // the leading arguments.
        return crate::closure::js_function_bind(recv, args_ptr, argc);
    }
    if let Some(result) = compiled_target_call(kind, recv, args_ptr, argc) {
        COMPILED_CALLS.fetch_add(1, Ordering::Relaxed);
        return result;
    }
    let name: &'static str = if kind == FN_CALL { "call" } else { "apply" };
    match crate::object::native_call_method::run_function_intrinsic(recv, name, args_ptr, argc) {
        Some(result) => result,
        None => crate::object::native_call_method::js_native_call_method(
            recv,
            name.as_ptr() as *const i8,
            name.len(),
            args_ptr,
            argc,
        ),
    }
}

/// `call` / `apply` on a compiled JavaScript body whose explicit receiver
/// needs no boxing and that keeps no re-bindable `this` capture: exactly what
/// the intrinsic does for it, with none of its probes. A compiled body
/// ([`crate::codegen_abi::FN_COMPILED_BODY`]) is never a Proxy, a built-in, a
/// bound or native-module function or a class constructor, so the
/// intrinsic's native special cases (stream / http construction, static
/// bound methods, value-called built-ins) cannot apply, and the call is the
/// ordinary compiled-body call with the arguments padded to its parameters.
/// `apply`'s argument list comes from a real Array, an `arguments` object,
/// or `null` / `undefined`; any other array-like (and everything else) is
/// `None`: the intrinsic's full arm runs.
///
/// # Safety
/// `recv` is a live closure; `args_ptr` holds `argc` values.
unsafe fn compiled_target_call(
    kind: u64,
    recv: f64,
    args_ptr: *const f64,
    argc: usize,
) -> Option<f64> {
    if kind != FN_CALL && kind != FN_APPLY {
        return None;
    }
    let closure =
        (recv.to_bits() & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
    let info: &'static crate::closure::JsFunctionInfo = &*(*closure).info;
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let arg = |i: usize| {
        if i < argc {
            *args_ptr.add(i)
        } else {
            undefined
        }
    };
    let this_arg = arg(0);
    if !receiver_passes_unchanged(closure, info, this_arg.to_bits()) {
        return None;
    }
    let this = crate::closure::JsThis::from_f64(this_arg);
    if kind == FN_CALL {
        let rest = if argc > 1 {
            args_ptr.add(1)
        } else {
            std::ptr::null()
        };
        return Some(enter_compiled_body(
            closure,
            info,
            this,
            rest,
            argc.saturating_sub(1),
        ));
    }
    let list = arg(1).to_bits();
    let values: Vec<f64> = if list == crate::value::TAG_UNDEFINED || list == crate::value::TAG_NULL
    {
        Vec::new()
    } else if list & !crate::value::POINTER_MASK == crate::value::POINTER_TAG {
        let addr = (list & crate::value::POINTER_MASK) as usize;
        if !crate::value::addr_class::is_above_handle_band(addr) {
            return None;
        }
        match crate::value::addr_class::try_read_gc_header(addr) {
            Some(h) if h.obj_type == crate::gc::GC_TYPE_ARRAY => {
                let arr = addr as *const crate::array::ArrayHeader;
                let n = crate::array::js_array_length(arr);
                (0..n)
                    .map(|i| crate::array::js_array_get_f64(arr, i))
                    .collect()
            }
            Some(h) if h.obj_type == crate::gc::GC_TYPE_OBJECT => {
                crate::object::arguments_object_to_vec(addr as *const crate::object::ObjectHeader)?
            }
            _ => return None,
        }
    } else {
        return None;
    };
    let (ptr, len) = if values.is_empty() {
        (std::ptr::null(), 0)
    } else {
        (values.as_ptr(), values.len())
    };
    Some(enter_compiled_body(closure, info, this, ptr, len))
}

/// Enter a compiled body with `n` arguments: directly, padded with
/// `undefined` to the parameters it declares, when it has no rest parameter
/// and both counts are small (the per-arity dispatcher's direct case);
/// otherwise through the compiled-body call, which bundles rest arguments.
///
/// # Safety
/// `closure` is a live closure on `info`, a compiled body; `args_ptr` holds
/// `n` values.
#[inline]
unsafe fn enter_compiled_body(
    closure: *const crate::closure::ClosureHeader,
    info: &'static crate::closure::JsFunctionInfo,
    this: crate::closure::JsThis,
    args_ptr: *const f64,
    n: usize,
) -> f64 {
    const DIRECT: usize = 6;
    let params = usize::from(info.params);
    if info.flags & crate::closure::FN_REST_MASK != 0 || n > DIRECT || params > DIRECT {
        return crate::closure::call_compiled_body_this(closure, info, this, args_ptr, n);
    }
    let mut a = [f64::from_bits(crate::value::TAG_UNDEFINED); DIRECT];
    for (i, slot) in a.iter_mut().enumerate().take(n) {
        *slot = *args_ptr.add(i);
    }
    let code = info.code;
    use crate::closure::body_call::js_body_call;
    match n.max(params) {
        0 => js_body_call!(code, closure, this),
        1 => js_body_call!(code, closure, this, a[0]),
        2 => js_body_call!(code, closure, this, a[0], a[1]),
        3 => js_body_call!(code, closure, this, a[0], a[1], a[2]),
        4 => js_body_call!(code, closure, this, a[0], a[1], a[2], a[3]),
        5 => js_body_call!(code, closure, this, a[0], a[1], a[2], a[3], a[4]),
        _ => js_body_call!(code, closure, this, a[0], a[1], a[2], a[3], a[4], a[5]),
    }
}

/// A compiled body, an explicit receiver `coerce_call_this` returns
/// unchanged (an object, `undefined`, `null`, or any value for a strict
/// body), and no `this` capture `rebind_explicit_this` would clone.
#[inline(always)]
unsafe fn receiver_passes_unchanged(
    closure: *const crate::closure::ClosureHeader,
    info: &crate::closure::JsFunctionInfo,
    this_bits: u64,
) -> bool {
    use crate::closure::{FN_ARROW, FN_STRICT};
    use crate::codegen_abi::FN_COMPILED_BODY;
    if info.flags & FN_COMPILED_BODY == 0 {
        return false;
    }
    let this_unchanged = this_bits & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
        || this_bits == crate::value::TAG_UNDEFINED
        || this_bits == crate::value::TAG_NULL
        || info.flags & FN_STRICT != 0;
    let count = (*closure).capture_count;
    let rebinds = count
        & (crate::closure::CAPTURES_THIS_FLAG | crate::closure::NO_THIS_REBIND_FLAG)
        == crate::closure::CAPTURES_THIS_FLAG
        && crate::closure::real_capture_count(count) > 0
        && info.flags & FN_ARROW == 0;
    this_unchanged && !rebinds
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
/// site that [`fast_miss`] did not take (one whose `code` is 0, or one that
/// no longer holds). `word` is the receiver's word.
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
    let Some(entry) = (*site).entries.iter().find(|e| {
        e.word == word & SHAPE_HALF && e.slot & KIND_MASK == METHOD_SITE_FUNCTION_INTRINSIC
    }) else {
        return Lookup::Unknown;
    };
    let holder = entry.closure;
    if holder == 0 {
        // An entry primed before the realm built `%Function.prototype%`
        // (`code` names its intrinsic) is stale once it has: prime the
        // holder-validated entry over it. A negative entry stays known.
        return if entry.code != 0 && crate::object::native_call_method::function_prototype_built() {
            Lookup::Unknown
        } else {
            Lookup::Known
        };
    }
    if entry.info == 0 || std::ptr::read(holder as *const u64) != entry.gen {
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
/// receiver (`word`, the receiver's word) does not own `name`, so it
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
    let word = word & SHAPE_HALF;
    // A key that is not an inherited intrinsic primes a NEGATIVE entry (no
    // holder, info 0, code 0): the site then dispatches this word without
    // asking the name again; so does a receiver word the facts refuse.
    // Before the realm has built `%Function.prototype%` only `call`, `apply`
    // and `bind` prime: an entry with no holder whose code names the
    // intrinsic (module docs).
    use crate::object::native_call_method::FunctionIntrinsicFacts;
    let entry = match crate::object::native_call_method::function_intrinsic_facts(recv, name) {
        FunctionIntrinsicFacts::Intrinsic(facts) if facts.index as u64 & KIND_MASK == 0 => {
            // The miss's first step runs the intrinsic only when the slot
            // holds the one the site's name names: `Function.prototype.call =
            // Function.prototype.apply` leaves `call` sites to the lookup,
            // which runs whatever intrinsic the slot holds.
            let code = match fn_kind(name) {
                Some(kind) if fn_kind(facts.which.as_bytes()) == Some(kind) => kind,
                _ => 0,
            };
            MethodEntry {
                word,
                slot: facts.index as u64 | METHOD_SITE_FUNCTION_INTRINSIC,
                info: facts.info,
                code,
                closure: facts.holder,
                gen: std::ptr::read(facts.holder as *const u64),
            }
        }
        FunctionIntrinsicFacts::NoPrototype => {
            let Some(kind) = fn_kind(name) else {
                return;
            };
            if crate::object::native_call_method::function_prototype_built() {
                return;
            }
            MethodEntry {
                word,
                slot: METHOD_SITE_FUNCTION_INTRINSIC,
                info: 0,
                code: kind,
                closure: 0,
                gen: 0,
            }
        }
        _ => MethodEntry {
            word,
            slot: METHOD_SITE_FUNCTION_INTRINSIC,
            info: 0,
            code: 0,
            closure: 0,
            gen: 0,
        },
    };
    let negative = entry.info == 0 && entry.code == 0;
    let unbuilt = entry.closure == 0 && entry.code != 0;
    if publish(slot, entry) {
        if negative {
            NEGATIVE_PRIMES.fetch_add(1, Ordering::Relaxed);
        } else {
            PRIMES.fetch_add(1, Ordering::Relaxed);
        }
        if unbuilt {
            UNBUILT_PRIMES.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// The miss handler's function-intrinsic step for `recv.<name>(args)` on a
/// function receiver whose ShapeId says it inherits from
/// `%Function.prototype%` (`word` is its word): answer from the site's entry,
/// or dispatch and prime one. `None` leaves the
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::closure::ClosureHeader;
    use crate::codegen_abi::FN_COMPILED_BODY;

    extern "C" fn target_body(_c: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
        42.0
    }

    extern "C" fn sum_body(
        _c: *const ClosureHeader,
        _this: crate::closure::JsThis,
        a: f64,
        b: f64,
    ) -> f64 {
        a + b
    }

    fn key(s: &[u8]) -> *mut crate::StringHeader {
        crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
    }

    /// Publish a `%Function.prototype%` whose `name` slot holds a closure
    /// over `info`; returns it.
    unsafe fn install_proto(
        name: &[u8],
        info: *const crate::closure::JsFunctionInfo,
    ) -> *mut crate::object::ObjectHeader {
        let proto = crate::object::js_object_alloc(0, 0);
        set_slot(proto, name, info);
        crate::closure::shape::FUNCTION_PROTOTYPE_PTR.store(proto as i64, Ordering::Release);
        proto
    }

    unsafe fn set_slot(
        proto: *mut crate::object::ObjectHeader,
        name: &[u8],
        info: *const crate::closure::JsFunctionInfo,
    ) {
        let method = crate::closure::js_closure_alloc(info, 0);
        crate::object::js_object_set_field_by_name(
            proto,
            key(name),
            crate::value::js_nanbox_pointer(method as i64),
        );
    }

    unsafe fn closure(info: *const crate::closure::JsFunctionInfo, captures: u32) -> f64 {
        let c = crate::closure::js_closure_alloc(info, captures);
        crate::value::js_nanbox_pointer(c as i64)
    }

    unsafe fn function(captures: u32) -> f64 {
        closure(crate::fn_info!(target_body, 0), captures)
    }

    unsafe fn word_of(f: f64) -> u64 {
        std::ptr::read((f.to_bits() & crate::value::POINTER_MASK) as *const u64)
    }

    unsafe fn entries_for(slot: MethodSiteSlot, f: f64) -> Vec<MethodEntry> {
        let word = word_of(f) & SHAPE_HALF;
        (*slot)
            .entries
            .iter()
            .filter(|e| e.word == word)
            .map(|e| MethodEntry { ..*e })
            .collect()
    }

    fn bind_thunk() -> *const crate::closure::JsFunctionInfo {
        crate::object::global_this::function_prototype_bind_thunk_for_test()
    }

    const UNDEF: f64 = f64::from_bits(crate::value::TAG_UNDEFINED);

    #[test]
    fn an_entry_is_keyed_on_the_shape_half_and_names_the_sites_intrinsic() {
        if !super::super::run_with_fresh_worker_gate(
            "an_entry_is_keyed_on_the_shape_half_and_names_the_sites_intrinsic",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        unsafe {
            let saved = crate::closure::shape::FUNCTION_PROTOTYPE_PTR.load(Ordering::Acquire);
            let proto = install_proto(b"bind", bind_thunk());
            let (a, b) = (function(0), function(2));
            assert_eq!(
                word_of(a) >> 32,
                word_of(b) >> 32,
                "one base Function shape"
            );
            assert_ne!(word_of(a), word_of(b), "the capture counts differ");
            let mut slot: MethodSiteSlot = std::ptr::null_mut();
            prime(&mut slot, a, b"bind");
            prime(&mut slot, b, b"bind");
            let found = entries_for(slot, a);
            assert_eq!(found.len(), 1, "both closures share one entry");
            let e = &found[0];
            assert_eq!(e.slot & KIND_MASK, METHOD_SITE_FUNCTION_INTRINSIC);
            assert_eq!(e.code, FN_BIND);
            assert_eq!(e.closure, proto as usize);
            assert_eq!(e.gen, std::ptr::read(proto as *const u64));
            assert_eq!(e.info, bind_thunk() as u64);
            crate::closure::shape::FUNCTION_PROTOTYPE_PTR.store(saved, Ordering::Release);
        }
    }

    #[test]
    fn a_slot_holding_another_intrinsic_gets_no_fast_step() {
        if !super::super::run_with_fresh_worker_gate(
            "a_slot_holding_another_intrinsic_gets_no_fast_step",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        unsafe {
            let saved = crate::closure::shape::FUNCTION_PROTOTYPE_PTR.load(Ordering::Acquire);
            // `Function.prototype.call = Function.prototype.bind`.
            install_proto(b"call", bind_thunk());
            let f = function(0);
            let mut slot: MethodSiteSlot = std::ptr::null_mut();
            prime(&mut slot, f, b"call");
            let found = entries_for(slot, f);
            assert_eq!(found.len(), 1);
            assert_ne!(
                found[0].info, 0,
                "the lookup still answers it from the slot"
            );
            assert_eq!(
                found[0].code, 0,
                "a `call` site must not run `call` for `bind`"
            );
            assert!(fast_miss(&mut slot, f, word_of(f), [UNDEF].as_ptr(), 1).is_none());
            crate::closure::shape::FUNCTION_PROTOTYPE_PTR.store(saved, Ordering::Release);
        }
    }

    #[test]
    fn the_fast_step_runs_the_intrinsic_only_while_the_entry_holds() {
        if !super::super::run_with_fresh_worker_gate(
            "the_fast_step_runs_the_intrinsic_only_while_the_entry_holds",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        unsafe {
            let saved = crate::closure::shape::FUNCTION_PROTOTYPE_PTR.load(Ordering::Acquire);
            let proto = install_proto(b"bind", bind_thunk());
            let f = function(1);
            let args = [UNDEF];
            let mut slot: MethodSiteSlot = std::ptr::null_mut();
            prime(&mut slot, f, b"bind");
            let bound =
                fast_miss(&mut slot, f, word_of(f), args.as_ptr(), 1).expect("the entry holds");
            let bound_ptr = (bound.to_bits() & crate::value::POINTER_MASK) as *const ClosureHeader;
            assert_eq!((*bound_ptr).code(), crate::closure::BOUND_FUNCTION_FUNC_PTR);
            assert_eq!(
                crate::closure::js_closure_call0(bound_ptr, crate::closure::plain_call_receiver()),
                42.0
            );
            // Another receiver kind never reaches the step: the miss's header
            // read classifies it first, and the in-place read agrees.
            let obj = crate::value::js_nanbox_pointer(crate::object::js_object_alloc(0, 0) as i64);
            assert!(!is_live_closure(
                (obj.to_bits() & crate::value::POINTER_MASK) as usize
            ));
            let e = MethodEntry {
                ..entries_for(slot, f)[0]
            };
            assert!(entry_holds(&e));
            // The holder's shape changed (a stale word).
            assert!(!entry_holds(&MethodEntry {
                gen: e.gen ^ 1,
                ..e
            }));
            // The slot was patched (#11886): the value is no longer the intrinsic.
            set_slot(proto, b"bind", crate::fn_info!(target_body, 0));
            assert!(
                fast_miss(&mut slot, f, word_of(f), args.as_ptr(), 1).is_none(),
                "patched slot"
            );
            crate::closure::shape::FUNCTION_PROTOTYPE_PTR.store(saved, Ordering::Release);
        }
    }

    #[test]
    fn before_the_realm_builds_the_prototype_an_entry_has_no_holder() {
        if !super::super::run_with_fresh_worker_gate(
            "before_the_realm_builds_the_prototype_an_entry_has_no_holder",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        unsafe {
            let saved = crate::closure::shape::FUNCTION_PROTOTYPE_PTR.load(Ordering::Acquire);
            crate::closure::shape::FUNCTION_PROTOTYPE_PTR.store(0, Ordering::Release);
            let f = function(1);
            let mut slot: MethodSiteSlot = std::ptr::null_mut();
            prime(&mut slot, f, b"toString");
            assert!(
                slot.is_null() || entries_for(slot, f).is_empty(),
                "only call/apply/bind prime before the build"
            );
            prime(&mut slot, f, b"bind");
            let found = entries_for(slot, f);
            assert_eq!(found.len(), 1);
            assert_eq!((found[0].closure, found[0].info), (0, 0));
            assert_eq!(found[0].code, FN_BIND);
            let args = [UNDEF];
            assert!(
                fast_miss(&mut slot, f, word_of(f), args.as_ptr(), 1).is_some(),
                "holds while unbuilt"
            );
            // The realm builds the prototype: the entry stops holding, the
            // lookup asks for a prime, and the prime replaces the entry.
            let proto = install_proto(b"bind", bind_thunk());
            assert!(
                fast_miss(&mut slot, f, word_of(f), args.as_ptr(), 1).is_none(),
                "stale once built"
            );
            assert!(matches!(
                lookup(&mut slot, word_of(f), f, args.as_ptr(), 1),
                Lookup::Unknown
            ));
            prime(&mut slot, f, b"bind");
            let found = entries_for(slot, f);
            assert_eq!(found.len(), 1, "the new entry replaces the holder-less one");
            assert_eq!(found[0].closure, proto as usize);
            assert!(fast_miss(&mut slot, f, word_of(f), args.as_ptr(), 1).is_some());
            crate::closure::shape::FUNCTION_PROTOTYPE_PTR.store(saved, Ordering::Release);
        }
    }

    /// The compiled-body step takes exactly the targets the intrinsic's
    /// probes cannot matter for, each disqualifier alone sending it back.
    #[test]
    fn a_compiled_target_is_called_with_no_probe_and_nothing_else_is() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        unsafe {
            let compiled = crate::fn_info!(sum_body, 2; with_flags(FN_COMPILED_BODY));
            let f = closure(compiled, 0);
            let obj = crate::value::js_nanbox_pointer(crate::object::js_object_alloc(0, 0) as i64);
            let call = |f: f64, this: f64| {
                let args = [this, 1.0, 2.0];
                compiled_target_call(FN_CALL, f, args.as_ptr(), 3)
            };
            assert_eq!(call(f, UNDEF), Some(3.0));
            assert_eq!(call(f, obj), Some(3.0));
            assert_eq!(call(f, f64::from_bits(crate::value::TAG_NULL)), Some(3.0));
            assert_eq!(
                call(f, 5.0),
                None,
                "a sloppy body boxes a primitive receiver"
            );
            let strict = closure(
                crate::fn_info!(sum_body, 2; with_flags(FN_COMPILED_BODY | crate::closure::FN_STRICT)),
                0,
            );
            assert_eq!(call(strict, 5.0), Some(3.0), "a strict body takes it as is");
            assert_eq!(
                call(closure(crate::fn_info!(sum_body, 2), 0), UNDEF),
                None,
                "built-in"
            );
            let rebinds = closure(compiled, 1 | crate::closure::CAPTURES_THIS_FLAG);
            assert_eq!(call(rebinds, obj), None, "a re-bindable `this` capture");
            let lexical = closure(
                compiled,
                1 | crate::closure::CAPTURES_THIS_FLAG | crate::closure::NO_THIS_REBIND_FLAG,
            );
            assert_eq!(call(lexical, obj), Some(3.0), "a lexical `this` capture");
            // Too few arguments: the body's parameters are padded.
            assert!(
                compiled_target_call(FN_CALL, f, [UNDEF, 1.0].as_ptr(), 2).is_some_and(f64::is_nan)
            );
            // apply: an Array, null, and an array-like that is neither.
            let arr = crate::array::js_array_alloc(2);
            let arr = crate::array::js_array_push_f64(arr, 4.0);
            let arr = crate::array::js_array_push_f64(arr, 5.0);
            let list = crate::value::js_nanbox_pointer(arr as i64);
            let apply = |list: f64| compiled_target_call(FN_APPLY, f, [UNDEF, list].as_ptr(), 2);
            assert_eq!(apply(list), Some(9.0));
            assert!(apply(f64::from_bits(crate::value::TAG_NULL)).is_some_and(f64::is_nan));
            assert_eq!(apply(obj), None, "a plain array-like takes the full arm");
            assert_eq!(compiled_target_call(FN_BIND, f, [UNDEF].as_ptr(), 1), None);
        }
    }
}
