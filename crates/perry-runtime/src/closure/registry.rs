//! Closure registries (rest/arity), dispatch-strategy resolution, and
//! rest-arg / arity-pad helpers.

use super::*;
use std::cell::RefCell;

#[derive(Clone, Copy)]
pub(crate) struct TrustedDirectTarget {
    pub func_ptr: *const u8,
    pub capture_count: u32,
    pub boxed_capture_mask: u64,
}

/// Everything module init records about one closure body, keyed by the
/// body's `func_ptr` in `CLOSURE_BODY_REGISTRY`.
///
/// #9707: these attributes used to live in TEN separate thread-local maps
/// (rest, arity, length, arrow, versioned-loop, strict, async, generator,
/// async-generator, dispatch cache), every one keyed by the same code
/// pointer. A ~59k-function program paid ~200 B per function for that —
/// ten key copies, ten buckets' worth of load-factor slack, and up to four
/// hash probes on a dispatch miss. One 16-byte record per body holds all of
/// it: the two arities and `.length` inline, the boolean attributes as flag
/// bits, and the rare compiler-private direct-call bodies as an index into
/// `TRUSTED_TARGETS` (only the eligible-arrow subset pays for those 64
/// bytes). A dispatch-strategy miss is now ONE probe that yields every
/// input the strategy needs, so the separate dispatch cache is gone too.
///
/// Stored as a thread-local rather than a global RwLock because closures are
/// thread-local in perry's runtime model (each thread has its own arena +
/// GC), so a per-thread copy avoids the per-call lock acquisition. Module
/// init runs on the main thread and populates one record per registered
/// body in the program; worker threads (issue #29 `perry/thread`) currently
/// don't see the table because they aren't supposed to invoke arbitrary user
/// closures across the boundary anyway.
#[derive(Clone, Copy, Default)]
pub(crate) struct ClosureBodyRecord {
    /// ECMAScript-visible `.length` (`js_register_closure_length`), valid
    /// when `HAS_LENGTH` is set. Unlike declared arity, function length stops
    /// at the first parameter with a default and before rest. Kept separate
    /// from `arity` because dispatch padding needs the body's real ABI arity
    /// while `fn.length` needs the ECMAScript-visible length.
    length: u32,
    /// Declared ABI param count (`js_register_closure_arity`), valid when
    /// `HAS_ARITY` is set — for closures WITHOUT a rest param. Looked up by
    /// `js_native_call_value` (the dynamic dispatch path used when a closure
    /// is stored as a class field and called method-style on an any-typed
    /// receiver) so the runtime can pad missing args with TAG_UNDEFINED to
    /// match the closure body's declared arity. Without this, calling a
    /// 3-param arrow with 1 arg through `js_native_call_method` →
    /// `js_native_call_value` → `js_closure_call1` transmutes the func_ptr to
    /// a 1-arg signature and the closure body reads garbage for params 2 and
    /// 3 (refs #421 — hono's `c.text(text)` / `setDefaultContentType` chain
    /// hit exactly this; uninit `headers` slot evaluated to a small denormal
    /// float, slow-path runs, `setDefaultContentType` returns a header object,
    /// `responseHeaders.set(k, v)` then fails with a #510-class TypeError).
    ///
    /// `u16`: closure dispatch tops out at 16 args (`js_closure_callN`), so
    /// the narrow field costs nothing; a registration above `u16::MAX`
    /// saturates rather than wraps.
    arity: u16,
    /// Number of fixed params declared BEFORE the rest param, valid when
    /// `HAS_REST` is set (the rest KIND rides in `REST_KIND_MASK`). Populated
    /// at module init by `js_register_closure_rest` for every closure body
    /// whose HIR signature ends in `...rest`. Looked up by `js_closure_callN`
    /// so that calls through dynamic dispatch (e.g. `obj.cb(a, b, c)` where
    /// `cb` is a class field holding an arrow) bundle trailing args into the
    /// rest array — the previous behavior passed unbundled args, leaving the
    /// rest param bound to the first trailing arg as a scalar (issue #493 /
    /// #421-rest fix). Static call sites (named functions, `Expr::FuncRef`,
    /// local closure-bound `let`) keep their existing bundling at the call
    /// site, which is faster — the registry is consulted only when needed.
    rest_arity: u16,
    /// `HAS_*` presence bits plus the boolean attributes (`ARROW`, `STRICT`,
    /// `ASYNC`, `GENERATOR`, `ASYNC_GENERATOR`) and the 2-bit rest kind.
    flags: u16,
    /// 1-based index into `TRUSTED_TARGETS`; 0 = this body has no
    /// compiler-private direct-call bodies. Only ever non-zero on an arrow.
    targets: u32,
}

/// `ClosureBodyRecord::flags` bits.
mod body_flags {
    /// `arity` is valid (`js_register_closure_arity` ran for this body).
    pub(super) const HAS_ARITY: u16 = 1 << 0;
    /// `rest_arity` + `REST_KIND_MASK` are valid (one of the three
    /// `js_register_closure_rest*` entry points ran for this body).
    pub(super) const HAS_REST: u16 = 1 << 1;
    /// Two-bit `RestDispatchKind` discriminant, valid iff `HAS_REST`.
    pub(super) const REST_KIND_SHIFT: u32 = 2;
    pub(super) const REST_KIND_MASK: u16 = 0b11 << REST_KIND_SHIFT;
    /// `length` is valid (`js_register_closure_length` ran for this body).
    pub(super) const HAS_LENGTH: u16 = 1 << 4;
    /// The body came from an arrow function. Arrows are callable but not
    /// constructable and inherit the restricted Function.prototype
    /// caller/arguments accessors. A small eligible subset also records an
    /// internal direct-call body that may use compiler-proven capture
    /// invariants (see `targets`). Dynamic callers retain the public body;
    /// only the exact-arrow resolver reads that optional target.
    pub(super) const ARROW: u16 = 1 << 5;
    /// The body is strict-mode code (file-level `"use strict"` or a body
    /// directive). Drives OrdinaryCallBindThis in `call`/`apply`/`bind`: a
    /// strict callee observes the raw primitive `thisArg`; a sloppy user
    /// callee gets it boxed once.
    pub(super) const STRICT: u16 = 1 << 6;
    /// The body came from an async function. `util.types.isAsyncFunction`
    /// uses this when the predicate sees a runtime closure value instead of
    /// a statically-known HIR node. Async generators set it too.
    pub(super) const ASYNC: u16 = 1 << 7;
    /// The source function was a generator function. The generator transform
    /// clears HIR's `is_generator` flag after lowering to a state machine, so
    /// codegen registers the wrapper/closure symbols here for util.types
    /// identity tests.
    pub(super) const GENERATOR: u16 = 1 << 8;
    /// #3664: the body came from an `async function*`. Async generators also
    /// carry `GENERATOR` (they lower to the same `{next,return,throw}`
    /// wrapper as sync generators), so this bit is what distinguishes the
    /// two — it drives `%AsyncGeneratorFunction%` vs `%GeneratorFunction%`
    /// intrinsic resolution.
    pub(super) const ASYNC_GENERATOR: u16 = 1 << 9;
}

impl ClosureBodyRecord {
    #[inline(always)]
    fn has(self, bit: u16) -> bool {
        self.flags & bit != 0
    }

