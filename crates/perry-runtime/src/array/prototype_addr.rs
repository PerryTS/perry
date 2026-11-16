//! The memoized `Array.prototype` / `Object.prototype` / `Function.prototype`
//! addresses (#6981, #10497), **one set per thread** (#7988).
//!
//! Three `usize` cells and the algebra over them: lazy resolution from
//! `globalThis`, healing through the GC forwarding chain, and the registered
//! root scanner that lets a relocating cycle rewrite them.
//!
//! Split out of `indexing.rs` because it is one subject with one invariant —
//! *every cell an accessor reads is a cell the collector rewrites* — and that
//! invariant is easiest to keep true when the cells, the accessors and the
//! scanner are the only things in the file (and it kept `indexing.rs` under
//! `scripts/check_file_size.sh`'s 2000-line cap).
//!
//! # Why the cells are PER-THREAD (#7988)
//!
//! Both cells hold a **raw address of an object in a thread-local arena**, and
//! the realm they name is per-thread: `js_get_global_this` bootstraps
//! `THREAD_GLOBAL_THIS` once *per thread*, so every `perry/thread` agent has
//! its own `Array.prototype` and its own `Object.prototype` in its own heap.
//! When the cells were process-global `AtomicUsize` statics they missed only
//! once *per process*, so the first thread to touch either intrinsic decided
//! the value for every other agent, with three consequences:
//!
//!   1. **Wrong identity.** [`object_prototype_addr_matches`] on agent B
//!      compared B's objects against *A's* `Object.prototype`, so B's own
//!      intrinsic was never recognised. `array_oob_prototype_get`'s hole/OOB
//!      fallbacks and the typed-feedback guards that consult it silently took
//!      the other branch on every thread but the first — B's
//!      `Object.prototype[7] = v` never even flipped `OBJECT_PROTO_HAS_INDEX`,
//!      because the write hook's "is this the prototype?" test compared against
//!      a foreign address.
//!   2. **Unattributed dereference.** [`heal_prototype_addr`] reads the cached
//!      address's `GcHeader` from *any* thread, and `note_array_index_write`
//!      calls it on every indexed array write. A's collector can sweep or move
//!      that object, and A's arena blocks are `dealloc`'d at thread exit, so
//!      the read was on memory the reading thread had no claim to.
//!   3. **Cross-thread root rewrite.** [`scan_prototype_addr_cache_roots_mut`]
//!      is registered per thread and *writes* the cell with **its own**
//!      to-space address, so agent B's collector could overwrite a cell naming
//!      A's heap with a B-heap address.
//!
//! All three are structural once the storage is per-thread: a cell only ever
//! holds an address this thread allocated, healed by this thread's forwarding
//! chain and rewritten by this thread's collector.
//!
//! # Why a thread-local is affordable here
//!
//! [`crate::perry_thread_local`] is not `std::thread_local!`: the address of
//! the value lands in this thread's [`crate::tls_hot::HotTls`] cache, which on
//! Apple aarch64 is reached with an `mrs` plus two loads that LLVM CSEs across
//! the enclosing function — not the out-of-line `_tlv_get_addr` call that made
//! "Darwin has no local-exec TLS" the recorded objection to this fix (#7955).
//! Both intrinsics share ONE declaration (an array indexed by
//! [`ARRAY_PROTO_CACHE`] / [`OBJECT_PROTO_CACHE`]) so a function that consults
//! both — `array_oob_prototype_get` does — pays one resolution, not two.

use std::cell::Cell;

/// How many intrinsic prototype addresses one realm memoizes.
///
/// This is the length of BOTH the per-thread cell array and the builtin-name
/// table below, and the root scanner iterates the cell array itself, so
/// "a cell an accessor reads that the collector never rewrites" — the #6981
/// defect — is not representable. Adding a third memoized intrinsic address
/// means bumping this and adding a row to [`PROTOTYPE_ADDR_BUILTINS`]; it is
/// then covered by both halves automatically.
const PROTOTYPE_ADDR_CACHE_COUNT: usize = crate::tls_hot::INLINE_PROTOTYPE_ADDR_ROWS;

/// Row index of the `Object.prototype` cell.
const OBJECT_PROTO_CACHE: usize = 1;

