//! Tag-aware dynamic index get/set + helpers for ambiguous index access.

use super::*;

fn finite_nonnegative_i32_index(index: f64) -> Option<i32> {
    let bits = index.to_bits();
    if (bits & TAG_MASK) == INT32_TAG {
        let index = JSValue::from_bits(bits).as_int32();
        return (index >= 0).then_some(index);
    }
    if index.is_finite() && index >= 0.0 && index.fract() == 0.0 && index <= i32::MAX as f64 {
        Some(index as i32)
    } else {
        None
    }
}

fn finite_nonnegative_u32_index(index: f64) -> Option<u32> {
    let bits = index.to_bits();
    if (bits & TAG_MASK) == INT32_TAG {
        let index = JSValue::from_bits(bits).as_int32();
        return (index >= 0).then_some(index as u32);
    }
    if index.is_finite() && index >= 0.0 && index.fract() == 0.0 && index < u32::MAX as f64 {
        Some(index as u32)
    } else {
        None
    }
}

/// Tag-aware dynamic index dispatch for `obj[key]` where `obj` has unknown
/// static type. Issue #514. Strings → js_string_char_at; objects stringify
/// numeric keys (`obj[0]` is `obj["0"]`), while arrays/buffers keep numeric
/// element reads. LAZY_ARRAY / FORWARDED arrays route through
/// `js_array_get_f64` to chase the materialized chain.
#[no_mangle]
pub extern "C" fn js_dyn_index_get(value: f64, index: f64) -> f64 {
    let bits = value.to_bits();
    // RequireObjectCoercible(base): `null[i]` / `undefined[i]` throw a
    // TypeError rather than returning undefined (test262
    // compound-assignment / prefix-increment null-base cases). Mirrors the
    // codegen-side guard on the by-name fallback in index_get.rs.
    if bits == TAG_UNDEFINED || bits == TAG_NULL {
        crate::object::has_own_helpers::throw_to_object_nullish_type_error();
    }
    let jsval = JSValue::from_bits(bits);
    // #5525: a Symbol *index* (`obj[Symbol.iterator]`) must resolve through the
    // symbol side-table, never the integer-index / stringify paths below (which
    // would coerce the symbol's NaN-boxed bits to a garbage i32). The codegen
    // routes all non-string-literal, unknown-receiver reads here, so the runtime
    // owns the symbol triage that the codegen-side fallback used to do inline.
    if unsafe { crate::symbol::js_is_symbol(index) } != 0 {
        return unsafe { crate::symbol::js_object_get_symbol_property(value, index) };
    }
    // #5525 hot fast path: `obj[i]` where `obj` is dynamically an owning numeric
    // typed array and `i` a canonical index. bcryptjs's Blowfish core reaches
    // its `Int32Array` P/S boxes through untyped `Array.<number>` params, so
    // every one of its ~600M element reads lands here. Collapsing the deep
    // dynamic-dispatch chain into a cached kind lookup + inline `load_at` is the
    // bulk of the #5525 speedup; non-typed-array and exotic-key cases fall
    // through to the full dispatch below unchanged.
    if jsval.is_pointer() {
        let raw_ptr = (bits & POINTER_MASK) as usize;
        if let Some(kind) = crate::typedarray::lookup_typed_array_kind(raw_ptr) {
            if let Some(v) = crate::typedarray::typed_array_fast_index_get(raw_ptr, kind, index) {
                return v;
            }
        }
    }
    if jsval.is_string() || jsval.is_short_string() {
        // Spec: string INDEXING `s[i]` returns `undefined` for a non-canonical
        // or out-of-bounds index — unlike `s.charAt(i)`, which returns "".
        // Route through the canonical-index helper (`js_string_index_get`,
        // #3987) so an OOB read here is `undefined`. Calling `js_string_char_at`
        // directly (charAt semantics) returned "" for OOB, which every
        // generator/async LOCAL string read hit: the CPS box pass erases the
        // local's static type, so `line[i]` reaches this dyn path instead of the
        // `is_string_expr` static path — the `yaml` lexer's `parseDocument`
        // `switch (line[n])` then never observed `undefined` at line-ends and
        // its `*lex` state machine spun forever (#6067).
        let s_ptr = js_get_string_pointer_unified(value) as *const crate::StringHeader;
        return crate::string::js_string_index_get(s_ptr, index);
    }
    // Class-ref value (INT32-tagged, top16 == 0x7FFE): `C[key]` where `C` is a
    // runtime class-ref value (e.g. a function parameter). Member-expression
    // access (`C.key`) already routes through `js_object_get_field_by_name_f64`,
    // which detects the class-ref tag and consults the static method / field /
    // CLASS_DYNAMIC_PROPS tables; the computed form must do the same instead of
    // falling through to the not-a-pointer `undefined` path below. (test262
    // class/elements propertyHelper `isWritable(C, "m")` does `C[name] = v`.)
    if (bits >> 48) == 0x7FFE {
        let idx_top16 = index.to_bits() >> 48;
        let key_ptr = if idx_top16 == 0x7FFF || idx_top16 == 0x7FF9 {
            js_get_string_pointer_unified(index) as *const crate::StringHeader
        } else {
            // Numeric / other index → ToString for the class-ref lookup.
            let s = crate::builtins::js_string_coerce(index);
            s as *const crate::StringHeader
        };
        if key_ptr.is_null() {
            return f64::from_bits(TAG_UNDEFINED);
        }
        return crate::object::js_object_get_field_by_name_f64(
            bits as *const crate::object::ObjectHeader,
            key_ptr,
        );
    }
    // A non-NaN-boxed f64 reaching here is a plain `number` (its `[idx]` is
    // `undefined` per JS). The old code kept a "raw I64 pointer passed as
    // DOUBLE" heuristic — `bits < 2^48 && (bits & 3) == 0 && bits >= 0x10000` —
    // that treated such a number's bits as a heap pointer, a relic of the
    // now-removed module-var raw-I64 representation (module vars are uniform
    // NaN-boxed doubles today, so a real object always takes the `is_pointer()`
    // branch above). The heuristic only ever MISfired on numbers whose f64 bits
    // land in that band — e.g. a subnormal `~1.7e-314` (bits `0x8_0000_0000`).
    // On the macOS host the resulting address was below the heap range so
    // `is_valid_obj_ptr` rejected it and this returned `undefined`; on Linux
    // (`HEAP_MIN = 0x1000`, needed for Android/Scudo low allocations) the same
    // address is *in range*, so it was dereferenced as an `ObjectHeader` →
    // garbage/crash. Drop the heuristic: a non-pointer receiver is a number and
    // its indexed read is `undefined` on every platform (#63/#321 denormal-safe).
    let raw_ptr = if jsval.is_pointer() {
        (bits & POINTER_MASK) as usize
    } else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    if crate::value::addr_class::is_small_handle(raw_ptr) {
        // #5989: registry HANDLES (fetch/native ids) live below HANDLE_BAND_MAX
        // (0x100000). The old guard only excluded the first 64KB, so a handle
        // in [0x10000, 0x100000) indexed as `h[key]` fell through to the raw
        // ObjectHeader walk below and dereferenced the id as a pointer —
        // react-server-dom's flight wake path indexes a handle-valued object
        // and segfaulted at the handle address. Route through the by-name read,
        // which triages small handles (HANDLE_PROPERTY_DISPATCH, recorded
        // prototypes) without ever dereferencing the id.
        let idx_top16 = index.to_bits() >> 48;
        let key_ptr = if idx_top16 == 0x7FFF || idx_top16 == 0x7FF9 {
            js_get_string_pointer_unified(index) as *const crate::StringHeader
        } else {
            crate::builtins::js_string_coerce(index) as *const crate::StringHeader
        };
        if key_ptr.is_null() {
            return f64::from_bits(TAG_UNDEFINED);
        }
        return crate::object::js_object_get_field_by_name_f64(
            raw_ptr as *const crate::object::ObjectHeader,
            key_ptr,
        );
    }
    // TypedArrays carry element-typed storage, not boxed ArrayHeader slots.
    // Probe the registry before any GC-header or raw ArrayHeader fallback so
    // values whose static type was erased by callback methods still read via
    // the per-kind accessor (`Uint16Array#map(...)[0]`, `(ta as any)[0]`).
    if crate::typedarray::lookup_typed_array_kind(raw_ptr).is_some() {
        return crate::typedarray::js_typed_array_index_get_dynamic(
            raw_ptr as *const crate::typedarray::TypedArrayHeader,
            index,
        );
    }
    if crate::buffer::is_registered_buffer(raw_ptr) {
        let Some(idx_i32) = finite_nonnegative_i32_index(index) else {
            return f64::from_bits(TAG_UNDEFINED);
        };
        let buf = raw_ptr as *const crate::buffer::BufferHeader;
        let len = unsafe { (*buf).length };
        if (idx_i32 as u32) >= len {
            return f64::from_bits(TAG_UNDEFINED);
        }
        let byte_val = crate::buffer::js_buffer_get(buf, idx_i32);
        return byte_val as f64;
    }
    if crate::set::is_registered_set(raw_ptr) || crate::map::is_registered_map(raw_ptr) {
        let Some(index) = finite_nonnegative_u32_index(index) else {
            return f64::from_bits(TAG_UNDEFINED);
        };
        return crate::array::js_array_get_f64(raw_ptr as *const crate::array::ArrayHeader, index);
    }
    // Issue #63 / #321 (Effect.runSync→fork SIGBUS): the raw-I64 fallback
    // above accepts arbitrary in-range bits — including denormal f64
    // payloads from non-pointer dataflow (e.g. effect's fiberRefs.ts loop
    // produced `bits ≈ 0x8_0000_0000` which passed every gate but is just
    // a number value, not a real I64 pointer). The unchecked
    // `(*gc_hdr).obj_type` read at the bottom of this fn then crossed
    // the macOS user/kernel boundary at `[raw_ptr - 8]` → SIGBUS.
    //
    // The platform-aware heap range used by `crate::object::is_valid_obj_ptr`
    // covers exactly the address space mimalloc / system malloc actually
    // hand out (macOS host: `[0x200_0000_0000, 0x8000_0000_0000)`; Linux /
    // iOS / Android: `[0x1000, 0x8000_0000_0000)`). Any value with
    // POINTER_TAG that codegen put there is trusted (it asked for a
    // pointer), so this gate only applies to the heuristic fallback.
    if !jsval.is_pointer() && !crate::object::is_valid_obj_ptr(raw_ptr as *const u8) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // Issue #957: if the index itself is a string, route through the
    // by-name object getter. Pre-fix, `obj["foo"]` lowered through
    // `IndexUpdate` re-entered this helper with a NaN-boxed string index
    // and the `index as i32` coercion produced garbage offsets, so
    // `++obj["foo"]` silently returned undefined.
    let idx_bits = index.to_bits();
    let idx_top16 = idx_bits >> 48;
    if idx_top16 == 0x7FFF || idx_top16 == 0x7FF9 {
        let key_ptr = js_get_string_pointer_unified(index) as *const crate::StringHeader;
        if !key_ptr.is_null() {
            return crate::object::js_object_get_field_by_name_f64(
                raw_ptr as *const crate::object::ObjectHeader,
                key_ptr,
            );
        }
        return f64::from_bits(TAG_UNDEFINED);
    }
    let idx_i32 = if index.is_nan() || index.is_infinite() {
        return f64::from_bits(TAG_UNDEFINED);
    } else {
        index as i32
    };
    if idx_i32 >= 0 {
        if let Some(value) = unsafe {
            crate::object::arguments_object_get_index(
                raw_ptr as *const crate::object::ObjectHeader,
                idx_i32 as u32,
            )
        } {
            return value;
        }
    }
    if raw_ptr >= crate::gc::GC_HEADER_SIZE {
        let gc_hdr = unsafe {
            (raw_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader
        };
        let obj_type = unsafe { (*gc_hdr).obj_type };
        let gc_flags = unsafe { (*gc_hdr).gc_flags };
        if obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
            || (gc_flags & crate::gc::GC_FLAG_FORWARDED) != 0
        {
            if idx_i32 < 0 {
                return f64::from_bits(TAG_UNDEFINED);
            }
            let arr = raw_ptr as *const crate::array::ArrayHeader;
            return crate::array::js_array_get_f64(arr, idx_i32 as u32);
        }
        // Issue #1069: bounds-check regular arrays so out-of-range reads
        // return TAG_UNDEFINED instead of whatever's in the slot. Without
        // this, an empty (or short) array — most visibly the synthetic
        // `arguments` array bundled by the call-site for caller arity 0 —
        // returns the raw 0.0 slot value because `js_array_alloc` rounds
        // capacity up to MIN_ARRAY_CAPACITY and the unchecked load reads
        // past `length` into zeroed-but-allocated storage. `arguments[0]`
        // on `function f() { arguments[0] }; f()` printed `0` instead of
        // `undefined`. The narrow gate (GC_TYPE_ARRAY) keeps object
        // numeric-key fast path unchanged.
        if obj_type == crate::gc::GC_TYPE_ARRAY {
            if idx_i32 < 0 {
                return f64::from_bits(TAG_UNDEFINED);
            }
            let arr = raw_ptr as *const crate::array::ArrayHeader;
            // When any property descriptor is live, an array element read may
            // resolve to an index accessor descriptor — own (`Object.define-
            // Property(arr, "0", {get})`) or inherited from a polluted
            // `Array.prototype`/`Object.prototype` — rather than the raw slot.
            // Route through `js_array_get_f64`, which fires the getter and
            // applies the out-of-bounds prototype fallback. The raw-slot fast
            // path below is preserved for the common no-descriptor case so the
            // hot dynamic-index path is unchanged. (test262 Object/define-
            // Propert{y,ies} Array-index accessor reads.)
            if crate::object::descriptors_in_use() {
                return crate::array::js_array_get_f64(arr, idx_i32 as u32);
            }
            let length = unsafe { (*arr).length };
            if (idx_i32 as u32) >= length {
                return f64::from_bits(TAG_UNDEFINED);
            }
        }
        if obj_type == crate::gc::GC_TYPE_OBJECT || obj_type == crate::gc::GC_TYPE_CLOSURE {
            let s = if index == (idx_i32 as f64) {
                idx_i32.to_string()
            } else {
                format!("{}", index)
            };
            let key = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            let v = crate::object::js_object_get_field_by_name_f64(
                raw_ptr as *const crate::object::ObjectHeader,
                key,
            );
            // An indexed property inherited from the canonical
            // `Object.prototype` (incl. a defineProperty accessor) shows
            // through any object/function receiver — e.g. `Array[1]` after
            // `Object.defineProperty(Object.prototype, "1", { get })`
            // (test262 filter/15.4.4.20-9-b-6).
            if v.to_bits() == crate::value::TAG_UNDEFINED
                && idx_i32 >= 0
                && index == (idx_i32 as f64)
                && crate::array::object_prototype_has_index_prop(idx_i32 as u32)
            {
                return crate::array::sort_object_prototype_index_get(idx_i32 as u32);
            }
            return v;
        }
    }
    if idx_i32 < 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let elem_addr = raw_ptr.wrapping_add(8 + (idx_i32 as usize) * 8);
    let v = unsafe { *(elem_addr as *const f64) };
    if v.to_bits() == crate::value::TAG_HOLE {
        return f64::from_bits(TAG_UNDEFINED);
    }
    v
}

/// Issue #957 — tag-aware dynamic index write counterpart to
/// `js_dyn_index_get`. Used by `Expr::IndexUpdate` codegen to write back
/// the incremented value without duplicating the IndexSet dispatch tree.
///
/// Routes by the receiver's `gc_type` byte: arrays go through
/// `js_array_set_index_or_string` (numeric/string-key spec dispatch);
/// everything else stringifies the index and routes through
/// `js_object_set_field_by_name`. Strings are immutable — no-op (matches
/// strict-mode `s[i] = x` semantics, close enough for the `++result[key]`
/// pattern this is added for).
#[no_mangle]
pub extern "C" fn js_dyn_index_set(obj: f64, index: f64, value: f64) -> f64 {
    let bits = obj.to_bits();
    let jsval = JSValue::from_bits(bits);
    // #5525: a Symbol *index* (`obj[sym] = v`) routes to the symbol side-table,
    // mirroring the get side. Codegen sends all non-string-literal unknown-
    // receiver writes here, so the runtime owns the symbol triage.
    if unsafe { crate::symbol::js_is_symbol(index) } != 0 {
        unsafe {
            crate::symbol::js_object_set_symbol_property(obj, index, value);
        }
        return value;
    }
    // #5525 hot fast path mirroring `js_dyn_index_get` — an owning numeric
    // typed array with a canonical index stores inline, skipping the dynamic
    // setter chain. Placed before the `note_object_prototype_index_write`
    // bookkeeping: that flag only governs plain-array hole/OOB reads, and a
    // typed array is never a plain array, so the fast-path store does not need
    // it (the slow path still flips it for the cases it owns).
    if jsval.is_pointer() {
        let raw_ptr = (bits & POINTER_MASK) as usize;
        if let Some(kind) = crate::typedarray::lookup_typed_array_kind(raw_ptr) {
            if crate::typedarray::typed_array_fast_index_set(raw_ptr, kind, index, value) {
                return value;
            }
        }
    }
    // `Object.prototype[i] = v` (computed write) makes the index visible
    // through every array's hole/OOB reads — flip the global flag.
    if jsval.is_pointer() {
        crate::array::note_object_prototype_index_write((bits & POINTER_MASK) as usize);
    }
    if jsval.is_string() || jsval.is_short_string() {
        return value;
    }
    // A `Temporal.*` value is an opaque immutable cell — a dynamic property
    // write (`temporalValue[key] = v`) is a no-op, never an ObjectHeader write.
    #[cfg(feature = "temporal")]
    if crate::temporal::is_temporal_value(obj) {
        return value;
    }
    // Class-ref value (INT32-tagged, top16 == 0x7FFE): `C[key] = v` where `C` is
    // a runtime class-ref value (e.g. a function parameter). Route to the
    // by-name setter, which detects the class-ref tag and stores into the
    // static-field / CLASS_DYNAMIC_PROPS side table — matching the member-write
    // form (`C.key = v`). Without this the write was silently dropped, so
    // propertyHelper's `isWritable(C, name)` (`C[name] = v`) reported a static
    // method as non-writable. (Mirrors the get arm above.)
    if (bits >> 48) == 0x7FFE {
        let idx_top16 = index.to_bits() >> 48;
        let key_ptr = if idx_top16 == 0x7FFF || idx_top16 == 0x7FF9 {
            js_get_string_pointer_unified(index) as *const crate::StringHeader
        } else {
            crate::builtins::js_string_coerce(index) as *const crate::StringHeader
        };
        if !key_ptr.is_null() {
            crate::object::js_object_set_field_by_name(
                bits as *mut crate::object::ObjectHeader,
                key_ptr,
                value,
            );
        }
        return value;
    }
    let raw_ptr = if jsval.is_pointer() {
        (bits & POINTER_MASK) as usize
    } else if !obj.is_nan()
        && bits != 0
        && bits < 0x0001_0000_0000_0000
        && (bits & 0x3) == 0
        && bits >= 0x10000
    {
        bits as usize
    } else {
        return value;
    };
    if raw_ptr < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return value;
    }
    if crate::typedarray::lookup_typed_array_kind(raw_ptr).is_some() {
        crate::typedarray_props::js_typed_array_index_set_dynamic(
            raw_ptr as *mut crate::typedarray::TypedArrayHeader,
            index,
            value,
        );
        return value;
    }
    if crate::buffer::is_registered_buffer(raw_ptr) {
        if let Some(idx_i32) = finite_nonnegative_i32_index(index) {
            crate::buffer::js_buffer_set(
                raw_ptr as *mut crate::buffer::BufferHeader,
                idx_i32,
                value as i32,
            );
        }
        return value;
    }
    if crate::set::is_registered_set(raw_ptr) || crate::map::is_registered_map(raw_ptr) {
        return value;
    }
    // Mirror the #63/#321 guard on the get side: heuristic-derived
    // pseudo-pointers from non-pointer dataflow must not be dereferenced.
    if !jsval.is_pointer() && !crate::object::is_valid_obj_ptr(raw_ptr as *const u8) {
        return value;
    }
    // #5579 / Issue #957 (set side): a STRING index (`obj["foo"] = v`) must
    // route through the ordinary receiver-aware `[[Set]]`, NOT the numeric
    // element path below. A NaN-boxed string index otherwise reached the
    // element path — for an arguments object that meant `args["gp"] = v`
    // clobbered `args[0]` (via `arguments_object_set_index`) and silently
    // dropped the named property, so test262 propertyHelper's
    // `isWritable(args, name)` (`args[name] = v` with an untyped `name`
    // param) reported a writable property as non-writable.
    // #5544 widened unknown-receiver string-key writes onto this helper,
    // exposing the gap. `js_put_value_set` is the canonical `[[Set]]` the
    // pre-#5544 path used: it invokes accessor setters with the correct
    // receiver and honours data-property writability across arrays / arguments
    // objects / plain objects / typed arrays, mirroring the IndexGet
    // string-index arm above (`js_object_get_field_by_name_f64`). Numeric
    // indices keep the fast element path below (gated by
    // `finite_nonnegative_u32_index`, so NaN/fractional keys fall through to
    // the ToString write instead of aliasing element 0), so the #5544 perf
    // win stands.
    let idx_top16 = index.to_bits() >> 48;
    if idx_top16 == 0x7FFF || idx_top16 == 0x7FF9 {
        // `target`/`receiver` must be a tagged value, not the raw heap address
        // (`obj` arrives as a module-slot raw I64 when top16 == 0).
        let target = if jsval.is_pointer() {
            obj
        } else {
            f64::from_bits(crate::value::js_nanbox_pointer(raw_ptr as i64).to_bits())
        };
        return crate::proxy::js_put_value_set(target, index, value, target, 0);
    }
    if let Some(idx_u32) = finite_nonnegative_u32_index(index) {
        if unsafe {
            crate::object::arguments_object_set_index(
                raw_ptr as *mut crate::object::ObjectHeader,
                idx_u32,
                value,
            )
        } {
            return value;
        }
    }
    let is_array = unsafe {
        let gc_header =
            (raw_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY
    };
    if is_array {
        crate::array::js_array_set_index_or_string(
            raw_ptr as *mut crate::array::ArrayHeader,
            index,
            value,
        );
        return value;
    }
    // Non-array object: stringify the index and write via the object setter.
    let bits = index.to_bits();
    let top16 = bits >> 48;
    let key_ptr: *const crate::StringHeader = if top16 == 0x7FFF {
        (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader
    } else if top16 == 0x7FF9 {
        crate::value::js_get_string_pointer_unified(index) as *const crate::StringHeader
    } else {
        crate::value::js_jsvalue_to_string(index)
    };
    if key_ptr.is_null() {
        return value;
    }
    crate::object::js_object_set_field_by_name(
        raw_ptr as *mut crate::object::ObjectHeader,
        key_ptr,
        value,
    );
    value
}

/// Check if a value should trigger a destructuring default.
/// Returns 1 if the value is TAG_UNDEFINED, or a bare IEEE NaN (e.g., from
/// out-of-bounds array read), 0 otherwise. All other NaN-boxed values
/// (strings, pointers, booleans, etc.) return 0 because their NaN payload
/// does not match NaN or TAG_UNDEFINED exactly.
#[no_mangle]
pub extern "C" fn js_is_undefined_or_bare_nan(value: f64) -> i32 {
    let bits = value.to_bits();
    // TAG_UNDEFINED = 0x7FFC_0000_0000_0001
    if bits == 0x7FFC_0000_0000_0001 {
        return 1;
    }
    // Bare IEEE NaN (0.0/0.0) — produced by OOB array reads
    // Canonical NaN is 0x7FF8_0000_0000_0000 on most platforms
    if bits == 0x7FF8_0000_0000_0000 {
        return 1;
    }
    0
}

// --- #1561: force-keep the dynamic-index FFI exports under LTO ---
//
// `js_dyn_index_get` / `js_dyn_index_set` / `js_is_undefined_or_bare_nan`
// are `#[no_mangle] pub extern "C"`, but they have **zero internal Rust
// callers** — they are only ever invoked from generated LLVM IR (codegen
// emits the calls in `perry-codegen/src/expr/index_get.rs` and
// `expr/instance_misc1.rs`). The default `.a` staticlib keeps them via
// staticlib-export semantics, but any build mode that round-trips the
// runtime through whole-program LLVM bitcode — the `PERRY_LLVM_BITCODE_LINK`
// path in `optimized_libs.rs`, cross-compile `-Zbuild-std` builds, or a
// future switch to fat LTO — is free to *internalize* an unreferenced
// `#[no_mangle]` symbol and dead-strip it, leaving the codegen-emitted call
// dangling: `Undefined symbols: _js_dyn_index_get` at final link.
//
// The `#[used]` statics below take the address of each export, creating a
// retained reference edge that LTO and the linker's `-dead_strip` must
// honor (the entries land in `@llvm.used` / a `no_dead_strip` section). This
// guarantees the symbols survive auto-optimize regardless of feature set or
// link mode. Function-pointer types are `Sync`, so no wrapper is needed.
#[used]
static KEEP_JS_DYN_INDEX_GET: extern "C" fn(f64, f64) -> f64 = js_dyn_index_get;
#[used]
static KEEP_JS_DYN_INDEX_SET: extern "C" fn(f64, f64, f64) -> f64 = js_dyn_index_set;
#[used]
static KEEP_JS_IS_UNDEFINED_OR_BARE_NAN: extern "C" fn(f64) -> i32 = js_is_undefined_or_bare_nan;