    #[inline(always)]
    fn arity(self) -> Option<u32> {
        self.has(body_flags::HAS_ARITY)
            .then_some(u32::from(self.arity))
    }

    #[inline(always)]
    fn rest(self) -> Option<(u32, RestDispatchKind)> {
        if !self.has(body_flags::HAS_REST) {
            return None;
        }
        let kind = match (self.flags & body_flags::REST_KIND_MASK) >> body_flags::REST_KIND_SHIFT {
            0 => RestDispatchKind::UserRest,
            1 => RestDispatchKind::SyntheticArguments,
            _ => RestDispatchKind::UserRestAndArguments,
        };
        Some((u32::from(self.rest_arity), kind))
    }

    #[inline(always)]
    fn length(self) -> Option<u32> {
        self.has(body_flags::HAS_LENGTH).then_some(self.length)
    }

    fn set_rest(&mut self, fixed_arity: u32, kind: RestDispatchKind) {
        let kind_bits: u16 = match kind {
            RestDispatchKind::UserRest => 0,
            RestDispatchKind::SyntheticArguments => 1,
            RestDispatchKind::UserRestAndArguments => 2,
        };
        self.rest_arity = saturate_u16(fixed_arity);
        self.flags = (self.flags & !body_flags::REST_KIND_MASK)
            | body_flags::HAS_REST
            | (kind_bits << body_flags::REST_KIND_SHIFT);
    }

    /// The dispatch strategy this body's attributes imply. Pure bit tests on
    /// an already-fetched record, so a strategy miss costs exactly one hash
    /// probe (the record's) instead of the old cache + rest + arity + arrow
    /// chain.
    #[inline(always)]
    fn dispatch_strategy(self) -> DispatchStrategy {
        let kind = if let Some((fixed_arity, rest_kind)) = self.rest() {
            DispatchKind::Rest(fixed_arity, rest_kind)
        } else if let Some(declared) = self.arity() {
            DispatchKind::Arity(declared)
        } else {
            DispatchKind::Direct
        };
        DispatchStrategy {
            kind,
            is_arrow: self.has(body_flags::ARROW),
        }
    }
}

#[inline(always)]
fn saturate_u16(value: u32) -> u16 {
    debug_assert!(
        value <= u32::from(u16::MAX),
        "closure arity {value} exceeds the u16 registry field"
    );
    u16::try_from(value).unwrap_or(u16::MAX)
}

/// The compiler-private direct-call bodies of one eligible arrow, reached
/// from `ClosureBodyRecord::targets`.
#[derive(Clone, Copy, Default)]
struct TrustedTargets {
    /// Internal direct-call body that may use compiler-proven capture
    /// invariants (`js_register_closure_trusted_direct`).
    direct: Option<TrustedDirectTarget>,
    /// Compiler-private callback body whose cold arms poison the caller's
    /// versioned loop before observable fallback code
    /// (`js_register_closure_versioned_loop_direct`).
    versioned_loop: Option<TrustedDirectTarget>,
}

crate::perry_thread_local! {
    /// One `ClosureBodyRecord` per registered closure body, keyed by the
    /// body's `func_ptr`. See the record's doc for what used to be here.
    static CLOSURE_BODY_REGISTRY: RefCell<crate::fast_hash::PtrHashMap<usize, ClosureBodyRecord>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());

    /// Dense side array for the eligible-arrow subset that has trusted
    /// direct-call bodies; `ClosureBodyRecord::targets` is a 1-based index
    /// into it. Append-only, so an index never moves.
    static TRUSTED_TARGETS: RefCell<Vec<TrustedTargets>> = RefCell::new(Vec::new());
}

/// The record for `func_ptr`, if module init registered anything about it.
#[inline(always)]
fn body_record(func_ptr: *const u8) -> Option<ClosureBodyRecord> {
    CLOSURE_BODY_REGISTRY.with(|r| r.borrow().get(&(func_ptr as usize)).copied())
}

/// Run `update` against the (possibly fresh) record for `func_ptr`.
#[inline(always)]
fn update_body_record(func_ptr: *const u8, update: impl FnOnce(&mut ClosureBodyRecord)) {
    CLOSURE_BODY_REGISTRY.with(|r| {
        update(r.borrow_mut().entry(func_ptr as usize).or_default());
    });
}

/// Magic value stored in ClosureHeader._reserved to identify closures at runtime.
/// Used by js_value_typeof to return "function" instead of "object" for closures.
pub const CLOSURE_MAGIC: u32 = 0x434C_4F53; // "CLOS" in ASCII

/// Per-call dispatch strategy for a closure body. Derived from the body's
/// `ClosureBodyRecord` on a miss and memoized in `DISPATCH_RECENT`.
/// Arrow-ness rides in the same entry so receiverless calls can avoid a
/// second TLS registry lookup.
#[derive(Clone, Copy)]
pub struct DispatchStrategy {
    kind: DispatchKind,
    is_arrow: bool,
}

impl DispatchStrategy {
    const fn direct() -> Self {
        Self {
            kind: DispatchKind::Direct,
            is_arrow: false,
        }
    }

    #[inline(always)]
    pub(crate) fn kind(self) -> DispatchKind {
        self.kind
    }

    #[inline(always)]
    pub(crate) fn is_arrow(self) -> bool {
        self.is_arrow
    }
}

#[derive(Clone, Copy)]
pub(crate) enum DispatchKind {
    /// Bound-method receiver pretending to be a closure (BOUND_METHOD_FUNC_PTR
    /// sentinel). Dispatch via `dispatch_bound_method`.
    BoundMethod,
    /// `Function.prototype.bind` result (BOUND_FUNCTION_FUNC_PTR sentinel).
    /// Dispatch via `dispatch_bound_function`.
    BoundFunction,
    /// Closure body has a rest-like runtime bundling requirement.
    Rest(u32, RestDispatchKind),
    /// Closure body declares an arity higher than the call sites use;
    /// dispatch must pad with TAG_UNDEFINED via `dispatch_with_arity`.
    Arity(u32),
    /// Direct callable: just transmute func_ptr to typed fn pointer
    /// (declared arity == call site arity, no rest, no bound method).
    /// The hot path for the vast majority of closure call sites.
    Direct,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RestDispatchKind {
    UserRest,
    SyntheticArguments,
    UserRestAndArguments,
}

crate::perry_thread_local! {
    /// Four-entry polymorphic cache for recently resolved closure bodies.
    /// Avoids the per-call HashMap::get + RefCell::borrow both when one body
    /// is invoked back-to-back and when a small set of bodies alternates,
    /// which are the steady-state shapes of:
    ///   - microtask drain (same then_v_arrow / __step body each iter)
    ///   - tight `array.sort` callbacks (same comparator every comparison)
    ///   - hot `array.map` / `array.forEach` loops
    ///   - stage pipelines that rotate through a few closure bodies
    ///
    /// Hits do not reorder the entries: that keeps the monomorphic first
    /// slot to one comparison and makes a warmed polymorphic cache read-only.
    static DISPATCH_RECENT: DispatchRecent = const { DispatchRecent::new() };
}

#[cfg(test)]
std::thread_local! {
    static RESOLVE_STRATEGY_SLOW_CALLS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Copy)]
struct DispatchCacheEntry {
    key: usize,
    strategy: DispatchStrategy,
}