/// **THIS THREAD's** lazily-memoized intrinsic prototype addresses, indexed
/// by [`ARRAY_PROTO_CACHE`] / [`OBJECT_PROTO_CACHE`]. `usize::MAX` marks a
/// row as not-yet-computed.
///
/// Row 0 is `Array.prototype`. An out-of-bounds element read on an ordinary
/// array must fall through to `Array.prototype[index]` (ECMA-262
/// OrdinaryGet → prototype chain), but in real code nobody adds numeric
/// indices to `Array.prototype`, so the hot OOB path stays one load until
/// the (rare) write flips `ARRAY_PROTO_HAS_INDEX`.
///
/// Row 1 is `Object.prototype`: a numeric index installed there
/// (`Object.prototype[2] = 2`, or a defineProperty accessor) shows through
/// array HOLES and OOB reads (chain: arr → Array.prototype →
/// Object.prototype; test262 concat/S15.4.4.4_A3_T3). Consulted by the
/// typed-feedback guards and the hole/OOB read fallbacks.
///
/// Row 2 is `Function.prototype` (#10497). The universal method dispatcher's
/// own-override check asks `js_object_has_own`, which asks "is this receiver
/// %Function.prototype%?" on every dispatched call, and that question used to
/// be answered by looking `globalThis.Function` up BY NAME and then reading its
/// `prototype` dynamic property — 5–6% of Perry's excess instructions over
/// Node across the package suite (#11464). The identity is fixed per realm:
/// `Function.prototype` is installed `{writable:false, configurable:false}`
/// (`populate_global_this_builtins`), and mutating the object — adding or
/// replacing `call`/`apply`/`bind`, `Object.setPrototypeOf` on it or on any
/// function — changes its KEYS, never which object it is. Those keys are still
/// read through the ordinary property path by every caller; only the identity
/// is memoized. The row is primed at the end of `populate_global_this_builtins`
/// ([`prime_prototype_addr_cache`]), so a later `globalThis.Function = X`
/// (writable per spec) cannot make it name `X.prototype`: it always names the
/// realm's intrinsic, which is what every caller means by %Function.prototype%.
///
/// ***THESE ARE RAW ADDRESSES OF MOVABLE OBJECTS*** (#6981).
/// `Array.prototype` relocates two different ways, and BOTH leave the cache
/// pointing at a `GC_FLAG_FORWARDED` stub while every reader resolves its
/// own receiver through `clean_arr_ptr` (which follows forwarding):
///
///   1. `js_array_grow` — an indexed write past the dense capacity
///      (`Array.prototype[300] = v`) reallocates and forwards the old head;
///   2. the copying young-gen minor — it evacuates the prototype and
///      forwards.
///
/// A stale cache is not merely a wrong value: `array_oob_prototype_get`'s
/// self-recursion guard is `proto != receiver`, and after a move those are
/// two different addresses **for the same object**, so the guard stops
/// firing and `js_array_get_f64` ⇄ `array_oob_prototype_get` recurse until
/// the stack guard page (SIGSEGV, "excessive recursion"). Hence the two
/// defences below: [`memoized_prototype_addr`] resolves the forwarding
/// chain and self-heals, and [`scan_prototype_addr_cache_roots_mut`] lets
/// the collector rewrite the slot so the address stays live even once the
/// from-space stub is recycled.
///
/// The rows live INLINE in this thread's [`crate::tls_hot::HotTls`]: they are
/// consulted on every indexed array write, and a generic hot slot cost one
/// more dependent load than the value itself.
#[inline(always)]
/// This thread's `Object.prototype` cell (the collector rewrites it; it holds
/// `usize::MAX` until resolved), for the unknown-receiver probe, which reads
/// it through the agent's shape directory instead of a thread-local access.
pub(crate) fn object_prototype_cell() -> *const usize {
    prototype_addrs()[OBJECT_PROTO_CACHE].as_ptr()
}

fn prototype_addrs() -> &'static [Cell<usize>; PROTOTYPE_ADDR_CACHE_COUNT] {
    &crate::tls_hot::hot().prototype_addrs
}

/// How many intrinsic entries a realm has: one per builtin declaration
/// (`BuiltinConstructorIdentity`, 1-based).
const INTRINSIC_ENTRY_COUNT: usize = crate::object::GLOBAL_THIS_BUILTIN_CONSTRUCTORS.len();

