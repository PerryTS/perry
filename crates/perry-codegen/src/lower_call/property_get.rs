//! String / array / class / Map / Set / Promise / fetch / static-method
//! / instance-method dispatch — the big PropertyGet branch of
//! `lower_call`. This is by far the longest helper in this directory.
//!
//! The dispatch tower's cohesive sub-arms live in sibling modules under
//! `property_get/` (pure code move; no behavior change). This trunk keeps the
//! orchestrating `try_lower_property_get_method_call` plus the string/array
//! routing that is interleaved with `is_string_expr`/`is_array_expr` gating.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, nanbox_pointer_inline, nanbox_string_inline, unbox_to_i64, FnCtx};
use crate::lower_array_method::lower_array_method;
use crate::lower_string_method::{is_known_string_method_name, lower_string_method};
use crate::nanbox::double_literal;
use crate::type_analysis::{
    is_array_expr, is_global_constructor_expr, is_map_expr, is_native_module_dynamic_index,
    is_promise_expr, is_set_expr, is_string_expr, is_url_search_params_expr, receiver_class_name,
};
use crate::types::{DOUBLE, I32, I64};

use super::{
    emit_guarded_direct_method_call, emit_own_method_override_check, lower_abort_controller_call,
    lower_event_target_call, lower_fetch_native_method,
};

mod dynamic_dispatch;
mod fetch_chain;
mod helpers;
mod map_set;
mod number_string;
mod promise_chain;
mod static_dispatch;

// Re-export the moved predicate / resolution helpers so the sibling modules
// (which begin with `use super::*;`) and the trunk can reach them by their
// original unqualified names.
pub(crate) use helpers::{
    class_chain_has_field_named, is_array_only_method_name, is_date_receiver,
    is_inherited_object_prototype_method, receiver_class_defines_method,
    resolve_static_dispatch_cls, string_only_method_arity_ok,
};

