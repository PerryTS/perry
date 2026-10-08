//! The method-call site memo: `recv.m(args)` on the One Path.
//!
//! A method call is a property read followed by a call. The emitted site
//! (`perry-codegen/src/expr/method_site.rs`) compares the receiver's
//! shape word against each entry's [`MethodEntry::word`] (ordinary objects
//! retain `class_id`; functions exclude capture count) and
//! then, with no runtime call:
//!
//! * **own entry** — loads the receiver's inline slot [`MethodEntry::slot`],
//!   proves the value is a closure whose body info equals
//!   [`MethodEntry::info`], and calls [`MethodEntry::code`] directly with the
//!   receiver as `this`;
//! * **inherited entry** ([`METHOD_SITE_INHERITED`] in `slot`) — compares the
//!   direct holder's word with [`MethodEntry::gen`], loads its slot, proves
//!   the loaded closure has [`MethodEntry::info`], and calls it directly.
//! * **ConstFn entry** ([`METHOD_SITE_CONSTFN`], own or inherited) — the
//!   compared shape (the receiver's, or for an inherited entry the holder's)
//!   carries a ConstFn lane for the slot, so the slot holds a closure of the
//!   recorded body: the hit loads it only as the callee environment and calls
//!   [`MethodEntry::code`] with exactly the call's arguments, with no kind or
//!   info check. Primed only for a body declaring at most the call's argument
//!   count (a body that wants `undefined` padding keeps a plain entry).
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
//! * An inherited entry holds the direct holder as a strong root. The
//!   receiver's shape pins that holder, and the holder's shape pins the
//!   inline slot. A structural change invalidates one of those word compares;
//!   a value overwrite is seen by loading the slot on every hit.
//!
//! What the prime refuses (they keep the ordinary dispatch): non-ordinary
//! receivers other than shape-described functions (class objects,
//! native-module namespaces, dictionaries,
//! `Object.prototype`, typed-array prototypes, exotic read receivers),
//! accessors, spill slots, and any value that is not a plain closure the call can enter directly for
//! this site's argument count (bound functions, fixed-ABI rest / `arguments` bodies,
//! the no-op builtin thunk, class constructors, closures that capture `this`).
//! A native argument-list body accepts the site's argument buffer directly.
//! A builtin method (`FN_BUILTIN`) whose thunk is direct-callable for the
//! argument count IS admitted on an ordinary receiver, own or inherited: its
//! prototype holds it in an inline slot like any method, the hit loads that
//! slot on every call, so `Object.prototype.hasOwnProperty = f` is seen.
//!
//! # The one site-memo module (shared)
//!
//! This is THE per-site memo for "the receiver's shape answers this key":
//! method calls use it today, and class accessors (step 3, S4) add their
//! entry kind here rather than growing a second table. The contract every
//! entry kind keeps:
//!
//! * an entry is facts of ONE receiver shape word, compared
//!   by the emitted code on every use; nothing is keyed on a class id, an
//!   address or a name alone;
//! * the kind lives in the top bits of [`MethodEntry::slot`]: `0` own inline,
//!   bit 63 inherited ([`METHOD_SITE_INHERITED`]), bit 62 own spill
//!   ([`METHOD_SITE_SPILL`]), bit 61 own function-bag
//!   ([`METHOD_SITE_FUNCTION_BAG`]: a function-object receiver whose method is
//!   an inline slot of its own-property object); bit 60 marks the native
//!   argument-list body ABI. A new kind extends the emitted `msite.other` dispatch and
//!   [`publish`], nothing else;
//! * an entry that holds a heap reference stores it in [`MethodEntry::closure`]
//!   and is registered by [`publish`], so [`scan_method_site_roots_mut`] marks
//!   and rewrites it;
//! * an inherited entry records the direct holder's word in
//!   [`MethodEntry::gen`]; deeper chains stay on ordinary dispatch;
//! * primes run after the ordinary dispatch, with collection suppressed.
//!
//! # GC
//!
//! [`MethodEntry::closure`] is a STRONG root for the inherited holder: marked,
//! and rewritten when it moves (`scan_method_site_roots_mut`). Every site that ever primed an
//! inherited entry is registered once for the scan.
//!
//! # Agents
//!
//! The first worker start atomically gates every emitted method site and
//! prevents further primes. It does not rewrite a live site while primary
//! code may be reading it. Existing inherited entries are no longer read or
//! traced by any agent after the gate; their stale words are inert and the
//! holders can be collected by the primary GC. The cost thereafter is
//! ordinary method dispatch at every site.

use crate::object::ObjectHeader;

pub(crate) mod chain_memo;
mod function_receiver;
mod holder_prime;
use holder_prime::prime_holder;
mod miss_entry;
pub use miss_entry::js_method_site_miss;
pub(crate) mod own_slot_memo;
pub(crate) mod read_holder;
use std::sync::atomic::{AtomicU64, Ordering};

/// `word` of a site no prime has touched: no receiver word is all-ones.
pub const METHOD_SITE_EMPTY: u64 = u64::MAX;
/// The `slot` bit that marks an inherited entry.
pub const METHOD_SITE_INHERITED: u64 = crate::codegen_abi::METHOD_SITE_INHERITED;
/// The `slot` bit that marks an own entry whose key lives in the receiver's
/// spill buffer (`ObjectMeta::spill`) at the index in the low bits.
pub const METHOD_SITE_SPILL: u64 = 1 << 62;
/// The `slot` bit that marks an own entry of a FUNCTION receiver: the key is
/// inline slot (low bits) of the function's own-property object
/// (`ClosureHeader::props`, `closure/props.rs`). A keyed Function ShapeId is
/// canonical per that object's key list, so the receiver word pins the slot.
pub const METHOD_SITE_FUNCTION_BAG: u64 = crate::codegen_abi::METHOD_SITE_FUNCTION_BAG;
/// An own inline method whose ShapeId owns the body's identity.
pub const METHOD_SITE_CONSTFN: u64 = crate::codegen_abi::METHOD_SITE_CONSTFN;
/// The index bits of an entry's `slot` word.
pub const METHOD_SITE_INDEX_MASK: u64 = crate::codegen_abi::METHOD_SITE_INDEX_MASK;

/// One entry of a site's memo. **Field offsets are baked into emitted code**
/// (`perry_abi::METHOD_SITE_*_OFFSET`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MethodEntry {
    /// The receiver's shape word; functions have zero in the low half.
    pub word: u64,
    /// Own entry: the inline slot, optionally tagged as ConstFn. Inherited
    /// entry: [`METHOD_SITE_INHERITED`] plus the direct holder's slot index.
    pub slot: u64,
    /// The method body's `JsFunctionInfo` (the identity an own hit compares
    /// the slot closure's info word with).
    pub info: u64,
    /// Inherited entry: the direct holder's address (a STRONG GC root).
    pub closure: usize,
    /// Inherited entry: the holder's full `(class_id | ShapeId)` word.
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

/// Every site that holds (or held) an inherited entry, for the primary agent's
/// root scan until a worker starts. The sites are process-lifetime
/// allocations, but their holders belong to the primary heap.
static METHOD_SITES: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());

/// Sticky process-wide gate: a worker cannot read a primary-heap holder from
/// a process-global method or read site. Emitted method sites read this byte
/// atomically and take the generic miss once it is set. Keeping the old words
/// intact avoids racing a worker startup write against a primary inline hit.
/// After the gate, no agent reads or traces the stale entries, so they do not
/// retain their primary-heap holders.
#[cfg_attr(not(test), export_name = "PERRY_METHOD_SITE_WORKERS_PRESENT")]
pub(crate) static WORKER_AGENTS_EXIST: std::sync::atomic::AtomicU8 =
    std::sync::atomic::AtomicU8::new(0);