/// The realm's [[Intrinsics]] for builtin prototypes: `%Date.prototype%`,
/// `%TypeError.prototype%`, `%Set.prototype%`, … — the object each family's
/// instances inherit from, recorded when the realm binds `globalThis.<Name>`
/// (`populate_global_this_builtins`). A later `globalThis.<Name> = X` replaces
/// the writable binding only; every "what does this builtin inherit from"
/// question is answered here, never through that binding.
///
/// The same entry holds the family's original constructor (`%Date%`,
/// `%Array%`, …): every "is this the builtin itself" question
/// (`C === %Promise%`, `Array.prototype`'s own `constructor`) and every
/// inherited `<instance>.constructor` answer reads it, never the binding.
///
/// One entry per [`crate::object::BuiltinConstructorIdentity`], all-zero
/// (unrecorded) at birth, so it lives in the zero-filled
/// [`crate::state::RuntimeState`] and costs nothing before the realm exists.
/// The cells hold raw addresses of movable objects and follow the same
/// algebra as the hot rows above: healed through the forwarding chain on
/// read, rewritten by [`scan_prototype_addr_cache_roots_mut`].
pub(crate) struct RealmIntrinsics {
    entries: [IntrinsicEntry; INTRINSIC_ENTRY_COUNT],
    /// The realm's function intrinsics that no family owns (`%parseFloat%`,
    /// `%parseInt%`): the same cell algebra as the family cells.
    functions: [Cell<usize>; REALM_FUNCTION_INTRINSIC_COUNT],
}

/// A function intrinsic with its own [[Intrinsics]] cell: `%parseFloat%` is
/// both `globalThis.parseFloat` and `Number.parseFloat` (ECMA-262 21.1.2.12),
/// so whichever of the two is observed first births it.
#[derive(Clone, Copy)]
pub(crate) enum RealmFunctionIntrinsic {
    ParseFloat = 0,
    ParseInt = 1,
}
const REALM_FUNCTION_INTRINSIC_COUNT: usize = 2;

/// The thread realm's [[Intrinsics]] record (the main realm of this thread).
#[inline(always)]
pub(crate) fn thread_realm() -> &'static RealmIntrinsics {
    &crate::state::state().realm_intrinsics
}

/// Whether `realm` is the thread realm's record (the one the thread
/// singletons memoize), not a `vm` realm's [`RealmRecord`].
#[inline(always)]
pub(crate) fn is_thread_realm(realm: &RealmIntrinsics) -> bool {
    std::ptr::eq(realm, thread_realm())
}

/// This realm's function intrinsic `which` if born, else 0.
#[inline]
pub(crate) fn realm_function_intrinsic_if_bound(which: RealmFunctionIntrinsic) -> usize {
    realm_function_intrinsic_if_bound_in(thread_realm(), which)
}

/// `realm`'s function intrinsic `which` if born, else 0.
#[inline(always)]
pub(crate) fn realm_function_intrinsic_if_bound_in(
    realm: &RealmIntrinsics,
    which: RealmFunctionIntrinsic,
) -> usize {
    let cell = &realm.functions[which as usize];
    match cell.get() {
        0 => 0,
        addr => heal_prototype_addr(cell, addr),
    }
}

/// Record the birth of the function intrinsic `which` (first birth wins).
pub(crate) fn note_realm_function_intrinsic(which: RealmFunctionIntrinsic, addr: usize) {
    note_realm_function_intrinsic_in(thread_realm(), which, addr)
}

/// Record in `realm` the birth of its function intrinsic `which` (first birth wins).
pub(crate) fn note_realm_function_intrinsic_in(
    realm: &RealmIntrinsics,
    which: RealmFunctionIntrinsic,
    addr: usize,
) {
    let cell = &realm.functions[which as usize];
    if cell.get() == 0 {
        cell.set(addr);
    }
}

/// One builtin's realm intrinsics: its constructor and its prototype.
struct IntrinsicEntry {
    prototype: Cell<usize>,
    constructor: Cell<usize>,
}

fn intrinsic_entry_in(
    realm: &RealmIntrinsics,
    identity: crate::object::BuiltinConstructorIdentity,
) -> &IntrinsicEntry {
    &realm.entries[identity as usize - 1]
}

/// Record this realm's intrinsic prototype and constructor of `identity`
/// (either may be 0: a builtin without a prototype object). The first realm
/// populated on a thread owns the entries, as it owns the hot rows
/// ([`prime_prototype_addr_cache`]).
pub(crate) fn note_realm_intrinsic(
    identity: crate::object::BuiltinConstructorIdentity,
    prototype: usize,
    constructor: usize,
) {
    note_realm_intrinsic_in(thread_realm(), identity, prototype, constructor)
}

/// Record `realm`'s intrinsic prototype and constructor of `identity`, under
/// the rule of [`note_realm_intrinsic`].
pub(crate) fn note_realm_intrinsic_in(
    realm: &RealmIntrinsics,
    identity: crate::object::BuiltinConstructorIdentity,
    prototype: usize,
    constructor: usize,
) {
    let entry = intrinsic_entry_in(realm, identity);
    if prototype != 0 && entry.prototype.get() == 0 {
        entry.prototype.set(prototype);
    }
    if constructor != 0 && entry.constructor.get() == 0 {
        entry.constructor.set(constructor);
    }
}

