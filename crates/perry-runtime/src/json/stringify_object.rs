//! The `JSON.stringify` object walk: an object's own enumerable members, the
//! `toJSON` probe that precedes them, and the direct walk of plain members
//! (#10696). Split out of `stringify.rs` for the 2000-line file gate; the
//! value dispatch that reaches it stays there.

use super::stringify::{
    arm_to_json_result_guard, array_get_to_json, check_stringify_nesting_depth, is_closure_value,
    is_symbol_value, json_key_non_enumerable, object_get_to_json, object_key_str,
    stringify_value_depth, tracked_keys, write_escaped_string, write_number, write_short_string,
    write_url_href_json,
};
use super::stringify_shape_template::{shape_template_for, try_emit_shape_element};
use super::*;
use crate::{js_string_from_bytes, JSValue, StringHeader};
use std::fmt::Write as FmtWrite;

/// SerializeJSONProperty step 2 (`toJSON`) for a heap-valued object member,
/// applied by the object walk BEFORE the member's key is written so a member
/// whose `toJSON` returns `undefined` can be OMITTED per spec (test262
/// JSON/stringify/value-tojson-arguments) instead of emitting `"k":null` — the
/// key used to be written first, with `toJSON` running only in the value
/// recursion below.
///
/// Returns `Some(result)` ONLY when a callable `toJSON` actually ran (the value
/// is a plain object/array carrying one); `result` is what it returned. The
/// caller then omits the member if `result` is `undefined`/function/Symbol, or
/// serializes `result` with the one-shot `SUPPRESS_NEXT_TO_JSON` guard armed so
/// its own walk doesn't re-invoke `toJSON`. Returns `None` when no `toJSON`
/// applies — a plain object/array without one, or any value that isn't a plain
/// object/array — so the caller serializes the ORIGINAL value through the
/// normal dispatch (never arming the guard: arming it for a value that then
/// doesn't self-probe would leak the one-shot into the next member's `toJSON`).
/// Because `None` means "serialize normally", a plain data object member keeps
/// the #6009 fast path (its own walk skips the `toJSON` probe when
/// `class_id == 0`), so this adds no probe there.
///
/// Guards, in an order safe for the `gc_obj_type` read below: handle ids and
/// buffers/typed arrays carry no `GcHeader`; RegExp shares the
/// `GC_TYPE_OBJECT` tag but is not an `ObjectHeader`; a boxed primitive is a
/// real object but must serialize as its primitive (see `stringify_value_depth`)
/// so it is left to the normal dispatch. Date/Temporal cells carry their own
/// `GC_TYPE_*` tags, so the `gc_obj_type` match's `_` arm already skips them.
/// The pending `toJSON` key must already be recorded.
unsafe fn member_to_json(value: f64) -> Option<f64> {
    if crate::proxy::js_proxy_is_proxy(value) != 0 {
        return super::stringify_proxy::to_json(value);
    }
    let bits = value.to_bits();
    let ptr = extract_pointer(bits)?;
    if crate::value::addr_class::is_handle_band(ptr as usize) {
        return None;
    }
    if crate::buffer::is_registered_buffer(ptr as usize) {
        return None;
    }
    if crate::typedarray::lookup_typed_array_kind(ptr as usize).is_some() {
        return None;
    }

    if crate::builtins::boxed_primitive_json_value(value).is_some() {
        return None;
    }
    match gc_obj_type(ptr) {
        crate::gc::GC_TYPE_ARRAY => array_get_to_json(ptr as *const crate::ArrayHeader),
        crate::gc::GC_TYPE_OBJECT => {
            let resolved = object_get_to_json(ptr);
            if resolved.is_none() {
                // Hand the verdict to the member's own walk (#10696); the
                // member loop clears it once the dispatch returns.
                TO_JSON_RESOLVED_FOR.with(|c| c.set(ptr as usize));
            }
            resolved
        }
        _ => None,
    }
}