impl DispatchCacheEntry {
    const EMPTY: Self = Self {
        key: 0,
        strategy: DispatchStrategy::direct(),
    };
}

struct DispatchRecent {
    entries: [std::cell::Cell<DispatchCacheEntry>; 4],
}

impl DispatchRecent {
    const fn new() -> Self {
        Self {
            entries: [const { std::cell::Cell::new(DispatchCacheEntry::EMPTY) }; 4],
        }
    }

    #[inline(always)]
    fn get(&self, key: usize) -> Option<DispatchStrategy> {
        for entry in &self.entries {
            let entry = entry.get();
            if entry.key == key {
                return Some(entry.strategy);
            }
        }
        None
    }

    #[inline(always)]
    fn insert(&self, key: usize, strategy: DispatchStrategy) {
        self.entries[3].set(self.entries[2].get());
        self.entries[2].set(self.entries[1].get());
        self.entries[1].set(self.entries[0].get());
        self.entries[0].set(DispatchCacheEntry { key, strategy });
    }

    fn invalidate(&self, key: usize) {
        for entry in &self.entries {
            if entry.get().key == key {
                entry.set(DispatchCacheEntry::EMPTY);
            }
        }
    }
}

#[inline(always)]
pub fn resolve_strategy(func_ptr: *const u8) -> DispatchStrategy {
    let key = func_ptr as usize;
    if let Some(strategy) = DISPATCH_RECENT.with(|cache| cache.get(key)) {
        return strategy;
    }
    let strategy = resolve_strategy_slow(func_ptr);
    DISPATCH_RECENT.with(|cache| cache.insert(key, strategy));
    strategy
}

#[inline(never)]
fn resolve_strategy_slow(func_ptr: *const u8) -> DispatchStrategy {
    #[cfg(test)]
    RESOLVE_STRATEGY_SLOW_CALLS.with(|calls| calls.set(calls.get() + 1));

    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return DispatchStrategy {
            kind: DispatchKind::BoundMethod,
            is_arrow: false,
        };
    }
    if func_ptr == BOUND_FUNCTION_FUNC_PTR {
        return DispatchStrategy {
            kind: DispatchKind::BoundFunction,
            is_arrow: false,
        };
    }
    // One probe of the unified record yields rest, arity AND arrow-ness;
    // an unregistered body (no record) is a plain direct callable. There is
    // no separate memo of the answer any more: deriving it from the record
    // is a couple of bit tests, cheaper than the second hash probe the old
    // `DISPATCH_CACHE` cost, and it cannot go stale — a late registration
    // (#6475) rewrites the very record the next miss reads.
    match body_record(func_ptr) {
        Some(record) => record.dispatch_strategy(),
        None => DispatchStrategy::direct(),
    }
}

/// #6475: evict a func_ptr from the recent-bodies dispatch cache.
/// `DISPATCH_RECENT` memoizes the FIRST resolution per func_ptr — but
/// effect's module-init graph calls `.pipe` (an `arguments`-object method)
/// during init, and inside an import cycle such a call can precede the
/// module's own `js_register_closure_*` batch. The pre-registration miss
/// resolved to `Arity`/`Direct` and was cached FOREVER: every later
/// `obj.pipe(a, b, c)` transmuted three positional args onto the 1-slot
/// `arguments` ABI, so `arguments.length` read 1 and effect's
/// `pipeArguments` applied only the first stage (`HttpApiBuilder.group`
/// returned the wrong pipe stage; web.ts died with "Not a valid effect:
/// undefined"). Registration is init-time-rare, so invalidating here keeps
/// the hot-path cache lock-free and unchanged. Also covers the late arity
/// registration for plain closures. (The hash-level dispatch cache this
/// used to purge as well is gone — #9707 derives the strategy from the
/// registration record itself, so there is nothing else to go stale.)
fn invalidate_dispatch_strategy(func_ptr: *const u8) {
    DISPATCH_RECENT.with(|cache| cache.invalidate(func_ptr as usize));
}

#[cfg(test)]
mod dispatch_recent_tests {
    use super::*;

    extern "C" fn body_a(_: *const ClosureHeader) -> f64 {
        1.0
    }
    extern "C" fn body_b(_: *const ClosureHeader) -> f64 {
        2.0
    }
    extern "C" fn body_c(_: *const ClosureHeader) -> f64 {
        3.0
    }
    extern "C" fn body_d(_: *const ClosureHeader) -> f64 {
        4.0
    }

    #[test]
    fn four_alternating_bodies_stay_out_of_the_hash_lookup() {
        let bodies = [
            body_a as *const u8,
            body_b as *const u8,
            body_c as *const u8,
            body_d as *const u8,
        ];
        let mut addresses = bodies.map(|body| body as usize);
        addresses.sort_unstable();
        assert!(addresses.windows(2).all(|pair| pair[0] != pair[1]));

        for body in bodies {
            invalidate_dispatch_strategy(body);
        }
        RESOLVE_STRATEGY_SLOW_CALLS.with(|calls| calls.set(0));

        for body in bodies {
            assert!(matches!(
                resolve_strategy(body).kind(),
                DispatchKind::Direct
            ));
        }
        assert_eq!(RESOLVE_STRATEGY_SLOW_CALLS.with(|calls| calls.get()), 4);

        RESOLVE_STRATEGY_SLOW_CALLS.with(|calls| calls.set(0));
        for body in bodies {
            assert!(matches!(
                resolve_strategy(body).kind(),
                DispatchKind::Direct
            ));
        }
        assert_eq!(
            RESOLVE_STRATEGY_SLOW_CALLS.with(|calls| calls.get()),
            0,
            "a warm four-body rotation must not reach resolve_strategy_slow"
        );

        js_register_closure_arity(body_c as *const u8, 3);
        assert!(matches!(
            resolve_strategy(body_c as *const u8).kind(),
            DispatchKind::Arity(3)
        ));
        assert_eq!(
            RESOLVE_STRATEGY_SLOW_CALLS.with(|calls| calls.get()),
            1,
            "late registration must evict a body from every recent-cache slot"
        );
    }
}

#[no_mangle]
pub extern "C" fn js_register_closure_rest(func_ptr: *const u8, fixed_arity: u32) {
    if func_ptr.is_null() {
        return;
    }
    update_body_record(func_ptr, |record| {
        record.set_rest(fixed_arity, RestDispatchKind::UserRest);
    });
    invalidate_dispatch_strategy(func_ptr);
}

/// Like `js_register_closure_rest`, but flags the rest param as the
/// synthesized `arguments` array. The HIR's `append_synthetic_arguments_param`
/// helper appends an `arguments` rest param whenever a function body reads
/// `arguments` and the user hasn't already declared a rest of their own.
/// JS spec semantics: `arguments.length` counts ALL passed args (data-first
/// AND any trailing). Without this flag, `dispatch_rest_bundled` was binding
/// fixed params first and then bundling only the post-`fixed_arity` tail
/// into the rest, so `function(a, b) { return arguments.length }` called as
/// `f(10, 20)` saw `arguments.length === 0`. Refs #915 (gap 1 from #899).
#[no_mangle]
pub extern "C" fn js_register_closure_synthetic_arguments(func_ptr: *const u8, fixed_arity: u32) {
    if func_ptr.is_null() {
        return;
    }
    update_body_record(func_ptr, |record| {
        record.set_rest(fixed_arity, RestDispatchKind::SyntheticArguments);
    });
    invalidate_dispatch_strategy(func_ptr);
}