/// This realm's intrinsic prototype of `identity`, or 0 when the realm has
/// none (a builtin not yet bound while the realm is being populated). Builds
/// the realm when this thread has none yet, exactly as the `globalThis`
/// read it replaces did.
pub(crate) fn intrinsic_prototype_addr(
    identity: crate::object::BuiltinConstructorIdentity,
) -> usize {
    intrinsic_prototype_addr_in(thread_realm(), identity)
}

/// `realm`'s intrinsic prototype of `identity`, born in `realm` first when
/// the family is still unborn there.
#[inline(always)]
pub(crate) fn intrinsic_prototype_addr_in(
    realm: &RealmIntrinsics,
    identity: crate::object::BuiltinConstructorIdentity,
) -> usize {
    realm_intrinsic_addr(realm, &intrinsic_entry_in(realm, identity).prototype, Some(identity))
}

/// This realm's intrinsic prototype of `identity` if the realm has bound it,
/// else 0. Never builds the realm: a prototype that was never born cannot
/// have been written either (the per-call `dv.get*` guard asks exactly that).
#[inline]
pub(crate) fn intrinsic_prototype_addr_if_bound(
    identity: crate::object::BuiltinConstructorIdentity,
) -> usize {
    intrinsic_prototype_addr_if_bound_in(thread_realm(), identity)
}

/// `realm`'s intrinsic prototype of `identity` if bound, else 0. Never builds.
#[inline(always)]
pub(crate) fn intrinsic_prototype_addr_if_bound_in(
    realm: &RealmIntrinsics,
    identity: crate::object::BuiltinConstructorIdentity,
) -> usize {
    let cell = &intrinsic_entry_in(realm, identity).prototype;
    match cell.get() {
        0 => 0,
        addr => heal_prototype_addr(cell, addr),
    }
}

/// This realm's original constructor of `identity` (`%Promise%`, …), or 0
/// while the realm is being populated, under the same rule as
/// [`intrinsic_prototype_addr`].
pub(crate) fn intrinsic_constructor_addr(
    identity: crate::object::BuiltinConstructorIdentity,
) -> usize {
    intrinsic_constructor_addr_in(thread_realm(), identity)
}

/// `realm`'s intrinsic constructor of `identity`, born in `realm` first when
/// the family is still unborn there.
#[inline(always)]
pub(crate) fn intrinsic_constructor_addr_in(
    realm: &RealmIntrinsics,
    identity: crate::object::BuiltinConstructorIdentity,
) -> usize {
    realm_intrinsic_addr(realm, &intrinsic_entry_in(realm, identity).constructor, Some(identity))
}

/// This realm's original constructor of `identity` if the realm has bound
/// it, else 0. Never builds the realm.
#[inline]
pub(crate) fn intrinsic_constructor_addr_if_bound(
    identity: crate::object::BuiltinConstructorIdentity,
) -> usize {
    intrinsic_constructor_addr_if_bound_in(thread_realm(), identity)
}

/// `realm`'s intrinsic constructor of `identity` if bound, else 0. Never builds.
#[inline(always)]
pub(crate) fn intrinsic_constructor_addr_if_bound_in(
    realm: &RealmIntrinsics,
    identity: crate::object::BuiltinConstructorIdentity,
) -> usize {
    let cell = &intrinsic_entry_in(realm, identity).constructor;
    match cell.get() {
        0 => 0,
        addr => heal_prototype_addr(cell, addr),
    }
}

/// The one materialize point of the realm's intrinsics: builds the realm when
/// this thread has none yet, then births a `lazy` family
/// (`BuiltinConstructorDeclaration::lazy`) whose prototype `family` names
/// the first time it is asked for. Every reader that may build goes through
/// here; the `_if_bound` readers never build (an unborn family was never
/// written either).
fn realm_intrinsic_addr(
    realm: &RealmIntrinsics,
    cell: &Cell<usize>,
    family: Option<crate::object::BuiltinConstructorIdentity>,
) -> usize {
    let mut addr = cell.get();
    let birth = |addr: usize| match family {
        Some(identity) if addr == 0 && identity.declaration().lazy => {
            crate::object::birth_lazy_family_in(realm, identity);
            cell.get()
        }
        // The root family Function is born by its own setup birth.
        Some(crate::object::BuiltinConstructorIdentity::Function) if addr == 0 => {
            crate::object::ensure_function_intrinsics_in(realm);
            cell.get()
        }
        _ => addr,
    };
    // A family is born by itself; only what no birth makes (a non-family
    // intrinsic, `Object`, `Buffer`) observes %Global%.
    addr = birth(addr);
    if addr == 0 && is_thread_realm(realm) && !crate::object::global_this_is_materialized() {
        crate::object::js_get_global_this();
        addr = birth(cell.get());
    }
    if addr == 0 {
        return 0;
    }
    heal_prototype_addr(cell, addr)
}

