/// JSON.stringify(value, replacer, spacer) — the full 3-arg form.
///
/// - `value`: NaN-boxed JSValue to stringify
/// - `replacer_f64`: NaN-boxed — a closure (function replacer), array (key whitelist), or null
/// - `spacer_f64`: NaN-boxed — a number (indent count), string (indent string), or null
///
/// Returns i64 JSValue bits: a heap or inline string, or TAG_UNDEFINED when
/// `JSON.stringify(undefined)` should return `undefined`.
#[no_mangle]
pub unsafe extern "C" fn js_json_stringify_full(
    value: f64,
    replacer_f64: f64,
    spacer_f64: f64,
) -> i64 {
    let value_bits = value.to_bits();
    let replacer_bits = replacer_f64.to_bits();
    let spacer_bits = spacer_f64.to_bits();
    let no_replacer = replacer_bits == TAG_NULL || replacer_bits == TAG_UNDEFINED;
    let no_spacer =
        spacer_bits == TAG_NULL || spacer_bits == TAG_UNDEFINED || spacer_bits == TAG_FALSE;
    // Numeric zero needs no indentation or observable coercion. Admit it to
    // the canonical emitters only; leave lazy source-copy eligibility below
    // unchanged so zero spacing still reaches the general lazy-array walk.
    let compact_emitter_spacer =
        no_spacer || spacer_f64 == 0.0 || spacer_bits == crate::value::INT32_TAG;
    if no_replacer && compact_emitter_spacer {
        // These primitives cannot invoke toJSON. Restrict the other arguments
        // to inert values so replacers and spacer coercions remain observable.
        if let Some(result) = super::stringify_small::try_primitive(value_bits) {
            return result.bits() as i64;
        }
        if let Some(result) = super::stringify_string::try_heap_string(value_bits) {
            return JSValue::string_ptr(result).bits() as i64;
        }
        if let Some(result) = super::stringify_record_output::try_object(value_bits) {
            return result.bits() as i64;
        }
        if let Some(result) = super::stringify_flat::try_object(value_bits) {
            return result.bits() as i64;
        }
    }

    // JSON.stringify(undefined) returns undefined per spec
    if value_bits == TAG_UNDEFINED {
        return TAG_UNDEFINED as i64;
    }

    // If the value is a closure/function, return undefined per spec
    if is_closure_value(value_bits) {
        return TAG_UNDEFINED as i64;
    }

    // A top-level Symbol is likewise unserializable — return undefined
    // (test262 JSON/stringify/value-symbol).
    if is_symbol_value(value_bits) {
        return TAG_UNDEFINED as i64;
    }

    // Issue #179 Phase 4: lazy-stringify fast path for unmutated
    // lazy arrays — only when no replacer / no indent (matches the
    // output `JSON.stringify(value)` produces; replacer/indent
    // require a real tree walk). The bench's 2-arg form (and most
    // real usage) hits this path.
    if no_replacer && no_spacer {
        if let Some(ptr) = try_stringify_lazy_array(value) {
            return JSValue::string_ptr(ptr).bits() as i64;
        }
    }
    // Lazy-but-materialized: the fast path's `materialized.is_null()`
    // check above returns None; fall back to the tree walk, but
    // point it at the materialized tree (not the lazy header
    // whose fields aren't element f64s).
    let value = redirect_lazy_to_materialized(value);
    let _value_bits = value.to_bits();

    // Determine spacer/indent. A `Number`/`String` wrapper object spacer
    // (`JSON.stringify(v, null, new Number(2))`) is coerced per ECMA-262
    // 25.5.2.1: a Number wrapper via ToNumber, a String wrapper via ToString —
    // both of which run the object's own `valueOf`/`toString` (so an overridden
    // `valueOf` is observed, and a throwing one propagates). Without this the
    // NaN-boxed pointer would be read as a raw indent count (test262
    // space-{number,string}-object).
    let spacer_f64 = match crate::builtins::boxed_primitive_payload(spacer_f64) {
        Some((cid, _)) => {
            const CLASS_ID_BOXED_NUMBER: u32 = 0xFFFF_00D0;
            const CLASS_ID_BOXED_STRING: u32 = 0xFFFF_00D1;
            if cid == CLASS_ID_BOXED_NUMBER {
                crate::builtins::js_number_coerce(spacer_f64)
            } else if cid == CLASS_ID_BOXED_STRING {
                let s = crate::value::js_jsvalue_to_string(spacer_f64);
                f64::from_bits(STRING_TAG | (s as u64 & POINTER_MASK))
            } else {
                spacer_f64
            }
        }
        None => spacer_f64,
    };
    let indent_str: String;
    let spacer_bits = spacer_f64.to_bits();
    let spacer_tag = spacer_bits & 0xFFFF_0000_0000_0000;
    if spacer_bits == TAG_NULL || spacer_bits == TAG_UNDEFINED || spacer_bits == TAG_FALSE {
        indent_str = String::new();
    } else if spacer_tag == STRING_TAG {
        let sp_ptr = (spacer_bits & POINTER_MASK) as *const StringHeader;
        // Spec §25.5.2 step 7a: only first 10 UTF-16 code units of string space are used.
        let full = str_from_header(sp_ptr).unwrap_or("");
        indent_str = truncate_to_10_utf16_units(full);
    } else if spacer_tag == crate::value::SHORT_STRING_TAG {
        // v0.5.213 SSO: spacer passed as inline short string
        // (e.g. `JSON.stringify(obj, null, "  ")` where "  " is 2
        // bytes — fits SSO). Decode into scratch, copy into the
        // indent_str buffer for the formatter.
        let jsval = JSValue::from_bits(spacer_bits);
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = jsval.short_string_to_buf(&mut scratch);
        // SSO strings are at most 5 bytes, always ≤ 10 UTF-16 code units; no truncation needed.
        indent_str = std::str::from_utf8(&scratch[..n]).unwrap_or("").to_string();
    } else if spacer_bits == TAG_TRUE {
        indent_str = String::new();
    } else {
        // Number — use that many spaces (clamped to 10)
        let n = spacer_f64 as usize;
        let n = n.min(10);
        indent_str = " ".repeat(n);
    }
    let use_pretty = !indent_str.is_empty();

    // Determine replacer type
    let replacer_bits = replacer_f64.to_bits();
    let is_null_replacer = replacer_bits == TAG_NULL || replacer_bits == TAG_UNDEFINED;

    // Check if replacer is an array (key whitelist)
    let array_replacer = if !is_null_replacer && is_array_value(replacer_bits) {
        let arr_ptr = if (replacer_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG {
            (replacer_bits & POINTER_MASK) as *const u8
        } else {
            replacer_bits as *const u8
        };
        Some(extract_string_array(arr_ptr))
    } else {
        None
    };

    // Check if replacer is a closure (function)
    let closure_replacer =
        if !is_null_replacer && array_replacer.is_none() && is_closure_value(replacer_bits) {
            let ptr = if (replacer_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG {
                (replacer_bits & POINTER_MASK) as *const crate::closure::ClosureHeader
            } else {
                replacer_bits as *const crate::closure::ClosureHeader
            };
            Some(ptr)
        } else {
            None
        };

    // Non-reentrant fast path (issue #67): same depth-counter trick as
    // js_json_stringify — skip shape_cache save for the outermost call.
    // Skip the pre-call STRINGIFY_STACK clear: the exit path below always
    // clears it on normal return, and the deep-recursion check at depth
    // > MAX_FAST_DEPTH is robust to leftover entries from a prior panic
    // (a stale ptr that happens to match is a false-positive TypeError,
    // which is a defensible degradation for pathological reentrant cases).
    let prior_depth = STRINGIFY_DEPTH.with(|d| {
        let c = d.get();
        d.set(c + 1);
        c
    });
    // Defensive: clear the one-shot `toJSON` suppression guard at the outermost
    // entry so a throw during a prior stringify can't leak it across calls.
    if prior_depth == 0 {
        SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
        // #10696: the `Object.prototype`-has-`toJSON` verdict is NOT force
        // invalidated here any more. "Arbitrary user code ran since the last
        // stringify" is exactly what the live signature comparison in
        // `object_proto_may_have_to_json` detects, on every probe rather than
        // only at entry; forcing it made the first probe of every call a
        // guaranteed recompute. See the note at the matching entry in
        // `stringify_api.rs` for the route-by-route argument. The
        // invalidations around user CALLBACKS (`call_replacer`,
        // `object_get_to_json`, `array_get_to_json`) stay.
    }
    let saved_cache = if prior_depth > 0 {
        Some(take_shape_cache())
    } else {
        None
    };
    let mut buf = take_stringify_buf();

    if let Some(ref allowed_keys) = array_replacer {
        // Array replacer (PropertyList): the key whitelist applies recursively
        // to EVERY nested object, whether the root is an object or an array
        // (`JSON.stringify([1, {a:2}], [])` → "[1,{}]"). The shared dispatcher
        // routes objects/arrays into the whitelist-aware walks and everything
        // else into the plain serializer.
        stringify_value_with_array_replacer(
            value,
            allowed_keys,
            &mut buf,
            &indent_str,
            0,
            use_pretty,
        );
    } else if let Some(closure_ptr) = closure_replacer {
        // Function replacer. Per spec SerializeJSONProperty: toJSON FIRST, then
        // the replacer, then serialize — threading `indent_str` so the 3-arg
        // form (replacer + space) pretty-prints, matching Node.
        //
        // #9445: see `js_json_stringify_with_replacer` — the closure and the
        // `""` key live across two user callbacks and are consumed after them.
        let root_scope = crate::gc::RuntimeHandleScope::new();
        let replacer_root = root_scope.root_raw_const_ptr(closure_ptr);
        let empty_str = js_string_from_bytes(b"".as_ptr(), 0);
        let empty_key_root = root_scope.root_nanbox_f64(nanbox_string_f64(empty_str));
        let value_after_to_json = apply_to_json_keyed(value, empty_key_root.get_nanbox_f64());
        let replaced_root = replacer_root.with_const_ptr::<crate::ClosureHeader, _>(|replacer| {
            call_replacer(
                replacer,
                empty_key_root.get_nanbox_f64(),
                value_after_to_json,
                root_holder(value_after_to_json),
            )
        });
        let replaced_bits = replaced_root.to_bits();
        if replaced_bits == TAG_UNDEFINED {
            STRINGIFY_STACK.with(|s| s.borrow_mut().clear());
            // Restore shape cache and decrement depth before early return
            // (we already incremented STRINGIFY_DEPTH and took the cache).
            restore_stringify_buf(buf);
            match saved_cache {
                Some(s) => restore_shape_cache(s),
                None => clear_shape_cache(),
            }
            STRINGIFY_DEPTH.with(|d| d.set(d.get() - 1));
            return TAG_UNDEFINED as i64;
        }
        // Serialize the root: scalars inline, pointers via the GC-tag dispatch
        // (object vs array) so the indent threads through nested structures.
        if !write_replaced_scalar(&mut buf, replaced_root) {
            let ptr = extract_pointer(replaced_bits).unwrap();
            replacer_root.with_const_ptr::<crate::ClosureHeader, _>(|replacer| {
                dispatch_pointer_with_replacer(
                    ptr,
                    replaced_root,
                    replacer,
                    &mut buf,
                    &indent_str,
                    0,
                );
            });
        }
    } else if !use_pretty
        && super::stringify_object::try_stringify_plain_root(value.to_bits(), &mut buf)
    {
        // No replacer, compact, and a root no `toJSON` can reach (#10696).
    } else {
        // No replacer. Pre-resolve the ROOT value's own `toJSON` here (same
        // `apply_to_json_keyed` the function-replacer branch above uses) so a
        // root whose `toJSON` returns `undefined` (or a function/Symbol) can
        // short-circuit to `JSON.stringify`'s `undefined` return — the
        // buffer-based walk below has no way to signal that once it starts
        // writing bytes (test262 JSON/stringify/value-tojson-arguments,
        // value-tojson-result's `arr.toJSON = () => {}` case). Arm the
        // one-shot suppression guard so the walk below doesn't re-invoke
        // `toJSON` on the same (already-resolved) root value.
        // The root's `toJSON` key is the empty String; it is only ever read
        // back as bytes, so no string needs to be allocated to carry it.
        reset_to_json_key();
        let value_after_to_json = apply_to_json(value);
        let after_bits = value_after_to_json.to_bits();
        if after_bits == TAG_UNDEFINED
            || crate::proxy::proxy_wraps_callable(value_after_to_json)
            || is_closure_value(after_bits)
            || is_symbol_value(after_bits)
        {
            STRINGIFY_STACK.with(|s| s.borrow_mut().clear());
            restore_stringify_buf(buf);
            match saved_cache {
                Some(s) => restore_shape_cache(s),
                None => clear_shape_cache(),
            }
            STRINGIFY_DEPTH.with(|d| d.set(d.get() - 1));
            return TAG_UNDEFINED as i64;
        }
        // Only arm suppression when `toJSON` actually substituted a
        // different value — arming unconditionally would suppress a
        // legitimate nested `toJSON` (e.g. `{a:1, b:{toJSON(){...}}}`) any
        // time the root object itself has no closure field to consume the
        // flag on its own dispatch (a plain data object never calls
        // `object_get_to_json` at all, so the flag would otherwise leak
        // straight through to the first real nested `toJSON`).
        if after_bits != value.to_bits() {
            arm_to_json_result_guard(value_after_to_json);
        }
        if use_pretty {
            // No replacer, but has spacer — pretty-print
            stringify_value_pretty(value_after_to_json, TYPE_UNKNOWN, &mut buf, &indent_str, 0);
        } else {
            // The root's `toJSON` lookup just ran and found nothing callable;
            // hand that verdict to the root's own walk instead of repeating
            // it there (#10696). The guard repeats `apply_to_json`'s, so the
            // token names only an object it really probed.
            if after_bits == value.to_bits() {
                if let Some(ptr) = extract_pointer(after_bits) {
                    if !crate::value::addr_class::is_handle_band(ptr as usize)
                        && ptr_derefable(ptr as usize)
                        && gc_obj_type(ptr) == crate::gc::GC_TYPE_OBJECT
                        && !crate::buffer::is_registered_buffer(ptr as usize)
                    {
                        TO_JSON_RESOLVED_FOR.with(|c| c.set(ptr as usize));
                    }
                }
            }
            // Plain stringify
            stringify_value(value_after_to_json, TYPE_UNKNOWN, &mut buf);
            TO_JSON_RESOLVED_FOR.with(|c| c.set(0));
        }
        SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
    }

    // Only touch STRINGIFY_STACK if we actually pushed to it (depth >
    // MAX_FAST_DEPTH was hit). The `borrow` path avoids the borrow_mut
    // cost on the common empty-stack case. Unpopped entries only exist
    // after a panic mid-traversal; see the entry-side comment for the
    // correctness argument.
    STRINGIFY_STACK.with(|s| {
        let stack = s.borrow();
        if !stack.is_empty() {
            drop(stack);
            s.borrow_mut().clear();
        }
    });

    let result = super::stringify_small::inline_ascii_output(buf.as_bytes()).unwrap_or_else(|| {
        JSValue::string_ptr(json_string_from_native_output_bytes(buf.as_bytes()))
    });
    restore_stringify_buf(buf);
    match saved_cache {
        Some(s) => restore_shape_cache(s),
        None => clear_shape_cache(),
    }
    STRINGIFY_DEPTH.with(|d| d.set(d.get() - 1));
    result.bits() as i64
}