/// Register a function with both a user-declared rest parameter and a hidden
/// raw-arguments slot. Dynamic dispatch must provide two arrays: the user rest
/// tail and the full argument list used to allocate the ECMAScript Arguments
/// object in the callee prologue.
#[no_mangle]
pub extern "C" fn js_register_closure_rest_and_arguments(func_ptr: *const u8, fixed_arity: u32) {
    if func_ptr.is_null() {
        return;
    }
    update_body_record(func_ptr, |record| {
        record.set_rest(fixed_arity, RestDispatchKind::UserRestAndArguments);
    });
    invalidate_dispatch_strategy(func_ptr);
}

#[no_mangle]
pub extern "C" fn js_register_closure_async_function(func_ptr: *const u8) {
    if func_ptr.is_null() {
        return;
    }
    update_body_record(func_ptr, |record| record.flags |= body_flags::ASYNC);
}

#[inline(always)]
pub fn is_registered_async_function(func_ptr: *const u8) -> bool {
    if func_ptr.is_null() {
        return false;
    }
    body_record(func_ptr).is_some_and(|record| record.has(body_flags::ASYNC))
}

#[inline(always)]
pub fn lookup_closure_rest(func_ptr: *const u8) -> Option<u32> {
    lookup_closure_rest_full(func_ptr).map(|(arity, _)| arity)
}

#[inline(always)]
pub fn lookup_closure_rest_full(func_ptr: *const u8) -> Option<(u32, RestDispatchKind)> {
    body_record(func_ptr)?.rest()
}

/// Register a closure body's declared param count (for closures WITHOUT a rest
/// param). Called once per non-rest closure literal at module init time.
/// See `ClosureBodyRecord::arity` for rationale.
#[no_mangle]
pub extern "C" fn js_register_closure_arity(func_ptr: *const u8, arity: u32) {
    if func_ptr.is_null() {
        return;
    }
    update_body_record(func_ptr, |record| {
        record.arity = saturate_u16(arity);
        record.flags |= body_flags::HAS_ARITY;
    });
    // #6475: same pre-registration cache-poisoning hazard as the rest-family
    // registrations above — a call before this registration caches `Direct`.
    invalidate_dispatch_strategy(func_ptr);
}

#[inline(always)]
pub fn lookup_closure_arity(func_ptr: *const u8) -> Option<u32> {
    body_record(func_ptr)?.arity()
}

/// Register a closure body's ECMAScript `.length` value.
#[no_mangle]
pub extern "C" fn js_register_closure_length(func_ptr: *const u8, length: u32) {
    if func_ptr.is_null() {
        return;
    }
    update_body_record(func_ptr, |record| {
        record.length = length;
        record.flags |= body_flags::HAS_LENGTH;
    });
}

#[inline(always)]
pub fn lookup_closure_length(func_ptr: *const u8) -> Option<u32> {
    body_record(func_ptr)?.length()
}

#[no_mangle]
pub extern "C" fn js_register_closure_arrow_function(func_ptr: *const u8) {
    if func_ptr.is_null() {
        return;
    }
    update_body_record(func_ptr, |record| record.flags |= body_flags::ARROW);
    // A closure can be invoked during a cyclic module-init graph before its
    // metadata batch runs. Keep arrow-ness coherent with the rest/arity data
    // memoized in the recent-bodies dispatch cache.
    invalidate_dispatch_strategy(func_ptr);
}

#[inline(always)]
pub fn is_registered_arrow_function(func_ptr: *const u8) -> bool {
    if func_ptr.is_null() {
        return false;
    }
    body_record(func_ptr).is_some_and(|record| record.has(body_flags::ARROW))
}

/// Attach one of the compiler-private direct-call bodies to the arrow at
/// `public_func_ptr`, allocating its `TRUSTED_TARGETS` slot on first use.
/// Refuses (returns `false`) for a body that is not a registered arrow, so a
/// registration can never turn an ordinary function into an arrow.
fn attach_trusted_target(
    public_func_ptr: *const u8,
    target: TrustedDirectTarget,
    select: impl FnOnce(&mut TrustedTargets) -> &mut Option<TrustedDirectTarget>,
) -> bool {
    CLOSURE_BODY_REGISTRY.with(|r| {
        let mut registry = r.borrow_mut();
        let Some(record) = registry.get_mut(&(public_func_ptr as usize)) else {
            return false;
        };
        if !record.has(body_flags::ARROW) {
            return false;
        }
        TRUSTED_TARGETS.with(|targets| {
            let mut targets = targets.borrow_mut();
            if record.targets == 0 {
                targets.push(TrustedTargets::default());
                record.targets = u32::try_from(targets.len())
                    .expect("more than u32::MAX trusted closure bodies");
            }
            *select(&mut targets[record.targets as usize - 1]) = Some(target);
        });
        true
    })
}

#[inline(always)]
fn trusted_targets(func_ptr: *const u8) -> Option<TrustedTargets> {
    let record = body_record(func_ptr)?;
    if record.targets == 0 {
        return None;
    }
    TRUSTED_TARGETS.with(|targets| targets.borrow().get(record.targets as usize - 1).copied())
}

/// Associate a public closure body with its compiler-private direct-call
/// body. Both addresses are static code pointers emitted by the same module.
#[no_mangle]
pub extern "C" fn js_register_closure_trusted_direct(
    public_func_ptr: *const u8,
    trusted_func_ptr: *const u8,
    capture_count: u32,
    boxed_capture_mask: u64,
) {
    if public_func_ptr.is_null() || trusted_func_ptr.is_null() {
        return;
    }
    attach_trusted_target(
        public_func_ptr,
        TrustedDirectTarget {
            func_ptr: trusted_func_ptr,
            capture_count,
            boxed_capture_mask,
        },
        |targets| &mut targets.direct,
    );
}

#[inline(always)]
pub(crate) fn lookup_closure_trusted_direct(func_ptr: *const u8) -> Option<TrustedDirectTarget> {
    trusted_targets(func_ptr)?.direct
}

#[no_mangle]
pub extern "C" fn js_register_closure_versioned_loop_direct(
    public_func_ptr: *const u8,
    versioned_func_ptr: *const u8,
    capture_count: u32,
    boxed_capture_mask: u64,
) {
    if public_func_ptr.is_null() || versioned_func_ptr.is_null() {
        return;
    }
    attach_trusted_target(
        public_func_ptr,
        TrustedDirectTarget {
            func_ptr: versioned_func_ptr,
            capture_count,
            boxed_capture_mask,
        },
        |targets| &mut targets.versioned_loop,
    );
}

#[inline(always)]
pub(crate) fn lookup_closure_versioned_loop_direct(
    func_ptr: *const u8,
) -> Option<TrustedDirectTarget> {
    trusted_targets(func_ptr)?.versioned_loop
}

/// Keepalive anchor for the auto-optimize whole-program build — registration
/// is referenced only by generated module-init code.
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_REGISTER_CLOSURE_TRUSTED_DIRECT: extern "C" fn(*const u8, *const u8, u32, u64) =
    js_register_closure_trusted_direct;

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_REGISTER_CLOSURE_VERSIONED_LOOP_DIRECT: extern "C" fn(
    *const u8,
    *const u8,
    u32,
    u64,
) = js_register_closure_versioned_loop_direct;

