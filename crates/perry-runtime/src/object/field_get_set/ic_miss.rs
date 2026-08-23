//! get_field_by_name_f64, IC-miss slow path, and private-brand guards.
//! Pure relocation out of field_get_set.rs (issue #1103 split).

use super::*;

/// Get a field by its string key name, returned as f64 (raw JSValue bits)
/// This preserves the NaN-boxing for strings and other pointer types
#[no_mangle]
pub extern "C" fn js_object_get_field_by_name_f64(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> f64 {
    if let Some(value) = private_member_get_by_name(obj, key) {
        return value;
    }
    if (obj as usize) > 0 && (obj as usize) < 0x10000 && !key.is_null() {
        if let Some(name) = unsafe { super::super::has_own_helpers::str_from_string_header(key) } {
            let class_id = obj as usize as u32;
            if name == "name" && !super::super::class_registry::class_is_key_deleted(class_id, name)
            {
                if let Some(cname) = super::super::class_registry::class_name_for_id(class_id) {
                    let s = crate::string::js_string_from_bytes(cname.as_ptr(), cname.len() as u32);
                    return crate::js_nanbox_string(s as i64);
                }
            }
        }
    }
    // date-fns `constructFrom`: `new date.constructor(value)`. A Date is a
    // NaN-boxed `DateCell` pointer (#2089); `js_object_get_field_by_name`
    // routes `.constructor` to the global Date constructor closure and every
    // other key to `undefined` without derefing the small cell as an object.
    let value = js_object_get_field_by_name(obj, key);
    // #4973: inherits-pattern instances (`http.Server.call(this, …)`) —
    // a read that missed every layer forwards to the aliased native handle
    // so `server.listen` / `server.address` resolve to bound callables on
    // the codegen static-typed read-then-call path.
    if value.bits() == crate::value::TAG_UNDEFINED
        && super::super::native_this_alias::alias_active()
        && !key.is_null()
    {
        if let Some(name) = unsafe { super::super::has_own_helpers::str_from_string_header(key) } {
            if let Some(fwd) =
                super::super::native_this_alias::alias_forward_property_read(obj as usize, name)
            {
                return fwd;
            }
        }
    }
    f64::from_bits(value.bits())
}

/// Read a field by name from a *boxed* receiver, returning `undefined` when the
/// receiver is not an object.
///
/// `js_object_get_field_by_name_f64` takes an already-unboxed `*const
/// ObjectHeader` and dereferences it on faith. That is fine when codegen has
/// proven the receiver is an object, but `Response.json(data, init)` reads its
/// fields off a *runtime* `init` value that can be anything — a number, a
/// string, a symbol. A non-integer double like `3.14` unboxes to a bit pattern
/// squarely inside the heap-pointer magnitude window, so the raw read SIGSEGVs
/// (observed on `Response.json(x, 3.14)`).
///
/// This wrapper applies the same handle-band / `is_valid_obj_ptr` guard the
/// runtime fetch-option reader uses, so a non-object `init` yields `undefined`
/// fields instead of dereferencing a forged pointer. Codegen calls this with
/// the boxed value rather than re-implementing the pointer checks in IR.
#[no_mangle]
pub extern "C" fn js_object_get_field_by_name_boxed(
    receiver: f64,
    key: *const crate::StringHeader,
) -> f64 {
    let value = crate::value::JSValue::from_bits(receiver.to_bits());
    if !value.is_pointer() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let raw = crate::value::js_nanbox_get_pointer(receiver);
    if raw == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // A handle-band id (a `Response`/`Request` forwarded as init) is not a heap
    // ObjectHeader; `js_object_get_field_by_name_f64` routes it through the
    // handle property dispatch, so hand it over directly.
    if crate::value::addr_class::is_handle_band(raw as usize) {
        return js_object_get_field_by_name_f64(raw as *const ObjectHeader, key);
    }
    if raw < 0x10000 || !crate::value::addr_class::is_valid_obj_ptr(raw as *const u8) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    js_object_get_field_by_name_f64(raw as *const ObjectHeader, key)
}

/// #2058: the universal `Object.prototype` methods inherited by every value,
/// including primitive numbers. Read as a property *value* (e.g.
/// `const f = n.toString`, `typeof n.isPrototypeOf`), these resolve to real
/// callable functions in Node — Perry binds them lazily via
/// `js_class_method_bind` so the value is both `typeof "function"` and
/// dispatchable through `js_native_call_method` (every name here has a
/// corresponding dispatch arm). `constructor` is excluded: it is a property
/// holding the `Number` function, not a bound method.
pub(crate) fn primitive_proto_method_name_static(key: &[u8]) -> Option<&'static [u8]> {
    match key {
        b"toString" => Some(b"toString"),
        b"valueOf" => Some(b"valueOf"),
        b"hasOwnProperty" => Some(b"hasOwnProperty"),
        b"isPrototypeOf" => Some(b"isPrototypeOf"),
        b"propertyIsEnumerable" => Some(b"propertyIsEnumerable"),
        b"toLocaleString" => Some(b"toLocaleString"),
        _ => None,
    }
}

/// Bind a primitive receiver's inherited method without allowing the closure
/// to retain the caller's key storage. Both finite-number guards route through
/// this helper so the pointer-lifetime rule has a single implementation.
pub(crate) unsafe fn bind_primitive_proto_method_static(
    receiver: f64,
    key: &[u8],
) -> Option<JSValue> {
    let method = primitive_proto_method_name_static(key)?;
    let result = super::super::js_class_method_bind(receiver, method.as_ptr(), method.len());
    Some(JSValue::from_bits(result.to_bits()))
}

/// Static-name lowering traffics in immutable AOT descriptors instead of
/// thread-local heap pointers. APIs below this wrapper still consume a
/// `StringHeader*`, so descriptors are lazily interned once per runtime thread.
#[no_mangle]
pub extern "C" fn js_object_get_field_by_property_id_f64(
    obj: *const ObjectHeader,
    property_id: i64,
) -> f64 {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(key_ref) = crate::string::perry_string_ref_from_dispatch_id(property_id, &mut scratch)
    else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    let key = crate::string::materialize_dispatch_key(key_ref);
    js_object_get_field_by_name_f64(obj, key)
}

/// By-id sibling of `js_object_set_field_by_name`. See
/// `js_object_get_field_by_property_id_f64` for descriptor materialization.
#[no_mangle]
pub extern "C" fn js_object_set_field_by_property_id(
    obj: *mut ObjectHeader,
    property_id: i64,
    value: f64,
) {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(key_ref) = crate::string::perry_string_ref_from_dispatch_id(property_id, &mut scratch)
    else {
        return;
    };
    let key = crate::string::materialize_dispatch_key(key_ref);
    js_object_set_field_by_name(obj, key, value);
}

pub(crate) fn is_array_method_value_name(key: &[u8]) -> bool {
    matches!(
        key,
        b"pop" | b"push" | b"shift" | b"unshift" | b"splice" | b"slice"
    )
}

pub(crate) fn set_method_value_name(key: &[u8]) -> Option<&'static [u8]> {
    match key {
        b"add" => Some(b"add"),
        b"clear" => Some(b"clear"),
        b"delete" => Some(b"delete"),
        b"entries" => Some(b"entries"),
        b"forEach" => Some(b"forEach"),
        b"has" => Some(b"has"),
        b"keys" => Some(b"keys"),
        b"values" => Some(b"values"),
        b"union" => Some(b"union"),
        b"intersection" => Some(b"intersection"),
        b"difference" => Some(b"difference"),
        b"symmetricDifference" => Some(b"symmetricDifference"),
        b"isSubsetOf" => Some(b"isSubsetOf"),
        b"isSupersetOf" => Some(b"isSupersetOf"),
        b"isDisjointFrom" => Some(b"isDisjointFrom"),
        b"@@iterator" => Some(b"@@iterator"),
        _ => None,
    }
}

/// A `Timeout` / `Immediate` handle method key, as a `'static` byte string:
/// the literal out of this list, never a borrow of `key`.
///
/// #8133 — this replaced the `is_timer_handle_method_key` PREDICATE it used to
/// be, and the replacement is the fix, not a refactor. Every caller derives
/// `key` as `key_string + size_of::<StringHeader>()` — the interior of a
/// movable GC heap string that is unreachable the moment the read returns —
/// and then handed that same pointer to `js_class_method_bind`, which captures
/// it into the bound closure for `dispatch_bound_method` to re-read at CALL
/// time. #7747 fixed exactly this on the Buffer path and its commit message
/// names the consequence: "whether the stale bytes still spell the method is an
/// allocator property, not a program property", which is why it passed locally
/// and took a SIGSEGV on conformance-smoke.
///
/// Returning the literal rather than answering `bool` is deliberate: with no
/// predicate left, a caller has nothing to pair with its own pointer, so the
/// bug cannot be reintroduced by writing the obvious code. Same shape as
/// `set_method_value_name` above and `buffer_method_name_static` (#7747).
pub(crate) fn timer_handle_method_name_static(key: &[u8]) -> Option<&'static [u8]> {
    match key {
        b"ref" => Some(b"ref"),
        b"unref" => Some(b"unref"),
        b"hasRef" => Some(b"hasRef"),
        b"refresh" => Some(b"refresh"),
        b"close" => Some(b"close"),
        b"__perry_dispose__" => Some(b"__perry_dispose__"),
        // `using t = setTimeout(...)` / `t[Symbol.dispose]` — the well-known
        // dispose symbol lowers to this key. (#1213)
        b"@@__perry_wk_dispose" => Some(b"@@__perry_wk_dispose"),
        b"@@__perry_wk_toPrimitive" => Some(b"@@__perry_wk_toPrimitive"),
        _ => None,
    }
}

/// Words in a per-site property-read cache global (`@perry_ic_N`). Codegen
/// emits `[PIC_CACHE_WORDS x i64] zeroinitializer`; this type is the runtime's
/// view of the same memory.
pub const PIC_CACHE_WORDS: usize = 12;