/// GC root scanner for this thread's memoized prototype addresses (#6981).
///
/// The cells hold raw addresses of movable objects, so a relocating cycle must
/// REWRITE them exactly like the other address-holding side tables
/// (`CLASS_PROTOTYPE_OBJECTS`, …). Forwarding-chain
/// healing alone is not sufficient: once the from-space stub is swept and its
/// block recycled the `GC_FLAG_FORWARDED` bit is gone, and the cache would then
/// name an unrelated live object. Both intrinsics are reachable from
/// `globalThis`, so the marking half of this visit is redundant; the rewriting
/// half is the point.
///
/// #7988: the scanner runs on the collecting thread and now visits **that
/// thread's** cells. Before the storage was per-thread it wrote the collecting
/// thread's to-space address into a cell that could be naming another agent's
/// heap.
pub fn scan_prototype_addr_cache_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    for cell in prototype_addrs() {
        rewrite_prototype_addr_slot(cell, visitor);
    }
    scan_realm_intrinsics(thread_realm(), visitor);
}

/// Rewrite every cell of one realm's [[Intrinsics]] record through the
/// forwarding chain, under the algebra of [`rewrite_prototype_addr_slot`].
pub(crate) fn scan_realm_intrinsics(
    realm: &RealmIntrinsics,
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    for entry in &realm.entries {
        rewrite_prototype_addr_slot(&entry.prototype, visitor);
        rewrite_prototype_addr_slot(&entry.constructor, visitor);
    }
    for cell in &realm.functions {
        rewrite_prototype_addr_slot(cell, visitor);
    }
}

/// A `vm` realm's [[Intrinsics]] record as a GC heap object
/// (`GC_TYPE_REALM_RECORD`): exactly the realm's [`RealmIntrinsics`] plus its
/// `global` cell. Reached only from the realm's own objects (its global's
/// `[[Intrinsics]]` slot); the thread realm's record stays thread state.
/// Every cell holds a raw address under the algebra of
/// [`scan_realm_intrinsics`] (0 = unborn), traced by the type's gc arm.
#[repr(C)]
pub(crate) struct RealmRecord {
    intrinsics: RealmIntrinsics,
    /// The realm global's address.
    global: Cell<usize>,
}

/// A new record of the realm whose global is `global`, every family unborn.
///
/// # Safety
/// Inside a no-move window; `global` is the realm global just born.
pub(crate) unsafe fn alloc_realm_record(global: usize) -> *mut RealmRecord {
    let size = std::mem::size_of::<RealmRecord>();
    let record = crate::arena::arena_alloc_gc(size, 8, crate::gc::GC_TYPE_REALM_RECORD)
        as *mut RealmRecord;
    // GC_STORE_AUDIT(INIT): unpublished record; zero cells are unborn, and the
    // global is born in the same no-move window (no older-to-younger edge).
    std::ptr::write_bytes(record as *mut u8, 0, size);
    (*record).global.set(global);
    record
}

/// The global of the `vm` realm whose record holds `realm`.
///
/// # Safety
/// `realm` is a `vm` realm's record intrinsics (not [`thread_realm`]).
pub(crate) unsafe fn realm_record_global(realm: &RealmIntrinsics) -> usize {
    // `RealmRecord` is `repr(C)` with its intrinsics first.
    (*(realm as *const RealmIntrinsics as *const RealmRecord)).global.get()
}

/// The [[Intrinsics]] record at `record` (a `GC_TYPE_REALM_RECORD` payload).
///
/// # Safety
/// `record` is a live `GC_TYPE_REALM_RECORD` payload.
pub(crate) unsafe fn realm_record_intrinsics<'a>(record: usize) -> &'a RealmIntrinsics {
    &(*(record as *const RealmRecord)).intrinsics
}