/// Register a compiled function address as strict-mode code. Emitted from
/// module init alongside the arrow-function registration.
#[no_mangle]
pub extern "C" fn js_register_closure_strict_function(func_ptr: *const u8) {
    if func_ptr.is_null() {
        return;
    }
    update_body_record(func_ptr, |record| record.flags |= body_flags::STRICT);
}

/// Keepalive anchor for the auto-optimize whole-program build — the strict
/// registration is emitted only from generated module-init code.
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_REGISTER_CLOSURE_STRICT_FUNCTION: extern "C" fn(*const u8) =
    js_register_closure_strict_function;

#[inline(always)]
pub fn is_registered_strict_function(func_ptr: *const u8) -> bool {
    if func_ptr.is_null() {
        return false;
    }
    body_record(func_ptr).is_some_and(|record| record.has(body_flags::STRICT))
}

pub fn closure_is_arrow(closure: *const ClosureHeader) -> bool {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return false;
    }
    // The unified dispatch strategy already carries arrow-ness (the same
    // registry answer, memoised per body and kept coherent by
    // `js_register_closure_arrow_function`'s invalidation), and the call
    // that follows a receiver rebind resolves it anyway — so answer from the
    // recent-bodies cache instead of a second thread-local hash probe per
    // call. (`this.handler(a, b)` on a closure-typed field paid both.)
    resolve_strategy(func_ptr).is_arrow()
}

/// True if `closure` is a bound-method / bound-function value (its body is the
/// `BOUND_METHOD_FUNC_PTR` sentinel). Class method/getter/setter values read
/// via `C.prototype.m`, instance method reads, and `Function.prototype.bind`
/// results all use this sentinel. None of them are constructors, so they have
/// no `prototype` own property (ECMA-262: methods, accessors, and bound
/// functions are non-constructors).
pub fn closure_is_bound_method(closure: *const ClosureHeader) -> bool {
    get_valid_func_ptr(closure) == BOUND_METHOD_FUNC_PTR
}

#[no_mangle]
pub extern "C" fn js_register_closure_generator_function(func_ptr: *const u8) {
    if func_ptr.is_null() {
        return;
    }
    update_body_record(func_ptr, |record| record.flags |= body_flags::GENERATOR);
}

#[inline(always)]
pub fn is_registered_generator_function(func_ptr: *const u8) -> bool {
    body_record(func_ptr).is_some_and(|record| record.has(body_flags::GENERATOR))
}

/// #3664: register a closure body `func_ptr` as an `async function*`. Also
/// marks it in the async-function registry so `util.types.isAsyncFunction`
/// reports `true` for async generators (matching Node).
#[no_mangle]
pub extern "C" fn js_register_closure_async_generator_function(func_ptr: *const u8) {
    if func_ptr.is_null() {
        return;
    }
    update_body_record(func_ptr, |record| {
        record.flags |= body_flags::ASYNC_GENERATOR | body_flags::ASYNC;
    });
}

#[inline(always)]
pub fn is_registered_async_generator_function(func_ptr: *const u8) -> bool {
    if func_ptr.is_null() {
        return false;
    }
    body_record(func_ptr).is_some_and(|record| record.has(body_flags::ASYNC_GENERATOR))
}

/// Public helper: given a `*const ClosureHeader` pointer, return the
/// closure's declared ABI arity if known. Falls back to the rest-registry
/// fixed-arity entry for closures declared with `...rest`.
pub fn closure_arity(closure: *const ClosureHeader) -> Option<u32> {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return None;
    }
    let record = body_record(func_ptr)?;
    // Closures declared with `...rest` register through a separate
    // registry path; prefer the fixed-arity portion of that entry when
    // present so `length` matches the user-visible declared params.
    if let Some((arity, _synth)) = record.rest() {
        return Some(arity);
    }
    record.arity()
}

/// Public helper for the ECMAScript-visible function `.length`.
///
/// Generated user closures register this explicitly because `.length` stops
/// at the first default parameter, while the dispatch arity must remain the
/// full declared parameter count for ABI-safe padding.
pub fn closure_length(closure: *const ClosureHeader) -> Option<u32> {
    if let Some(length) = crate::object::builtin_closure_length(closure as usize) {
        return Some(length);
    }
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return None;
    }
    let record = body_record(func_ptr)?;
    if let Some(length) = record.length() {
        return Some(length);
    }
    if let Some((arity, _synth)) = record.rest() {
        return Some(arity);
    }
    record.arity()
}

/// Build a JS array from a slice of NaN-boxed f64 values and return it
/// NaN-boxed as a pointer. Used by the rest-bundling helper below.
#[inline(always)]
pub unsafe fn build_rest_array(values: &[f64], arguments_object: bool) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handles: Vec<_> = values
        .iter()
        .map(|value| scope.root_nanbox_f64(*value))
        .collect();
    let arr = crate::array::js_array_alloc(values.len() as u32);
    let mut cur = arr;
    for handle in value_handles.iter() {
        cur = crate::array::js_array_push_f64(cur, handle.get_nanbox_f64());
    }
    if arguments_object {
        crate::array::mark_array_as_arguments_object(cur as *const crate::array::ArrayHeader);
    }
    f64::from_bits(crate::value::JSValue::pointer(cur as *mut u8).bits())
}