pub(crate) unsafe fn stringify_object_inner(ptr: *const u8, buf: &mut String, depth: u32) {
    // Taken unconditionally so a verdict for this object can never reach a
    // later walk (see `TO_JSON_RESOLVED_FOR`). An armed one-shot suppression
    // means this object IS a `toJSON` result, which also settles the question
    // for it — and it must be consumed here: a walk that never probes (a plain
    // record) would otherwise leak it into its first child's `toJSON`.
    let resolved_by_parent = TO_JSON_RESOLVED_FOR.with(|c| c.replace(0)) == ptr as usize;
    let is_to_json_result = SUPPRESS_NEXT_TO_JSON.with(|c| c.replace(false));
    let mut global_proof = false;
    stringify_object_walk(
        ptr,
        buf,
        depth,
        resolved_by_parent || is_to_json_result,
        None,
        &mut global_proof,
    );
}

/// Compact `JSON.stringify(value)` of a root that
/// `stringify_tojson_probe::plain_object_member` admits: an ordinary object no
/// `toJSON` can reach, walked directly with the facts that proof resolved
/// (#10696). The generic root route reaches the same walk through the root
/// `toJSON` lookup (and its empty-key allocation), `stringify_value`'s
/// dispatch chain and `is_object_pointer`; only the node-stream probe that
/// dispatch applies to a root object is kept. Returns false, having written
/// nothing and called nothing, for any other value.
pub(super) unsafe fn try_stringify_plain_root(value_bits: u64, buf: &mut String) -> bool {
    let mut global_proof = false;
    let Some((ptr, member)) =
        super::stringify_tojson_probe::plain_object_member(value_bits, &mut global_proof)
    else {
        return false;
    };
    if !crate::node_stream::try_stringify_node_stream_json(ptr, buf) {
        stringify_object_walk(ptr, buf, 0, true, Some(member), &mut global_proof);
    }
    true
}

/// Write `"key":` for the member in slot `f` of an object walk.
///
/// A key can itself contain `"`/`\`/control characters (e.g. a
/// `Symbol`-adjacent computed key or `Object.defineProperty` literal name) —
/// it must go through the same escaper as string values, not a raw
/// `push_str` (test262 JSON/stringify/value-string-escape-ascii, where the
/// property name embeds all 32 ASCII control characters).
#[inline]
unsafe fn write_member_key(buf: &mut String, key_bits: u64, f: u32) {
    let mut key_sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    // An inline short key — most property names — is quoted straight from its
    // payload: plain ASCII needs neither UTF-8 validation nor an escape scan,
    // and anything else declines before writing and takes the general decode.
    if key_bits & crate::value::TAG_MASK == crate::value::SHORT_STRING_TAG {
        let len = JSValue::from_bits(key_bits).short_string_to_buf(&mut key_sso);
        if write_short_string(buf, &key_sso[..len]) {
            buf.push(':');
            return;
        }
    }
    if let Some(key_str) = object_key_str(key_bits, &mut key_sso) {
        write_escaped_string(buf, key_str);
        buf.push(':');
    } else if crate::string::js_string_key_bytes(JSValue::from_bits(key_bits), &mut key_sso)
        .is_some_and(|bytes| super::stringify_scalars::write_wtf8_key(buf, bytes))
    {
        buf.push(':');
    } else {
        let _ = write!(buf, "\"field{}\":", f);
    }
}