/// The record at `user_ptr` as its cell words: every field of
/// [`RealmRecord`] is a `Cell<usize>` (the family entries, the function
/// intrinsics, `global`), so the record is exactly its cells, each under the
/// algebra of [`scan_realm_intrinsics`] ([`realm_record_cell_is_born`]).
///
/// # Safety
/// `user_ptr` is a live `GC_TYPE_REALM_RECORD` payload.
unsafe fn realm_record_cell_words(user_ptr: usize) -> (*mut usize, usize) {
    const WORDS: usize = std::mem::size_of::<RealmRecord>() / std::mem::size_of::<usize>();
    const _: () = assert!(WORDS * std::mem::size_of::<usize>() == std::mem::size_of::<RealmRecord>());
    (user_ptr as *mut usize, WORDS)
}

/// The `GC_TYPE_REALM_RECORD` arm's view of a record (see `crate::feature_hooks`).
/// Records are born only by `vm` realm globals, so the `vm` install fills it
/// before any record exists; empty, no record exists and the arm never runs.
static REALM_RECORD_CELLS: crate::feature_hooks::Hook<unsafe fn(usize) -> (*mut usize, usize)> =
    crate::feature_hooks::Hook::empty();

/// The cell words of the record at `user_ptr` for the GC arm of
/// `GC_TYPE_REALM_RECORD` (mark, relocation and rescan).
///
/// # Safety
/// `user_ptr` is a live `GC_TYPE_REALM_RECORD` payload.
#[inline(always)]
pub(crate) unsafe fn realm_record_cells(user_ptr: usize) -> (*mut usize, usize) {
    match REALM_RECORD_CELLS.peek() {
        Some(cells) => cells(user_ptr),
        None => (std::ptr::null_mut(), 0),
    }
}

/// Whether a record cell word names a born object (0 = unborn, `usize::MAX`
/// = the not-a-constructor mark), the algebra of [`scan_realm_intrinsics`].
#[inline(always)]
pub(crate) fn realm_record_cell_is_born(word: usize) -> bool {
    word != 0 && word != usize::MAX
}

/// The `vm` install's part here: the only writer of [`REALM_RECORD_CELLS`].
pub(crate) fn install_realm_record_cells() {
    REALM_RECORD_CELLS.set(realm_record_cell_words);
}

#[cfg(test)]
mod realm_record_tests {
    use super::*;

    /// The record is an ordinary heap object of its realm: its cells keep
    /// what they name alive and follow it through moving collections, and the
    /// global's `[[Intrinsics]]` slot follows the record.
    #[test]
    fn vm_realm_record_traces_and_rewrites_its_cells() {
        crate::object::js_nm_install_vm();
        let scope = crate::gc::RuntimeHandleScope::new();
        let global = crate::object::global_this::js_new_realm_global();
        let global = scope.root_raw_mut_ptr((global.to_bits() & crate::value::POINTER_MASK) as *mut crate::object::ObjectHeader);
        let target = crate::object::js_object_alloc(0, 0);
        let target = scope.root_raw_mut_ptr(target);
        global.with_mut_ptr::<crate::object::ObjectHeader, _>(|g| unsafe {
            let realm = crate::object::global_this::realm_of(g);
            assert!(!std::ptr::eq(realm, &crate::state::state().realm_intrinsics));
            target.with_mut_ptr::<crate::object::ObjectHeader, _>(|t| {
                realm.functions[0].set(t as usize);
            });
        });
        for _ in 0..3 {
            let _ = crate::gc::gc_collect_minor();
        }
        global.with_mut_ptr::<crate::object::ObjectHeader, _>(|g| unsafe {
            let realm = crate::object::global_this::realm_of(g);
            target.with_mut_ptr::<crate::object::ObjectHeader, _>(|t| {
                assert_eq!(realm.functions[0].get(), t as usize, "the cell follows its object");
            });
            let record = (realm as *const RealmIntrinsics) as *const RealmRecord;
            assert_eq!((*record).global.get(), g as usize, "the global cell follows the global");
            assert_eq!(realm.entries[0].prototype.get(), 0, "families stay unborn");
        });
    }
}

/// The per-cell half of [`scan_prototype_addr_cache_roots_mut`].
///
/// Split out so the #6981 rewrite algebra can be exercised on a cell the test
/// owns privately: driving it through the realm's real intrinsics made the
/// assertion depend on no other libtest thread touching them meanwhile, which
/// is the #7955 flake.
fn rewrite_prototype_addr_slot(
    cache: &Cell<usize>,
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    let cached = cache.get();
    if cached == usize::MAX || cached == 0 {
        return;
    }
    let mut addr = cached;
    if visitor.visit_usize_slot(&mut addr) {
        // GC_STORE_AUDIT(ROOT): this IS the collector's root-rewrite of a
        // registered side-table slot, running inside a root scan with the
        // mutator stopped. `visit_usize_slot` returns true only when it
        // relocated the object, and the value written is the visitor's own
        // to-space address — barriering it would push an edge into the
        // remembered set that this very cycle is rebuilding.
        cache.set(addr);
    }
}