/// Dispatch a closure with `args` to its body using a rest-bundled call.
/// `func_ptr` is already validated and known non-BOUND. `fixed_arity` is the
/// closure body's declared arity minus 1 (the +1 being the rest array).
///
/// Behavior matches the static-call-site bundling path in `lower_call.rs`:
/// the first `fixed_arity` args are forwarded as-is (padded with `undefined`
/// when the caller passed fewer than expected); everything from index
/// `fixed_arity` onwards is bundled into a fresh JS Array passed as the
/// last arg. The body is then invoked with exactly `fixed_arity + 1` doubles.
///
/// Currently supports `fixed_arity` in `0..=15` — the same ceiling as
/// `js_closure_callN`. A program that defines a rest closure with more than
/// 15 fixed params before the rest is unsupported (and would already trip
/// the `Phase D.1: closure call with N args (max 16)` guard in lower_call).
#[inline(never)]
pub unsafe fn dispatch_rest_bundled(
    closure: *const ClosureHeader,
    func_ptr: *const u8,
    args: &[f64],
    fixed_arity: u32,
    kind: RestDispatchKind,
) -> f64 {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let k = fixed_arity as usize;
    let provided = args.len();
    let arg_scope = crate::gc::RuntimeHandleScope::new();
    let arg_handles: Vec<_> = args
        .iter()
        .map(|value| arg_scope.root_nanbox_f64(*value))
        .collect();

    let rest_slice: &[f64] = if kind == RestDispatchKind::SyntheticArguments {
        args
    } else if provided > k {
        &args[k..]
    } else {
        &[]
    };
    let rest_double = build_rest_array(rest_slice, kind == RestDispatchKind::SyntheticArguments);
    let all_arguments_double = if kind == RestDispatchKind::UserRestAndArguments {
        Some(build_rest_array(args, true))
    } else {
        None
    };

    // Read fixed args, padding with undefined when caller under-supplied.
    macro_rules! a {
        ($i:expr) => {
            if $i < provided {
                arg_handles[$i].get_nanbox_f64()
            } else {
                undef
            }
        };
    }

    // Use a macro for higher rest arities so this stays in sync with the
    // documented 0..=15 support and the generated closure-call ceiling.
    macro_rules! rest_arm {
        (@ty $i:tt) => {
            f64
        };
        ($($i:tt),* $(,)?) => {{
            if let Some(arguments_double) = all_arguments_double {
                #[cfg(panic = "abort")]
                let f: extern "C" fn(*const ClosureHeader $(, rest_arm!(@ty $i))*, f64, f64) -> f64 =
                    std::mem::transmute(func_ptr);
                #[cfg(not(panic = "abort"))]
                let f: extern "C-unwind" fn(*const ClosureHeader $(, rest_arm!(@ty $i))*, f64, f64) -> f64 =
                    std::mem::transmute(func_ptr);
                f(closure $(, a!($i))*, rest_double, arguments_double)
            } else {
                #[cfg(panic = "abort")]
                let f: extern "C" fn(*const ClosureHeader $(, rest_arm!(@ty $i))*, f64) -> f64 =
                    std::mem::transmute(func_ptr);
                #[cfg(not(panic = "abort"))]
                let f: extern "C-unwind" fn(*const ClosureHeader $(, rest_arm!(@ty $i))*, f64) -> f64 =
                    std::mem::transmute(func_ptr);
                f(closure $(, a!($i))*, rest_double)
            }
        }};
    }

    match k {
        0 => {
            if let Some(arguments_double) = all_arguments_double {
                #[cfg(panic = "abort")]
                let f: extern "C" fn(*const ClosureHeader, f64, f64) -> f64 =
                    std::mem::transmute(func_ptr);
                #[cfg(not(panic = "abort"))]
                let f: extern "C-unwind" fn(*const ClosureHeader, f64, f64) -> f64 =
                    std::mem::transmute(func_ptr);
                f(closure, rest_double, arguments_double)
            } else {
                #[cfg(panic = "abort")]
                let f: extern "C" fn(*const ClosureHeader, f64) -> f64 =
                    std::mem::transmute(func_ptr);
                #[cfg(not(panic = "abort"))]
                let f: extern "C-unwind" fn(*const ClosureHeader, f64) -> f64 =
                    std::mem::transmute(func_ptr);
                f(closure, rest_double)
            }
        }
        1 => {
            if let Some(arguments_double) = all_arguments_double {
                let f: extern "C" fn(*const ClosureHeader, f64, f64, f64) -> f64 =
                    std::mem::transmute(func_ptr);
                f(closure, a!(0), rest_double, arguments_double)
            } else {
                let f: extern "C" fn(*const ClosureHeader, f64, f64) -> f64 =
                    std::mem::transmute(func_ptr);
                f(closure, a!(0), rest_double)
            }
        }
        2 => {
            if let Some(arguments_double) = all_arguments_double {
                let f: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64) -> f64 =
                    std::mem::transmute(func_ptr);
                f(closure, a!(0), a!(1), rest_double, arguments_double)
            } else {
                let f: extern "C" fn(*const ClosureHeader, f64, f64, f64) -> f64 =
                    std::mem::transmute(func_ptr);
                f(closure, a!(0), a!(1), rest_double)
            }
        }
        3 => {
            if let Some(arguments_double) = all_arguments_double {
                let f: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64) -> f64 =
                    std::mem::transmute(func_ptr);
                f(closure, a!(0), a!(1), a!(2), rest_double, arguments_double)
            } else {
                let f: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64) -> f64 =
                    std::mem::transmute(func_ptr);
                f(closure, a!(0), a!(1), a!(2), rest_double)
            }
        }
        4 => {
            if let Some(arguments_double) = all_arguments_double {
                let f: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64) -> f64 =
                    std::mem::transmute(func_ptr);
                f(
                    closure,
                    a!(0),
                    a!(1),
                    a!(2),
                    a!(3),
                    rest_double,
                    arguments_double,
                )
            } else {
                let f: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64) -> f64 =
                    std::mem::transmute(func_ptr);
                f(closure, a!(0), a!(1), a!(2), a!(3), rest_double)
            }
        }
        5 => {
            if let Some(arguments_double) = all_arguments_double {
                let f: extern "C" fn(
                    *const ClosureHeader,
                    f64,
                    f64,
                    f64,
                    f64,
                    f64,
                    f64,
                    f64,
                ) -> f64 = std::mem::transmute(func_ptr);
                f(
                    closure,
                    a!(0),
                    a!(1),
                    a!(2),
                    a!(3),
                    a!(4),
                    rest_double,
                    arguments_double,
                )
            } else {
                let f: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64) -> f64 =
                    std::mem::transmute(func_ptr);
                f(closure, a!(0), a!(1), a!(2), a!(3), a!(4), rest_double)
            }
        }
        6 => {
            if let Some(arguments_double) = all_arguments_double {
                let f: extern "C" fn(
                    *const ClosureHeader,
                    f64,
                    f64,
                    f64,
                    f64,
                    f64,
                    f64,
                    f64,
                    f64,
                ) -> f64 = std::mem::transmute(func_ptr);
                f(
                    closure,
                    a!(0),
                    a!(1),
                    a!(2),
                    a!(3),
                    a!(4),
                    a!(5),
                    rest_double,
                    arguments_double,
                )
            } else {
                let f: extern "C" fn(
                    *const ClosureHeader,
                    f64,
                    f64,
                    f64,
                    f64,
                    f64,
                    f64,
                    f64,
                ) -> f64 = std::mem::transmute(func_ptr);
                f(
                    closure,
                    a!(0),
                    a!(1),
                    a!(2),
                    a!(3),
                    a!(4),
                    a!(5),
                    rest_double,
                )
            }
        }
        7 => {
            if let Some(arguments_double) = all_arguments_double {
                let f: extern "C" fn(
                    *const ClosureHeader,
                    f64,
                    f64,
                    f64,
                    f64,
                    f64,
                    f64,
                    f64,
                    f64,
                    f64,
                ) -> f64 = std::mem::transmute(func_ptr);
                f(
                    closure,
                    a!(0),
                    a!(1),
                    a!(2),
                    a!(3),
                    a!(4),
                    a!(5),
                    a!(6),
                    rest_double,
                    arguments_double,
                )
            } else {
                let f: extern "C" fn(
                    *const ClosureHeader,
                    f64,
                    f64,
                    f64,
                    f64,
                    f64,
                    f64,
                    f64,
                    f64,
                ) -> f64 = std::mem::transmute(func_ptr);
                f(
                    closure,
                    a!(0),
                    a!(1),
                    a!(2),
                    a!(3),
                    a!(4),
                    a!(5),
                    a!(6),
                    rest_double,
                )
            }
        }
        8 => rest_arm!(0, 1, 2, 3, 4, 5, 6, 7),
        9 => rest_arm!(0, 1, 2, 3, 4, 5, 6, 7, 8),
        10 => rest_arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9),
        11 => rest_arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10),
        12 => rest_arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11),
        13 => rest_arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12),
        14 => rest_arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13),
        15 => rest_arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14),
        _ => {
            // Unsupported arity — fall back to undefined so we don't
            // mis-call the body and trigger UB. This mirrors the upper
            // bound that lower_call's static-bundling path enforces.
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
    }
}