/// The object walk behind [`stringify_object_inner`].
///
/// `to_json_resolved` says this object's own `toJSON` question is already
/// settled. `plain_member` is `Some` when the caller admitted this object
/// through `stringify_tojson_probe::plain_object_member`, and carries what
/// that proof resolved, with nothing that allocates or calls user code in
/// between — so the walk reuses it instead of probing again (#10696).
/// `global_proof` is the stringify-wide `toJSON` proof that function
/// documents; the walk clears it before anything that can run user code, and
/// hands it to the plain members it walks.
unsafe fn stringify_object_walk(
    ptr: *const u8,
    buf: &mut String,
    depth: u32,
    to_json_resolved: bool,
    plain_member: Option<super::stringify_tojson_probe::PlainMember>,
    global_proof: &mut bool,
) {
    check_stringify_nesting_depth(depth as usize);
    let obj = ptr as *const crate::ObjectHeader;
    // #6519: a WHATWG `URL` instance is a plain `GC_TYPE_OBJECT` (class_id 0)
    // whose `searchParams` field points back at the URL — walking its fields
    // trips the circular-structure detector. Node serializes a URL via
    // `URL.prototype.toJSON()`, i.e. its `href` string. Top-level
    // `JSON.stringify(url)` is intercepted at HIR-lowering time
    // (`UrlInstanceToJSON`, module_static.rs), but a URL *nested* inside another
    // object/array is invisible to that interception and only reaches this
    // runtime walker — so detect the URL shape here and emit its href. This is
    // the single chokepoint every object walk funnels through (the direct
    // dispatch arms, the array slow loop, and per-field descent all land here).
    // `is_url_object_shape` declines every class id but 0, so only those pay
    // for it; reading the class id is no more of a dereference than the shape
    // read just below, which every caller already relied on.
    if (*obj).class_id == 0 && crate::url::is_url_object_shape(ptr as *mut crate::ObjectHeader) {
        *global_proof = false;
        write_url_href_json(ptr as *mut crate::ObjectHeader, buf);
        return;
    }
    // One shape-table probe answers both the keys and the live inline-slot
    // bound; the member loop below re-derives the keys only after a call that
    // can run user code or collect. Re-deriving them at every key access cost
    // five probes per object (#10696).
    let (mut keys_view, mut num_fields) = match plain_member {
        Some(member) => (member.keys, member.live_slots),
        None => crate::object::object_keys_and_live_slot_count(obj),
    };
    // Whether the admission's answer about array-index keys still describes
    // `keys_view` (it stops doing so if the facts are re-resolved below).
    let mut index_keys_known = plain_member.map(|member| member.has_index_key);
    // #1704: an object with a null `keys_array` has no own enumerable
    // properties — empty objects come out of `js_object_alloc` with
    // `keys_array == null` and only get one once a field is set. This is the
    // shape of `Object.fromEntries([])`, `Object.fromEntries(emptyURLSearchParams)`,
    // and a never-mutated `{}` literal. Recursion into a nested empty object
    // reaches here directly (the `GC_TYPE_OBJECT` arm in `stringify_value_depth`
    // skips `is_object_pointer`), so the `(*keys_arr).length` read below would
    // dereference null and segfault (the `Object.fromEntries(URL.searchParams)`
    // crash inside a `@hono/perry-server` handler). Emit "{}" and return — an
    // empty object has no children, so it can't be part of a cycle and the
    // circular-reference tracking below is unnecessary.
    if keys_view.is_null() {
        // A null `keys_array` means no own enumerable properties — but a class
        // instance with no instance fields (only methods, e.g. a `class {
        // toJSON() {…} }`) still has a `toJSON` on its prototype/vtable that
        // must be honoured before falling back to "{}". A plain empty object
        // literal / `Object.fromEntries([])` carries `class_id == 0`, so the
        // probe is skipped for them. (#321)
        if (*obj).class_id != 0 && !to_json_resolved {
            *global_proof = false;
            if let Some(to_json_val) = object_get_to_json(ptr) {
                arm_to_json_result_guard(to_json_val);
                // Thread depth so a `toJSON` returning a cycle trips the
                // circular-detection push instead of overflowing the stack —
                // see the matching note in the keyed-object branch below.
                stringify_value_depth(to_json_val, TYPE_UNKNOWN, buf, depth + 1);
                SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
                return;
            }
        }
        buf.push_str("{}");
        return;
    }
    if depth > MAX_FAST_DEPTH {
        // Deep nesting — switch to full circular detection
        if STRINGIFY_STACK.with(|s| s.borrow().contains(&(ptr as usize))) {
            let msg = "Converting circular structure to JSON";
            let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
            let err_ptr = crate::error::js_typeerror_new(msg_ptr);
            crate::exception::js_throw(f64::from_bits(
                POINTER_TAG | (err_ptr as u64 & POINTER_MASK),
            ));
        }
        STRINGIFY_STACK.with(|s| s.borrow_mut().push(ptr as usize));
    }

    // Templated fast path (#64 follow-up): if this object's shape has been
    // seen before in this stringify call, emit via the cached prefix table
    // and skip per-object `has_pointer_fields` / `object_get_to_json` /
    // key-lookup work. `try_emit_shape_element` rolls back the buffer and
    // returns false on any element-specific mismatch (different shape,
    // stray UNDEFINED, closure), at which point we fall through to the
    // slow path below.
    //
    // Guard (issue #67): skip the template machinery for small objects.
    // `shape_template_for` allocates a Box<ShapeTemplate> + Vec<String>
    // + one String per field on miss (~4-5 heap allocs), and the cache
    // is wiped at every top-level call exit — so for a one-shot small
    // top-level stringify the build is pure overhead vs. the inline slow
    // path below. The arrayof-objects fast path (stringify_array_depth)
    // uses a separate build_shape_prefix_template that's unaffected.
    // Skip the shape-template fast path when the object has overflow fields
    // (keys_len > num_fields — see object.rs:32 OVERFLOW_FIELDS, ≥9 stored
    // fields per #307). The template's per-field key prefix array is built
    // from `min(keys_len, field_count)`, so an overflow object would only
    // emit its first 8 fields. Falling through to the slow path below uses
    // `read_field_bits` which routes overflow reads through
    // `js_object_get_field`'s overflow_get fallback.
    // Whether the class can contribute anything a raw own-field emitter
    // would miss: a `toJSON` on its chain, or private/runtime-internal keys.
    // A plain member's admission already answered it.
    let class_plain_record =
        plain_member.is_some() || super::stringify_tojson_probe::object_is_plain_record(obj);
    // Whether a `toJSON` could come from anywhere but an own closure field.
    // The raw emitters below read own fields only, so they run only when it
    // cannot; the general walk probes when it can. When the parent already
    // performed this object's `toJSON` lookup (`TO_JSON_RESOLVED_FOR`) there
    // is nothing left to ask (#10696).
    let inherited_to_json_possible = !to_json_resolved
        && super::stringify_tojson_probe::inherited_to_json_possible_without_gc(
            ptr,
            class_plain_record,
        );
    let has_overflow_fields = keys_view.count() > num_fields;
    // The shape-template fast path emits every key in the shape; it can't
    // honor per-key `enumerable: false`, so fall through to the slow path
    // (which filters) whenever any descriptor exists on this thread.
    // Class instances (class_id != 0) route through the slow path: it honours a
    // prototype/own `toJSON` and filters private (`#x`) elements, neither of
    // which the shape-template fast path handles. Plain data objects (class_id
    // == 0 — the common JSON shape) keep the fast path. The root is walked
    // directly: a call visits it once, so its template could never be reused
    // and building one cost more than the walk (#10696).
    if depth > 0
        && num_fields >= 5
        && !has_overflow_fields
        && crate::object::key_attrs::object_summary(obj) == 0
        && !inherited_to_json_possible
        // A private field (#11791) is an own key no template may emit.
        && crate::object::key_attrs::object_summary(obj) & crate::object::key_attrs::SUMMARY_PRIVATE
            == 0
    {
        if let Some(tmpl_ptr) = shape_template_for(ptr) {
            // Same proof, same contract: the template establishes it lazily
            // and clears it before any path that can call user code.
            if try_emit_shape_element(
                make_pointer_bits(ptr),
                &*tmpl_ptr,
                buf,
                depth,
                None,
                global_proof,
            ) {
                if depth > MAX_FAST_DEPTH {
                    STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
                }
                return;
            }
            // A declined attempt can have walked a member (and run its
            // `toJSON`) before rolling back, so re-resolve the facts.
            (keys_view, num_fields) = crate::object::object_keys_and_live_slot_count(obj);
            index_keys_known = None;
        }
    }
    // Not an ObjectHeader after all (a Promise / WeakMap / ArrayBuffer that
    // reached here via a static TYPE_OBJECT hint). Node serializes those as
    // `{}`; walking the slot as an ArrayHeader would fault. A plain member's
    // admission validated its GC header and its keys array, which came from
    // the shape table, whose keys word the collector maintains — while those
    // facts are still the current ones.
    let keys_view = if index_keys_known.is_some() {
        keys_view
    } else if let Some(keys_view) = tracked_keys(keys_view) {
        keys_view
    } else {
        buf.push_str("{}");
        return;
    };
    let keys_len = keys_view.count();
    // Root the object for the enumeration below and re-derive the keys/field
    // buffers from the CURRENT header after anything that can run user code:
    // a user getter (`json_object_getter_value`), a `toJSON` somewhere inside
    // a nested value, or any allocation in the recursive
    // `stringify_value_depth` call can trigger a GC that sweeps or moves this
    // object — bare Rust locals are invisible to the collector (production
    // runs no conservative stack scan), and alloc-point minors can be MOVING
    // under the evacuation policy. The keys array is re-derived THROUGH the
    // object header (its shape's keys word is rewritten by the collector when
    // it moves, and user code can move the object to another shape).
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_const_ptr(obj);
    let current_key_slots = || -> *const f64 {
        obj_handle.with_const_ptr(|obj: *const crate::ObjectHeader| {
            let keys_arr = crate::object::object_keys(obj).arr();
            crate::array::array_elements_ptr(keys_arr as *const crate::ArrayHeader) as *const f64
        })
    };
    // Closes #307: iterate up to keys_len, not min(num_fields, keys_len).
    // Parser-built objects with ≥9 fields cap field_count at the inline
    // alloc_limit (max(field_count, 8) physical slots) and store the overflow
    // values in OVERFLOW_FIELDS (object.rs:32) — so num_fields can be smaller
    // than keys_len. For inline slots (f < alloc_limit) we still read directly
    // off fields_ptr; for overflow slots we route through `js_object_get_field`
    // which checks field_count and falls through to `overflow_get`. Pre-fix
    // (`std::cmp::min(num_fields, keys_len)`) silently dropped the overflow
    // fields and `is_object_pointer`'s `keys_len <= field_count` guard
    // returned false, so `JSON.stringify` emitted the literal string "null"
    // for any parsed object with ≥9 fields.
    let alloc_limit = std::cmp::max(num_fields, crate::object::INLINE_SLOT_FLOOR as u32);
    let read_field_bits = |f: u32| -> u64 {
        obj_handle.with_const_ptr(|obj: *const crate::ObjectHeader| {
            if f < alloc_limit {
                let fields_ptr = (obj as *const u8).add(std::mem::size_of::<crate::ObjectHeader>())
                    as *const f64;
                (*fields_ptr.add(f as usize)).to_bits()
            } else {
                crate::object::js_object_get_field(obj, f).bits()
            }
        })
    };
    let actual_fields = keys_len;

    // Nested one-field leaves and wide inline objects can prove primitive
    // fields while emitting them in one raw walk, avoiding both the generic
    // closure scan and the separate ordinary-key ordering scan. An array-index
    // key or complex value rolls the native buffer back before the general
    // path computes the required ordering. No pointer/BigInt field, descriptor
    // or class can reach the borrowed emit interval.
    // A one-field object whose field is itself a container (a nesting level)
    // would only be written and rolled back, so it is not attempted.
    if (actual_fields > 32
        || (actual_fields == 1
            && super::stringify_primitive_object::field_is_primitive(read_field_bits(0))))
        && !has_overflow_fields
        && !inherited_to_json_possible
        && !crate::object::object_has_descriptors(ptr as usize)
        // A private field (#11791) is an own key the raw walk would emit.
        && crate::object::key_attrs::object_summary(obj) & crate::object::key_attrs::SUMMARY_PRIVATE
            == 0
        && super::stringify_primitive_object::try_emit(obj, keys_view, buf)
    {
        if depth > MAX_FAST_DEPTH {
            STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
        }
        return;
    }

    // #2438: enumerate own keys in ECMA-262 OrdinaryOwnPropertyKeys order —
    // array-index keys first (ascending numeric), then string keys in
    // insertion order. `None` means no array-index keys, so insertion order
    // already matches spec and the loop walks `0..actual_fields` directly.
    // A plain member's admission already scanned its keys for this.
    let key_order = match index_keys_known {
        Some(false) => None,
        _ => crate::object::ecma_own_key_order(keys_view),
    };

    // Private elements (`#x`) and runtime-internal keys live only in declared
    // classes' instances, never in a plain record's keys (the same fact the
    // raw emitters above rely on), so only other classes filter them.
    let hide_private_keys = !class_plain_record;

    // Deferred toJSON + closure checks (issue #67 tightening): scan fields
    // once to detect if any field is actually a closure. For data-only
    // objects with nested arrays/objects (e.g. `{a:1, b:"", c:[...]}`) the
    // earlier has_pointer_fields heuristic false-positived because any
    // POINTER_TAG field triggered the `object_get_to_json` key walk — even
    // though a toJSON method requires the *value* at the "toJSON" key to
    // be a closure. Reading offset 12 (CLOSURE_MAGIC) per pointer field is
    // cheaper (~3ns/field) than walking the keys array looking for a
    // "toJSON" string that almost never exists (~15ns).
    let has_closure_field = {
        let mut found = false;
        for f in 0..actual_fields {
            let bits = read_field_bits(f);
            let tag = bits & 0xFFFF_0000_0000_0000;
            let ptr_candidate = if tag == POINTER_TAG {
                (bits & POINTER_MASK) as *const u8
            } else if is_raw_pointer(bits) {
                bits as *const u8
            } else {
                std::ptr::null()
            };
            // #2154 — a POINTER_TAG field can be a native *handle id* (a small
            // integer, e.g. an `http.Agent` in an object literal, a fetch/zlib/
            // stream handle, or a revocable-Proxy id), not a real heap pointer.
            // Reading the GC header of such a value
            // segfaults. Skip the whole small-handle band `[0, 0x100000)` — not
            // just the `< 0x1000` low guard (#4904/#1843 — a Proxy id at 0xF000D
            // in a Next.js render object crashed exactly here). Real closures
            // live far above the band.
            if crate::value::addr_class::is_above_handle_band(ptr_candidate as usize) {
                // A Symbol-valued field must also be dropped (test262
                // JSON/stringify/value-symbol): a Symbol is POINTER_TAG'd but
                // not a closure, so it needs its own probe alongside the
                // closure kind check.
                if crate::closure::closure_kind_probe(ptr_candidate as usize)
                    || crate::symbol::is_registered_symbol(ptr_candidate as usize)
                {
                    found = true;
                    break;
                }
            }
        }
        found
    };

    // A `toJSON` can live as an OWN closure-typed field (a plain object
    // literal `{ toJSON() {…} }`) OR on the object's prototype / class-method
    // chain — a `class { toJSON() {…} }` instance stores `toJSON` on the class
    // vtable, and an `Object.create(proto)` result inherits it from `proto`.
    // Neither of those carries an own closure field, so the cheap
    // `has_closure_field` scan misses them; `inherited_to_json_possible`
    // covers them (and `Object.setPrototypeOf` / `Object.prototype.toJSON`),
    // so probe `object_get_to_json` (which resolves own+prototype via
    // `js_object_get_field_by_name`) in that case too. This is what lets
    // `JSON.stringify` honour a prototype `toJSON` (#321 — Effect
    // `Inspectable`). Object literals are anonymous shape classes, so
    // `class_id != 0` alone no longer decides it (#10529).
    // An own ACCESSOR `toJSON` (`{ get toJSON() {…} }`) is neither a closure
    // field nor inherited; every accessor sets the descriptor header flag.
    let has_own_accessors = crate::object::object_has_descriptors(ptr as usize);
    // Whether a call below may have run user code or collected since the key
    // slots were last derived. The probe can do both even when it finds no
    // `toJSON` (a getter-valued `toJSON`, the first `Object.prototype` lookup).
    let mut keys_stale = false;
    if (has_closure_field || inherited_to_json_possible || has_own_accessors) && !to_json_resolved {
        *global_proof = false;
        if let Some(to_json_val) = object_get_to_json(ptr) {
            if depth > MAX_FAST_DEPTH {
                STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
            }
            arm_to_json_result_guard(to_json_val);
            // Thread the current depth into the toJSON-result walk (do NOT
            // reset to the depth-0 `stringify_value` entry). A `toJSON` that
            // returns a structure re-entering an object still open higher in
            // the walk (`obj.toJSON = () => circular; circular.prop = obj`)
            // would otherwise recurse forever: each re-entry restarted at
            // depth 0, so the `depth > MAX_FAST_DEPTH` circular-detection push
            // never engaged and the stack overflowed (SIGSEGV). Accumulating
            // depth makes the detection fire and throw the spec TypeError
            // (test262 JSON/stringify/value-tojson-object-circular).
            stringify_value_depth(to_json_val, TYPE_UNKNOWN, buf, depth + 1);
            SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
            return;
        }
        keys_stale = true;
    }
    // Only dereferenced once the loop has refreshed it, if `keys_stale`.
    let mut key_slots =
        crate::array::array_elements_ptr(keys_view.arr() as *const crate::ArrayHeader)
            as *const f64;

    // Only own ENUMERABLE keys are serialized; gated on the process-wide
    // atomic AND the per-object `OBJ_FLAG_HAS_DESCRIPTORS` header flag
    // (#6009) — the global flag flips for good the first time ANY program
    // descriptor is installed, which made every later stringify pay a
    // per-key thread-local HashMap probe (`json_key_non_enumerable` +
    // `json_object_getter_value`) on objects that never had a descriptor.
    let filter_non_enum =
        crate::object::key_attrs::object_summary(ptr as *const crate::ObjectHeader)
            & crate::object::key_attrs::SUMMARY_KEY_BITS
            != 0
            || (crate::object::object_has_descriptors(ptr as usize));
    buf.push('{');
    let mut first = true;
    // `pos(j)` maps the j-th enumerated slot to its key/field index: spec
    // order when array-index keys are present, else slot `j` (no allocation).
    let pos = |j: u32| -> u32 {
        match &key_order {
            Some(ord) => ord[j as usize],
            None => j,
        }
    };
    for j in 0..actual_fields {
        let f = pos(j);
        if keys_stale {
            key_slots = current_key_slots();
            keys_stale = false;
        }
        let mut key_bits = (*key_slots.add(f as usize)).to_bits();
        // Tombstoned slot from an O(1) delete: not a key, not serialized.
        if key_bits == crate::value::TAG_HOLE || is_symbol_value(key_bits) {
            continue;
        }
        // Private elements (`#x`) live in a class instance's keys_array but are
        // not serializable own properties.
        if hide_private_keys
            && obj_handle.with_const_ptr(|obj: *const crate::ObjectHeader| {
                crate::object::field_get_set::own_slot_hidden(obj, f, JSValue::from_bits(key_bits))
            })
        {
            continue;
        }
        // Skip non-enumerable own keys (e.g. `Object.defineProperty(o, k,
        // { enumerable: false })`) before touching the value.
        if filter_non_enum
            && obj_handle.with_const_ptr(|obj: *const crate::ObjectHeader| {
                json_key_non_enumerable(obj, f64::from_bits(key_bits))
            })
        {
            continue;
        }
        let mut field_bits = read_field_bits(f);
        // Own accessor properties: serialize the getter's return value (Node
        // invokes the getter), not the raw slot — which holds the getter
        // closure (object-literal `get x() {}`) or an empty placeholder
        // (`Object.defineProperty(o, k, { get })`). Gated on the descriptor flag.
        // The getter is USER CODE: every pointer below is re-derived from the
        // rooted handle after it returns.
        if filter_non_enum {
            *global_proof = false;
            let getter_value = obj_handle.with_const_ptr(|obj: *const crate::ObjectHeader| {
                crate::object::json_object_getter_value(obj, f64::from_bits(key_bits))
            });
            key_slots = current_key_slots();
            key_bits = (*key_slots.add(f as usize)).to_bits();
            if let Some(gv) = getter_value {
                field_bits = gv.to_bits();
            }
        }
        let mut field_val = f64::from_bits(field_bits);
        // Skip undefined per JSON spec (incl. a getter that returned undefined).
        if field_bits == TAG_UNDEFINED {
            continue;
        }
        // Skip closures and Symbols per JSON spec (only possible for
        // pointer-tagged values). Guarded by has_closure_field: if no field
        // is a closure/Symbol, the in-loop check is skipped entirely for
        // every field.
        if has_closure_field && (is_closure_value(field_bits) || is_symbol_value(field_bits)) {
            continue;
        }

        // An ordinary object member that no `toJSON` can reach is walked
        // directly, reusing the shape facts that admitted it: SerializeJSONProperty
        // step 2 has nothing to call, and every other `stringify_value_depth`
        // dispatch arm is excluded by the same proof (#10696). The member's
        // `toJSON` key is not published either — only a `toJSON` call reads it,
        // and each of the member's own members publishes its own first.
        if let Some((member_ptr, member)) =
            super::stringify_tojson_probe::plain_object_member(field_bits, global_proof)
        {
            if !first {
                buf.push(',');
            }
            first = false;
            write_member_key(buf, key_bits, f);
            stringify_object_walk(member_ptr, buf, depth + 1, true, Some(member), global_proof);
            keys_stale = true;
            continue;
        }

        // SerializeJSONProperty step 2 (#5909): apply a heap-valued member's
        // `toJSON` HERE, before the comma/key are written, so a member whose
        // `toJSON` returns `undefined` (or a function/Symbol) is OMITTED per
        // spec — the value recursion below runs `toJSON` only AFTER the key, so
        // such a member wrongly emitted `"k":null`. A member's `toJSON` key is
        // its own property name; the synthetic `field{f}` fallback name is
        // unreadable, so pass "" as it did before.
        let mut member_probed = false;
        if (field_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG || is_raw_pointer(field_bits) {
            // Everything from here to the member's dispatch can run user code.
            *global_proof = false;
            let mut key_sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            set_to_json_key_str(object_key_str(key_bits, &mut key_sso).unwrap_or(""));
            if let Some(resolved) = member_to_json(field_val) {
                let rb = resolved.to_bits();
                if rb == TAG_UNDEFINED || is_closure_value(rb) || is_symbol_value(rb) {
                    keys_stale = true;
                    continue;
                }
                field_bits = rb;
                field_val = resolved;
                // `toJSON` already ran; arm the one-shot guard so the resolved
                // value's own serialization doesn't invoke `toJSON` a second
                // time (SerializeJSONProperty applies it once). Disarmed after
                // the pointer dispatch below, gated on `member_probed`.
                arm_to_json_result_guard(resolved);
                member_probed = true;
            }
        }

        if !first {
            buf.push(',');
        }
        first = false;

        // `member_to_json` may have collected and moved a heap key. Re-read
        // the slot through the rooted object after that call; SSO keys remain
        // self-contained and use the same decoder.
        if member_probed {
            key_slots = current_key_slots();
            key_bits = (*key_slots.add(f as usize)).to_bits();
        }
        write_member_key(buf, key_bits, f);

        // Inline value dispatch for common types to avoid function call
        // overhead. `field_bits`/`field_val` are the post-`toJSON` value when a
        // `toJSON` ran above.
        let val_tag = field_bits & 0xFFFF_0000_0000_0000;
        if field_bits == TAG_NULL {
            buf.push_str("null");
        } else if field_bits == TAG_TRUE {
            buf.push_str("true");
        } else if field_bits == TAG_FALSE {
            buf.push_str("false");
        } else if val_tag == STRING_TAG {
            let str_ptr = (field_bits & POINTER_MASK) as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if val_tag == crate::value::SHORT_STRING_TAG {
            // v0.5.213 SSO — decode inline 5-byte string and emit.
            let jsval = JSValue::from_bits(field_bits);
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let n = jsval.short_string_to_buf(&mut scratch);
            if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if val_tag == POINTER_TAG || is_raw_pointer(field_bits) {
            // Nested object/array (or the object/array `toJSON` returned). The
            // `toJSON` key was recorded above; `member_probed` armed the guard.
            stringify_value_depth(field_val, TYPE_UNKNOWN, buf, depth + 1);
            if member_probed {
                SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
            }
            // `member_to_json`'s verdict is only for this member's own walk.
            TO_JSON_RESOLVED_FOR.with(|c| c.set(0));
            keys_stale = true;
        } else {
            // Number (most common for data objects) — or Date, handled
            // centrally by `write_number` via DATE_REGISTRY lookup. A BigInt
            // member funnels through `write_number` to `serialize_bigint` /
            // `bigint_apply_to_json`, which reads the pending `toJSON` key, so
            // record this member's key first (#5909).
            if val_tag == BIGINT_TAG {
                let mut key_sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                set_to_json_key_str(object_key_str(key_bits, &mut key_sso).unwrap_or(""));
                // A `BigInt.prototype.toJSON` is user code.
                keys_stale = true;
                *global_proof = false;
            }
            write_number(buf, field_val);
        }
    }
    buf.push('}');
    if depth > MAX_FAST_DEPTH {
        STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
    }
}