/// The runtime view of a `@perry_ic_N` property-read cache.
///
/// Layout (#7753 — the ways are new; words 0..2 are unchanged from #51/#6080a
/// so the monomorphic path is bit-for-bit what it always was):
///
/// | word | meaning |
/// |---|---|
/// | 0 | `tok0` — most-recently-used ShapeId token |
/// | 1 | `slot0` — its resolved field slot |
/// | 2 | reserved non-identity scratch |
/// | 3,4 / 5,6 / 7,8 / 9,10 | `(tok, slot)` ways |
/// | 11 | round-robin victim index for the ways |
pub type PicCache = [i64; PIC_CACHE_WORDS];

/// First word of the polymorphic way array.
///
/// The ways start at 4, not 3, so that [`PIC_WAY_STATE`] can sit at word 3 —
/// inside the same 64-byte line as the MRU entry the miss path has already
/// touched. Parked after the ways instead (word 11, byte 88) the gate load
/// pulled in a SECOND cache line on every miss, which on a site that misses
/// every read cost ~9% all by itself.
pub(crate) const PIC_WAY_BASE: usize = 4;
/// Number of `(token, slot)` ways beyond the MRU entry. Total shapes a site
/// can resolve inline is `PIC_WAYS + 1`.
pub(crate) const PIC_WAYS: usize = 4;
/// Word holding the way state, which the emitted gate reads as a single signed
/// compare:
///
/// | value | meaning | emitted code |
/// |---|---|---|
/// | `0` | no way is populated (fresh site) | skip the compares |
/// | `> 0` | armed: bit 0 set, bits 1..7 the round-robin victim, bits 8.. the *consecutive* capacity-eviction run | run the compares |
/// | `< 0` | **megamorphic** — the rotation is wider than the ways hold. The magnitude is a countdown: each further miss adds 1, and at 0 the site is armed again | skip the compares |
pub(crate) const PIC_WAY_STATE: usize = 3;
/// Bit 0 of [`PIC_WAY_STATE`]: at least one way is populated. Carried
/// explicitly so an armed site with victim 0 and no evictions is still `> 0`,
/// which is the whole predicate the emitted gate evaluates.
const PIC_STATE_ARMED: i64 = 1;
/// **Consecutive** capacity evictions tolerated before a site latches
/// megamorphic.
///
/// Consecutive is load-bearing. A cumulative count latches any long-running
/// site that ever sees an extra shape: the interpreter's `evalNode` handles
/// `let`/`fun` nodes twice per round, which is 80 stray evictions over a run,
/// so a cumulative counter turned the ways off on the very site they were built
/// for and gave back the entire win (2.39 s → 3.03 s, measured). Any prime that
/// finds room — a free way, or its shape already in one — proves the site is
/// coping and resets the run to zero.
const PIC_MEGAMORPHIC_EVICTIONS: i64 = 16;
/// Misses a megamorphic site serves before the ways get another chance.
///
/// The latch must NOT be permanent. "Megamorphic" is a property of a program
/// *phase*, not of a site: the interpreter's `evalNode` sees five hot node kinds
/// while it is running `fib`, and a different set while it is running the
/// string-building program. A sticky latch let the second phase kill the site
/// for the rest of the process — 2.39 s → 3.02 s, measured, with the ways
/// working perfectly right up until the first phase change and never again.
///
/// Counting down instead costs a megamorphic site one increment per miss and a
/// re-warm every `PIC_LATCH_RETRY` misses (16 way-compares out of 2048 reads),
/// while a phase-changed site recovers within one such window.
const PIC_LATCH_RETRY: i64 = 2048;

/// Prime the MRU entry, cascading the shape it evicts into the ways.
///
/// Word 0 keeps exactly its pre-#7753 meaning — last shape seen, always
/// overwritten — so a genuinely monomorphic site behaves identically. What
/// changes is that the *evicted* shape is no longer thrown away: it moves into
/// a way, and the emitted poly block (reached only after word 0 misses)
/// resolves it inline instead of calling back into this handler. A site that
/// alternates between k ≤ `PIC_WAYS + 1` shapes therefore stops thrashing.
///
/// Every token is derived from an authoritative, never-reused ShapeId. A shape
/// transition therefore makes an old way go cold without requiring an address
/// epoch or any GC-visible cache rewriting.
///
/// # Safety
/// `cache` must point at a live `[i64; PIC_CACHE_WORDS]` (the codegen-emitted
/// per-site global, or a stack array of that type).
pub(crate) unsafe fn pic_prime_get(cache: *mut PicCache, token: i64, slot: i64) {
    let c = &mut *cache;
    let prev_tok = c[0];
    let prev_slot = c[1];
    c[0] = token;
    c[1] = slot;
    // Megamorphic. A rotation wider than the ways hold never hits one, so the
    // compare sequence becomes pure cost — measured at **+37%** on a 7-shape
    // site, against a 2.5x SPEEDUP on a 5-shape one. That asymmetry is the whole
    // reason this state word exists: without it the ways pay well inside
    // capacity and punish just past it, which is not a trade a compiler gets to
    // make on the user's behalf. The ways are already zeroed when the latch is
    // set and the emitted gate stops reading them, so a latched site is left
    // with exactly its pre-#7753 code path.
    //
    // The countdown is what keeps that from being a one-way door — see
    // [`PIC_LATCH_RETRY`].
    let state = c[PIC_WAY_STATE];
    if state < 0 {
        c[PIC_WAY_STATE] = state + 1;
        return;
    }
    let cascade = prev_tok != 0 && prev_tok != token;
    // One pass over the ways does three things:
    //   * evicts `token` from a way if it has one — it now lives in the MRU
    //     entry, and leaving the stale copy behind would permanently cost a way
    //     (a k-shape rotation would then only ever cache k-1 of them);
    //   * refreshes `prev_tok`'s way if it already has one;
    //   * remembers the first empty way for the cascade.
    let mut free: Option<usize> = None;
    let mut prev_present = false;
    for w in 0..PIC_WAYS {
        let ti = PIC_WAY_BASE + w * 2;
        if c[ti] == token {
            c[ti] = 0;
            c[ti + 1] = 0;
        } else if cascade && c[ti] == prev_tok {
            c[ti + 1] = prev_slot;
            prev_present = true;
            continue;
        }
        if c[ti] == 0 && free.is_none() {
            free = Some(ti);
        }
    }
    if prev_present {
        // The shape is already cached: the site is coping, so the eviction run
        // resets here too.
        c[PIC_WAY_STATE] = PIC_STATE_ARMED | (((c[PIC_WAY_STATE] >> 1) & 0x7f) << 1);
        return;
    }
    if !cascade {
        return;
    }
    let victim = (state >> 1) & 0x7f;
    let ti = match free {
        Some(ti) => {
            // Room was available, so the site is coping: reset the eviction run.
            c[PIC_WAY_STATE] = PIC_STATE_ARMED | (victim << 1);
            ti
        }
        None => {
            // No free way: this shape displaces another. Inside capacity that
            // happens only during warm-up; past it, on every single miss.
            let run = (state >> 8) + 1;
            if run >= PIC_MEGAMORPHIC_EVICTIONS {
                for w in 0..PIC_WAYS {
                    c[PIC_WAY_BASE + w * 2] = 0;
                    c[PIC_WAY_BASE + w * 2 + 1] = 0;
                }
                c[PIC_WAY_STATE] = -PIC_LATCH_RETRY;
                return;
            }
            let v = (victim + 1) % PIC_WAYS as i64;
            c[PIC_WAY_STATE] = PIC_STATE_ARMED | (v << 1) | (run << 8);
            PIC_WAY_BASE + v as usize * 2
        }
    };
    c[ti] = prev_tok;
    c[ti + 1] = prev_slot;
}

/// The receiver's GC object type, or `None` when the address does not carry a
/// readable `GcHeader`.
///
/// # Safety
/// `obj` is only *inspected*; `try_read_gc_header` validates the address first.
#[inline]
unsafe fn gc_type_of(obj: *const ObjectHeader) -> Option<u8> {
    crate::value::addr_class::try_read_gc_header(obj as usize).map(|h| h.obj_type)
}

/// Does this heap property key have exactly these bytes?
///
/// Length first, so a mismatched key costs one `u32` load and a compare — the
/// point is to keep the fast-path probe cheaper than the ladder it skips.
///
/// # Safety
/// `key` must be null or a live heap `StringHeader` (the same contract every
/// other key read in this file relies on — property-name literals are interned
/// as heap strings, never SSO immediates).
#[inline]
unsafe fn key_bytes_are(key: *const crate::StringHeader, want: &[u8]) -> bool {
    if key.is_null() || (*key).byte_len as usize != want.len() {
        return false;
    }
    let p = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    std::slice::from_raw_parts(p, want.len()) == want
}