/// Run a gate-sensitive unit in a fresh test process. Worker startup is
/// process-wide and sticky: clearing it in a parallel libtest process can
/// re-enable a worker's access to primary-heap holder pointers.
#[cfg(test)]
pub(crate) fn run_with_fresh_worker_gate(filter: &str) -> bool {
    const MARKER: &str = "PERRY_A2_FRESH_WORKER_GATE_TEST";
    if std::env::var_os(MARKER).as_deref() == Some(std::ffi::OsStr::new(filter)) {
        return true;
    }
    let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .arg("--test-threads=1")
        .arg(filter)
        .env(MARKER, filter)
        .output()
        .expect("run filtered test in a fresh process");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("running 1 test") && stdout.contains("1 passed"),
        "isolated test {filter} failed or matched no test:\n{stdout}\n{stderr}",
    );
    false
}

/// Called by `agent::enter_worker_agent` before the worker runs any code.
pub fn note_worker_agent() {
    // Publish the gate under the same lock as `publish`: every in-flight
    // write finishes before a worker can run emitted code, and all later
    // publishes decline. No site word is written at worker startup.
    let _sites = METHOD_SITES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let first = WORKER_AGENTS_EXIST.swap(1, Ordering::SeqCst) == 0;
    drop(_sites);
    if first {
        super::proto_validity::bump_proto_validity();
    }
}

/// Why a miss did not prime (diagnostic; `PERRY_METHOD_SITE_STATS` prints it),
/// by the index [`refuse`] counts under. One string, see
/// [`crate::hot_diag::report_name`].
fn refusal_name(reason: usize) -> &'static str {
    const NAMES: &str = "not_object_pointer not_ordinary dictionary own_spill_slot own_accessor own_not_direct_callable inh_class_instance inh_proto_not_in_shape inh_hop_refused inh_not_found inh_not_direct_callable inh_workers dc_not_closure dc_special dc_rest dc_captures_this dc_arity_pad dc_bound site_megamorphic function_implicit_own_key";
    crate::hot_diag::report_name(NAMES, reason)
}
per_test_global! {
    static SITE_REFUSED: [AtomicU64; 20] = [const { AtomicU64::new(0) }; 20];
}
#[inline]
fn refuse(reason: usize) {
    SITE_REFUSED[reason].fetch_add(1, Ordering::Relaxed);
}

per_test_global! {
    static PRIMES_OWN: AtomicU64 = AtomicU64::new(0);
    static PRIMES_INHERITED: AtomicU64 = AtomicU64::new(0);
    static HOLDER_REWRITES: AtomicU64 = AtomicU64::new(0);
    static MISSES: AtomicU64 = AtomicU64::new(0);
    static PRIMES_FUNCTION: AtomicU64 = AtomicU64::new(0);
    static PRIMES_CONSTFN: AtomicU64 = AtomicU64::new(0);
    static PRIMES_BUILTIN: AtomicU64 = AtomicU64::new(0);
}