/// Try to lower a `Call { callee: PropertyGet { .. } }` via the
/// string/array/class/Map/Set/Promise/fetch/static/instance dispatch tower.
pub fn try_lower_property_get_method_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    // String/array method dispatch (Phase B.12) and class method
    // dispatch (Phase C.2). For PropertyGet receivers, dispatch based
    // on the receiver's static type.
    let Expr::PropertyGet { object, property } = callee else {
        return Ok(None);
    };
    // #5247: capture this call's source byte offset now, before any argument
    // (which may be a nested call that overwrites the pending offset) is
    // lowered. The dynamic `js_native_call_method` fallback below emits the
    // `js_set_call_location` from this captured value, immediately before the
    // throwing dispatch. `0` (and the default build) → no emission.
    let call_byte_offset = ctx.strings.pending_call_offset();
    if let Some(value) =
        super::web_storage::try_lower_web_storage_method_call(ctx, object, property, args)?
    {
        return Ok(Some(value));
    }

    // Number `.toFixed`/`.toPrecision`/`.toExponential`, Buffer/Number
    // `.toString(encoding|radix)`, and the universal `.toString()` arms.
    if let Some(value) =
        number_string::try_lower_number_string_methods(ctx, object, property, args)?
    {
        return Ok(Some(value));
    }

    if is_string_expr(ctx, object)
        && !is_array_only_method_name(property)
        && is_known_string_method_name(property)
    {
        return Ok(Some(lower_string_method(ctx, object, property, args)?));
    }
    // String method fallback for Any-typed receivers: when the method
    // name is a well-known string method that has no array/object
    // equivalent, route through the string dispatcher. This handles
    // the common pattern where a cross-module function returns a string
    // but the local is typed as Any (e.g., `readFileSync(path).split('\n')`).
    // Without this, .split/.charCodeAt/.charAt/etc. on Any-typed strings
    // fall through to js_native_call_method which returns [object Object].
    {
        // Only include methods that are EXCLUSIVELY string methods
        // (no array/map/set equivalent). Exclude: slice, indexOf,
        // lastIndexOf, includes, at, concat — these also exist on
        // arrays and would break when the receiver is an Any-typed
        // array. startsWith/endsWith are string-only in JS so the
        // 2-arg form (searchString, position) is also unambiguous.
        let is_string_only_method = match property.as_str() {
            "split" | "charCodeAt" | "charAt" | "trim" | "trimStart" | "trimEnd" | "substring"
            | "substr" | "toLowerCase" | "toUpperCase" | "toLocaleLowerCase"
            | "toLocaleUpperCase" | "replaceAll" | "padStart" | "padEnd" | "repeat"
            | "codePointAt" | "localeCompare" => true,
            // Annex B §B.2.2 HTML wrappers (`bold`, `link`, `anchor`, …) are
            // string-only in the spec but collide with common user method
            // names — chalk's `chalk.bold(s)` is a styled-string builder
            // (#5039). Forcing the string path here coerced the chalk closure
            // to its source text and wrapped it in `<b>…</b>`. An Any-typed
            // receiver that really is a string still gets them via the
            // `jsval.is_string()` arm of `js_native_call_method`.
            // (`normalize` is intentionally NOT in this unconditional list — the
            // arg-gated `"normalize" if args.len() <= 1` arm below handles it so
            // user 2-arg `normalize(pathname, matched)` methods fall through.)
            // Issue #638: `replace` is also string-exclusive, but routing
            // it here unconditionally caused regressions in async dispatch
            // pathways. Only fire when args[1] is statically detectable as
            // a closure literal — that's the failing case (replace
            // callback got coerced to "[object Object]" via the runtime
            // fallback path because the string-method dispatch never
            // saw it). When args[1] is a string, the existing
            // js_native_call_method fallback handles it correctly via
            // js_string_replace_string.
            "replace" if args.len() == 2 && matches!(&args[1], Expr::Closure { .. }) => true,
            // `slice` exists on strings, arrays, buffers, and Blob-like
            // objects. Let the runtime dispatcher choose by receiver shape;
            // forcing the string path here turns Blob slices into string
            // slices for Any-typed native-module results.
            "slice" => false,
            // `indexOf` / `includes` are NOT string-forced here: an
            // Any-typed receiver may be a runtime array (e.g. a native
            // module property like `PerformanceObserver.supportedEntryTypes`),
            // and forcing the string path made `arr.includes(x)` always
            // return false (string-includes on a non-string). Falling
            // through routes both to `js_native_call_method`, which
            // dispatches on the runtime type and handles string + array
            // (with content-aware element comparison). Refs #1341.
            // startsWith / endsWith are NOT string-forced here: an Any-typed
            // receiver may be a user/library object with its OWN same-named
            // method that returns something other than the String builtin's
            // boolean — e.g. Zod's `z.string().startsWith("./")` returns a
            // refined ZodString schema, not a boolean. Forcing the static
            // string path made `.startsWith()`/`.endsWith()` return a boolean,
            // so a chained `.describe()`/`.optional()` threw
            // `(boolean).describe is not a function` (broke the bundled-CLI TUI
            // schema init). Falling through routes to `js_native_call_method`,
            // which dispatches on the runtime type and still services a genuine
            // Any-typed string receiver via its `jsval.is_string()` arm (the
            // runtime grew full string-method arms in #421/#514). Refs #1341
            // (the same fix already applied to indexOf/includes above).
            // `normalize` is NOT force-routed to the string path for Any-typed
            // receivers at any arity. User classes commonly define a 1-arg
            // `normalize(pathname)` method (Next.js route normalizers:
            // `this.normalize(matchedPath)`, `normalizer.normalize(initPathname)`)
            // — forcing the string path made the pathname argument the Unicode
            // `form`, throwing `RangeError: The normalization form should be one
            // of NFC, NFD, NFKC, NFKD` (Next.js wall 50). A receiver that really
            // is a string still gets `String.prototype.normalize` two ways: the
            // statically-typed-string fast path above (`is_string_expr`), and the
            // `jsval.is_string()` arm of `js_native_call_method` for Any-typed
            // strings. So nothing is lost by falling through here.
            // `lastIndexOf` (number-returning) shares the startsWith/endsWith
            // hazard above — an Any-typed object's own `lastIndexOf` would be
            // clobbered by the String builtin. Fall through to the runtime,
            // which services a genuine string receiver via `jsval.is_string()`.
            _ => false,
        };
        // Don't route buffer/Uint8Array methods through the string path —
        // buffers have a different header layout and their indexOf/includes
        // go through dispatch_buffer_method via js_native_call_method.
        let is_buffer = matches!(
            crate::type_analysis::static_type_of(ctx, object),
            Some(perry_types::Type::Named(ref n)) if n == "Uint8Array" || n == "Buffer"
        );
        // #1760: a dynamic native-module sub-namespace receiver
        // (`(path as any)[k]` → `path.win32`) is NOT a string, even though a
        // method like `normalize` collides with a String.prototype name.
        // Falling through here routes it to the generic `js_native_call_method`
        // dispatch (→ `dispatch_native_module_method`); forcing the string path
        // hands the namespace pointer to a string FFI and SIGSEGVs.
        // #5271: a builtin-named method on a receiver that is NOT provably a
        // string (object literal, `any`, unknown) may be a USER method that
        // merely shares the name — joi's `internals.trim(value, schema)`, or
        // any `{ trim() {…} }.trim()`. Forcing the static String path there
        // hands the object pointer to a string FFI: it either aborts codegen
        // on the String arity guard (`String.trim takes no args, got 2`) or
        // bit-casts the object as a string and returns "[object Object]".
        //
        // Take the static String fast path only when:
        //   * the receiver is NOT a known object-literal local — `o.trim()`
        //     on `const o = { trim() {…} }` is the object's OWN method, never
        //     `String.prototype.trim`, even when the arity matches; AND
        //   * the arg count is plausible for the String builtin — when it is
        //     NOT (joi's `internals.trim(value, schema)`: 2 args to a 0-arg
        //     builtin), the call is a user method sharing the name.
        // Otherwise fall through to `js_native_call_method`, which resolves
        // the receiver's own member at runtime and still services a genuine
        // (Any-typed) string receiver via its `jsval.is_string()` arm — so
        // the earlier "[object Object]" hazard the comment above warns about
        // no longer applies (the runtime grew full string-method arms in
        // #421/#514). See #5271.
        let receiver_is_object_literal = matches!(
            &**object,
            Expr::LocalGet(id) if ctx.object_literal_locals.contains(id)
        ) || matches!(&**object, Expr::Object(_));
        if is_string_only_method
            && string_only_method_arity_ok(property, args.len())
            && !receiver_is_object_literal
            // A receiver whose statically-known class defines its OWN method of
            // this name is calling THAT method, never the String builtin — even
            // when the arity matches. Critical for the char-access methods
            // (`charAt`/`charCodeAt`/`codePointAt`), whose arity gate above is a
            // no-op (any arg count is spec-valid), so a user `charAt(n)` helper
            // (e.g. the `yaml` package's `Lexer`) would otherwise be coerced to
            // `String.prototype.charAt` on a `"[object Object]"` receiver.
            && !receiver_class_defines_method(ctx, object, property)
            && !is_array_expr(ctx, object)
            && !is_buffer
            && !is_native_module_dynamic_index(object)
        {
            return Ok(Some(lower_string_method(ctx, object, property, args)?));
        }
    }
    if is_array_expr(ctx, object) && !is_inherited_object_prototype_method(property) {
        return Ok(Some(lower_array_method(ctx, object, property, args)?));
    }

    // -------- Promise.then / .catch / .finally --------
    if let Some(value) = promise_chain::try_lower_promise_chain_method(ctx, object, property, args)?
    {
        return Ok(Some(value));
    }

    // -------- Map/Set methods on PropertyGet receivers --------
    if let Some(value) = map_set::try_lower_map_set_methods(ctx, object, property, args)? {
        return Ok(Some(value));
    }

    // -------- Map.forEach / Set.forEach / URLSearchParams.forEach --------
    if let Some(value) = map_set::try_lower_collection_foreach(ctx, object, property, args)? {
        return Ok(Some(value));
    }

    // ── AbortController / AbortSignal / EventTarget + chained Web Fetch ──
    if let Some(value) = fetch_chain::try_lower_fetch_chain(ctx, object, property, args)? {
        return Ok(Some(value));
    }

    // Issue #687 — ClassRef receiver static-method dispatch.
    if let Some(value) =
        static_dispatch::try_lower_static_dispatch(ctx, callee, object, property, args)?
    {
        return Ok(Some(value));
    }

    // Class instance method call (interface/dynamic dispatch tower +
    // static-fallback / virtual-override tower).
    if let Some(value) = dynamic_dispatch::try_lower_instance_method_call(
        ctx,
        object,
        property,
        args,
        call_byte_offset,
    )? {
        return Ok(Some(value));
    }

    Ok(None)
}