/// Monomorphic inline cache miss handler (issue #51).
///
/// Called when the codegen-emitted ShapeId check misses.
/// fails. Performs the full field lookup via `js_object_get_field_by_name`,
/// then populates the per-site cache so subsequent calls with the same shape
/// hit the inline fast path (no function call, direct field load).
///
/// `cache` layout: see [`PicCache`]. Words 0..1 are the ShapeId-token MRU entry;
/// word 2 is reserved scratch, and words 3.. are the polymorphic ways filled by
/// [`pic_prime_get`] (#7753).
///
/// Only caches when:
/// - obj is a valid ObjectHeader (not null, not handle, not string/array/etc.)
/// - field exists and its slot index is below `shape.live_inline_slot_count`
///
/// Overflow fields (slot >= alloc_limit) are NOT cached and fall through to
/// the slow path — the fast path loads from `obj_ptr + 24 + slot*8` which
/// would read past the inline allocation.
#[no_mangle]
pub extern "C" fn js_object_get_field_ic_miss(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    cache: *mut PicCache,
) -> f64 {
    // SSO receiver — never cacheable. Route through the SSO-aware
    // `js_object_get_field_by_name` which handles `.length` inline
    // and returns undefined for other keys.
    if !key.is_null() {
        let obj_bits = obj as u64;
        if (obj_bits & crate::value::TAG_MASK) == crate::value::SHORT_STRING_TAG {
            let v = js_object_get_field_by_name(obj, key);
            return f64::from_bits(v.bits());
        }
    }
    if obj.is_null() || key.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // A Proxy value may reach the inline-cache miss handler when a fused
    // property read `proxy.col` misses its monomorphic shape check (a Proxy
    // has no stable `keys_array`, so every read is a miss). Proxies are encoded
    // as small fake pointers in the band [0xF0000, 0x100000); deref-ing one as
    // an ObjectHeader — or passing it to `closure_dynamic_prop_by_key`, which
    // reads `CLOSURE_MAGIC` at offset 12 via `is_closure_ptr` — reads unmapped
    // memory and SIGSEGVs (drizzle's aliased-column Proxy in `findMany`). Route
    // to the proxy get dispatch first, exactly like `js_object_get_field_by_name`
    // (#2846). `js_proxy_is_proxy` validates the value is a *registered* proxy so
    // a real heap object whose address happens to be small isn't misrouted.
    {
        let addr = obj as u64;
        if crate::value::addr_class::is_proxy_id_band(addr as usize) {
            const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
            let boxed = f64::from_bits(POINTER_TAG | (addr & 0x0000_FFFF_FFFF_FFFF));
            if crate::proxy::js_proxy_is_proxy(boxed) != 0 {
                let key_f64 = f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits());
                return crate::proxy::js_proxy_get(boxed, key_f64);
            }
        }
    }
    // Only run the closure / buffer / typedarray probes on real heap
    // receivers (>= 0x100000). A Web-Fetch handle (Headers/Request/Response/
    // Blob, id in [0x40000, 0x100000)) or any other small native handle is NOT
    // a heap pointer; `closure_dynamic_prop_by_key` reaches `is_closure_ptr`,
    // which dereferences `[obj + 12]` for CLOSURE_MAGIC and SIGSEGVs on the
    // handle's unmapped low address (hit by hono's logger reading a property
    // off a Response/Headers handle). Small handles fall through to the
    // `< 0x100000` proxy / HANDLE_PROPERTY_DISPATCH routing below — matching
    // the ordering in `js_object_get_field_by_name`. The macOS heap floor
    // (0x200_0000_0000 in is_valid_obj_ptr) masked this; Linux's is 0x1000.
    if !key.is_null() {
        unsafe {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            if let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len)) {
                if let Some(value) =
                    crate::async_hooks::try_async_resource_property_dispatch(obj as i64, name)
                {
                    return value;
                }
            }
        }
    }
    if crate::value::addr_class::is_above_handle_band(obj as usize) {
        // #7753: `arr.length` on a receiver codegen could not prove is an array.
        //
        // The inline cache can never serve this read — it requires a
        // GC_TYPE_OBJECT receiver by construction (#72, so an Array's
        // `element[1]` is never mistaken for `keys_array`) — so EVERY dynamic
        // `.length` lands here, and then walks a ladder built for objects: a
        // closure-magic deref, two side-table registry probes behind
        // thread-locals, then `js_object_get_field_by_name`'s own dispatch,
        // which repeats the registry probes before finally reaching the array
        // arm. On a tree-walking interpreter whose variable lookup is
        // `for (i = 0; i < names.length; i++)`, that one read was 22% of total
        // run time — more than the entire polymorphic-dispatch fix above saved.
        //
        // `GC_TYPE_ARRAY` is a genuine dense array: buffers, typed arrays, lazy
        // arrays, Sets and Maps all carry their own distinct `obj_type`, and an
        // `class X extends Array` instance is an `ObjectHeader`
        // (`GC_TYPE_OBJECT`). `js_array_length` still resolves growth-forwarding
        // stubs, proxies and subclass receivers, so this only skips probes that
        // cannot match — the expression returned is exactly the one
        // `get_field_by_name_object_tail`'s array arm computes for this key,
        // which is what makes it a pure short-circuit rather than a second
        // implementation.
        if unsafe { gc_type_of(obj) } == Some(crate::gc::GC_TYPE_ARRAY)
            && unsafe { key_bytes_are(key, b"length") }
        {
            let arr = obj as *const crate::array::ArrayHeader;
            return crate::array::js_array_length(arr) as f64;
        }
        unsafe {
            if let Some(val) = closure_dynamic_prop_by_key(obj as usize, key) {
                return val;
            }
            // Buffers have no GcHeader. The generic IC-miss object path below may
            // inspect GC/object metadata, so mirror js_object_get_field_by_name's
            // buffer-first dispatch here.
            if crate::buffer::is_registered_buffer(obj as usize) {
                let value = js_object_get_field_by_name(obj, key);
                return f64::from_bits(value.bits());
            }
            if crate::typedarray::lookup_typed_array_kind(obj as usize).is_some() {
                let value = js_object_get_field_by_name(obj, key);
                return f64::from_bits(value.bits());
            }
        }
    }
    // Issue #340: small-handle receivers (axios, fastify, ioredis,
    // ...) are passed here from the codegen IC miss path with the
    // lower-48 of the NaN-box stripped — `obj as usize` is the
    // raw handle id (1, 2, 3, ...). Route to HANDLE_PROPERTY_DISPATCH
    // (registered by stdlib via js_register_handle_property_dispatch)
    // so `r.status` / `r.data` and similar handle-property accesses
    // dispatch to the per-module accessor instead of silently
    // returning undefined.
    if crate::value::addr_class::is_small_handle(obj as usize) {
        // #2846: a revocable Proxy is encoded as a small fake pointer in the
        // proxy-id range (also `< 0x100000`). A generic `proxy.key` read funnels
        // here via the IC-miss path; route it to the proxy get dispatch (which
        // forwards to the target, or throws on a revoked proxy) before the
        // handle-dispatch fallback. `js_proxy_is_proxy` validates the value is a
        // registered proxy so real small handles aren't misrouted.
        {
            const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
            let boxed = f64::from_bits(POINTER_TAG | ((obj as u64) & 0x0000_FFFF_FFFF_FFFF));
            if crate::proxy::js_proxy_is_proxy(boxed) != 0 {
                let key_f64 = f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits());
                return crate::proxy::js_proxy_get(boxed, key_f64);
            }
        }
        // #1213: Timeout/Immediate handle methods (ref/unref/hasRef/refresh/
        // close) read as bound-method function values so `typeof t.ref ===
        // "function"` holds (the call form already works via
        // js_native_call_method). The IC fast path funnels small handles here,
        // bypassing the identical block in `js_object_get_field_by_name`, so it
        // must be mirrored.
        unsafe {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
            if key_bytes == b"constructor" {
                if let Some(value) = crate::timer::timer_constructor_value(obj as i64) {
                    return value;
                }
            }
            if let Some(method) = timer_handle_method_name_static(key_bytes) {
                if crate::timer::is_known_timer_id(obj as i64) {
                    let this_f64 =
                        f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                    // #8133: the `'static` literal, NOT `key_ptr` — that is the
                    // interior of a movable heap string this read does not own.
                    return super::super::js_class_method_bind(
                        this_f64,
                        method.as_ptr(),
                        method.len(),
                    );
                }
            }
            // TextDecoder/TextEncoder registry handles — IC-miss mirror of
            // the arms in `js_object_get_field_by_name` /
            // `get_field_by_name_object_tail`; static-name reads (`td.decode`,
            // `td.encoding`) funnel here. See `text_handle_property`.
            if let Some(v) = crate::text::text_handle_property(obj as usize, key_bytes) {
                return f64::from_bits(v.bits());
            }
        }
        // Drizzle-sqlite blocker: synth `data.constructor` for small-handle
        // receivers — IC-miss path mirror of the constructor intercept in
        // `js_object_get_field_by_name`. Refs #645 deeper followup.
        unsafe {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
            if key_bytes == b"constructor" {
                if let Some(dispatch) = handle_property_dispatch() {
                    let bits = dispatch(obj as i64, key_ptr, key_len);
                    if bits.to_bits() != crate::value::TAG_UNDEFINED {
                        return bits;
                    }
                }
                let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
            }
        }
        if let Some(dispatch) = handle_property_dispatch() {
            unsafe {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let bits = dispatch(obj as i64, key_ptr, key_len);
                // Wall 10 — fall back to a `setPrototypeOf(handle, proto)` member
                // (Express's augmented `res`/`req`) when the native dispatch
                // doesn't know the key. Mirrors `js_object_get_field_by_name`.
                if bits.to_bits() == crate::value::TAG_UNDEFINED {
                    if let Some(v) = crate::object::prototype_chain::object_static_prototype(
                        obj as usize,
                    )
                    .and(
                        crate::object::prototype_chain::resolve_inherited_field(obj as usize, key),
                    ) {
                        if v.bits() != crate::value::TAG_UNDEFINED {
                            return f64::from_bits(v.bits());
                        }
                    }
                }
                return bits;
            }
        }
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if (obj as usize) < 0x10000 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    unsafe {
        // Issue #72: validate this really is a GC_TYPE_OBJECT before reading
        // crate::object::object_keys_array(obj) — otherwise an Array/String/Buffer/etc. receiver
        // (whose word at offset 0 collides with a real `class_id` — since
        // #8113 that is an array's `length`, so ANY length-N array impersonates
        // class N) would be treated as cacheable and seed the per-site PIC with
        // garbage from element[1].
        // The codegen guard funnels non-OBJECT receivers here too, so this
        // belt-and-braces check keeps the cache from being primed with
        // values that would survive into the inline hot path.
        let is_object = (obj as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000
            && is_valid_obj_ptr(obj as *const u8)
            && {
                let gc_header =
                    (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT
            };
        let has_own_descriptors = is_object && super::super::object_has_descriptors(obj as usize);
        // #8122: ONE shape-table probe. `object_is_regular` is `GC_TYPE_OBJECT
        // && !FORWARDED && descriptor.object_kind == Ordinary`; the kind test
        // was already `GC_TYPE_OBJECT` above, so read the descriptor once and
        // take the kind, the keys edge, the key count and the live bound from
        // it — this path used to probe three times (regularity, the
        // descriptor, then `object_shape_id` for the PIC token).
        let shape = if is_object {
            let gc_header =
                (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc_header).gc_flags & crate::gc::GC_FLAG_FORWARDED == 0 {
                crate::object::shapes::object_shape_descriptor(obj)
            } else {
                None
            }
        } else {
            None
        };
        let is_regular = shape.is_some_and(|shape| {
            shape.object_kind == crate::object::shapes::ShapeObjectKind::Ordinary
        });
        // Descriptor-bearing receivers must not prime this PIC: its generated
        // hit path is a raw slot load and would bypass their getter / property
        // semantics. This per-object bit replaces the old process-wide
        // `accessors_in_use` gate. `note_descriptor_target` sets the bit and
        // transitions the receiver's ShapeId before an installed descriptor is
        // observable, while unrelated accessors cannot affect an own data
        // property on this receiver. The emitted hit path independently checks
        // the same bit, so both cache population and cache use fail closed.
        // Gate-neutral builtin accessors set the owner bit as well.
        if is_regular && !has_own_descriptors {
            let Some(shape) = shape else {
                let value = js_object_get_field_by_name(obj, key);
                return f64::from_bits(value.bits());
            };
            let keys = shape.keys as usize as *mut crate::array::ArrayHeader;
            if keys.is_null() || (keys as usize) <= 0x10000 {
                let value = js_object_get_field_by_name(obj, key);
                return f64::from_bits(value.bits());
            }
            let key_count = shape.logical_key_count as usize;
            let keys_data = (keys as *const u8).add(8) as *const f64;
            let alloc_limit = shape.live_inline_slot_count as usize;
            for i in 0..key_count {
                let k_bits = (*keys_data.add(i)).to_bits();
                let k_ptr = (k_bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
                if !k_ptr.is_null() && crate::string::js_string_equals(k_ptr, key) != 0 {
                    if i >= alloc_limit {
                        // Field is in the overflow map — fall through to the
                        // slow path which handles overflow correctly.
                        break;
                    }
                    // The codegen IC fast path computes `obj + object_header_size + slot*8`
                    // and does a direct load. Any inline slot (`i <
                    // alloc_limit`) is reachable via that path, so cache
                    // every inline slot — including the ones at index >= 8
                    // for classes whose `field_count` exceeds the
                    // MIN_FIELD_SLOTS=8 baseline (e.g. World.commandBuffer
                    // sits at slot 12). Pre-fix this branch capped the cache
                    // at `i < 8` which left every >8-slot field permanently
                    // missing the cache: every access fell through to a
                    // fresh keys_array walk + js_string_equals chain. On
                    // perf-comprehensive's hot loops that path was hit
                    // ~900k times per run (40% inclusive samples per
                    // perfcomp.profile).
                    //
                    // The runtime and emitted hit path share one identity:
                    // the authoritative, never-reused ShapeId token.
                    // The descriptor resolved above, so the header stamp IS the
                    // shape id — no second probe.
                    let stamp = crate::object::shapes::object_shape_stamp(obj);
                    let token = (stamp as u64 | crate::object::shapes::PIC_ID_TOKEN_BIT) as i64;
                    pic_prime_get(cache, token, i as i64);
                    let field_ptr = (obj as *const u8)
                        .add(std::mem::size_of::<ObjectHeader>() + i * 8)
                        as *const f64;
                    return *field_ptr;
                }
            }
        }
    }
    let value = js_object_get_field_by_name(obj, key);
    f64::from_bits(value.bits())
}

/// #5391 path 3: full-outlined generic property GET.
///
/// In oversized (full-outline) modules the inline generic-get diamond expands to
/// ~60 IR instructions and ~13 basic blocks per property-get site: receiver-tag
/// routing (SSO / INT32 class-ref / valid-pointer / nullish), a monomorphic
/// inline cache (shape check + hit/miss), typed-feedback recording, and the
/// nullish-throw. On a large minified bundle that is the single biggest
/// contributor to generated `__text`. This helper collapses the whole site to one
/// call by reproducing that branch ladder here, dispatching to the *exact same*
/// runtime entries the inline code calls — so behavior is unchanged. The only
/// thing dropped is the inline monomorphic fast-load: every read goes through the
/// cache-priming slow path (`js_object_get_field_ic_miss`), trading a little speed
/// for a large code-size win, the same trade the class-field GET/SET full-outline
/// paths (`js_class_field_get_ic` / `js_class_field_set_ic`) already make.
///
/// Argument shapes mirror the inline site operands exactly:
/// - `obj_bits`: the receiver's full (unmasked) NaN-box bits
/// - `key`: the property-name `StringHeader`, already masked to a raw pointer
/// - `site_id`: the typed-feedback site id
/// - `cache`: the per-site monomorphic IC cache global (primed by `..._ic_miss`)
#[no_mangle]
pub extern "C" fn js_object_get_field_ic(
    obj_bits: i64,
    key: *const crate::StringHeader,
    site_id: u64,
    cache: *mut PicCache,
) -> f64 {
    // POINTER_MASK: lower 48 bits — strips the NaN-box tag to a raw heap pointer.
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
    let bits = obj_bits as u64;
    let tag = bits >> 48;
    // `obj_bits` reinterpreted as a pointer keeps the tag bits (the SSO / class-ref
    // / by-name helpers need the unmasked value); `obj_handle` is the masked heap
    // pointer the inline-cache miss handler + feedback observe expect.
    let obj_unmasked = bits as usize as *const ObjectHeader;
    let obj_handle = (bits & POINTER_MASK) as usize as *const ObjectHeader;

    // SSO receiver (SHORT_STRING_TAG = 0x7FF9): the SSO-aware by-name helper reads
    // `.length` from the NaN-box payload and returns undefined for other keys.
    if tag == 0x7FF9 {
        return js_object_get_field_by_name_f64(obj_unmasked, key);
    }
    // INT32-tagged class ref (0x7FFE): static-field / dynamic-prop / synthetic
    // `constructor` lookup via the feedback-wrapped by-name helper. Passes the
    // unmasked bits so the runtime can detect the INT32 tag.
    if tag == 0x7FFE {
        return crate::typed_feedback::js_typed_feedback_object_get_field_by_name_f64(
            site_id,
            obj_unmasked,
            key,
        );
    }
    // Valid heap pointer or string (masked tag 0x7FFD): record feedback, then route
    // through the cache-priming inline-cache-miss handler — the same entry the
    // inline diamond's miss arm calls (objects, closures, buffers, typed arrays,
    // proxies, small handles all dispatch correctly there, and the per-site cache
    // is primed for any future inline sites sharing this global).
    if (tag & 0xFFFD) == 0x7FFD {
        crate::typed_feedback::js_typed_feedback_observe_property_get(site_id, obj_handle, key);
        return js_object_get_field_ic_miss(obj_handle, key, cache);
    }
    // Invalid (non-pointer) receiver. `undefined`/`null` throw a TypeError (#462 —
    // matches the inline nullish path, which aborts with a node-shaped message);
    // other primitives route through the by-name helper, which can still resolve
    // typed-shape reads (e.g. Date `.constructor`).
    if bits == crate::value::TAG_UNDEFINED || bits == crate::value::TAG_NULL {
        let is_null = u32::from(bits == crate::value::TAG_NULL);
        let (ptr, len) = unsafe {
            match super::super::has_own_helpers::str_from_string_header(key) {
                Some(s) => (s.as_ptr(), s.len()),
                None => (std::ptr::null(), 0),
            }
        };
        crate::error::js_throw_type_error_property_access(is_null, ptr, len);
    }
    js_object_get_field_by_name_f64(obj_unmasked, key)
}

// Polymorphic numeric-key get/set (`js_object_get_index_polymorphic` /
// `js_object_set_index_polymorphic`) live in `polymorphic_index.rs`:
// they dispatch by GC type (array vs object vs closure vs buffer) rather
// than touching object field storage directly, so they were split out
// of this module. See `polymorphic_index.rs` for the implementations
// and the #471 fix notes.

#[cfg(test)]
mod sso_tests_1781 {
    use super::super::*;

    #[test]
    fn object_keys_values_entries_on_string_do_not_crash() {
        // Regression: Object.keys/values/entries on a string segfaulted
        // (the value was deref'd as an ObjectHeader; SSO strings aren't even
        // pointers). Now they yield index keys / chars / [index,char].
        let heap = crate::string::js_string_from_bytes(b"abc".as_ptr(), 3);
        let v = crate::value::js_nanbox_string(heap as i64);
        assert_eq!(crate::array::js_array_length(js_object_keys_value(v)), 3);
        assert_eq!(crate::array::js_array_length(js_object_values_value(v)), 3);
        assert_eq!(crate::array::js_array_length(js_object_entries_value(v)), 3);
        // SSO string (<= 5 bytes) — the non-pointer case that crashed hardest.
        let sso = crate::value::JSValue::try_short_string(b"hi").unwrap();
        assert_eq!(
            crate::array::js_array_length(js_object_keys_value(f64::from_bits(sso.bits()))),
            2
        );
        // Number / boolean primitives → empty array (no own enumerable keys).
        assert_eq!(crate::array::js_array_length(js_object_keys_value(42.0)), 0);
    }

    /// #1781: `"id" in obj` for a key <= 5 bytes — the lookup key arrives as
    /// an inline SSO value (tag 0x7FF9). `is_string()` (STRING_TAG-only)
    /// rejected it, so `js_object_has_property` returned false even though the
    /// object had the key (stored keys are always heap, so materializing the
    /// SSO lookup key lets js_string_equals match).
    #[test]
    fn in_operator_finds_object_key_via_sso_lookup() {
        {
            let obj = crate::object::js_object_alloc(0, 0);
            let key = crate::string::js_string_from_bytes(b"id".as_ptr(), 2);
            crate::object::js_object_set_field_by_name(obj, key, 42.0);

            let obj_box = crate::value::js_nanbox_pointer(obj as i64);
            let sso = crate::value::JSValue::try_short_string(b"id").unwrap();
            assert!(sso.is_short_string());
            let present = js_object_has_property(obj_box, f64::from_bits(sso.bits()));
            assert_ne!(
                crate::value::js_is_truthy(present),
                0,
                "SSO key 'id' should be found via `in`"
            );

            let missing = crate::value::JSValue::try_short_string(b"zz").unwrap();
            let absent = js_object_has_property(obj_box, f64::from_bits(missing.bits()));
            assert_eq!(
                crate::value::js_is_truthy(absent),
                0,
                "absent SSO key 'zz' should not be found"
            );
        }
    }
}

crate::perry_thread_local! {
    /// The ClassDefinitionEvaluation captured by the currently executing
    /// method function. Each vtable call pushes one entry, including an
    /// `undefined` delimiter for ordinary classes, so a nested call never
    /// inherits its caller's private-name environment by accident.
    static PRIVATE_LEXICAL_BRAND_STACK: std::cell::RefCell<Vec<u64>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

pub(crate) fn private_lexical_brand_push(value: f64) {
    PRIVATE_LEXICAL_BRAND_STACK.with(|stack| stack.borrow_mut().push(value.to_bits()));
}

pub(crate) fn private_lexical_brand_pop() {
    PRIVATE_LEXICAL_BRAND_STACK.with(|stack| {
        stack.borrow_mut().pop();
    });
}

pub(crate) fn private_lexical_brand_stack_savepoint() -> usize {
    PRIVATE_LEXICAL_BRAND_STACK.with(|stack| stack.borrow().len())
}

pub(crate) fn private_lexical_brand_stack_restore(depth: usize) {
    PRIVATE_LEXICAL_BRAND_STACK.with(|stack| stack.borrow_mut().truncate(depth));
}

pub(crate) fn scan_private_lexical_brand_roots_mut(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    PRIVATE_LEXICAL_BRAND_STACK.with(|stack| {
        for bits in stack.borrow_mut().iter_mut() {
            visitor.visit_nanbox_u64_slot(bits);
        }
    });
}

fn current_private_lexical_brand(declaring_class_id: u32) -> Option<u64> {
    PRIVATE_LEXICAL_BRAND_STACK.with(|stack| {
        let bits = *stack.borrow().last()?;
        let value = f64::from_bits(bits);
        private_evaluation_brand(value, declaring_class_id).map(|_| bits)
    })
}

/// Stamp an instance constructed through a `ClassExprFresh` value with the
/// identity of that particular class evaluation. The brand lives in the
/// object's traced metadata record so it neither shifts user field slots nor
/// changes the instance's ShapeId / own-key enumeration.
pub(crate) unsafe fn stamp_private_evaluation_brand(obj: *mut ObjectHeader, class_value: f64) {
    if obj.is_null() || !super::super::class_registry::is_class_object_value(class_value) {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let class_handle = scope.root_nanbox_f64(class_value);
    let (meta, _) =
        obj_handle.across_mut::<ObjectHeader, _>(|| crate::object::object_meta_ensure(obj));
    let brand = class_handle.get_nanbox_f64().to_bits();
    (*meta).private_evaluation_brand = brand;
    crate::gc::runtime_write_barrier_slot(
        meta as usize,
        &(*meta).private_evaluation_brand as *const u64 as usize,
        brand,
    );
}

/// Return the per-evaluation brand carried by `value`, provided it belongs to
/// `declaring_class_id`'s compile-time template. A fresh class object is its
/// own static brand; instances carry that object in the hidden slot above.
fn private_evaluation_brand(value: f64, declaring_class_id: u32) -> Option<u64> {
    if declaring_class_id == 0 {
        return None;
    }
    let value = crate::proxy::private_element_receiver(value);
    if super::super::class_registry::is_class_object_value(value) {
        let object = JSValue::from_bits(value.to_bits()).as_pointer::<ObjectHeader>();
        if !object.is_null() && js_object_get_class_id(object) == declaring_class_id {
            return Some(value.to_bits());
        }
    }
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let object = value.as_pointer::<ObjectHeader>();
    let brand = unsafe {
        if object.is_null() || !crate::object::object_is_shaped(object) || (*object).meta.is_null()
        {
            return None;
        }
        f64::from_bits((*(*object).meta).private_evaluation_brand)
    };
    if !super::super::class_registry::is_class_object_value(brand) {
        return None;
    }
    let object = JSValue::from_bits(brand.to_bits()).as_pointer::<ObjectHeader>();
    (!object.is_null() && js_object_get_class_id(object) == declaring_class_id)
        .then_some(brand.to_bits())
}

/// Return the fresh ClassDefinitionEvaluation object carried by a constructor
/// or instance. Unlike `private_evaluation_brand`, this does not require the
/// caller to know the compile-time template id; method dispatch uses it to
/// establish the callee's lexical private-name environment.
pub(crate) fn private_evaluation_brand_value(value: f64) -> Option<f64> {
    let value = crate::proxy::private_element_receiver(value);
    if super::super::class_registry::is_class_object_value(value) {
        return Some(value);
    }
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let object = value.as_pointer::<ObjectHeader>();
    let brand = unsafe {
        if object.is_null() || !crate::object::object_is_shaped(object) || (*object).meta.is_null()
        {
            return None;
        }
        f64::from_bits((*(*object).meta).private_evaluation_brand)
    };
    super::super::class_registry::is_class_object_value(brand).then_some(brand)
}

include!("ic_miss/private_member_access.rs");

fn private_field_marker_key(
    declaring_class_id: u32,
    field_name_ptr: *const u8,
    field_name_len: u32,
) -> Option<String> {
    if field_name_ptr.is_null() || field_name_len == 0 {
        return None;
    }
    let field_name = unsafe {
        std::str::from_utf8(std::slice::from_raw_parts(
            field_name_ptr,
            field_name_len as usize,
        ))
        .ok()?
    };
    Some(format!(
        "#<perry:private-field:{declaring_class_id}:{field_name}>"
    ))
}

fn private_marker_is_present(storage: f64, marker: &str) -> bool {
    crate::object::js_object_get_own_field_or_undef(storage, marker.as_ptr(), marker.len())
        .to_bits()
        != crate::value::TAG_UNDEFINED
}

fn private_instance_element_is_present(
    storage: f64,
    declaring_class_id: u32,
    field_name_ptr: *const u8,
    field_name_len: u32,
    kind: u32,
) -> bool {
    let marker = if kind == 0 {
        private_field_marker_key(declaring_class_id, field_name_ptr, field_name_len)
    } else {
        Some(private_brand_key(declaring_class_id))
    };
    marker.is_some_and(|marker| private_marker_is_present(storage, &marker))
}

/// Install one class's instance-private brand on `obj`.
///
/// A class contributes one brand regardless of how many private fields,
/// methods, or accessors it declares.  Re-installing that brand on the same
/// object is the observable error required by PrivateFieldAdd and
/// PrivateMethodOrAccessorAdd (for example when a base constructor returns an
/// object that was already initialized by the derived class once).
#[no_mangle]
pub extern "C" fn js_private_brand_add(obj: f64, declaring_class_id: u32) -> f64 {
    if declaring_class_id == 0 {
        return obj;
    }
    let storage = crate::proxy::private_element_receiver(obj);
    let marker = private_brand_key(declaring_class_id);
    if private_marker_is_present(storage, &marker) {
        throw_private_type_error("Cannot initialize private elements twice on the same object");
    }
    let value = JSValue::from_bits(storage.to_bits());
    if !value.is_pointer() {
        throw_private_type_error("Cannot initialize private elements on a non-object");
    }
    let object = value.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
    if object.is_null() || !crate::value::addr_class::is_plausible_heap_addr(object as usize) {
        throw_private_type_error("Cannot initialize private elements on a non-object");
    }
    // The marker-key allocation can evacuate both the receiver and any live
    // value. Root first, then derive raw pointers only inside scoped handle
    // accessors after the allocation.
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(object);
    let key = crate::string::js_string_from_bytes(marker.as_ptr(), marker.len() as u32);
    let key = scope.root_string_ptr(key);
    object.with_mut_ptr::<ObjectHeader, _>(|object| {
        key.with_const_ptr::<crate::StringHeader, _>(|key| {
            js_object_set_field_by_name(object, key, f64::from_bits(crate::value::TAG_TRUE));
        });
    });
    obj
}

/// Define an instance private field without going through Proxy [[Set]].  The
/// corresponding class brand is installed once by `js_private_brand_add`.
#[no_mangle]
pub extern "C" fn js_private_field_add(
    obj: f64,
    declaring_class_id: u32,
    field_key: f64,
    value: f64,
) -> f64 {
    // A short-string key may be materialized here, so root every GC-managed
    // operand before asking for a StringHeader and use only refreshed handles
    // afterwards.
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_nanbox_f64(obj);
    let field_key = scope.root_nanbox_f64(field_key);
    let value = scope.root_nanbox_f64(value);
    let field_key_ptr = crate::value::js_get_string_pointer_unified(field_key.get_nanbox_f64())
        as *const crate::StringHeader;
    if field_key_ptr.is_null() {
        throw_private_type_error("Invalid private field name");
    }
    let field_name_len = unsafe { (*field_key_ptr).byte_len };
    let field_name_ptr = crate::string::string_data(field_key_ptr);
    let Some(marker) = private_field_marker_key(declaring_class_id, field_name_ptr, field_name_len)
    else {
        throw_private_type_error("Invalid private field name");
    };
    let field_name = unsafe {
        std::str::from_utf8(std::slice::from_raw_parts(
            field_name_ptr,
            field_name_len as usize,
        ))
    }
    .unwrap_or_else(|_| throw_private_type_error("Invalid private field name"));
    let storage_name = format!("#<perry:private-value:{declaring_class_id}:{field_name}>");
    let storage = crate::proxy::private_element_receiver(obj.get_nanbox_f64());
    if private_marker_is_present(storage, &marker) {
        throw_private_type_error("Cannot initialize a private field twice on the same object");
    }
    let receiver = JSValue::from_bits(storage.to_bits());
    if !receiver.is_pointer() {
        throw_private_type_error("Cannot initialize a private field on a non-object");
    }
    let object = receiver.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
    if object.is_null() || !crate::value::addr_class::is_plausible_heap_addr(object as usize) {
        throw_private_type_error("Cannot initialize a private field on a non-object");
    }
    let object = scope.root_raw_mut_ptr(object);
    let storage_key =
        crate::string::js_string_from_bytes(storage_name.as_ptr(), storage_name.len() as u32);
    let storage_key = scope.root_string_ptr(storage_key);
    let marker_key = crate::string::js_string_from_bytes(marker.as_ptr(), marker.len() as u32);
    let marker_key = scope.root_string_ptr(marker_key);
    object.with_mut_ptr::<ObjectHeader, _>(|object| {
        storage_key.with_const_ptr::<crate::StringHeader, _>(|storage_key| {
            js_object_set_field_by_name(object, storage_key, value.get_nanbox_f64());
        });
    });
    object.with_mut_ptr::<ObjectHeader, _>(|object| {
        marker_key.with_const_ptr::<crate::StringHeader, _>(|marker_key| {
            js_object_set_field_by_name(object, marker_key, f64::from_bits(crate::value::TAG_TRUE));
        });
    });
    value.get_nanbox_f64()
}

/// Define one public class field with DefineField/CreateDataProperty
/// semantics. In particular, inherited setters are bypassed and Proxy
/// receivers observe `defineProperty`, not `set`.
#[no_mangle]
pub extern "C" fn js_class_field_add(receiver: f64, key: f64, value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let key = scope.root_nanbox_f64(key);
    let value = scope.root_nanbox_f64(value);
    // Use [[DefineOwnProperty]] for both ordinary objects and Proxies.  The
    // ordinary define path bypasses an inherited setter; the Proxy path invokes
    // the receiver's `defineProperty` trap.  A normal `[[Set]]` helper cannot
    // provide both sides of DefineField semantics.
    if !crate::proxy::create_data_property(
        receiver.get_nanbox_f64(),
        key.get_nanbox_f64(),
        value.get_nanbox_f64(),
    ) {
        throw_private_type_error("Cannot define class field on receiver");
    }
    value.get_nanbox_f64()
}

/// Brand + kind/op guard for a private member access `obj.#name`. Returns
/// `obj` unchanged when the access is legal; otherwise throws a `TypeError`.
///
/// The enclosing `PropertyGet` / `PropertySet` / method-call lowering operates
/// on the returned receiver, so this helper only enforces the two access
/// preconditions the spec attaches to a PrivateReference:
///   1. The receiver must carry the private brand (be an instance of the
///      declaring class). A plain object, or an instance of an unrelated /
///      enclosing class, throws.
///   2. The operation must match the member kind — reading a setter-only
///      accessor, or writing a getter-only accessor or a private method,
///      throws.
///
/// `kind`: 0=field, 1=method, 2=getter-only, 3=setter-only, 4=getter+setter.
/// `op`:   0=read, 1=write (instance); 2=read, 3=write (static).
///
/// For a STATIC private member the brand is identity-based: the receiver must
/// BE the declaring class constructor itself (static private elements are not
/// inherited, so a subclass constructor does not carry them). For an INSTANCE
/// member the receiver must be an instance of the declaring class (or a
/// subclass).
///
/// `declaring_class_id == 0` means codegen could not resolve the declaring
/// class (e.g. an unusual class-expression shape); the guard then degrades to
/// a no-op so it can never reject a legal access.
#[no_mangle]
pub extern "C" fn js_private_guard(
    obj: f64,
    brand_owner: f64,
    declaring_class_id: u32,
    _field_name_ptr: *const u8,
    _field_name_len: u32,
    kind: u32,
    op: u32,
) -> f64 {
    if declaring_class_id == 0 {
        return obj;
    }
    let is_static = op >= 2;
    let read_write = op & 1; // 0=read, 1=write
    if is_static && crate::proxy::js_proxy_is_proxy(obj) != 0 {
        throw_private_type_error(
            "Cannot access private member from an object whose class did not declare it",
        );
    }
    let has_brand = private_evaluation_brand_matches(obj, brand_owner, declaring_class_id)
        .unwrap_or_else(|| {
            if is_static {
                // Static private brand: the receiver must be exactly the
                // declaring class constructor (identity), not an instance or
                // a subclass.
                super::super::class_ref_id(obj) == Some(declaring_class_id)
            } else {
                private_instance_element_is_present(
                    crate::proxy::private_element_receiver(obj),
                    declaring_class_id,
                    _field_name_ptr,
                    _field_name_len,
                    kind,
                )
            }
        });
    if !has_brand {
        throw_private_type_error(
            "Cannot access private member from an object whose class did not declare it",
        );
    }
    if !is_static {
        let storage = crate::proxy::private_element_receiver(obj);
        if !private_instance_element_is_present(
            storage,
            declaring_class_id,
            _field_name_ptr,
            _field_name_len,
            kind,
        ) {
            throw_private_type_error("Cannot access private member before it has been initialized");
        }
    }
    let op = read_write;
    // Kind/op legality, after the brand check (spec order).
    let illegal = matches!(
        (op, kind),
        (0, 3) /* read setter-only: [[Get]] of accessor without getter */
            | (1, 2) /* write getter-only: [[Set]] of accessor without setter */
            | (1, 1) /* write private method */
    );
    if illegal {
        throw_private_type_error("Invalid private member operation for its kind");
    }
    if kind != 0 {
        let field_name = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(
                _field_name_ptr,
                _field_name_len as usize,
            ))
            .unwrap_or("")
            .to_string()
        };
        PRIVATE_MEMBER_ACCESS_HINTS.with(|hints| {
            hints.borrow_mut().push(PrivateMemberAccessHint {
                class_id: declaring_class_id,
                name: field_name.clone(),
                kind,
                is_static,
                is_write: read_write != 0,
            });
        });
    }
    if kind == 1 && read_write == 0 {
        let field_name = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(
                _field_name_ptr,
                _field_name_len as usize,
            ))
            .unwrap_or("")
            .to_string()
        };
        PRIVATE_METHOD_OWNER_HINT.with(|hint| {
            *hint.borrow_mut() = Some((declaring_class_id, field_name));
        });
    }
    if is_static {
        obj
    } else {
        crate::proxy::private_element_receiver(obj)
    }
}