/// Dispatch a closure call where the caller supplied fewer args than the
/// closure declared. Pad the missing slots with `undefined` and call the
/// body's actual declared signature so the body's `LocalGet(N)` reads
/// correctly initialised slots instead of stale registers.
///
/// `func_ptr` is already validated and known non-BOUND, non-rest.
/// `declared_arity` is what `js_register_closure_arity` recorded for this body
/// at module init time. Callers reach here only when `args.len() < declared_arity`.
///
/// Refs #420: drizzle's `pgTable` is `(name, columns, extraConfig) => …`
/// (3 params); user calls it as `pgTable("users", cols)` (2 args). Without
/// this padding, the body's `extraConfig` slot reads garbage and downstream
/// `if (extraConfig)` evaluated truthy on bit patterns that should have been
/// `undefined`. Symptom: `pgTable("users", {})` returned a malformed table
/// object, breaking every downstream property read.
#[inline(never)]
pub unsafe fn dispatch_with_arity(
    closure: *const ClosureHeader,
    func_ptr: *const u8,
    args: &[f64],
    declared_arity: u32,
) -> f64 {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let k = declared_arity as usize;
    let provided = args.len();
    macro_rules! a {
        ($i:expr) => {
            if $i < provided {
                args[$i]
            } else {
                undef
            }
        };
    }
    // One match arm per declared arity. Each arm transmutes `func_ptr` to
    // the concrete `(closure, f64 x N)` signature and forwards the (padded)
    // args. Arities up to 32 are supported so high-arity closures dispatched
    // dynamically — e.g. qs's recursive `stringify`, which declares 18
    // params and self-calls with 18 args (#3527) — call their body
    // correctly instead of mis-calling and corrupting registers. The
    // `arm!` macro builds the fn type and the (padded) call args from the
    // arg-index token list; `arm!(@ty $i)` maps any index token to `f64`.
    macro_rules! arm {
        (@ty $i:tt) => { f64 };
        ($($i:tt),* $(,)?) => {{
            let f: extern "C" fn(*const ClosureHeader $(, arm!(@ty $i))*) -> f64 =
                std::mem::transmute(func_ptr);
            f(closure $(, a!($i))*)
        }};
    }
    match k {
        0 => {
            let f: extern "C" fn(*const ClosureHeader) -> f64 = std::mem::transmute(func_ptr);
            f(closure)
        }
        1 => arm!(0),
        2 => arm!(0, 1),
        3 => arm!(0, 1, 2),
        4 => arm!(0, 1, 2, 3),
        5 => arm!(0, 1, 2, 3, 4),
        6 => arm!(0, 1, 2, 3, 4, 5),
        7 => arm!(0, 1, 2, 3, 4, 5, 6),
        8 => arm!(0, 1, 2, 3, 4, 5, 6, 7),
        9 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8),
        10 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9),
        11 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10),
        12 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11),
        13 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12),
        14 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13),
        15 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14),
        16 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15),
        17 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16),
        18 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17),
        19 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18),
        20 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19),
        21 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20),
        22 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21),
        23 => {
            arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22)
        }
        24 => arm!(
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23
        ),
        25 => arm!(
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24
        ),
        26 => arm!(
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25
        ),
        27 => arm!(
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26
        ),
        28 => arm!(
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27
        ),
        29 => arm!(
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28
        ),
        30 => arm!(
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29
        ),
        31 => arm!(
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30
        ),
        32 => arm!(
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31
        ),
        _ => {
            // Unsupported arity (>32 declared params). Fall back to
            // undefined rather than mis-calling and triggering UB.
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
    }
}

/// Sentinel func_ptr value indicating this closure is a "bound method" on a native module.
/// When js_closure_callN detects this, it extracts captures and dispatches via js_native_call_method.
/// Captures layout: [0] = namespace_obj (f64), [1] = method_name_ptr (i64), [2] = method_name_len (i64)
pub const BOUND_METHOD_FUNC_PTR: *const u8 = 0xBADD_DEAD_u64 as *const u8;

/// Sentinel func_ptr value indicating this closure is a `Function.prototype.bind`
/// result: a bound function with a fixed `this`, prepended partial args, and an
/// adjusted `.name` / `.length`. When `js_closure_callN` (or `js_native_call_value`)
/// detects this sentinel it dispatches via `dispatch_bound_function`, which
/// prepends the bound args, sets `IMPLICIT_THIS` to the bound receiver, and calls
/// the target closure.
///
/// Captures layout:
///   [0] = target closure value (f64, NaN-boxed)
///   [1] = bound `this` value (f64)
///   [2] = bound-args JS Array pointer (i64; 0 when no partial args)
pub const BOUND_FUNCTION_FUNC_PTR: *const u8 = 0xBADD_B12D_u64 as *const u8;

/// Flag stored in the high bit of capture_count to indicate that capture slot 0
/// holds `this` (i.e., this closure is an object literal method that captures `this`).
/// When the closure is detached from the object (assigned to a variable via PropertyGet),
/// `js_closure_unbind_this` clones it and clears slot 0 so `this` becomes undefined.
pub const CAPTURES_THIS_FLAG: u32 = 0x8000_0000;

/// Flag stored in bit 30 of `capture_count` marking a closure whose captured
/// `this` slot is LEXICAL and must never be re-bound by `Function.prototype.call`
/// / method dispatch (`clone_closure_rebind_this`). Set per-closure (on the
/// header itself) by `js_generator_attach_prototype` for the generator
/// state-machine `next`/`return`/`throw` step closures: their `this` is the
/// generator BODY's receiver, fixed at generator-creation time, and the `yield*`
/// delegation desugar (`next.call(iter, v)`) would otherwise clobber it with the
/// iterator object. Capture counts never approach 2^30, so bit 30 is free.
pub const NO_THIS_REBIND_FLAG: u32 = 0x4000_0000;

/// Extract the real capture count (masking out the flag bits stored in the
/// two high bits: `CAPTURES_THIS_FLAG` and `NO_THIS_REBIND_FLAG`).
#[inline(always)]
pub fn real_capture_count(capture_count: u32) -> u32 {
    capture_count & !(CAPTURES_THIS_FLAG | NO_THIS_REBIND_FLAG)
}

/// `PERRY_GC_CENSUS`: entries and estimated bytes of the per-`func_ptr`
/// registry filled from module init. `closure.body_registry` is the unified
/// record table (#9707 — it replaced the ten `closure.*_registry` /
/// `closure.dispatch_cache` rows an older census printed);
/// `closure.trusted_targets` is the dense side array only eligible arrows
/// with compiler-private bodies occupy.
pub(crate) fn closure_registry_census() -> Vec<crate::gc::census::SideTableRow> {
    use crate::gc::census::{map_bytes, vec_bytes};
    let mut rows = Vec::new();
    CLOSURE_BODY_REGISTRY.with(|m| {
        let m = m.borrow();
        rows.push(("closure.body_registry", m.len(), map_bytes(&m)));
    });
    TRUSTED_TARGETS.with(|v| {
        let v = v.borrow();
        rows.push(("closure.trusted_targets", v.len(), vec_bytes(&v)));
    });
    rows
}

#[cfg(test)]
mod body_record_tests {
    use super::*;