/// Count a published entry whose body is a builtin (`FN_BUILTIN`) thunk.
#[inline]
fn note_builtin_prime(info: &crate::closure::JsFunctionInfo) {
    if info.flags & crate::closure::FN_BUILTIN != 0 {
        PRIMES_BUILTIN.fetch_add(1, Ordering::Relaxed);
    }
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
/// 3 function-bag primes, 4 ConstFn own primes, 5 chain memo ways recorded.
/// Exposed so gap tests can prove a path ran.
#[no_mangle]
pub extern "C" fn js_method_site_stats(which: i32) -> f64 {
    let (a, b, c) = method_site_stats();
    (match which {
        0 => a,
        1 => b,
        3 => method_site_function_primes(),
        4 => PRIMES_CONSTFN.load(Ordering::Relaxed),
        5 => chain_memo::chain_memo_records(),
        _ => c,
    }) as f64
}

/// Is `PERRY_METHOD_SITE_STATS` set? Read once; the reader that settles the
/// answer installs the exit report. A tri-state byte (0 unread, 1 off, 2 on)
/// rather than a `OnceLock<bool>`: `OnceLock` initialises through a `dyn`
/// closure whose vtable is load-time relocations in every program that links
/// the miss path.
fn stats_report_enabled() -> bool {
    per_test_global! {
        static ON: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
    }
    match ON.load(Ordering::Relaxed) {
        0 => {}
        state => return state == 2,
    }
    let on = std::env::var_os("PERRY_METHOD_SITE_STATS").is_some();
    let state = if on { 2 } else { 1 };
    if ON
        .compare_exchange(0, state, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        if on {
            extern "C" fn report() {
                let (a, b, c) = method_site_stats();
                let mut refused = String::new();
                for (i, n) in SITE_REFUSED.iter().enumerate() {
                    let n = n.load(Ordering::Relaxed);
                    if n != 0 {
                        refused.push_str(&format!(" refused.{}={n}", refusal_name(i)));
                    }
                }
                let (hd, ha, hr) = read_holder::read_holder_stats();
                let (ap, ah) = read_holder::read_accessor_stats();
                let (cp, ch, cr) = read_holder::class_read_stats();
                eprintln!(
                    "[method-site] primes_own={a} primes_inherited={b} primes_builtin={} primes_function={} primes_constfn={} chain_memo_records={} holder_rewrites={} misses={c} read_holder_primes={hd} read_absent_primes={ha} read_accessor_primes={ap} read_accessor_hits={ah} read_accessor_class_primes={} class_read_primes={cp} class_read_hits={ch} class_read_root_rewrites={cr} read_holder_rewrites={} read_accessor_rewrites={} read_accessor_same_shape_relinks={} read_holder_refused={hr}{refused}",
                    PRIMES_BUILTIN.load(Ordering::Relaxed),
                    method_site_function_primes(),
                    PRIMES_CONSTFN.load(Ordering::Relaxed),
                    chain_memo::chain_memo_records(),
                    HOLDER_REWRITES.load(Ordering::Relaxed),
                    read_holder::read_accessor_class_primes(),
                    read_holder::read_holder_rewrites(),
                    read_holder::read_accessor_rewrites(),
                    read_holder::read_accessor_same_shape_relinks()
                );
            }
            unsafe { libc::atexit(report) };
        }
    }
    on
}

/// [`js_method_site_miss`] for a heap object: prime the site, then dispatch.
#[inline(never)]
unsafe fn method_site_miss_object(
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
    if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0 {
        refuse(11);
        return crate::typed_feedback::js_typed_feedback_native_call_method(
            site_id,
            recv,
            name_ref.ptr as *const i8,
            name_ref.len,
            args_ptr,
            argc,
        );
    }
    let name = std::slice::from_raw_parts(name_ref.ptr, name_ref.len);
    // One read of the receiver's header classifies it for both steps below.
    let receiver = miss_receiver(recv);
    if let MissReceiver::Payload = receiver {
        if let Some(result) =
            crate::native_payload::try_payload_method_fast_dispatch(recv, name, args_ptr, argc)
        {
            return result;
        }
    }
    // Ordinary objects and described function shapes can prime. Every
    // other receiver keeps generic dispatch.
    let megamorphic = site_is_megamorphic(slot);
    if megamorphic || !prime_candidate(receiver, name) {
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
    // Resolve the requested property from the shapes/prototype. If its
    // callable body consumes the original arguments directly, bypass its
    // rest-array construction. Aliases qualify by body; overrides decline.
    if let MissReceiver::Ordinary = receiver {
        if let Some(ops) = super::nm_ee_ops() {
            if let Some(result) = (ops.emit_call)(recv, method_id, name, args_ptr, argc) {
                return result;
            }
        }
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

/// What [`js_method_site_prepare`] returns when the method read is not
/// observable and the call should dispatch by name AFTER its arguments
/// through the site's miss (which may prime): a bit pattern no JS value takes
/// (the array-hole marker never leaves an array).
pub const METHOD_SITE_BY_NAME: u64 = crate::codegen_abi::METHOD_SITE_BY_NAME;
const _: () = assert!(METHOD_SITE_BY_NAME == crate::value::TAG_HOLE);
/// [`METHOD_SITE_BY_NAME`] for a receiver no site entry can describe (a
/// primitive, a native cell such as a Map or an array): the call goes
/// straight to the universal dispatcher. Also a bit pattern no JS value takes.
pub const METHOD_SITE_BY_NAME_DIRECT: u64 = crate::codegen_abi::METHOD_SITE_BY_NAME_DIRECT;
const _: () = assert!(
    METHOD_SITE_BY_NAME_DIRECT & !0xFF == crate::value::TAG_HOLE & !0xFF
        && METHOD_SITE_BY_NAME_DIRECT != crate::value::TAG_HOLE
        && METHOD_SITE_BY_NAME_DIRECT != crate::value::TAG_TDZ
        && METHOD_SITE_BY_NAME_DIRECT > crate::value::TAG_TRUE
);

/// The lookup half of a method call whose arguments can run code
/// (`o.m(f())`, #11910). ECMA-262 13.3.6.1 reads `o.m` BEFORE the arguments,
/// so a getter, a Proxy `get` trap or a nullish receiver's TypeError must run
/// first. The emitted site performs its hit (shape compare + slot load) before
/// the arguments and calls this only when the hit does not answer.
///
/// Returns the method value to call after the arguments, or one of the
/// by-name answers when the read cannot be observed and the receiver keeps
/// its by-name dispatch after the arguments:
///
/// * an ordinary object performs its one [[Get]] here (any getter or trap on
///   its chain runs now) and hands back a plain closure as the value, so an
///   argument that reassigns `o.m` cannot change which function runs; any
///   other value is called by name ([`METHOD_SITE_BY_NAME`]) unless the read
///   ran code, in which case the value itself is returned;
/// * a native cell (a collection, an array, a buffer, a function object) has
///   its methods answered by name and reads no prototype slot (#11394
///   handles a patched builtin prototype at compile time); the one read the
///   dispatcher performs that can run code is the receiver's OWN accessor,
///   which is answered here, from the cell's descriptor summary;
/// * a primitive's chain is its wrapper prototype and `Object.prototype`:
///   their shapes say whether either holds the key as an accessor.
///
/// # Safety
/// `slot` is null or a live method-site slot.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_method_site_prepare(
    slot: *mut MethodSiteSlot,
    recv: f64,
    method_id: i64,
    argc: usize,
) -> f64 {
    prepare(slot, recv, method_id, argc)
}

/// [`js_method_site_prepare`]'s body, shared with [`lookup_entry`]'s miss.
/// Kept out of line so the runtime carries one copy of it, not two.
#[inline(never)]
unsafe fn prepare(slot: *mut MethodSiteSlot, recv: f64, method_id: i64, argc: usize) -> f64 {
    let direct = f64::from_bits(METHOD_SITE_BY_NAME_DIRECT);
    let bits = recv.to_bits();
    let nullish = bits == crate::value::TAG_UNDEFINED || bits == crate::value::TAG_NULL;
    let is_pointer = bits & !crate::value::POINTER_MASK == crate::value::POINTER_TAG;
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    // One header read classifies a heap receiver; only an ordinary object
    // or a function object needs more than the accessor facts below.
    let header = if is_pointer
        && !crate::value::addr_class::is_proxy_id_band(addr)
        && crate::value::addr_class::is_above_handle_band(addr)
    {
        crate::value::addr_class::try_read_gc_header(addr)
    } else {
        None
    };
    // A pre-growth alias the program still holds (an array's forwarding
    // stub) is answered from the live head: growth carried the descriptor
    // bit to its header and re-keyed the array's own properties to it.
    let (header, addr) = match header {
        Some(h)
            if h.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
                && h.obj_type == crate::gc::GC_TYPE_ARRAY =>
        {
            let live =
                crate::array::clean_arr_ptr(addr as *const crate::array::ArrayHeader) as usize;
            if live != 0 && live != addr {
                (crate::value::addr_class::try_read_gc_header(live), live)
            } else {
                (Some(h), addr)
            }
        }
        h => (h, addr),
    };
    let kind = header.map(|h| {
        if h.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
            u8::MAX
        } else {
            h.obj_type
        }
    });
    let cell = matches!(kind, Some(t) if t != crate::gc::GC_TYPE_OBJECT
        && t != crate::gc::GC_TYPE_CLOSURE && t != u8::MAX);
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(name_ref) = crate::string::perry_string_ref_from_dispatch_id(method_id, &mut scratch)
    else {
        return direct;
    };
    let name = std::slice::from_raw_parts(name_ref.ptr, name_ref.len);
    if !is_pointer {
        return if nullish || primitive_chain_has_accessor(recv, name) {
            spec_get(recv, name)
        } else {
            direct
        };
    }
    match kind {
        Some(crate::gc::GC_TYPE_OBJECT) => prepare_ordinary(slot, recv, addr, name, argc),
        // A function method is read and primed from its shape like an
        // ordinary method, before its arguments can change the slot.
        Some(crate::gc::GC_TYPE_CLOSURE) => prepare_function(slot, recv, name, argc),
        _ if cell => {
            // An array that never had an own descriptor installed says so in
            // its header (`OBJ_FLAG_ARRAY_DESCRIPTORS`, armed on the live
            // head by every install, carried by growth and monotone for the
            // allocation; a pre-growth alias was resolved to that head above).
            let own = if kind == Some(crate::gc::GC_TYPE_ARRAY)
                && header.is_some_and(|h| h._reserved & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS == 0)
            {
                None
            } else {
                own_getter_value(addr, recv, name)
            };
            match own {
                Some(value) => value,
                None if cell_chain_has_accessor(recv, kind, name) => spec_get(recv, name),
                None => direct,
            }
        }
        // A Proxy, a native handle or a forwarded alias: the full walk (rare).
        _ => {
            if lookup_is_observable(recv, name) {
                spec_get(recv, name)
            } else {
                direct
            }
        }
    }
}

/// The lookup half of a split method site (#11910), in one call shared by
/// every such site: the site's memo hit first, exactly as the fused site's
/// emitted hit decides it (`perry-codegen/src/expr/method_site.rs`: receiver
/// word against each entry's word, the entry's kind, the slot load, and for a
/// plain entry the closure-of-this-body proof), else
/// [`js_method_site_prepare`].
///
/// A hit returns the loaded closure (the callee environment) and stores the
/// entry's code address through `code_out`, which the call half after the
/// arguments calls directly with the receiver as `this`. Anything else stores
/// `0` and returns what [`js_method_site_prepare`] answered: the method value,
/// or one of the by-name answers, which [`js_method_site_call_split`] turns
/// into the call.
///
/// # Safety
/// `slot` is a live method-site slot; `code_out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_method_site_lookup(
    slot: *mut MethodSiteSlot,
    recv: f64,
    method_id: i64,
    argc: usize,
    code_out: *mut u64,
) -> f64 {
    *code_out = 0;
    if bare_collection(recv.to_bits()) {
        return f64::from_bits(METHOD_SITE_BY_NAME_DIRECT);
    }
    lookup_entry(slot, recv, method_id, argc, code_out)
}

/// [`js_method_site_lookup`] past the bare-collection answer: the memo hit,
/// else [`prepare`]. Out of line, so that answer (the one a hot split site
/// on a collection takes on every call, `counts.set(k, f(v))`) runs in a
/// frameless leaf instead of paying this body's prologue.
#[inline(never)]
unsafe fn lookup_entry(
    slot: *mut MethodSiteSlot,
    recv: f64,
    method_id: i64,
    argc: usize,
    code_out: *mut u64,
) -> f64 {
    if let Some((value, code)) = memo_hit(slot, recv.to_bits()) {
        *code_out = code;
        return f64::from_bits(value);
    }
    prepare(slot, recv, method_id, argc)
}

/// A Map or a Set with no metadata record: it has no own property, and the
/// dispatcher answers its methods by name without reading any prototype slot
/// (a patched builtin prototype takes the compile-time route, #11394), so its
/// read is unobservable and no memo entry describes it.
#[inline(always)]
unsafe fn bare_collection(bits: u64) -> bool {
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG
        || !crate::value::addr_class::is_above_handle_band(addr)
    {
        return false;
    }
    let header = &*((addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader);
    if header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        return false;
    }
    match header.obj_type {
        crate::gc::GC_TYPE_MAP => (*(addr as *const crate::map::MapHeader)).meta.is_null(),
        crate::gc::GC_TYPE_SET => (*(addr as *const crate::set::SetHeader)).meta.is_null(),
        _ => false,
    }
}

/// Ordinary objects compare the class/shape word; exotic receivers compare
/// their shape alone. No ordinary-layout prime admits an exotic shape.
#[inline]
fn receiver_word(word: u64) -> u64 {
    const _: () = assert!(
        crate::codegen_abi::METHOD_SITE_SHAPE_ONLY_FROM == super::shapes::EXOTIC_SHAPE_ID_BASE
    );
    if (word >> 32) as u32 >= crate::codegen_abi::METHOD_SITE_SHAPE_ONLY_FROM {
        word & 0xFFFF_FFFF_0000_0000
    } else {
        word
    }
}

/// The fused site's emitted hit, read from the same entry words: `Some((the
/// loaded closure, the entry's code))` exactly when the emitted site would
/// call the memoized code. A plain entry whose slot no longer holds a
/// closure of the recorded body resumes at the next way; every other failed
/// check misses.
#[inline]
unsafe fn memo_hit(slot: *mut MethodSiteSlot, bits: u64) -> Option<(u64, u64)> {
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG
        || !crate::value::addr_class::is_above_handle_band(addr)
    {
        return None;
    }
    // Paired with the runtime's publication of the site record, and with the
    // sticky worker gate (see the emitted site).
    let site = (*(slot as *const std::sync::atomic::AtomicPtr<MethodSite>)).load(Ordering::Acquire);
    if site.is_null() || WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0 {
        return None;
    }
    let word = receiver_word(std::ptr::read(addr as *const u64));
    for e in &(*site).entries {
        if e.word != word {
            continue;
        }
        let s = e.slot;
        let index = (s & METHOD_SITE_INDEX_MASK) as u32;
        let value = match (s & !crate::codegen_abi::METHOD_SITE_NATIVE_ARGS) >> 59 {
            0 => field_bits(addr, s as u32),
            1 => return Some((field_bits(addr, index), e.code)),
            0x11 => {
                let holder = e.closure;
                if std::ptr::read(holder as *const u64) != e.gen {
                    return None;
                }
                return Some((field_bits(holder, index), e.code));
            }
            _ if s & METHOD_SITE_INHERITED != 0 => {
                let holder = e.closure;
                if std::ptr::read(holder as *const u64) != e.gen {
                    return None;
                }
                field_bits(holder, index)
            }
            _ if s & METHOD_SITE_SPILL != 0 => {
                let meta = (*(addr as *const ObjectHeader)).meta;
                if meta.is_null() || (*meta).spill == 0 {
                    return None;
                }
                let spill = (*meta).spill as usize as *const crate::array::ArrayHeader;
                if index >= (*spill).length {
                    return None;
                }
                let elems = (spill as *const u8).add(crate::codegen_abi::ARRAY_HEADER_SIZE);
                std::ptr::read((elems as *const u64).add(index as usize))
            }
            _ if s & METHOD_SITE_FUNCTION_BAG != 0 => {
                let props = (*(addr as *const crate::closure::ClosureHeader)).props;
                if props.is_null() {
                    return None;
                }
                field_bits(props as usize, index)
            }
            _ => return None,
        };
        if closure_of_body(value, e.info) {
            return Some((
                value,
                if s & crate::codegen_abi::METHOD_SITE_NATIVE_ARGS != 0 {
                    0
                } else {
                    e.code
                },
            ));
        }
    }
    None
}

/// Is `value` a live closure (not a forwarded stub) whose body info is
/// `info`? The emitted own-function check.
#[inline]
unsafe fn closure_of_body(value: u64, info: u64) -> bool {
    let h = (value & crate::value::POINTER_MASK) as usize;
    if value & !crate::value::POINTER_MASK != crate::value::POINTER_TAG
        || !crate::value::addr_class::is_above_handle_band(h)
    {
        return false;
    }
    let header = &*((h - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader);
    header.obj_type == crate::gc::GC_TYPE_CLOSURE
        && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
        && (*(h as *const crate::closure::ClosureHeader)).info as u64 == info
}

/// The call half of a split method site when the lookup did not hit: `value`
/// is what [`js_method_site_lookup`] returned. [`METHOD_SITE_BY_NAME`]
/// dispatches through the site's miss (which may prime the memo),
/// [`METHOD_SITE_BY_NAME_DIRECT`] through the universal dispatcher, and any
/// other value is the method itself, called with `recv` as `this`.
///
/// # Safety
/// As [`js_method_site_miss`].
#[no_mangle]
pub unsafe extern "C-unwind" fn js_method_site_call_split(
    value: f64,
    slot: *mut MethodSiteSlot,
    site_id: u64,
    recv: f64,
    method_id: i64,
    args_ptr: *const f64,
    argc: usize,
) -> f64 {
    match value.to_bits() {
        METHOD_SITE_BY_NAME => js_method_site_miss(slot, site_id, recv, method_id, args_ptr, argc),
        METHOD_SITE_BY_NAME_DIRECT => {
            crate::typed_feedback::js_typed_feedback_native_call_method_by_id(
                site_id, recv, method_id, args_ptr, argc,
            )
        }
        _ => js_method_site_call_value(value, recv, args_ptr, argc),
    }
}

/// Does a native cell's chain hold `name` as an accessor the dispatcher would
/// run? An array's chain is `Array.prototype` (whose shape pins its own
/// [[Prototype]]) and `Object.prototype`, unless some array has had its
/// prototype replaced; any other cell kind is answered for `Object.prototype`
/// and, only when that holds the key as an accessor, by the full walk.
unsafe fn cell_chain_has_accessor(recv: f64, kind: Option<u8>, name: &[u8]) -> bool {
    let object = crate::array::object_prototype_addr_if_resolved();
    if object == 0 {
        return lookup_is_observable(recv, name);
    }
    if kind == Some(crate::gc::GC_TYPE_ARRAY) {
        // An array whose [[Prototype]] was replaced sets the process latch.
        let array = crate::array::array_prototype_addr();
        if super::prototype_chain::array_static_proto_recorded() {
            return lookup_is_observable(recv, name);
        }
        return match builtin_prototype_has_accessor(array, object, name) {
            Some(held) => {
                held || super::key_attrs::object_key_is_accessor(
                    object as *const ObjectHeader,
                    name,
                )
            }
            None => lookup_is_observable(recv, name),
        };
    }
    super::key_attrs::object_key_is_accessor(object as *const ObjectHeader, name)
        && lookup_is_observable(recv, name)
}

/// Does the builtin prototype `proto` (`Array.prototype`, a primitive's
/// wrapper prototype, `%Function.prototype%`) hold `name` as an accessor,
/// given that its own [[Prototype]] is the realm's `Object.prototype`
/// (`object`)? `None` when that link is not known (the caller walks):
///
/// * an ordinary object answers from its shape: the link it pins is the realm
///   default, or a recorded link naming `object`, and its key summary holds
///   the accessor bit;
/// * an array exotic (`Array.prototype` is one) inherits the realm default
///   unless the array-prototype latch is set (checked by the caller); its
///   named-property holder shape answers whether `name` is an accessor.
unsafe fn builtin_prototype_has_accessor(proto: usize, object: usize, name: &[u8]) -> Option<bool> {
    if proto == 0 {
        return None;
    }
    match crate::value::addr_class::try_read_gc_header(proto) {
        Some(h) if h.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 => None,
        Some(h) if h.obj_type == crate::gc::GC_TYPE_OBJECT => {
            let p = proto as *const ObjectHeader;
            let linked = match read_holder::admitted_proto_id(p) {
                Some(super::shapes::PROTO_ID_DEFAULT) => true,
                Some(_) => {
                    let (_, word) = read_holder::stated_link(p);
                    word & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
                        && (word & crate::value::POINTER_MASK) as usize == object
                }
                None => false,
            };
            linked.then(|| super::key_attrs::object_key_is_accessor(p, name))
        }
        Some(h)
            if h.obj_type == crate::gc::GC_TYPE_ARRAY
                && !super::prototype_chain::array_static_proto_recorded() =>
        {
            Some(super::descriptor_state::owner_key_is_accessor(proto, name))
        }
        _ => None,
    }
}

/// A function method is a property read before the arguments, just as on
/// an ordinary object. Prime the same shape entry, then snapshot its value:
/// arguments may install an own property or replace a prototype method.
unsafe fn prepare_function(slot: *mut MethodSiteSlot, recv: f64, name: &[u8], argc: usize) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_h = scope.root_nanbox_f64(recv);
    if !slot.is_null() && !site_is_megamorphic(slot) {
        let _no_move = crate::gc::GcSuppressScope::new();
        prime(slot, recv_h.get_nanbox_f64(), name, argc);
    }
    spec_get(recv_h.get_nanbox_f64(), name)
}

/// [`js_method_site_prepare`] for a heap object whose header says object.
unsafe fn prepare_ordinary(
    slot: *mut MethodSiteSlot,
    recv: f64,
    addr: usize,
    name: &[u8],
    argc: usize,
) -> f64 {
    let by_name = f64::from_bits(METHOD_SITE_BY_NAME);
    // Namespace objects, class objects, dictionaries and other exotic readers
    // keep their dispatcher; only an observable read is performed here.
    if ordinary_receiver(addr).is_none() {
        return if lookup_is_observable(recv, name) {
            spec_get(recv, name)
        } else {
            by_name
        };
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_h = scope.root_nanbox_f64(recv);
    if !slot.is_null()
        && WORKER_AGENTS_EXIST.load(Ordering::SeqCst) == 0
        && !site_is_megamorphic(slot)
    {
        let _no_move = crate::gc::GcSuppressScope::new();
        prime(slot, recv_h.get_nanbox_f64(), name, argc);
    }
    // The one [[Get]]: a getter or a Proxy trap on the chain runs here,
    // before the arguments.
    let value = spec_get(recv_h.get_nanbox_f64(), name);
    let vb = value.to_bits();
    if vb & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
        && crate::closure::is_closure_ptr((vb & crate::value::POINTER_MASK) as usize)
    {
        return value;
    }
    // An own data property (the receiver's shape lists the key and no
    // descriptor makes it an accessor) ran nothing: a heap value is called
    // as read, anything else by name.
    let recv = recv_h.get_nanbox_f64();
    let obj = addr_of(recv) as *const ObjectHeader;
    if own_data_key(obj, name) {
        return if vb & !crate::value::POINTER_MASK == crate::value::POINTER_TAG {
            value
        } else {
            by_name
        };
    }
    // Anything else (an inherited intrinsic, a missing method) is answered
    // by name after the arguments, with the dispatcher's own TypeError for a
    // missing one. That re-reads the key, so a read that ran code keeps the
    // value it produced.
    if lookup_is_observable(recv, name) {
        value
    } else {
        by_name
    }
}

fn addr_of(v: f64) -> usize {
    (v.to_bits() & crate::value::POINTER_MASK) as usize
}

/// Does the ordinary object `obj`'s shape list `name` as an own DATA key?
unsafe fn own_data_key(obj: *const ObjectHeader, name: &[u8]) -> bool {
    let Some(shape) = super::shapes::object_shape_descriptor(obj) else {
        return false;
    };
    let keys = shape.keys as usize as *const crate::array::ArrayHeader;
    !keys.is_null()
        && super::keys_find_slot_by_bytes_resolved(keys, shape.logical_key_count, name)
            .is_some_and(|slot| !super::key_attrs::key_is_accessor_at(keys, slot))
}

/// The receiver's OWN accessor for `name`, run now with the receiver as
/// `this`: the one read of a native cell the by-name dispatcher performs that
/// can run code (`native_call_method`'s accessor arm, gated alike). `None`
/// when the cell has no such getter.
unsafe fn own_getter_value(addr: usize, recv: f64, name: &[u8]) -> Option<f64> {
    let key = std::str::from_utf8(name).ok()?;
    let acc = super::descriptor_state::get_accessor_descriptor(addr, key)?;
    let getter = (acc.get & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
    if acc.get == 0 || getter.is_null() {
        return None;
    }
    Some(crate::closure::js_closure_call0(
        getter,
        crate::closure::JsThis::from_f64(recv),
    ))
}

/// Does a primitive's chain hold `name` as an accessor? The chain is its
/// wrapper prototype, whose shape pins its own [[Prototype]], and then
/// `Object.prototype`; each shape's summary answers a prototype with no
/// accessor in one load. Symbols, bigints, an unbuilt realm and a wrapper
/// prototype that no longer inherits from the realm default take the full
/// walk.
unsafe fn primitive_chain_has_accessor(recv: f64, name: &[u8]) -> bool {
    let wrapper = crate::array::primitive_wrapper_prototype_addr(recv);
    let object = crate::array::object_prototype_addr_if_resolved();
    if object == 0 {
        return lookup_is_observable(recv, name);
    }
    match builtin_prototype_has_accessor(wrapper, object, name) {
        Some(held) => {
            held || super::key_attrs::object_key_is_accessor(object as *const ObjectHeader, name)
        }
        None => lookup_is_observable(recv, name),
    }
}

/// Call the value [`js_method_site_prepare`] returned with `recv` as `this`:
/// an object-literal method's baked `this` capture is rebound to the receiver
/// first (#6475), exactly as the by-name dispatcher does. Only that rebind
/// allocates, so only then are the arguments copied and rooted across it.
///
/// # Safety
/// `args_ptr` holds `argc` values.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_method_site_call_value(
    func: f64,
    recv: f64,
    args_ptr: *const f64,
    argc: usize,
) -> f64 {
    // The value a getter or a trap returned is called as is: a nullish one
    // is not callable (the generic value call would answer `undefined`).
    let fb = func.to_bits();
    if fb == crate::value::TAG_UNDEFINED || fb == crate::value::TAG_NULL {
        crate::closure::throw_not_callable();
    }
    let fptr = (fb & crate::value::POINTER_MASK) as usize;
    let rebinds = fb & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
        && crate::closure::is_closure_ptr(fptr)
        && crate::closure::closure_reads_this_from_capture(
            fptr as *const crate::closure::ClosureHeader,
        );
    if !rebinds {
        return crate::closure::native_call_value_this(
            func,
            crate::closure::JsThis::from_f64(recv),
            args_ptr,
            argc,
        );
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_h = scope.root_nanbox_f64(recv);
    let original: Vec<f64> = if argc > 0 && !args_ptr.is_null() {
        std::slice::from_raw_parts(args_ptr, argc).to_vec()
    } else {
        Vec::new()
    };
    let arg_handles = scope.root_nanbox_f64_slice(&original);
    let bound = crate::closure::clone_closure_rebind_this(fb, recv_h.get_nanbox_f64());
    let args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    let this = recv_h.get_nanbox_f64();
    crate::closure::native_call_value_this(
        f64::from_bits(bound),
        crate::closure::JsThis::from_f64(this),
        args.as_ptr(),
        args.len(),
    )
}

/// [`js_method_site_call_value`] for a spread call: `args` is the array the
/// caller bundled every argument into.
///
/// # Safety
/// `args` is null or a live array.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_method_site_call_value_apply(
    func: f64,
    recv: f64,
    args: i64,
) -> f64 {
    let arr = args as *const crate::array::ArrayHeader;
    let len = if arr.is_null() {
        0
    } else {
        crate::array::js_array_length(arr) as usize
    };
    let buf: Vec<f64> = (0..len)
        .map(|i| crate::array::js_array_get_f64(arr, i as u32))
        .collect();
    js_method_site_call_value(func, recv, buf.as_ptr(), buf.len())
}

/// Can reading `name` off `recv` run code or throw? A nullish receiver throws,
/// a Proxy anywhere on the chain traps, and an accessor anywhere on the chain
/// runs its getter. Each holder shape answers whether the key is an accessor.
unsafe fn lookup_is_observable(recv: f64, name: &[u8]) -> bool {
    let bits = recv.to_bits();
    if bits == crate::value::TAG_UNDEFINED || bits == crate::value::TAG_NULL {
        return true;
    }
    let is_pointer = |b: u64| b & !crate::value::POINTER_MASK == crate::value::POINTER_TAG;
    let name_str = std::str::from_utf8(name).ok();
    let mut cur = recv;
    // A prototype chain is acyclic; the bound only guards a corrupt one.
    for _ in 0..256 {
        let b = cur.to_bits();
        if is_pointer(b) {
            let addr = (b & crate::value::POINTER_MASK) as usize;
            if crate::value::addr_class::is_proxy_id_band(addr) {
                return true;
            }
            // Every cell kind answers: an ordinary object from its shape's
            // summary and keys, any other cell (an array exotic prototype, a
            // collection) from its own property holder's shape.
            if crate::value::addr_class::is_above_handle_band(addr) {
                match name_str {
                    Some(n) => {
                        if super::descriptor_state::get_accessor_descriptor(addr, n).is_some() {
                            return true;
                        }
                    }
                    None => return true,
                }
            }
        }
        // The internal read: `js_object_get_prototype_of` is the user-facing
        // one and publishes an exposed iterator prototype (#10086).
        let proto = crate::object::object_ops::get_prototype_of_resolved(cur);
        let pb = proto.to_bits();
        if pb == crate::value::TAG_NULL || pb == crate::value::TAG_UNDEFINED || pb == b {
            return false;
        }
        cur = proto;
    }
    false
}

/// The spec [[Get]] of `name` on `recv`: runs a getter (with `recv` as
/// `this`), a Proxy trap, or throws for a nullish receiver.
unsafe fn spec_get(recv: f64, name: &[u8]) -> f64 {
    // A nullish receiver throws node's TypeError before any argument runs.
    match recv.to_bits() {
        crate::value::TAG_NULL => {
            crate::error::js_throw_type_error_property_access(1, name.as_ptr(), name.len())
        }
        crate::value::TAG_UNDEFINED => {
            crate::error::js_throw_type_error_property_access(0, name.as_ptr(), name.len())
        }
        _ => {}
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_h = scope.root_nanbox_f64(recv);
    // The canonical interned key: no allocation per read.
    let key = crate::string::canonical_key(name);
    let value = crate::object::js_object_get_field_by_name_f64(
        recv_h.get_nanbox_f64().to_bits() as usize as *const ObjectHeader,
        key,
    );
    if value.to_bits() == METHOD_SITE_BY_NAME || value.to_bits() == METHOD_SITE_BY_NAME_DIRECT {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        value
    }
}

/// What one read of a miss receiver's header says.
#[derive(Clone, Copy)]
enum MissReceiver {
    /// A heap object whose GcHeader says ordinary object.
    Ordinary,
    /// A heap function object: its word (`capture_count | ShapeId`), whether
    /// its ShapeId is the base Function shape (keyless, over
    /// `%Function.prototype%`), and whether it is keyed (neither that nor the
    /// FunctionDictionary shape).
    Function { word: u64, base: bool, keyed: bool },
    /// An instance of a native-payload family (`native_payload.rs`).
    Payload,
    /// Anything else: primitives, handles, arrays, strings.
    Other,
}

// A closure's first word is `capture_count | ShapeId`: the ShapeId is its
// high half.
const _: () = assert!(crate::closure::CLOSURE_SHAPE_OFFSET == 4);

#[inline]
fn miss_receiver(recv: f64) -> MissReceiver {
    let bits = recv.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return MissReceiver::Other;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_above_handle_band(addr) {
        return MissReceiver::Other;
    }
    match unsafe { crate::value::addr_class::try_read_gc_header(addr) } {
        Some(h) if h.obj_type == crate::gc::GC_TYPE_OBJECT => {
            // #11919 P0: a native-payload instance's methods are builtins on
            // its family prototype, which a site never memoizes: priming
            // would fail on every call, and the tower would walk every probe
            // before reaching them. The miss answers them directly.
            // SAFETY: the header says a live ordinary object.
            let class_id = unsafe { (*(addr as *const crate::object::ObjectHeader)).class_id };
            if crate::native_class_ids::is_native_payload_class_id(class_id) {
                MissReceiver::Payload
            } else {
                MissReceiver::Ordinary
            }
        }
        Some(h) if h.obj_type == crate::gc::GC_TYPE_CLOSURE => {
            // SAFETY: the header says a live closure; its first word is the
            // `capture_count | ShapeId` word.
            let word = unsafe { std::ptr::read(addr as *const u64) };
            let id = (word >> 32) as u32;
            let (base, dictionary) = crate::closure::shape::function_base_and_dictionary_shapes();
            let keyed = id != base && super::shapes::is_exotic_shape_id(id) && id != dictionary;
            MissReceiver::Function {
                word,
                base: id == base,
                keyed,
            }
        }
        _ => MissReceiver::Other,
    }
}

/// An ordinary heap object or a function whose shape describes its own
/// keys and prototype. Dictionary functions keep generic dispatch.
#[inline]
fn prime_candidate(receiver: MissReceiver, _name: &[u8]) -> bool {
    match receiver {
        MissReceiver::Ordinary => true,
        MissReceiver::Function { word, base, keyed } => {
            base || (keyed
                && super::shapes::shape_object_kind_by_id((word >> 32) as u32)
                    == Some(super::shapes::ShapeObjectKind::Function))
        }
        MissReceiver::Payload | MissReceiver::Other => false,
    }
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
    let Ok(mut sites) = METHOD_SITES.lock() else {
        return false;
    };
    if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0 {
        return false;
    }
    let site = site_of(slot);
    if site.is_null() {
        return false;
    }
    let site = &mut *site;
    // An inherited entry replaces the one for its word (a newer generation);
    // an own entry replaces only the one naming the same slot AND body, so
    // objects of one shape holding different bodies each get an entry (the
    // emitted own hit falls through to the next way on a body mismatch).
    let inherited = entry.slot & METHOD_SITE_INHERITED != 0;
    let idx = site
        .entries
        .iter()
        .position(|e| {
            e.word == entry.word
                && if inherited {
                    e.slot & METHOD_SITE_INHERITED != 0
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
        sites.push(site as *mut MethodSite as usize);
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
        || WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0
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
        function_receiver::prime_function(slot, addr, name, argc);
        return;
    }
    // A per-evaluation class object (`ClassExprFresh`) serves its OWN keys
    // only: its static methods are own data properties of it, born in its
    // template's final shape, but what it inherits follows its pinned
    // parent, not a [[Prototype]] a holder entry could name.
    let (obj, own_only) = match ordinary_receiver(addr) {
        Some(obj) => (obj, false),
        None => match class_object_receiver(addr) {
            Some(obj) => (obj, true),
            None => {
                let dict = crate::value::addr_class::try_read_gc_header(addr)
                    .is_some_and(|h| h.obj_type == crate::gc::GC_TYPE_OBJECT)
                    && !crate::closure::is_closure_ptr(addr)
                    && super::dictionary::is_dictionary(addr as *const ObjectHeader);
                refuse(if dict { 2 } else { 1 });
                return;
            }
        },
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
        if super::key_attrs::key_is_accessor_at(keys, s) {
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
        // A builtin closure (`FN_BUILTIN`, e.g. `o.m = Array.prototype.pop`)
        // is admitted like any body on an ordinary receiver: its thunk takes
        // the receiver as `this` and is exactly what the call would run (see
        // `direct_callable`). A class object is a function object: a borrowed
        // builtin there keeps the dispatcher's native arm, as for any
        // function-object receiver (`prime_function`).
        let Some(info) = direct_callable(value, argc) else {
            refuse(5);
            return;
        };
        if own_only && !is_user_method(value, name) {
            refuse(13);
            return;
        }
        let slot_word = if s < crate::object::field_rep::REP_SLOTS
            && shape.special_constfn_mask & (1 << s) != 0
        {
            // The shape, not this closure object, owns the body fact. A
            // freshly allocated factory closure may have different captures;
            // the hit must still load that receiver's current slot.
            let body = shape
                .constfn_infos()
                .iter()
                .find(|entry| u32::from(entry.slot) == s)
                .map(|entry| entry.info);
            if body != Some(info as *const crate::closure::JsFunctionInfo as u64)
                || slot_word & METHOD_SITE_SPILL != 0
            {
                #[cfg(any(debug_assertions, feature = "field-rep-assert", perry_gc_instruments))]
                if super::field_rep_store::field_rep_verify_enabled() {
                    if let Some(record) =
                        super::shapes::shape_record_by_id(super::shapes::object_shape_stamp(obj))
                    {
                        super::field_rep_store::assert_constfn_slot_body(
                            obj, record, s as usize, value,
                        );
                    }
                }
                refuse(17);
                return;
            }
            if native_args_tag(info) == 0 && declares_at_most(info, argc) {
                slot_word | METHOD_SITE_CONSTFN
            } else {
                // The ConstFn hit passes exactly the call's arguments; a body
                // that wants padding keeps the info-checked plain entry.
                slot_word
            }
        } else {
            slot_word
        };
        let entry = MethodEntry {
            word,
            slot: slot_word | native_args_tag(info),
            info: info as *const crate::closure::JsFunctionInfo as u64,
            code: info.code as u64,
            closure: 0,
            gen: 0,
        };
        if publish(slot, entry) {
            PRIMES_OWN.fetch_add(1, Ordering::Relaxed);
            note_builtin_prime(info);
            if slot_word & METHOD_SITE_CONSTFN != 0 {
                PRIMES_CONSTFN.fetch_add(1, Ordering::Relaxed);
            }
        }
        return;
    }
    if own_only {
        refuse(1);
        return;
    }
    prime_inherited(slot, obj, word, name, argc);
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

/// A class object an own entry may serve: everything [`ordinary_receiver`]
/// asks of an ordinary object, on an object whose shape kind is `Class`.
///
/// # Safety
/// `addr` is a plausible object address.
unsafe fn class_object_receiver(addr: usize) -> Option<*const ObjectHeader> {
    if !crate::value::addr_class::is_above_handle_band(addr) {
        return None;
    }
    let header = crate::value::addr_class::try_read_gc_header(addr)?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || crate::closure::is_closure_ptr(addr)
        || !address_is_prime_stable(addr)
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
        || !super::class_registry::is_class_object_ptr(addr as *const u8)
    {
        return None;
    }
    // A class object is not `object_is_regular` (its kind is `Class`), but
    // its own keys live in inline slots exactly as an ordinary object's do.
    let obj = addr as *const ObjectHeader;
    if super::dictionary::is_dictionary(obj)
        || !super::shapes::is_shape_id(super::shapes::object_shape_stamp(obj))
    {
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

/// A function-object receiver holding a borrowed builtin
/// (`F.get = Map.prototype.get`) keeps the dispatcher's native arm, exactly as
/// `own_override::resolve_own_user_method` decides. Ordinary receivers call
/// the builtin's thunk directly.
fn is_user_method(value_bits: u64, name: &[u8]) -> bool {
    match std::str::from_utf8(name) {
        Ok(name) => crate::array::value_is_own_user_method(f64::from_bits(value_bits), name),
        Err(_) => false,
    }
}

/// A ConstFn hit calls the body with exactly the call's `argc` arguments (the
/// plain hit pads with `undefined`, see `method_site_padded_argc`), so a
/// ConstFn entry is admitted only for a body declaring at most `argc`.
fn declares_at_most(info: &crate::closure::JsFunctionInfo, argc: usize) -> bool {
    matches!(
        crate::closure::resolve_strategy(info).kind(),
        crate::closure::DispatchKind::Arity(declared) if declared as usize <= argc
    )
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
    if native_args_tag(info) == 0 && crate::closure::info_rest(info).is_some() {
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
    if native_args_tag(info) != 0 {
        return Some(info);
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

fn native_args_tag(info: &crate::closure::JsFunctionInfo) -> u64 {
    if info.flags & crate::codegen_abi::FN_REST_NATIVE_ARGS != 0 {
        crate::codegen_abi::METHOD_SITE_NATIVE_ARGS
    } else {
        0
    }
}

/// Prime an inherited entry when the receiver's shape pins a direct holder
/// with `name` in a plain inline data slot. Deeper chains use ordinary dispatch.
unsafe fn prime_inherited(
    slot: *mut MethodSiteSlot,
    obj: *const ObjectHeader,
    word: u64,
    name: &[u8],
    argc: usize,
) {
    if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0 {
        refuse(11);
        return;
    }
    // A declared-class instance's direct prototype is its class's prototype
    // object (a bare CLASS identity: the class's function object keeps that
    // link for the agent's life, and a relink retires the displaced
    // prototype's ShapeId) or the serial a MIXED identity records. Its
    // methods are real slots of that object (class prototypes hold function
    // objects of their bodies, with ConstFn lanes), so the entry is the same
    // holder entry as for any receiver.
    let class_id = (*obj).class_id;
    let class_instance = class_id != 0
        && class_id < super::class_registry::prototype_objects::SYNTHETIC_CLASS_ID_BASE
        && !super::is_anon_shape_class_id(class_id);
    // The caller already proved the receiver lacks the name. Only the
    // holder's resolved slot can carry its descriptor facts.
    let class_holder = if class_instance {
        match read_holder::class_link(obj) {
            Some(holder) => Some(holder),
            None => {
                refuse(6);
                return;
            }
        }
    } else {
        None
    };
    // Otherwise only a serial or the realm-default identity pins one direct
    // prototype.
    let proto_id = match class_holder {
        Some(_) => super::shapes::PROTO_ID_CLASS,
        None => match read_holder::admitted_proto_id(obj) {
            Some(pid) => pid,
            None => {
                refuse(7);
                return;
            }
        },
    };
    let next = if let Some(holder) = class_holder {
        holder
    } else if proto_id == super::shapes::PROTO_ID_DEFAULT {
        crate::array::object_prototype_addr_if_resolved() as *const ObjectHeader
    } else {
        next_prototype(obj)
    };
    prime_holder(slot, next, word, name, argc);
}

/// The next prototype the way the inherited-read walk resolves it: the meta
/// record's `[[Prototype]]`, else a synthetic class's (`Object.create`, an ES5
/// constructor) registered prototype. Null for a default builtin prototype.
unsafe fn next_prototype(obj: *const ObjectHeader) -> *const ObjectHeader {
    let recorded = crate::object::shapes::object_prototype_word(obj);
    if recorded != 0 {
        let p = crate::value::JSValue::from_bits(recorded);
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

/// Root scan: before workers exist, every inherited entry's holder is marked
/// and rewritten. After the sticky gate, no emitted or runtime path reads an
/// entry; returning here lets otherwise-dead holders collect. A primary
/// inline hit begun before the gate cannot safepoint between its entry read
/// and method call, so no primary GC can observe an in-flight holder read.
pub(crate) fn scan_method_site_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if crate::agent::current_agent() != crate::agent::PRIMARY_AGENT
        || WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0
    {
        return;
    }
    if let Ok(sites) = METHOD_SITES.lock() {
        for &site in sites.iter() {
            for e in unsafe { (*(site as *mut MethodSite)).entries.iter_mut() } {
                if e.closure != 0 {
                    if visitor.visit_tagged_usize_slot(&mut e.closure, crate::value::POINTER_TAG) {
                        HOLDER_REWRITES.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod constfn_tests {
    use super::*;

    extern "C" fn method(
        _closure: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        7.0
    }

    unsafe fn one_method(info: *const crate::closure::JsFunctionInfo) -> (*mut ObjectHeader, u32) {
        let closure = crate::closure::js_closure_alloc(info, 0);
        let obj = crate::object::js_object_alloc(0, 4);
        let key = b"constfn_site_method";
        let name = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
        crate::object::js_object_set_field_by_name(
            obj,
            name,
            crate::value::js_nanbox_pointer(closure as i64),
        );
        (obj, super::super::shapes::object_shape_stamp(obj))
    }

    unsafe fn primed_slot(obj: *mut ObjectHeader) -> u64 {
        let mut slot: MethodSiteSlot = std::ptr::null_mut();
        prime(
            &mut slot,
            crate::value::js_nanbox_pointer(obj as i64),
            b"constfn_site_method",
            0,
        );
        assert!(!slot.is_null(), "eligible method site must prime");
        let word = std::ptr::read(obj as *const u64);
        (*slot)
            .entries
            .iter()
            .find(|entry| entry.word == word)
            .expect("site entry for receiver shape")
            .slot
    }

    #[test]
    fn constfn_site_uses_shape_body_fact_only_for_permanent_images() {
        if !run_with_fresh_worker_gate(
            "constfn_site_uses_shape_body_fact_only_for_permanent_images",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let _no_move = crate::gc::GcSuppressScope::new();
            let permanent =
                crate::fn_info!(method, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE));
            let (object, id) = one_method(permanent);
            let d = super::super::shapes::shape_descriptor_by_id(id).expect("shape");
            assert_eq!(d.special_constfn_mask, 1);
            assert_eq!(primed_slot(object), METHOD_SITE_CONSTFN);

            // An unloadable image has no ConstFn shape fact. It may still use
            // the existing guarded own-method entry, which validates the
            // closure's kind and body info on every hit.
            let transient = crate::fn_info!(method, 0);
            let (object, id) = one_method(transient);
            let d = super::super::shapes::shape_descriptor_by_id(id).expect("shape");
            assert_eq!(d.special_constfn_mask, 0);
            assert_eq!(primed_slot(object), 0);
        }
    }

    #[test]
    fn constfn_static_captured_this_arrow_primes_and_rebinding_closure_refuses() {
        if !run_with_fresh_worker_gate(
            "constfn_static_captured_this_arrow_primes_and_rebinding_closure_refuses",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let _no_gc = crate::gc::GcSuppressScope::new();
            let arrow = crate::fn_info!(method, 0; with_flags(
                crate::codegen_abi::FN_PERMANENT_IMAGE | crate::closure::FN_ARROW
            ));
            let rebinding =
                crate::fn_info!(method, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE));
            let packed = b"constfn_site_method\0";
            let keys =
                super::super::static_shapes::canonical_keys_for_names(&[b"constfn_site_method"]);
            for (info, admitted) in [(arrow, true), (rebinding, false)] {
                let obj = crate::object::alloc_plain::alloc_plain_record_inline_keys_stamped(
                    1,
                    keys.arr() as *mut _,
                    0,
                );
                let base = super::super::shapes::object_shape_stamp(obj);
                let birth = super::super::shapes::shape_descriptor_by_id(base).unwrap();
                assert_eq!(
                    birth.object_kind,
                    super::super::shapes::ShapeObjectKind::Ordinary
                );
                assert_eq!(birth.special_constfn_mask, 0, "allocation must stay Any");
                let c =
                    crate::closure::js_closure_alloc(info, crate::closure::CAPTURES_THIS_FLAG | 1);
                crate::closure::js_closure_set_capture_bits(
                    c,
                    0,
                    crate::JSValue::object_ptr(obj.cast()).bits(),
                );
                crate::object::store_object_field_slot(
                    obj,
                    0,
                    crate::JSValue::object_ptr(c.cast()).bits(),
                );
                let entries = [super::super::static_shapes::ConstFnStaticEntry { slot: 0, info }];
                let finalized = super::super::static_shapes::js_object_finalize_constfn_static(
                    obj as usize as u64,
                    0,
                    packed.as_ptr(),
                    packed.len() as u32,
                    1,
                    1,
                    0,
                    super::super::field_rep::REP_SPECIAL,
                    entries.as_ptr(),
                    1,
                ) as usize as *mut ObjectHeader;
                let id = super::super::shapes::object_shape_stamp(finalized);
                let d = super::super::shapes::shape_descriptor_by_id(id).unwrap();
                assert_eq!(d.special_constfn_mask != 0, admitted);
                if admitted {
                    assert_ne!(base, id, "ordinary allocation must finalize after stores");
                    assert!(super::super::field_rep_store::final_shape_matches_birth(
                        id, base
                    ));
                    assert_eq!(primed_slot(finalized), METHOD_SITE_CONSTFN);
                    assert_eq!(
                        crate::closure::js_closure_get_capture_bits(c, 0),
                        crate::JSValue::object_ptr(finalized.cast()).bits()
                    );
                } else {
                    assert_eq!(id, base, "captured-this rebinding remains excluded");
                }
            }
        }
    }
}

#[cfg(test)]
mod report_names_line_up {
    #[test]
    fn refusal_names_cover_every_counter() {
        assert_eq!(super::refusal_name(0), "not_object_pointer");
        assert_eq!(super::refusal_name(11), "inh_workers");
        assert_eq!(super::refusal_name(18), "site_megamorphic");
        assert_eq!(super::refusal_name(19), "function_implicit_own_key");
        for i in 0..super::SITE_REFUSED.len() {
            assert_ne!(super::refusal_name(i), "?", "unnamed counter {i}");
        }
        assert_eq!(super::refusal_name(super::SITE_REFUSED.len()), "?");
    }
}