#[cfg(test)]
mod private_evaluation_brand_tests {
    use super::*;

    #[test]
    fn stamping_a_fresh_brand_preserves_instance_shape_and_slots() {
        unsafe {
            const CID: u32 = 62_441;
            let class = crate::object::js_object_alloc(CID, 0);
            crate::object::class_registry::js_object_mark_class(class as i64);
            let class_value = crate::value::js_nanbox_pointer(class as i64);
            assert!(crate::object::class_registry::is_class_object_value(
                class_value
            ));

            let instance = crate::object::js_object_alloc(CID, 2);
            let shape_before = crate::object::shapes::object_shape_id(instance);
            let keys_before = crate::object::object_keys_array(instance);
            let slots_before = crate::object::object_live_slot_count(instance);

            stamp_private_evaluation_brand(instance, class_value);

            assert_eq!(
                crate::object::shapes::object_shape_id(instance),
                shape_before
            );
            assert_eq!(crate::object::object_keys_array(instance), keys_before);
            assert_eq!(
                crate::object::object_live_slot_count(instance),
                slots_before
            );
            assert_eq!(
                private_evaluation_brand(crate::value::js_nanbox_pointer(instance as i64), CID),
                Some(class_value.to_bits())
            );
        }
    }
}

#[cfg(test)]
mod poly_pic_tests {
    use super::{pic_prime_get, PicCache, PIC_CACHE_WORDS, PIC_WAYS, PIC_WAY_BASE, PIC_WAY_STATE};
    use crate::object::shapes::PIC_ID_TOKEN_BIT;