/// Read a memoized prototype address, resolving it through the GC forwarding
/// chain. `None` means "not resolved yet" — the caller must run the
/// `globalThis` bootstrap.
#[inline]
fn memoized_prototype_addr(cache: &Cell<usize>) -> Option<usize> {
    let cached = cache.get();
    (cached != usize::MAX).then(|| heal_prototype_addr(cache, cached))
}

/// Re-read a memoized prototype address through the GC forwarding chain and
/// write the healed address back, so every caller compares (and dereferences)
/// the object's CURRENT location. See the [`PROTOTYPE_ADDRS`] doc for why an
/// unresolved cache is a hang, not just a wrong answer (#6981).
///
/// `note_array_index_write` calls this on every indexed array write until the
/// prototype is polluted, so the not-forwarded case must stay call-free: the
/// `try_read_gc_header` probe is `#[inline(always)]` and reduces to two range
/// compares plus one load of a `gc_flags` byte at a fixed, permanently-hot
/// address. It also classifies the address band before dereferencing, so the
/// not-yet-resolved sentinel (`usize::MAX`) and any non-heap value fall
/// straight through.
///
/// #7988: the address is now always one THIS thread allocated, so the header
/// read has an owner. It used to be able to land on another agent's arena
/// block — possibly already swept, moved, or `dealloc`'d at that thread's exit
/// — and a stray `GC_FLAG_FORWARDED` byte there sent `resolve_forwarding` one
/// word further into it.
#[inline]
fn heal_prototype_addr(cache: &Cell<usize>, cached: usize) -> usize {
    let forwarded = unsafe {
        crate::value::addr_class::try_read_gc_header(cached)
            .is_some_and(|header| header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0)
    };
    if !forwarded {
        return cached;
    }
    let resolved = crate::value::resolve_forwarding(cached);
    if resolved != cached {
        cache.set(resolved);
    }
    resolved
}

/// %Object.prototype%'s row: written when the intrinsic is built
/// (`note_object_prototype_intrinsic`), healed through forwarding; 0 while it
/// is not built. Object is not a lazy family: its one builder is
/// `ensure_object_intrinsics`, never %Global%. Every other builtin prototype
/// has one record only: its family cell in [[Intrinsics]].
#[inline]
fn object_prototype_row() -> usize {
    memoized_prototype_addr(&prototype_addrs()[OBJECT_PROTO_CACHE]).unwrap_or(0)
}

#[cold]
#[inline(never)]
fn build_object_prototype() -> usize {
    let (_, proto) = crate::object::ensure_object_intrinsics();
    proto as usize
}

/// A family prototype's value: the family cell when born, else its birth
/// (`intrinsic_prototype_addr`). The born case is one load and a branch.
#[inline]
fn family_prototype_addr(identity: crate::object::BuiltinConstructorIdentity) -> usize {
    match intrinsic_prototype_addr_if_bound(identity) {
        0 => intrinsic_prototype_addr(identity),
        addr => addr,
    }
}

/// `%Array.prototype%` if born, else 0 (identity questions: an unborn
/// prototype cannot be the object asked about). Never births.
#[inline]
pub(crate) fn array_prototype_addr_if_resolved() -> usize {
    intrinsic_prototype_addr_if_bound(crate::object::BuiltinConstructorIdentity::Array)
}

#[inline]
pub(crate) fn array_prototype_addr() -> usize {
    family_prototype_addr(crate::object::BuiltinConstructorIdentity::Array)
}

pub(crate) fn object_prototype_addr() -> usize {
    let addr = match object_prototype_row() {
        0 => build_object_prototype(),
        addr => addr,
    };
    if addr != 0
        && crate::object::OBJECT_INTRINSIC_PTR.load(std::sync::atomic::Ordering::Acquire) == 0
    {
        let (_, complete) = crate::object::ensure_object_intrinsics();
        return complete as usize;
    }
    addr
}