    extern "C" fn rec_a(_: *const ClosureHeader) -> f64 {
        1.0
    }
    extern "C" fn rec_b(_: *const ClosureHeader) -> f64 {
        2.0
    }
    extern "C" fn rec_c(_: *const ClosureHeader) -> f64 {
        3.0
    }
    extern "C" fn rec_d(_: *const ClosureHeader) -> f64 {
        4.0
    }
    extern "C" fn rec_trusted(_: *const ClosureHeader) -> f64 {
        5.0
    }
    extern "C" fn rec_versioned(_: *const ClosureHeader) -> f64 {
        6.0
    }
    extern "C" fn rec_e(_: *const ClosureHeader) -> f64 {
        7.0
    }
    extern "C" fn rec_f(_: *const ClosureHeader) -> f64 {
        8.0
    }

    /// The whole point of #9707: one record per body, small enough that the
    /// map's bucket is a third of what the ten-map layout summed to.
    #[test]
    fn record_is_sixteen_bytes_and_the_bucket_is_twenty_four() {
        assert_eq!(std::mem::size_of::<ClosureBodyRecord>(), 16);
        assert_eq!(std::mem::size_of::<(usize, ClosureBodyRecord)>(), 24);
    }

    /// Every attribute a body can carry survives being packed into the one
    /// record alongside every other attribute, and each reader still
    /// answers from its own field.
    #[test]
    fn all_attributes_coexist_in_one_record() {
        let body = rec_a as *const u8;
        js_register_closure_rest_and_arguments(body, 3);
        js_register_closure_length(body, 2);
        js_register_closure_arrow_function(body);
        js_register_closure_strict_function(body);
        js_register_closure_async_generator_function(body);
        js_register_closure_generator_function(body);

        assert!(matches!(
            lookup_closure_rest_full(body),
            Some((3, RestDispatchKind::UserRestAndArguments))
        ));
        assert_eq!(lookup_closure_rest(body), Some(3));
        assert_eq!(
            lookup_closure_arity(body),
            None,
            "rest does not imply arity"
        );
        assert_eq!(lookup_closure_length(body), Some(2));
        assert!(is_registered_arrow_function(body));
        assert!(is_registered_strict_function(body));
        assert!(
            is_registered_async_function(body),
            "async generators are async"
        );
        assert!(is_registered_generator_function(body));
        assert!(is_registered_async_generator_function(body));
        let strategy = resolve_strategy(body);
        assert!(strategy.is_arrow());
        assert!(matches!(
            strategy.kind(),
            DispatchKind::Rest(3, RestDispatchKind::UserRestAndArguments)
        ));

        // Registering the same body's arity afterwards adds the attribute
        // without disturbing the others, and rest still wins dispatch.
        js_register_closure_arity(body, 4);
        assert_eq!(lookup_closure_arity(body), Some(4));
        assert_eq!(lookup_closure_rest(body), Some(3));
        assert!(matches!(
            resolve_strategy(body).kind(),
            DispatchKind::Rest(3, RestDispatchKind::UserRestAndArguments)
        ));
    }

    /// A body nobody registered has no record and is a plain direct
    /// callable; a body with only a non-dispatch attribute has a record and
    /// is STILL a plain direct callable.
    #[test]
    fn unregistered_and_flag_only_bodies_dispatch_direct() {
        let unregistered = rec_b as *const u8;
        assert!(body_record(unregistered).is_none());
        let strategy = resolve_strategy(unregistered);
        assert!(matches!(strategy.kind(), DispatchKind::Direct));
        assert!(!strategy.is_arrow());
        assert_eq!(lookup_closure_arity(unregistered), None);
        assert_eq!(lookup_closure_length(unregistered), None);
        assert!(!is_registered_strict_function(unregistered));

        let strict_only = rec_c as *const u8;
        js_register_closure_strict_function(strict_only);
        assert!(body_record(strict_only).is_some());
        assert!(is_registered_strict_function(strict_only));
        assert!(matches!(
            resolve_strategy(strict_only).kind(),
            DispatchKind::Direct
        ));
        assert_eq!(lookup_closure_arity(strict_only), None);
        assert_eq!(lookup_closure_rest(strict_only), None);
    }

    /// The two compiler-private bodies live in the dense side array, are
    /// reachable only through an ARROW record, and a registration against a
    /// non-arrow must not create one.
    #[test]
    fn trusted_targets_hang_off_the_arrow_record() {
        let arrow = rec_d as *const u8;
        let plain = rec_e as *const u8;
        js_register_closure_arity(plain, 1);

        // Not an arrow (yet): both registrations are refused.
        js_register_closure_trusted_direct(arrow, rec_trusted as *const u8, 2, 0b10);
        js_register_closure_versioned_loop_direct(arrow, rec_versioned as *const u8, 2, 0b10);
        assert!(lookup_closure_trusted_direct(arrow).is_none());
        assert!(lookup_closure_versioned_loop_direct(arrow).is_none());
        assert!(
            !is_registered_arrow_function(arrow),
            "refusal must not mint an arrow"
        );

        js_register_closure_arrow_function(arrow);
        js_register_closure_trusted_direct(arrow, rec_trusted as *const u8, 2, 0b10);
        let direct = lookup_closure_trusted_direct(arrow).expect("direct target");
        assert_eq!(direct.func_ptr, rec_trusted as *const u8);
        assert_eq!(direct.capture_count, 2);
        assert_eq!(direct.boxed_capture_mask, 0b10);
        assert!(
            lookup_closure_versioned_loop_direct(arrow).is_none(),
            "the versioned slot is independent of the direct slot"
        );

        js_register_closure_versioned_loop_direct(arrow, rec_versioned as *const u8, 2, 0b10);
        let versioned = lookup_closure_versioned_loop_direct(arrow).expect("versioned target");
        assert_eq!(versioned.func_ptr, rec_versioned as *const u8);
        // Both targets share ONE side-array slot, so the second registration
        // reused the first's index instead of appending.
        let slot = body_record(arrow).unwrap().targets;
        assert!(slot != 0);
        assert_eq!(
            lookup_closure_trusted_direct(arrow).unwrap().func_ptr,
            rec_trusted as *const u8,
            "attaching the versioned body must not clobber the direct one"
        );

        // A registered non-arrow is still refused, and stays targetless.
        js_register_closure_trusted_direct(plain, rec_trusted as *const u8, 0, 0);
        assert!(lookup_closure_trusted_direct(plain).is_none());
        assert_eq!(body_record(plain).unwrap().targets, 0);
        assert!(!is_registered_arrow_function(plain));
    }

    /// The census reports the unified table by its new name, with one entry
    /// per registered body — not one per attribute.
    #[test]
    fn census_counts_bodies_not_attributes() {
        let body = rec_f as *const u8;
        js_register_closure_arity(body, 1);
        js_register_closure_length(body, 1);
        js_register_closure_strict_function(body);
        let rows = closure_registry_census();
        let (name, entries, bytes) = rows
            .iter()
            .find(|(name, _, _)| *name == "closure.body_registry")
            .copied()
            .expect("body registry row");
        assert_eq!(name, "closure.body_registry");
        let distinct_bodies = CLOSURE_BODY_REGISTRY.with(|m| m.borrow().len());
        assert_eq!(entries, distinct_bodies);
        assert!(entries >= 1);
        assert!(bytes > 0);
        assert!(rows
            .iter()
            .any(|(name, _, _)| *name == "closure.trusted_targets"));
    }
}