    fn id_tok(n: u64) -> i64 {
        (n | PIC_ID_TOKEN_BIT) as i64
    }

    /// Paired with `pic_cache_layout_matches_runtime` in
    /// `perry-codegen/src/expr/property_get/generic_dispatch.rs`: codegen emits
    /// `[PIC_CACHE_WORDS x i64]` for each `@perry_ic_N` and the runtime writes
    /// that memory as `[i64; PIC_CACHE_WORDS]`. Widening one side alone is an
    /// out-of-bounds store into another global, so both tests pin the number.
    #[test]
    fn pic_cache_words_match_codegen() {
        assert_eq!(
            PIC_CACHE_WORDS, 12,
            "codegen emits `[12 x i64]`; update both sides together"
        );
        assert!(
            PIC_WAY_STATE < PIC_CACHE_WORDS,
            "the way-state word must fit inside the emitted global"
        );
        assert_eq!(
            PIC_WAY_STATE, 3,
            "the gate word must share the MRU entry's cache line"
        );
        assert_eq!(
            PIC_WAY_BASE + PIC_WAYS * 2,
            PIC_CACHE_WORDS,
            "the ways must fill the global exactly"
        );
    }

    /// The MRU entry is always overwritten. A monomorphic site never fills a
    /// polymorphic way, and its reserved scratch word stays non-identifying.
    #[test]
    fn monomorphic_site_never_fills_a_way() {
        let mut c: PicCache = [0; PIC_CACHE_WORDS];
        unsafe {
            for _ in 0..8 {
                pic_prime_get(&mut c, id_tok(7), 2);
            }
        }
        assert_eq!(c[0], id_tok(7));
        assert_eq!(c[1], 2);
        assert_eq!(c[2], 0);
        for w in 0..PIC_WAYS {
            assert_eq!(
                c[PIC_WAY_BASE + w * 2],
                0,
                "a site that only ever sees one shape must not fill a way"
            );
        }
    }