/// Complete `%Object.prototype%` if this thread has ALREADY memoized it, else
/// 0 — never returns the shape-only sentinel and never bootstraps. For a
/// caller that may itself be reached from the
/// bootstrap's `globalThis.Object` read: the inherited-read cache's walk runs
/// under `js_object_get_field_by_name`, which that bootstrap calls, so asking
/// [`object_prototype_addr`] from there recurses until the stack overflows.
/// The memo is primed at the end of `populate_global_this_builtins`, so after
/// startup this answers exactly what `object_prototype_addr` does.
pub(crate) fn object_prototype_addr_if_resolved() -> usize {
    if crate::object::OBJECT_INTRINSIC_PTR.load(std::sync::atomic::Ordering::Acquire) == 0 {
        return 0;
    }
    object_prototype_row()
}

/// Drop the memoized root for the shape-only Object.prototype sentinel before
/// the complete intrinsic replaces it. Called only under the intrinsic
/// builder's no-move window.
pub(crate) fn forget_object_prototype_intrinsic() {
    prototype_addrs()[OBJECT_PROTO_CACHE].set(usize::MAX);
}

/// The prototype a primitive's property read starts at (`%String.prototype%`,
/// `%Number.prototype%`, `%Boolean.prototype%`), or 0 while this thread
/// has no `globalThis`. `value` is a NaN-boxed primitive; any other kind
/// (a symbol, a bigint, an object) answers 0. The rows are primed while the
/// realm still owns its original constructor bindings, so reassignment before
/// the first primitive read cannot change the intrinsic prototype identity.
pub(crate) fn primitive_wrapper_prototype_addr(value: f64) -> usize {
    let v = crate::value::JSValue::from_bits(value.to_bits());
    let family = if v.is_number() {
        crate::object::BuiltinConstructorIdentity::Number
    } else if v.is_any_string() {
        crate::object::BuiltinConstructorIdentity::String
    } else if v.is_bool() {
        crate::object::BuiltinConstructorIdentity::Boolean
    } else {
        return 0;
    };
    family_prototype_addr(family)
}

/// Memoize THIS realm's `%Object.prototype%` the moment it is built, so the
/// store path's "is this Object.prototype?" check answers for it even before
/// the realm global exists (the class prototype chain reaches it first).
pub(crate) fn note_object_prototype_intrinsic(addr: usize) {
    prototype_addrs()[OBJECT_PROTO_CACHE].set(addr);
}

/// **This realm's** `%Function.prototype%` address, or 0 while this thread
/// has no `globalThis` yet (no intrinsic exists, so nothing can be it). See
/// the row-2 note on [`prototype_addrs`] (#10497).
#[inline]
pub(crate) fn function_prototype_addr() -> usize {
    family_prototype_addr(crate::object::BuiltinConstructorIdentity::Function)
}

/// `%Function.prototype%` if born, else 0. Never births (identity questions).
#[inline]
pub(crate) fn function_prototype_addr_if_bound() -> usize {
    intrinsic_prototype_addr_if_bound(crate::object::BuiltinConstructorIdentity::Function)
}

/// `true` when `addr` is **this realm's** canonical `Object.prototype` (cheap:
/// one slot load + compare; lazily computes the address on first use).
/// `#[inline]`: the object-field set funnel calls this on every store, and
/// without the hint a small growth of that caller made LLVM outline it.
#[inline]
pub(crate) fn object_prototype_addr_matches(addr: usize) -> bool {
    // Identity-only users may compare against the shape sentinel without
    // making it observable and without materializing method closures.
    addr != 0 && addr == object_prototype_row()
}

/// Test-only handle on the shipped wiring, for the read-only assertion in
/// `gc::tests::runtime_roots::prototype_addr_cache`. Returns each accessor's
/// row index paired with the `globalThis` builtin that row bootstraps from.
/// The mutating #6981 cases run on cells they own; nothing hands out a writable
/// reference to the realm's real intrinsic cells (#7955).
#[cfg(test)]
pub(crate) fn test_prototype_addr_cache_wiring() -> [(usize, &'static [u8]); 1] {
    [(OBJECT_PROTO_CACHE, b"Object")]
}

#[cfg(test)]
pub(crate) fn test_prototype_addr_cell_count() -> usize {
    prototype_addrs().len()
}

/// The two halves of the #6981 defences, exported so the tests can drive them
/// on a private cell instead of this thread's real ones (#7955).
#[cfg(test)]
pub(crate) fn test_memoized_prototype_addr(cache: &Cell<usize>) -> Option<usize> {
    memoized_prototype_addr(cache)
}

#[cfg(test)]
pub(crate) fn test_rewrite_prototype_addr_slot(
    cache: &Cell<usize>,
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    rewrite_prototype_addr_slot(cache, visitor)
}