    /// The property the whole change rests on: a site alternating between
    /// `PIC_WAYS + 1` shapes ends up with EVERY shape resolvable inline —
    /// the one in the MRU entry plus the rest spread across the ways, each
    /// still paired with its own slot. Before #7753 the 2nd..nth shape had
    /// nowhere to live and every read called the miss handler.
    #[test]
    fn alternating_shapes_all_become_inline_resolvable() {
        let mut c: PicCache = [0; PIC_CACHE_WORDS];
        let shapes: Vec<(i64, i64)> = (0..(PIC_WAYS + 1))
            .map(|i| (id_tok(100 + i as u64), i as i64))
            .collect();
        unsafe {
            // Two full rotations: the first fills, the second must not disturb.
            for _ in 0..2 {
                for (tok, slot) in &shapes {
                    pic_prime_get(&mut c, *tok, *slot);
                }
            }
        }
        for (tok, slot) in &shapes {
            let in_mru = c[0] == *tok && c[1] == *slot;
            let in_way = (0..PIC_WAYS)
                .any(|w| c[PIC_WAY_BASE + w * 2] == *tok && c[PIC_WAY_BASE + w * 2 + 1] == *slot);
            assert!(
                in_mru || in_way,
                "shape {tok:#x} (slot {slot}) must be resolvable inline; cache = {c:?}"
            );
        }
        assert!(
            c[PIC_WAY_STATE] > 0,
            "the emitted gate reads PIC_WAY_STATE > 0; a populated way set must arm it"
        );
        // …and no shape is duplicated across two ways (the dedupe arm works),
        // otherwise capacity silently halves.
        for w in 0..PIC_WAYS {
            for v in (w + 1)..PIC_WAYS {
                let a = c[PIC_WAY_BASE + w * 2];
                let b = c[PIC_WAY_BASE + v * 2];
                assert!(a == 0 || a != b, "ways {w} and {v} hold the same token");
            }
        }
    }

    /// The asymmetry that makes the ways a real trade rather than a free win: a
    /// rotation of `PIC_WAYS + 1` shapes is a 2.5x SPEEDUP, and one shape more
    /// is a 37% REGRESSION — four dependent loads per read that can never hit.
    ///
    /// So a site that keeps evicting a way by capacity latches the ways off,
    /// leaving no readable way behind (the emitted gate is the only thing
    /// standing between a megamorphic site and that 37%) — and then COUNTS
    /// DOWN, because "megamorphic" is a property of a program phase, not of a
    /// site. Both halves are asserted: it latches, it stays latched across the
    /// misses that follow, and it comes back on its own.
    #[test]
    fn a_wider_than_capacity_rotation_latches_then_re_arms() {
        let mut c: PicCache = [0; PIC_CACHE_WORDS];
        let shapes: Vec<i64> = (0..(PIC_WAYS as i64 + 3))
            .map(|i| 0x5000_0000_0000 + i * 8)
            .collect();
        unsafe {
            for _ in 0..40 {
                for (slot, tok) in shapes.iter().enumerate() {
                    pic_prime_get(&mut c, *tok, slot as i64);
                }
            }
        }
        assert!(
            c[PIC_WAY_STATE] < 0,
            "a rotation wider than the ways must latch megamorphic: {c:?}"
        );
        for w in 0..PIC_WAYS {
            assert_eq!(
                c[PIC_WAY_BASE + w * 2],
                0,
                "a latched site must leave no readable way: {c:?}"
            );
        }
        // Still latched a few misses later, and still holding no way.
        let latched = c[PIC_WAY_STATE];
        unsafe {
            for _ in 0..8 {
                pic_prime_get(&mut c, shapes[0], 0);
            }
        }
        assert!(c[PIC_WAY_STATE] < 0, "the latch must not clear immediately");
        assert!(
            c[PIC_WAY_STATE] > latched,
            "each miss while latched must count down toward a retry"
        );
        for w in 0..PIC_WAYS {
            assert_eq!(c[PIC_WAY_BASE + w * 2], 0, "latched site re-armed a way");
        }
        // …and the MRU entry keeps working exactly as it always did.
        assert_eq!(c[0], shapes[0]);
        assert_eq!(c[1], 0);

        // Bounded recovery: enough misses and the site gets another chance, so
        // a phase change cannot kill it for the rest of the process.
        unsafe {
            while c[PIC_WAY_STATE] < 0 {
                pic_prime_get(&mut c, shapes[0], 0);
            }
            // Two shapes is well inside capacity: the ways must fill again.
            pic_prime_get(&mut c, shapes[1], 1);
            pic_prime_get(&mut c, shapes[0], 0);
        }
        assert!(
            c[PIC_WAY_STATE] > 0,
            "a latched site must re-arm after its countdown: {c:?}"
        );
        assert!(
            (0..PIC_WAYS).any(|w| c[PIC_WAY_BASE + w * 2] != 0),
            "a re-armed site must be able to fill a way again: {c:?}"
        );
    }

    /// A rotation exactly AT capacity must not latch — otherwise the threshold
    /// is set so tight it turns off the very case the ways exist for.
    #[test]
    fn a_rotation_at_capacity_never_latches() {
        let mut c: PicCache = [0; PIC_CACHE_WORDS];
        unsafe {
            for _ in 0..200 {
                for i in 0..(PIC_WAYS as i64 + 1) {
                    pic_prime_get(&mut c, 0x6000_0000_0000 + i * 8, i);
                }
            }
        }
        assert!(
            c[PIC_WAY_STATE] > 0,
            "a {}-shape rotation fits the ways and must stay armed: {c:?}",
            PIC_WAYS + 1
        );
    }

    /// The bug the *consecutive* eviction run exists to prevent, and the one a
    /// cumulative counter shipped: a site that fits the ways but sees a rare
    /// extra shape must never latch.
    ///
    /// This is not hypothetical. The interpreter's `evalNode` dispatches on five
    /// hot node kinds plus `let`/`fun` twice per round — 80 stray evictions
    /// across a run. Counted cumulatively that trips any sane threshold, so the
    /// ways switched themselves off on the exact site they were built for and
    /// handed back the whole win: 2.39 s → 3.03 s, measured end to end.
    #[test]
    fn a_rare_extra_shape_does_not_latch_a_site_that_fits() {
        let mut c: PicCache = [0; PIC_CACHE_WORDS];
        let hot: Vec<i64> = (0..(PIC_WAYS as i64 + 1))
            .map(|i| 0x7000_0000_0000 + i * 8)
            .collect();
        unsafe {
            for round in 0..400 {
                for (slot, tok) in hot.iter().enumerate() {
                    pic_prime_get(&mut c, *tok, slot as i64);
                }
                // One interloper every round — far more than the 80 the
                // interpreter produced, and 10x the raw threshold.
                pic_prime_get(&mut c, 0x7000_FFFF_0000 + round, 0);
            }
        }
        assert!(
            c[PIC_WAY_STATE] > 0,
            "a fitting site with a rare extra shape must stay armed: {c:?}"
        );
    }

    /// ShapeId tokens cascade into the polymorphic ways without losing their
    /// paired slot.
    #[test]
    fn shape_id_tokens_do_reach_a_way() {
        let mut c: PicCache = [0; PIC_CACHE_WORDS];
        let shape_a = id_tok(41);
        let shape_b = id_tok(42);
        unsafe {
            pic_prime_get(&mut c, shape_a, 1);
            pic_prime_get(&mut c, shape_b, 2);
        }
        assert_eq!(c[0], shape_b);
        assert!(
            (0..PIC_WAYS)
                .any(|w| c[PIC_WAY_BASE + w * 2] == shape_a && c[PIC_WAY_BASE + w * 2 + 1] == 1),
            "the evicted ShapeId token must land in a way: {c:?}"
        );
    }

    /// More distinct shapes than the site can hold must degrade to "some miss",
    /// never to a wrong answer: every occupied way still carries the slot it was
    /// primed with, so the emitted compare can only hit on a token it stored.
    #[test]
    fn overflow_rotates_without_corrupting_pairs() {
        let mut c: PicCache = [0; PIC_CACHE_WORDS];
        unsafe {
            for i in 0..(PIC_WAYS as u64 * 4) {
                pic_prime_get(&mut c, id_tok(200 + i), i as i64);
            }
        }
        for w in 0..PIC_WAYS {
            let tok = c[PIC_WAY_BASE + w * 2];
            if tok == 0 {
                continue;
            }
            let slot = c[PIC_WAY_BASE + w * 2 + 1];
            let expected = (tok as u64 & !PIC_ID_TOKEN_BIT) - 200;
            assert_eq!(
                slot, expected as i64,
                "way {w} pairs token {tok:#x} with the wrong slot"
            );
        }
    }
}

#[cfg(test)]
mod c3c_pic_tests {
    /// Installing an accessor on one object must not permanently disable every
    /// property-read PIC in the process. Descriptor ownership is recorded on
    /// the owning object's GC header, so a different descriptor-free object's
    /// own data field remains safe to cache.
    #[test]
    fn unrelated_accessor_does_not_poison_plain_receiver_pic() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let unrelated = crate::object::js_object_alloc(0, 1);
        let unrelated = scope.root_raw_mut_ptr(unrelated);
        crate::object::set_accessor_descriptor(
            unrelated.with_mut_ptr(|o: *mut crate::object::ObjectHeader| o as usize),
            "pic_unrelated_accessor".to_string(),
            crate::object::AccessorDescriptor::default(),
        );
        assert!(
            crate::state::state().descriptors.accessors_in_use.get(),
            "test premise: the process-wide accessor latch is active"
        );

        let obj = crate::object::js_object_alloc(0, 8);
        let obj = scope.root_raw_mut_ptr(obj);
        let key_bytes = b"pic_plain_data";
        let key = crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
        let key = scope.root_string_ptr(key);
        obj.with_mut_ptr(|o| {
            key.with_const_ptr(|k| crate::object::js_object_set_field_by_name(o, k, 42.0))
        });

        let mut cache = [0i64; super::PIC_CACHE_WORDS];
        assert_eq!(
            obj.with_mut_ptr(
                |o| key.with_const_ptr(|k| super::js_object_get_field_ic_miss(o, k, &mut cache))
            ),
            42.0
        );
        assert_ne!(
            cache[0], 0,
            "an accessor owned by an unrelated object must not prevent this \
             descriptor-free receiver from priming its read PIC"
        );
        assert_eq!(cache[1], 0, "the first own data field lives in slot 0");
    }

    /// The receiver-local half of the proof: an accessor-bearing object must
    /// keep taking descriptor-aware lookup and must never seed a raw-slot hit.
    #[test]
    fn accessor_bearing_receiver_does_not_prime_plain_data_pic() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj = crate::object::js_object_alloc(0, 8);
        let obj = scope.root_raw_mut_ptr(obj);
        let key_bytes = b"pic_guarded_data";
        let key = crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
        let key = scope.root_string_ptr(key);
        obj.with_mut_ptr(|o| {
            key.with_const_ptr(|k| crate::object::js_object_set_field_by_name(o, k, 17.0))
        });
        crate::object::set_accessor_descriptor(
            obj.with_mut_ptr(|o: *mut crate::object::ObjectHeader| o as usize),
            "pic_guarded_data".to_string(),
            crate::object::AccessorDescriptor::default(),
        );

        let mut cache = [0i64; super::PIC_CACHE_WORDS];
        let via_pic = obj.with_mut_ptr(|o| {
            key.with_const_ptr(|k| super::js_object_get_field_ic_miss(o, k, &mut cache))
        });
        let via_ladder = obj
            .with_mut_ptr(|o| key.with_const_ptr(|k| super::js_object_get_field_by_name_f64(o, k)));
        assert_eq!(
            via_pic.to_bits(),
            via_ladder.to_bits(),
            "the miss path must preserve this receiver's accessor semantics"
        );
        assert_eq!(
            cache[0], 0,
            "an accessor-bearing receiver must not prime a raw-slot PIC"
        );
    }

    /// A class instance primes the same authoritative ShapeId token that the
    /// emitted guard reads from the receiver.
    #[test]
    fn a_class_instance_primes_an_id_token_after_rung1() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let obj = crate::object::js_object_alloc(0x6080, 8);
            let key = crate::string::js_string_from_bytes(b"pic6080_x".as_ptr(), 9);
            crate::object::js_object_set_field_by_name(obj, key, 7.0);
            let keys = crate::object::object_keys_array(obj);
            assert!(!keys.is_null(), "test premise: field append built keys");
            assert_eq!((*obj).class_id, 0x6080, "test premise: a class instance");

            let mut cache = [0i64; super::PIC_CACHE_WORDS];
            let v = super::js_object_get_field_ic_miss(obj, key, &mut cache);
            assert_eq!(v, 7.0);

            let stamp = crate::object::shapes::object_shape_stamp(obj);
            assert!(
                stamp != 0,
                "the miss handler did not stamp a class instance — rung 1 is inert"
            );
            assert_eq!(
                cache[0] as u64,
                stamp as u64 | crate::object::shapes::PIC_ID_TOKEN_BIT,
                "a stamped class instance must prime the ID token the emitted \
                 PIC computes for it, not its keys pointer"
            );
            assert_ne!(
                cache[0], keys as i64,
                "primed the keys pointer for a stamped receiver — every hit at \
                 this site would miss forever"
            );
            assert_eq!(cache[2], 0, "word 2 is non-identity scratch");
        }
    }

    /// ★ #6759 C3 rung 1 opens a NEW correctness surface, and this is it.
    ///
    /// A delete-compacted class instance receives a semantic successor ShapeId,
    /// so the emitted hit path can serve it without confusing it with a
    /// pristine sibling. A token that failed to move across the
    /// compaction would therefore be read as a pristine sibling's shape at a
    /// site that has both — the one-slot shift the whole ladder is about.
    ///
    /// Pins: the compacted instance's primed token differs from a pristine
    /// sibling's, AND the slot it primes is the post-compaction slot.
    #[test]
    fn a_compacted_class_instance_primes_a_token_a_pristine_sibling_cannot_match() {
        let _lock = crate::gc::global_side_table_test_lock();
        {
            let packed = b"picdel_a\0picdel_b\0picdel_c";
            let mk = || {
                crate::object::js_object_alloc_class_with_keys(
                    0x6081,
                    0,
                    3,
                    packed.as_ptr(),
                    packed.len() as u32,
                )
            };
            let key = |n: &str| crate::string::js_string_from_bytes(n.as_ptr(), n.len() as u32);
            let pristine = mk();
            let compacted = mk();
            for (i, v) in [1.0f64, 2.0, 3.0].iter().enumerate() {
                crate::object::js_object_set_field(
                    pristine,
                    i as u32,
                    crate::JSValue::from_bits(v.to_bits()),
                );
                crate::object::js_object_set_field(
                    compacted,
                    i as u32,
                    crate::JSValue::from_bits(v.to_bits()),
                );
            }
            assert_eq!(
                crate::object::js_object_delete_field(compacted, key("picdel_a")),
                1
            );

            let mut c_pristine = [0i64; super::PIC_CACHE_WORDS];
            let vp = super::js_object_get_field_ic_miss(pristine, key("picdel_c"), &mut c_pristine);
            assert_eq!(vp, 3.0, "pristine `c` is slot 2");

            let mut c_compacted = [0i64; super::PIC_CACHE_WORDS];
            let vc =
                super::js_object_get_field_ic_miss(compacted, key("picdel_c"), &mut c_compacted);
            assert_eq!(
                vc, 3.0,
                "compacted `c` shifted to slot 1 and must still read 3"
            );

            assert_ne!(
                c_compacted[0], 0,
                "the compacted instance primed nothing — rung 1's new surface is inert"
            );
            assert_ne!(
                c_compacted[0], c_pristine[0],
                "the compacted instance primed its pristine sibling's token — an \
                 id-comparing PIC would read slot {} for a receiver whose `c` is \
                 at slot {}",
                c_pristine[1], c_compacted[1]
            );
            assert_eq!(c_pristine[1], 2, "pristine `c` slot");
            assert_eq!(c_compacted[1], 1, "compacted `c` slot");
        }
    }

    /// The PIC cache token the EMITTED code computes for `obj`, transcribed
    /// from `perry-codegen/src/expr/property_get/generic_dispatch.rs`:
    ///
    /// ```text
    /// token = valid_shape_id ? (shape_id | 1<<62) : 0
    /// ```
    ///
    /// The runtime never calls this; it exists so a test can compare what the
    /// miss handler PRIMES against what the hit path will COMPUTE, which is
    /// the only pair whose agreement decides whether a site can ever hit.
    unsafe fn emitted_pic_token(obj: *const super::ObjectHeader) -> u64 {
        let shape_id = crate::object::shapes::object_shape_id(obj);
        shape_id as u64 | crate::object::shapes::PIC_ID_TOKEN_BIT
    }

    /// ★ The invariant #6759 C3 rung 1 broke, asserted where it broke.
    ///
    /// A shape's population must be UNIFORMLY stamped: the token the miss
    /// handler primes from one instance is only useful if a DIFFERENT,
    /// freshly-allocated instance of the same class computes the same token.
    /// A prior implementation stamped class instances lazily, so instance #1
    /// primed an id token while every newborn sibling computed a different
    /// identity. `token_eq` then failed at every field-read site until the
    /// sibling took the miss path itself.
    ///
    /// This is deliberately NOT "the newborn carries a stamp" — that is a
    /// presence check two different states satisfy (both-stamped and
    /// both-unstamped are each fine; the mixture is the bug). Comparing the
    /// primed token against a fresh sibling's COMPUTED token is what fails
    /// under either half of the split.
    #[test]
    fn a_fresh_class_instance_computes_the_token_the_miss_handler_primed() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let packed = b"picbirth_x\0picbirth_y";
            let mk = || {
                crate::object::js_object_alloc_class_with_keys(
                    0x6082,
                    0,
                    2,
                    packed.as_ptr(),
                    packed.len() as u32,
                )
            };
            let key = crate::string::js_string_from_bytes(b"picbirth_x".as_ptr(), 10);

            let primed_from = mk();
            crate::object::js_object_set_field(
                primed_from,
                0,
                crate::JSValue::from_bits(5.0f64.to_bits()),
            );
            assert_eq!(
                (*primed_from).class_id,
                0x6082,
                "test premise: the receiver is a class instance, not a literal"
            );

            let mut cache = [0i64; super::PIC_CACHE_WORDS];
            assert_eq!(
                super::js_object_get_field_ic_miss(primed_from, key, &mut cache),
                5.0,
                "test premise: the miss handler resolved the field"
            );
            assert_ne!(
                cache[0], 0,
                "test premise: the miss handler primed SOMETHING — a zero token \
                 never hits, so the comparison below would be vacuous"
            );

            // The next `new C(...)`. Nothing has resolved a field on it.
            let fresh = mk();
            assert_eq!(
                emitted_pic_token(fresh),
                cache[0] as u64,
                "a freshly allocated instance of the SAME class computes a \
                 different PIC token than the one primed from its sibling, so \
                 every read of a newborn instance's field misses the cache and \
                 takes the full miss handler — #7983's split population"
            );

            // And the same must hold once the fresh one has itself resolved:
            // priming from either instance is interchangeable.
            let mut cache2 = [0i64; super::PIC_CACHE_WORDS];
            super::js_object_get_field_ic_miss(fresh, key, &mut cache2);
            assert_eq!(
                cache2[0], cache[0],
                "two instances of one class primed two different tokens — the \
                 site thrashes between them"
            );
        }
    }
}

#[cfg(test)]
mod array_length_fast_path_tests {
    /// #7753: the `arr.length` short-circuit must answer EXACTLY what the full
    /// ladder answers, for a fresh array, a grown one, and an empty one — and
    /// must not fire for any other key on an array receiver, nor for `length`
    /// on a non-array. Comparing against `js_object_get_field_by_name_f64` (the
    /// path the read took before the short-circuit) is what makes this a
    /// behaviour-equivalence test rather than a restatement of the fast path.
    #[test]
    fn array_length_short_circuit_agrees_with_the_full_ladder() {
        let _lock = crate::gc::global_side_table_test_lock();
        {
            let len_key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
            let other_key = crate::string::js_string_from_bytes(b"lengtx".as_ptr(), 6);
            for n in [0u32, 1, 5, 40] {
                let mut arr = crate::array::js_array_alloc(n.max(1));
                for i in 0..n {
                    arr = crate::array::js_array_push(arr, crate::value::JSValue::number(i as f64));
                }
                let obj = arr as *const super::ObjectHeader;
                let mut cache = [0i64; super::PIC_CACHE_WORDS];
                let via_ic = super::js_object_get_field_ic_miss(obj, len_key, &mut cache);
                let via_ladder = super::js_object_get_field_by_name_f64(obj, len_key);
                assert_eq!(
                    via_ic.to_bits(),
                    via_ladder.to_bits(),
                    "length disagreed for a {n}-element array"
                );
                assert_eq!(via_ic, n as f64, "length wrong for a {n}-element array");
                // A same-length key that is not `length` must not be captured
                // by the fast path.
                assert_eq!(
                    super::js_object_get_field_ic_miss(obj, other_key, &mut cache).to_bits(),
                    super::js_object_get_field_by_name_f64(obj, other_key).to_bits(),
                    "a non-`length` key on an array must take the normal path"
                );
            }
            // `length` on a plain OBJECT must not be answered by the array
            // short-circuit — it is an ordinary (absent) property there.
            let plain = crate::object::js_object_alloc(0, 0);
            let mut cache = [0i64; super::PIC_CACHE_WORDS];
            assert_eq!(
                super::js_object_get_field_ic_miss(plain, len_key, &mut cache).to_bits(),
                super::js_object_get_field_by_name_f64(plain, len_key).to_bits(),
                "`length` on a plain object must keep its normal answer"
            );
        }
    }
}
