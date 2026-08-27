//! Literals, variables, update, DateNow.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
use perry_hir::types::Type as HirType;
use perry_hir::{BinaryOp, Expr, UpdateOp};

use crate::lower_string_concat::{
    can_lower_string_self_append, flatten_string_add_chain, lower_string_self_append,
    lower_string_self_append_chain,
};
use crate::nanbox::double_literal;
use crate::type_analysis::{is_map_expr, is_set_expr, receiver_class_name};
use crate::types::{DOUBLE, I32, I64};

use super::{
    can_lower_expr_as_i32_in_current_region, emit_root_nanbox_store_on_block,
    emit_shadow_slot_clear, emit_shadow_slot_update_for_expr, emit_write_barrier,
    is_global_this_builtin_function_name, lower_expr, lower_expr_as_i32,
    lower_pod_local_reassignment, materialize_pod_value_copy, nanbox_string_inline, FnCtx,
    TrustedBoxCapturePtr,
};

/// Load the current value from a compiler-proven raw box capture.
///
/// The exact-arrow resolver has already validated `capture.ptr`, so the hot
/// path is a direct cell load. Preserve lexical TDZ behavior with a cold call
/// to the existing trusted accessor only for the reserved sentinel; that
/// helper owns both ReferenceError construction and Perry's internal TDZ
/// suppression window semantics.
fn load_trusted_box_capture_bits(ctx: &mut FnCtx<'_>, capture: &TrustedBoxCapturePtr) -> String {
    let bits = ctx.block().load(I64, &capture.ptr);
    let is_tdz = ctx.block().icmp_eq(I64, &bits, crate::nanbox::TAG_TDZ_I64);
    let slow_idx = ctx.new_block("trusted_box.tdz");
    let merge_idx = ctx.new_block("trusted_box.read");
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    let fast_label = ctx.block().label.clone();
    ctx.block().cond_br(&is_tdz, &slow_label, &merge_label);

    ctx.current_block = slow_idx;
    // The trusted accessor throws for a real TDZ read (and can allocate while
    // constructing the error). A versioned-loop clone must poison its caller
    // before entering that observable cold arm, just like a PIC miss or
    // dynamic `+` fallback.
    crate::expr::emit_versioned_loop_callback_deopt(ctx);
    let slow_bits = ctx
        .block()
        .call(I64, "js_box_get_bits_trusted", &[(I64, &capture.bits)]);
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block()
        .phi(I64, &[(&bits, &fast_label), (&slow_bits, &slow_end)])
}

/// A box, closure cell, or module root is the storage for the source binding,
/// not an alias of the string it currently owns. An ordinary read extracts a
/// second copy of that value, so demote a heap string before it can outlive the
/// cell and be silently changed by a later in-place append (#8432).
///
/// The append lowering reads these targets directly and therefore deliberately
/// bypasses this rule. Limit the call to declared-string bindings: only those
/// bindings can select in-place append, and erased annotations remain safe
/// because the runtime helper checks the live tag.
/// Read capture slot `capture_idx` of the running closure as raw bits, inline.
///
/// `js_closure_get_capture_bits` is a null test, a bounds test against the
/// header's capture count, and one load. Inside a closure body neither test
/// can fail: `closure_ptr` is the rooted `%this_closure` (re-read by
/// `current_closure_ptr_value`, so a relocation is already accounted for), and
/// `capture_idx` is the index this compiler assigned when it laid the closure
/// out — the same layout `codegen/closure.rs` walks at entry for boxed
/// captures. A benchmark loop closure reading its captured world/entities on
/// every iteration paid a call per read; this is the load the call did.
fn load_closure_capture_bits_inline(
    ctx: &mut FnCtx<'_>,
    closure_ptr: &str,
    capture_idx: u32,
) -> String {
    let offset =
        crate::target_layout::closure_header_size_bytes(ctx.target_triple) + 8 * capture_idx as u64;
    let blk = ctx.block();
    let slot_addr = blk.add(I64, closure_ptr, &offset.to_string());
    let slot_ptr = blk.inttoptr(I64, &slot_addr);
    blk.load(I64, &slot_ptr)
}

fn demote_extracted_string_binding(ctx: &mut FnCtx<'_>, id: u32, value: &str) {
    let persistent_binding = ctx.closure_captures.contains_key(&id)
        || (ctx.boxed_vars.contains(&id) && !ctx.module_globals.contains_key(&id))
        || ctx.module_globals.contains_key(&id);
    if persistent_binding && matches!(ctx.local_type_hint(&id), Some(HirType::String)) {
        super::helpers::emit_string_addref_if_heap_string(ctx, value);
    }
}

/// #1380: method names addressable on a `Set` instance, used by the
/// `typeof set.<name>` fold to report "function" (Set method values are
/// not materialized as real function objects). Includes the ES2024
/// composition methods.
fn is_set_method_name(name: &str) -> bool {
    matches!(
        name,
        "has"
            | "add"
            | "delete"
            | "clear"
            | "forEach"
            | "entries"
            | "values"
            | "keys"
            | "union"
            | "intersection"
            | "difference"
            | "symmetricDifference"
            | "isSubsetOf"
            | "isSupersetOf"
            | "isDisjointFrom"
    )
}

/// #1380: method names addressable on a `Map` instance, used by the
/// `typeof map.<name>` fold (same rationale as `is_set_method_name`).
fn is_map_method_name(name: &str) -> bool {
    matches!(
        name,
        "has" | "get" | "set" | "delete" | "clear" | "forEach" | "entries" | "values" | "keys"
    )
}

fn is_headers_method_name(name: &str) -> bool {
    matches!(
        name,
        "append"
            | "delete"
            | "entries"
            | "forEach"
            | "get"
            | "getSetCookie"
            | "has"
            | "keys"
            | "set"
            | "Symbol.iterator"
            | "@@iterator"
            | "values"
    )
}

fn is_headers_instance_method(ctx: &FnCtx<'_>, object: &Expr, property: &str) -> bool {
    is_headers_method_name(property)
        && matches!(receiver_class_name(ctx, object).as_deref(), Some("Headers"))
        // #6003: a user-defined `class Headers` owns the receiver type —
        // its members are ordinary class members, not the native surface.
        && !ctx.classes.contains_key("Headers")
}

fn is_classic_stream_method_name(name: &str) -> bool {
    matches!(
        name,
        "on" | "addListener"
            | "once"
            | "prependListener"
            | "prependOnceListener"
            | "emit"
            | "listeners"
            | "rawListeners"
            | "eventNames"
            | "listenerCount"
            | "removeListener"
            | "off"
            | "removeAllListeners"
            | "setMaxListeners"
            | "getMaxListeners"
    )
}

fn is_classic_stream_instance_method(ctx: &FnCtx<'_>, object: &Expr, property: &str) -> bool {
    if !is_classic_stream_method_name(property) {
        return false;
    }
    matches!(
        receiver_class_name(ctx, object).as_deref(),
        Some("Readable" | "Writable" | "Duplex" | "Transform" | "PassThrough" | "Stream")
    )
}

fn fs_lchmod_callable_on_target(target_triple: &str) -> bool {
    let target = target_triple.to_ascii_lowercase();
    target.contains("darwin")
        || target.contains("macos")
        || target.contains("ios")
        || target.contains("freebsd")
        || target.contains("netbsd")
        || target.contains("openbsd")
        || target.contains("dragonfly")
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::Integer(i) => Ok(double_literal(*i as f64)),
        Expr::Number(f) => Ok(double_literal(*f)),
        // Booleans are NaN-boxed using TAG_TRUE/TAG_FALSE — both are
        // double bit patterns inside the NaN range, emitted as hex
        // literals (LLVM's `0x{16-hex}` form for non-finite doubles).
        Expr::Bool(b) => {
            let tag = if *b {
                crate::nanbox::TAG_TRUE
            } else {
                crate::nanbox::TAG_FALSE
            };
            Ok(double_literal(f64::from_bits(tag)))
        }
        // `undefined` and `null` lower to their NaN-tagged bit patterns.
        Expr::Undefined => Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))),
        Expr::Null => Ok(double_literal(f64::from_bits(crate::nanbox::TAG_NULL))),
        Expr::NewTarget => {
            if let Some(slot) = ctx.new_target_stack.last().cloned() {
                Ok(ctx.block().load(DOUBLE, &slot))
            } else {
                Ok(ctx.block().call(DOUBLE, "js_new_target_get", &[]))
            }
        }

        // `void <expr>` — evaluate the operand for side effects, return
        // undefined. Used both as `void 0` (a common idiom for `undefined`)
        // and `void (sideEffect = 42)` for discarding an assignment value.
        Expr::Void(operand) => {
            let _ = lower_expr(ctx, operand)?;
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }

        // `typeof <expr>` — calls js_value_typeof which returns a runtime
        // string handle ("number", "string", "boolean", "undefined",
        // "object", "function"). The result is NaN-boxed with STRING_TAG.
        Expr::TypeOf(operand) => {
            // Issue #574: short-circuit known compile-time shapes that
            // `js_value_typeof` would misclassify because the runtime
            // representation collides with a different tag:
            //
            //   * Namespace ExternFuncRef (`import * as Lib from "./m"`)
            //     lowers as a TAG_TRUE sentinel → typeof reads "boolean".
            //     Emit "object" instead.
            //   * Class refs (local `Expr::ClassRef` and imported
            //     `Expr::ExternFuncRef` resolving via `class_ids`, plus
            //     the namespace-member class case `Lib.A`) lower as
            //     INT32-tagged class ids → typeof reads "number". Emit
            //     "function" to match JS spec for class objects.
            let typeof_short_circuit: Option<&'static str> = match operand.as_ref() {
                Expr::ExternFuncRef { name, .. } if ctx.namespace_imports.contains(name) => {
                    Some("object")
                }
                Expr::ExternFuncRef { name, .. } if ctx.class_ids.contains_key(name) => {
                    Some("function")
                }
                Expr::ClassRef(_) => Some("function"),
                Expr::NativeMethodCall {
                    module,
                    class_name: None,
                    object: Some(_),
                    method,
                    ..
                } if module == "Headers" && is_headers_method_name(method) => Some("function"),
                // Issue #623: native-module default-imports (`import process
                // from "node:process"`) lower as `NativeModuleRef`, which the
                // codegen represents as a `0.0` stub double. `js_value_typeof`
                // reads it as a number; per spec native-module bindings are
                // objects.
                Expr::NativeModuleRef(_) => Some("object"),
                // Issue #623: bare `typeof globalThis` — perry models the
                // global object as `GlobalGet(0)` lowering to `0.0`, same
                // misclassification.
                Expr::GlobalGet(_) => Some("object"),
                Expr::PropertyGet {
                    object, property, ..
                } => {
                    // #1380: `typeof set.has` / `typeof map.get` → "function".
                    // Set/Map methods aren't materialized as real function
                    // objects — a bare `set.has` read returns the (absent)
                    // data property, so `js_value_typeof` would report
                    // "undefined". The receiver type is known here via
                    // `is_set_expr`/`is_map_expr` (the same routing that makes
                    // `set.size` resolve to a number), so fold known method
                    // names to "function". Covers
                    // `process.allowedNodeEnvironmentFlags` (lowered to a Set)
                    // whose `.has`/`.size` callers feature-detect with typeof.
                    if (is_set_expr(ctx, object) && is_set_method_name(property))
                        || (is_map_expr(ctx, object) && is_map_method_name(property))
                    {
                        Some("function")
                    } else if is_headers_instance_method(ctx, object, property) {
                        Some("function")
                    } else if is_classic_stream_instance_method(ctx, object, property) {
                        Some("function")
                    } else if let Expr::ExternFuncRef { name, .. } = object.as_ref() {
                        if ctx.namespace_imports.contains(name)
                            && ctx.class_ids.contains_key(property)
                        {
                            Some("function")
                        } else {
                            None
                        }
                    } else if matches!(object.as_ref(), Expr::GlobalGet(_)) {
                        // Issue #623: `(globalThis as any).process` /
                        // `globalThis.console` — known Node globals that are
                        // objects in spec. The codegen lowers
                        // `globalThis.<name>` to a generic property read that
                        // produces a stub double; typeof would read "number"
                        // without this short-circuit. Function-shaped globals
                        // (Buffer, Promise, URL, etc.) intentionally fall
                        // through so `typeof Buffer === "function"` keeps
                        // working through the existing class-ref path.
                        //
                        // lodash followup: built-in constructors exposed on
                        // globalThis (`Array`, `Object`, `Function`, …) now
                        // also lower the bare PropertyGet to a real value
                        // (a backing-object pointer materialized by
                        // `js_get_global_this`'s singleton populator).
                        // Without the typeof short-circuit, `typeof
                        // globalThis.Array` would read "object" (the value
                        // is a real pointer); spec says "function". Math /
                        // JSON / Reflect stay "object" — they're namespaces,
                        // not constructors.
                        match property.as_str() {
                            "process" | "console" | "globalThis" | "performance" | "navigator"
                            | "crypto" | "localStorage" | "sessionStorage" => Some("object"),
                            "Math" | "JSON" | "Reflect" | "Atomics" | "Intl" | "Temporal" => {
                                Some("object")
                            }
                            n if is_global_this_builtin_function_name(n) => Some("function"),
                            _ => None,
                        }
                    } else if let Expr::NativeModuleRef(module) = object.as_ref() {
                        // #1343: `typeof <nativeModule>.<member>` (e.g.
                        // `typeof crypto.randomBytes`, `typeof process.cwd`).
                        // A method is only addressable through the call-
                        // dispatch arms, so reading it as a plain value yields
                        // the module's `0.0` stub and `js_value_typeof` reports
                        // "undefined"/"number". Short-circuit only methods and
                        // exported classes to "function". Properties fall
                        // through (`None`): their value is materialized for
                        // real, so the generic typeof already reports the right
                        // primitive/object kind (`process.pid` → "number",
                        // `os.EOL` → "string", `crypto.constants` → "object").
                        if matches!(module.as_str(), "fs" | "node:fs")
                            && matches!(property.as_str(), "lchmod" | "lchmodSync")
                            && !fs_lchmod_callable_on_target(ctx.target_triple)
                        {
                            None
                        } else {
                            match perry_api_manifest::module_has_symbol(module, property) {
                                Some(e)
                                    if matches!(
                                        e.kind,
                                        perry_api_manifest::ApiKind::Method { .. }
                                            | perry_api_manifest::ApiKind::Class
                                    ) =>
                                {
                                    Some("function")
                                }
                                _ => None,
                            }
                        }
                    } else {
                        // Refs #915 (gap 2 from #899): `typeof C.staticMethod`
                        // where `C` is `Expr::ClassRef` or a `LocalGet`
                        // aliased to a class. Without this fold, the
                        // generic PropertyGet path returns `undefined`
                        // for static methods (the runtime `class_has_own_method`
                        // checks the prototype vtable, not the static
                        // method registry), so `typeof Cls.pipe` reported
                        // `"undefined"` instead of `"function"`. The actual
                        // dispatch fix lives in `lower_call.rs`'s ClassRef
                        // static-method arm — but a typeof read isn't a
                        // call, so it needs its own fold here.
                        let cls_opt: Option<String> = match object.as_ref() {
                            Expr::ClassRef(cls_name) => Some(cls_name.clone()),
                            Expr::LocalGet(id) => ctx
                                .local_id_to_name
                                .get(id)
                                .and_then(|name| ctx.local_class_aliases.get(name).cloned()),
                            _ => None,
                        };
                        if let Some(cls) = cls_opt {
                            // Walk own static methods + extends chain.
                            let mut cur = Some(cls);
                            let mut found = false;
                            while let Some(c) = cur {
                                if let Some(class_info) = ctx.classes.get(&c) {
                                    if class_info
                                        .static_methods
                                        .iter()
                                        .any(|m| m.name == *property)
                                    {
                                        found = true;
                                        break;
                                    }
                                    cur = class_info.extends_name.clone();
                                } else {
                                    break;
                                }
                            }
                            if found {
                                Some("function")
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                }
                _ => None,
            };
            if let Some(s) = typeof_short_circuit {
                let idx = ctx.strings.intern(s);
                let entry = ctx.strings.entry(idx);
                let handle_global = format!("@{}", entry.handle_global);
                return Ok(ctx.block().load(DOUBLE, &handle_global));
            }
            let v = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_value_typeof", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }

        // String literals are pre-allocated at module init via the
        // StringPool's hoisting strategy (see `crate::strings`). At the use
        // site we just load the cached NaN-boxed handle from the pool's
        // `.handle` global. ONE instruction, no per-use allocation.
        Expr::String(s) => {
            let idx = ctx.strings.intern(s);
            let entry = ctx.strings.entry(idx);
            // Clone the global name out so we don't keep `entry` borrowed
            // across the call to `ctx.block()` (which mutably borrows
            // `ctx.func`, distinct from `ctx.strings` but the borrow checker
            // sees `entry` as borrowing `ctx`).
            let handle_global = format!("@{}", entry.handle_global);
            Ok(ctx.block().load(DOUBLE, &handle_global))
        }

        // WTF-8 string literals (contain lone surrogates U+D800..U+DFFF).
        // Same hoisting strategy as Expr::String, but initialized via
        // js_string_from_wtf8_bytes which sets STRING_FLAG_HAS_LONE_SURROGATES.
        Expr::WtfString(bytes) => {
            let idx = ctx.strings.intern_wtf8(bytes);
            let entry = ctx.strings.entry(idx);
            let handle_global = format!("@{}", entry.handle_global);
            Ok(ctx.block().load(DOUBLE, &handle_global))
        }

        // -------- Variables --------
        // LocalGet lookup order:
        //   1. Closure captures (when lowering inside a closure body) →
        //      runtime js_closure_get_capture_bits(this_closure, idx)
        //   2. Function-local alloca slots
        //   3. Module-level globals
        //
        // This lets closures read captured outer variables, regular
        // functions read their own params/lets, and any function read
        // module-scope `let`s (the ones in `hir.init` at top level).
        Expr::LocalGet(id) => {
            if ctx.pod_records.contains_key(id) {
                return materialize_pod_value_copy(ctx, *id);
            }
            // Captured by closure (from outer scope):
            if let Some(&capture_idx) = ctx.closure_captures.get(id) {
                // If the captured id is a boxed var, the capture slot holds a
                // raw box pointer. Read the capture, extract the box pointer,
                // and deref via js_box_get_bits.
                if ctx.boxed_vars.contains(id) {
                    if let Some(capture) = ctx.trusted_box_capture_ptrs.get(id).cloned() {
                        let bits = load_trusted_box_capture_bits(ctx, &capture);
                        let value = ctx.block().bitcast_i64_to_double(&bits);
                        demote_extracted_string_binding(ctx, *id, &value);
                        return Ok(value);
                    }
                    let closure_ptr =
                        super::current_closure_ptr_value(ctx, "captured boxed local")?;
                    let getter = if ctx.trusted_box_captures {
                        "js_box_get_bits_trusted"
                    } else {
                        "js_box_get_bits"
                    };
                    let box_ptr = load_closure_capture_bits_inline(ctx, &closure_ptr, capture_idx);
                    let blk = ctx.block();
                    let bits = blk.call(I64, getter, &[(I64, &box_ptr)]);
                    let value = blk.bitcast_i64_to_double(&bits);
                    demote_extracted_string_binding(ctx, *id, &value);
                    return Ok(value);
                }
                let closure_ptr = super::current_closure_ptr_value(ctx, "captured local")?;
                let bits = load_closure_capture_bits_inline(ctx, &closure_ptr, capture_idx);
                let value = ctx.block().bitcast_i64_to_double(&bits);
                demote_extracted_string_binding(ctx, *id, &value);
                return Ok(value);
            }
            // Boxed local in enclosing function: load the slot (box
            // pointer), deref via js_box_get_bits.
            //
            // #6369: never for a MODULE GLOBAL. Its storage is the
            // `@perry_global_*` cell, which holds the VALUE — `Stmt::Let`
            // stores it there directly and never allocates a box — so a
            // box deref here would reinterpret e.g. an array pointer as a
            // box pointer. `LocalSet` and `Update` (below) already carry
            // this exclusion; the read path was the odd one out, and it
            // only stayed latent because a module global normally has no
            // `ctx.locals` slot to find. The packed-loop invariant-global
            // read cache installs exactly such a slot (`loops.rs` aliases
            // the global into `ctx.locals` for the duration of the loop),
            // so any module global that landed in the module-wide boxed
            // union — which every closure inherits wholesale — was read
            // back as garbage (`NaN`) there.
            if ctx.boxed_vars.contains(id) && !ctx.module_globals.contains_key(id) {
                if let Some(slot) = ctx.locals.get(id).cloned() {
                    let blk = ctx.block();
                    let box_ptr = blk.load(I64, &slot);
                    let bits = blk.call(I64, "js_box_get_bits", &[(I64, &box_ptr)]);
                    let value = blk.bitcast_i64_to_double(&bits);
                    demote_extracted_string_binding(ctx, *id, &value);
                    return Ok(value);
                }
            }
            // Repsel Phase 1: a canonical-i32 local's ONLY storage is the i32
            // slot — materialize the boxed view (`sitofp`/`uitofp`) here, at
            // the boxed use site.
            if let Some(v) = crate::expr::load_canonical_local_boxed(ctx, *id) {
                return Ok(v);
            }
            if let Some(slot) = ctx.locals.get(id).cloned() {
                // Issue #48: prefer the i32 slot for int32-stable locals so
                // LLVM can promote the alloca to an i32 SSA value and skip the
                // double round-trip. The double slot is still maintained (for
                // closures or escape sites) but mem2reg + DSE will eliminate
                // it when the i32 path covers every read.
                if let Some(i32_slot) = ctx.i32_counter_slots.get(id).cloned() {
                    let i = ctx.block().load(I32, &i32_slot);
                    let v = if ctx.unsigned_i32_locals.contains(id) {
                        ctx.block().uitofp(I32, &i, DOUBLE)
                    } else {
                        ctx.block().sitofp(I32, &i, DOUBLE)
                    };
                    return Ok(v);
                }
                let value = ctx.block().load(DOUBLE, &slot);
                demote_extracted_string_binding(ctx, *id, &value);
                Ok(value)
            } else if let Some(global_name) = ctx.module_globals.get(id).cloned() {
                let g_ref = format!("@{}", global_name);
                let value = ctx.block().load(DOUBLE, &g_ref);
                demote_extracted_string_binding(ctx, *id, &value);
                Ok(value)
            } else {
                // Soft fallback: the HIR sometimes carries stale
                // local references that don't correspond to any
                // declared param/let/global in the current scope
                // (curry-style nested closures, async transformer
                // intermediate ids, etc.). Return undefined so
                // compilation succeeds without fabricating a numeric 0.
                Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
            }
        }

        // `total = expr` — store the new value into the local's alloca slot
        // and return it (matches JS semantics: assignment is an expression
        // whose value is the assigned value).
        //
        // SPECIAL FAST PATH: `x = x + y` where `x` is a string-typed local.
        // Uses
        // `js_string_append` (in-place for refcount=1 unique owners)
        // instead of `js_string_concat` (always allocates). For a 10K-
        // iteration `str = str + "a"` build loop, this turns O(n²) total
        // work into O(n) and is the difference between 700 ms and 200 ms
        // on bench_string_ops.
        Expr::LocalSet(id, value) => {
            super::invalidate_local_write_facts(ctx, *id);
            super::record_local_value_alias_for_write(ctx, *id, value.as_ref());
            if let Some(v) = lower_pod_local_reassignment(ctx, *id, value)? {
                super::record_native_arena_owner_assignment(ctx, *id, value.as_ref());
                return Ok(v);
            }
            // Detect the `x = x + y` self-append pattern. For a longer
            // left-associated concat (`x = x + a + b + c`), retain `x` as
            // the accumulator and rebuild only `a + b + c` as the append
            // operand. Lowering the whole expression through
            // `js_string_concat_chain` would copy the growing `x` prefix on
            // every loop iteration, turning the otherwise-amortized append
            // path back into O(n^2) work (#8394).
            // The append helper abstracts over plain slots, module roots,
            // closure captures, and variable boxes. Ordinary reads from the
            // latter three storage families demote a heap string to shared;
            // this owner read deliberately bypasses that extraction rule so
            // the binding can retain uniqueness across iterations (#8432).
            // #7841: the tag-dispatched helper validates the destination's
            // current value before choosing append versus ordinary JS `+`.
            // This is therefore a dispatch hint, not a binding proof; using
            // the stable-only query would disable the optimization for every
            // self-append because this `LocalSet` is itself a reassignment.
            if matches!(ctx.local_type_hint(id), Some(HirType::String))
                && can_lower_string_self_append(ctx, *id)
            {
                if let Expr::Binary {
                    op: BinaryOp::Add,
                    left,
                    right,
                } = value.as_ref()
                {
                    if let Expr::LocalGet(left_id) = left.as_ref() {
                        if left_id == id {
                            let v = lower_string_self_append(ctx, *id, right)?;
                            emit_shadow_slot_update_for_expr(ctx, *id, &v, value);
                            super::record_native_arena_owner_assignment(ctx, *id, value.as_ref());
                            return Ok(v);
                        }
                    }

                    // `flatten_string_add_chain` applies the same soundness
                    // rules as the ordinary n-way fold. In particular, it
                    // stops before an Add whose operands do not guarantee
                    // string semantics, so splitting after the leading local
                    // cannot change a numeric `+` into concatenation.
                    let in_loop = ctx
                        .loop_targets
                        .iter()
                        .any(|(continue_label, _, _)| !continue_label.is_empty());
                    let accumulator_parts = if in_loop {
                        flatten_string_add_chain(ctx, left, right).filter(|parts| {
                            parts.len() >= 3
                                && matches!(parts[0], Expr::LocalGet(left_id) if left_id == id)
                        })
                    } else {
                        None
                    };
                    if let Some(parts) = accumulator_parts {
                        let v = lower_string_self_append_chain(ctx, *id, &parts[1..])?;
                        emit_shadow_slot_update_for_expr(ctx, *id, &v, value);
                        super::record_native_arena_owner_assignment(ctx, *id, value.as_ref());
                        return Ok(v);
                    }
                }
            }

            // Issue #49: integer-arithmetic fast path. When the target has an
            // i32 slot (i.e. it's in `integer_locals`) and every leaf of the
            // rhs can be sourced in i32, emit the whole rhs as i32 and store
            // directly to the i32 slot. Skips the `sitofp→...fadd/fmul...→
            // fptosi` round-trip that the fp path otherwise forces on every
            // `acc = acc + byte * k` iteration. The double slot is maintained
            // via one sitofp per write so non-int readers (e.g. `acc / K`)
            // still see the current value.
            if let Some(i32_slot) = ctx.i32_counter_slots.get(id).cloned() {
                let structurally_i32 = can_lower_expr_as_i32_in_current_region(ctx, value);
                // Within a stable-packed numeric clone, when `x` is a
                // canonical raw Number, `x | 0` may feed the canonical i32
                // slot directly: materializing a double here only to convert
                // it back would duplicate the spec ToInt32. Keep BOTH gates
                // explicit. Declared Number types are erased, so a local can
                // still hold a String or BigInt at runtime; and other loop
                // clones own narrower indexed-load contracts that their
                // existing assignment lowering must continue to see.
                //
                // The canonical gate is a claim about the VALUE the ordinary
                // lowering of `x` produces, NOT a licence to re-evaluate `x`'s
                // operands natively — see the lowering below.
                let explicit_numeric_toint32 = matches!(
                    value.as_ref(),
                    Expr::Binary {
                        op: BinaryOp::BitOr,
                        left,
                        right,
                        ..
                    } if matches!(right.as_ref(), Expr::Integer(0))
                        && crate::type_analysis::expr_produces_canonical_raw_f64(ctx, left)
                ) && !ctx.stable_packed_loop_facts.is_empty();
                if !ctx.closure_captures.contains_key(id)
                    && !(ctx.boxed_vars.contains(id) && !ctx.module_globals.contains_key(id))
                    && (structurally_i32 || explicit_numeric_toint32)
                {
                    let v_i32 = if structurally_i32 {
                        lower_expr_as_i32(ctx, value)?
                    } else {
                        // `expr_produces_canonical_raw_f64` vouches for the
                        // RESULT of `x | 0`; it says nothing about `x`'s
                        // OPERANDS. Lowering the tree as native i32 would let
                        // `lower_expr_native_i32`'s i32-chain arm `fptosi`
                        // every operand it cannot lower natively — turning
                        // `h ^ recv.charCodeAt(i)` on an unproven receiver into
                        // an inline `xor i32` over whatever that method
                        // returned, so a BigInt silently yields garbage instead
                        // of the spec's TypeError (the #7773 family: only a
                        // live guard is a proof, an inferred type is not).
                        //
                        // Lower through `lower_expr` instead. `expr/binary.rs`
                        // applies `is_provably_not_bigint` PER OPERAND, so a
                        // proven tree still gets the inline `xor i32`/`or i32`
                        // and an unproven one keeps the BigInt-aware
                        // `js_dynamic_bit*` helper. The trailing conversion is
                        // free: `x | 0` always lowers to `sitofp i32`, so
                        // instcombine folds `trunc(fptosi(sitofp(v)))` back to
                        // `v` — and that `sitofp` is exactly why `toint32_fast`
                        // (whose contract is a known-finite input) is the right
                        // conversion here.
                        let d = lower_expr(ctx, value)?;
                        ctx.block().toint32_fast(&d)
                    };
                    let unsigned_i32 = ctx.unsigned_i32_locals.contains(id);
                    let blk = ctx.block();
                    blk.store(I32, &v_i32, &i32_slot);
                    let v_dbl = if unsigned_i32 {
                        blk.uitofp(I32, &v_i32, DOUBLE)
                    } else {
                        blk.sitofp(I32, &v_i32, DOUBLE)
                    };
                    // Repsel Phase 1: a canonical-i32 local has no double slot
                    // to mirror (the `ctx.locals` lookup below misses) and its
                    // shadow slot is never bound, so no clear is needed — the
                    // materialized `v_dbl` above only serves as the assignment
                    // expression's value (DCE'd when discarded).
                    let is_canonical = ctx.local_slot_reps.contains_key(id);
                    if let Some(slot) = ctx.locals.get(id).cloned() {
                        ctx.block().store(DOUBLE, &v_dbl, &slot);
                    } else if let Some(global_name) = ctx.module_globals.get(id).cloned() {
                        let g_ref = format!("@{}", global_name);
                        // GC_STORE_AUDIT(ROOT): module global slot is registered as a mutable GC root.
                        emit_root_nanbox_store_on_block(ctx.block(), &v_dbl, &g_ref);
                    }
                    if !is_canonical {
                        if let Some(slot_idx) = ctx.shadow_slot_map.get(id).copied() {
                            emit_shadow_slot_clear(ctx, slot_idx);
                        }
                    }
                    super::record_native_arena_owner_assignment(ctx, *id, value.as_ref());
                    super::record_int_facts_for_local_set(ctx, *id, value);
                    return Ok(v_dbl);
                }
            }

            let v = lower_expr(ctx, value)?;
            // `target = source` creates the same string-buffer alias as a
            // `let target = source` initializer. The declaration path has
            // demoted this shape since #5552, but assignment aliases were
            // previously missed. Async lowering expresses mid-body snapshot
            // variables as LocalSet, making that gap observable as the saved
            // string growing in place with its boxed accumulator (#8432).
            if matches!(value.as_ref(), Expr::LocalGet(source_id) if source_id != id) {
                super::helpers::emit_string_addref_if_heap_string(ctx, &v);
            }
            // Closure captures first (write through the runtime), then
            // locals, then module globals.
            if let Some(&capture_idx) = ctx.closure_captures.get(id) {
                let idx_str = capture_idx.to_string();
                // Boxed captured var: read the box pointer from the
                // capture slot, then js_box_set_bits to update the shared
                // cell. Do NOT overwrite the capture slot — it holds
                // the box pointer, not the value.
                if ctx.boxed_vars.contains(id) {
                    if let Some(capture) = ctx.trusted_box_capture_ptrs.get(id).cloned() {
                        let v_bits = ctx.block().bitcast_double_to_i64(&v);
                        ctx.block().store(I64, &v_bits, &capture.ptr);
                        // Gen-GC Phase C2: barrier — box is the parent.
                        emit_write_barrier(ctx, &capture.bits, &v_bits);
                    } else {
                        let closure_ptr =
                            super::current_closure_ptr_value(ctx, "captured boxed local set")?;
                        let setter = if ctx.trusted_box_captures {
                            "js_box_set_bits_trusted_no_barrier"
                        } else {
                            "js_box_set_bits"
                        };
                        let blk = ctx.block();
                        let box_ptr = blk.call(
                            I64,
                            "js_closure_get_capture_bits",
                            &[(I64, &closure_ptr), (I32, &idx_str)],
                        );
                        let v_bits = blk.bitcast_double_to_i64(&v);
                        blk.call_void(setter, &[(I64, &box_ptr), (I64, &v_bits)]);
                        // Gen-GC Phase C2: barrier — box is the parent.
                        emit_write_barrier(ctx, &box_ptr, &v_bits);
                    }
                } else {
                    let closure_ptr = super::current_closure_ptr_value(ctx, "captured local set")?;
                    let v_bits = ctx.block().bitcast_double_to_i64(&v);
                    ctx.block().call_void(
                        "js_closure_set_capture_bits",
                        &[(I64, &closure_ptr), (I32, &idx_str), (I64, &v_bits)],
                    );
                    // Gen-GC Phase C2: barrier — closure is the parent.
                    emit_write_barrier(ctx, &closure_ptr, &v_bits);
                }
            } else if ctx.boxed_vars.contains(id) && !ctx.module_globals.contains_key(id) {
                // Box path — only for non-global locals. Module globals
                // have their own shared storage and don't need boxing.
                // Without the !module_globals guard, closures that
                // modify a module-level variable would silently skip
                // the store (ctx.locals doesn't have the global's slot).
                if let Some(slot) = ctx.locals.get(id).cloned() {
                    let blk = ctx.block();
                    let box_ptr = blk.load(I64, &slot);
                    let v_bits = blk.bitcast_double_to_i64(&v);
                    blk.call_void("js_box_set_bits", &[(I64, &box_ptr), (I64, &v_bits)]);
                    // Gen-GC Phase C2: barrier — box is the parent (mirror the
                    // captured-box path above; an old box can else miss a young
                    // object/string/array value).
                    emit_write_barrier(ctx, &box_ptr, &v_bits);
                }
            } else if crate::expr::store_canonical_local_from_double(ctx, *id, &v, Some(value)) {
                // Repsel Phase 1: canonical-i32 local — the NaN-safe helper
                // stored the value into the (only) i32 slot. No double store,
                // no shadow-frame traffic (the slot is never bound: the value
                // is a number, never a pointer).
            } else if let Some(slot) = ctx.locals.get(id).cloned() {
                ctx.block().store(DOUBLE, &v, &slot);
                // Gen-GC Phase A sub-phase 3b: mirror pointer-typed
                // writes into the shadow frame. See stmt.rs::Let
                // for the allocation-site mirror; LocalSet is the
                // reassignment-site mirror.
                emit_shadow_slot_update_for_expr(ctx, *id, &v, value);
                // Mirror to the parallel i32 slot allocated for int32-stable
                // locals (issue #48). Without this, the i32 slot would go
                // stale on every `sum = (sum + i) | 0` write.
                // Use fptosi→i64 + trunc→i32 to safely handle unsigned values
                // (e.g. xorshift state `s = ... >>> 0` where double > INT32_MAX).
                if let Some(i32_slot) = ctx.i32_counter_slots.get(id).cloned() {
                    let v_i64 = ctx.block().fptosi(DOUBLE, &v, crate::types::I64);
                    let v_i32 = ctx.block().trunc(crate::types::I64, &v_i64, I32);
                    ctx.block().store(I32, &v_i32, &i32_slot);
                }
            } else if let Some(global_name) = ctx.module_globals.get(id).cloned() {
                let g_ref = format!("@{}", global_name);
                // GC_STORE_AUDIT(ROOT): module global slot is registered as a mutable GC root.
                emit_root_nanbox_store_on_block(ctx.block(), &v, &g_ref);
            }
            super::record_native_arena_owner_assignment(ctx, *id, value.as_ref());
            if ctx.buffer_view_slots.contains_key(id)
                || matches!(
                    value.as_ref(),
                    Expr::BufferAlloc { .. } | Expr::BufferAllocUnsafe(_) | Expr::Uint8ArrayNew(_)
                )
            {
                super::update_buffer_view_for_assignment(ctx, *id, value, &v);
            }
            super::record_int_facts_for_local_set(ctx, *id, value);
            // Soft fallback: drop the store on the floor for missing
            // locals. See LocalGet for the rationale.
            Ok(v)
        }

        // `i++` / `++i` / `i--` / `--i`. Postfix returns the OLD value,
        // prefix returns the NEW value. Closure captures, locals, then
        // module globals.
        Expr::Update { id, op, prefix } => {
            super::invalidate_local_write_facts(ctx, *id);
            // Spec ToNumeric: `x++`/`++x` coerce the operand (ToNumber on
            // bool/null/string/object-with-valueOf, or BigInt passthrough)
            // before the add/sub, and the *returned* value (postfix) is the
            // coerced numeric, not the original boxed operand. Statically-
            // integer loop counters already hold a real f64 and skip the call
            // to keep the hot path a single `fadd`. The slow path routes
            // through `js_to_numeric`/`js_numeric_step` so a BigInt operand
            // stays a BigInt (`let i = 10n; i++` → `11n`, not the Number `11`
            // which would make a later `i + 87n` throw a mixed-type
            // TypeError; test262 BigInt/prototype/toString/a-z).
            let needs_numeric_coerce =
                !ctx.integer_locals.contains(id) && !ctx.unsigned_i32_locals.contains(id);
            let is_increment_arg = match op {
                UpdateOp::Increment => "1",
                UpdateOp::Decrement => "0",
            };
            let coerce_old = |blk: &mut crate::block::LlBlock, raw: &str| -> String {
                if needs_numeric_coerce {
                    blk.call(DOUBLE, "js_to_numeric", &[(DOUBLE, raw)])
                } else {
                    raw.to_string()
                }
            };
            let step_new = |blk: &mut crate::block::LlBlock, old_num: &str| -> String {
                if needs_numeric_coerce {
                    blk.call(
                        DOUBLE,
                        "js_numeric_step",
                        &[(DOUBLE, old_num), (I32, is_increment_arg)],
                    )
                } else {
                    match op {
                        UpdateOp::Increment => blk.fadd(old_num, "1.0"),
                        UpdateOp::Decrement => blk.fsub(old_num, "1.0"),
                    }
                }
            };
            // Closure capture path: runtime get + add/sub + runtime set.
            if let Some(&capture_idx) = ctx.closure_captures.get(id) {
                let idx_str = capture_idx.to_string();
                // Boxed captured var: deref box bits, modify, store back.
                //
                // `box_ptr` deliberately survives the `coerce_old`/`step_new`
                // calls below even though those can collect: a box is
                // `std::alloc::alloc`'d by `js_box_alloc_bits`, its memory is
                // never handed back to the allocator, and it is never relocated
                // (`scan_box_roots_mut` rewrites the JSValue *inside* the box,
                // not the box's address), so an address read before a
                // collection still names the same live cell after it. The
                // closure pointer has no such guarantee, which is why the
                // non-boxed arm below re-reads it.
                //
                // #8208 added a release/reuse path for completed async
                // activations, so "never freed" is no longer literally true and
                // the argument is now stated on the properties that ARE:
                // (1) cell memory is never returned to the allocator, so the
                // address never stops naming 8 bytes of box cell; (2) the
                // runtime counts each raw box capture; and (3) a
                // terminal cell stays live until both queued/running steps and
                // capturing closures are gone. A capture from an enclosing
                // activation therefore cannot become reusable inside the
                // nested user frame `coerce_old`/`step_new` may enter.
                if ctx.boxed_vars.contains(id) {
                    if let Some(capture) = ctx.trusted_box_capture_ptrs.get(id).cloned() {
                        let old_bits = load_trusted_box_capture_bits(ctx, &capture);
                        let old = ctx.block().bitcast_i64_to_double(&old_bits);
                        if needs_numeric_coerce && ctx.versioned_loop_deopt_context.is_some() {
                            let is_number = crate::stmt::emit_js_value_is_number(ctx, &old);
                            let fast_idx = ctx.new_block("versioned_update.number");
                            let slow_idx = ctx.new_block("versioned_update.tonumeric");
                            let merge_idx = ctx.new_block("versioned_update.merge");
                            let fast_label = ctx.block_label(fast_idx);
                            let slow_label = ctx.block_label(slow_idx);
                            let merge_label = ctx.block_label(merge_idx);
                            ctx.block().cond_br(&is_number, &fast_label, &slow_label);

                            ctx.current_block = fast_idx;
                            let fast_new = match op {
                                UpdateOp::Increment => ctx.block().fadd(&old, "1.0"),
                                UpdateOp::Decrement => ctx.block().fsub(&old, "1.0"),
                            };
                            let fast_new_bits = ctx.block().bitcast_double_to_i64(&fast_new);
                            ctx.block().store(I64, &fast_new_bits, &capture.ptr);
                            let fast_end = ctx.block().label.clone();
                            ctx.block().br(&merge_label);

                            ctx.current_block = slow_idx;
                            // ToNumeric can invoke user code and collect. Mark
                            // the exact resume index before it becomes
                            // observable; the caller exits after this update
                            // and resumes the guarded loop at the next entity.
                            crate::expr::emit_versioned_loop_callback_deopt(ctx);
                            let slow_old = coerce_old(ctx.block(), &old);
                            let slow_new = step_new(ctx.block(), &slow_old);
                            let slow_new_bits = ctx.block().bitcast_double_to_i64(&slow_new);
                            ctx.block().store(I64, &slow_new_bits, &capture.ptr);
                            // Only the cold arm can produce a BigInt pointer.
                            emit_write_barrier(ctx, &capture.bits, &slow_new_bits);
                            let slow_end = ctx.block().label.clone();
                            ctx.block().br(&merge_label);

                            ctx.current_block = merge_idx;
                            return Ok(ctx.block().phi(
                                DOUBLE,
                                &[
                                    (if *prefix { &fast_new } else { &old }, &fast_end),
                                    (if *prefix { &slow_new } else { &slow_old }, &slow_end),
                                ],
                            ));
                        }
                        let old = coerce_old(ctx.block(), &old);
                        let new = step_new(ctx.block(), &old);
                        let new_bits = ctx.block().bitcast_double_to_i64(&new);
                        ctx.block().store(I64, &new_bits, &capture.ptr);
                        // Gen-GC Phase C2: `++`/`--` on a BigInt yields a heap
                        // pointer via js_numeric_step — barrier the box parent.
                        emit_write_barrier(ctx, &capture.bits, &new_bits);
                        return Ok(if *prefix { new } else { old });
                    }
                    let closure_ptr =
                        super::current_closure_ptr_value(ctx, "captured boxed local update")?;
                    let getter = if ctx.trusted_box_captures {
                        "js_box_get_bits_trusted"
                    } else {
                        "js_box_get_bits"
                    };
                    let setter = if ctx.trusted_box_captures {
                        "js_box_set_bits_trusted_no_barrier"
                    } else {
                        "js_box_set_bits"
                    };
                    let blk = ctx.block();
                    let box_ptr = blk.call(
                        I64,
                        "js_closure_get_capture_bits",
                        &[(I64, &closure_ptr), (I32, &idx_str)],
                    );
                    let old_bits = blk.call(I64, getter, &[(I64, &box_ptr)]);
                    let old = blk.bitcast_i64_to_double(&old_bits);
                    let old = coerce_old(blk, &old);
                    let new = step_new(blk, &old);
                    let new_bits = blk.bitcast_double_to_i64(&new);
                    blk.call_void(setter, &[(I64, &box_ptr), (I64, &new_bits)]);
                    // Gen-GC Phase C2: `++`/`--` on a BigInt yields a heap
                    // pointer via js_numeric_step — barrier the box parent.
                    emit_write_barrier(ctx, &box_ptr, &new_bits);
                    return Ok(if *prefix { new } else { old });
                }
                let closure_ptr = super::current_closure_ptr_value(ctx, "captured local update")?;
                let old_bits = ctx.block().call(
                    I64,
                    "js_closure_get_capture_bits",
                    &[(I64, &closure_ptr), (I32, &idx_str)],
                );
                let old = ctx.block().bitcast_i64_to_double(&old_bits);
                let blk = ctx.block();
                let old = coerce_old(blk, &old);
                let new = step_new(blk, &old);
                let new_bits = blk.bitcast_double_to_i64(&new);
                // #7055: `coerce_old` emits `js_to_numeric`, which runs a user
                // `valueOf` — arbitrary JS, including allocating loops that
                // reach a `js_gc_loop_safepoint` and relocate this very
                // closure. `js_closure_set_capture_bits` does NOT validate its
                // pointer (unlike `js_closure_get_capture_bits`, which bounds-
                // checks and returns 0), so writing through a pre-coercion
                // `closure_ptr` would store into whatever the mutator has since
                // put at that recycled from-space address. Re-read the rooted
                // slot after the coercion; the collector has rewritten it.
                // Only the coercing path pays the reload — a statically
                // integer-typed counter emits no call in between.
                let closure_ptr = if needs_numeric_coerce {
                    super::current_closure_ptr_value(ctx, "captured local update")?
                } else {
                    closure_ptr
                };
                ctx.block().call_void(
                    "js_closure_set_capture_bits",
                    &[(I64, &closure_ptr), (I32, &idx_str), (I64, &new_bits)],
                );
                // Gen-GC Phase C2: barrier — closure is the parent (BigInt
                // `++`/`--` can store a young heap pointer).
                emit_write_barrier(ctx, &closure_ptr, &new_bits);
                return Ok(if *prefix { new } else { old });
            }
            // Boxed enclosing-scope var: load slot (box ptr), deref,
            // increment, box_set_bits. Skip for module globals (they
            // have their own shared storage).
            if ctx.boxed_vars.contains(id) && !ctx.module_globals.contains_key(id) {
                if let Some(slot) = ctx.locals.get(id).cloned() {
                    let blk = ctx.block();
                    let box_ptr = blk.load(I64, &slot);
                    let old_bits = blk.call(I64, "js_box_get_bits", &[(I64, &box_ptr)]);
                    let old = blk.bitcast_i64_to_double(&old_bits);
                    let old = coerce_old(blk, &old);
                    let new = step_new(blk, &old);
                    let new_bits = blk.bitcast_double_to_i64(&new);
                    blk.call_void("js_box_set_bits", &[(I64, &box_ptr), (I64, &new_bits)]);
                    // Gen-GC Phase C2: barrier — box is the parent (BigInt
                    // `++`/`--` can store a young heap pointer).
                    emit_write_barrier(ctx, &box_ptr, &new_bits);
                    return Ok(if *prefix { new } else { old });
                }
            }
            // Repsel Phase 1: canonical-i32 local — the whole update happens
            // in the i32 slot (`load` / `add ±1` / `store`), which post-`-O3`
            // promotes to a clean `phi i32` induction variable. The boxed
            // double views exist only as the expression's value; LLVM DCEs
            // them when the update is a statement. `++`/`--` on an unsigned
            // (`>>> 0`-written) local never qualifies for a slot (the
            // collector disqualifies Update writes), so the rep here is
            // always `I32` — materialize with `sitofp`.
            if let Some((i32_slot, _rep)) = crate::expr::canonical_local_i32_slot(ctx, *id) {
                let blk = ctx.block();
                let old_i32 = blk.load(I32, &i32_slot);
                let delta = match op {
                    UpdateOp::Increment => "1",
                    UpdateOp::Decrement => "-1",
                };
                let new_i32 = blk.add(I32, &old_i32, delta);
                blk.store(I32, &new_i32, &i32_slot);
                let old = blk.sitofp(I32, &old_i32, DOUBLE);
                let new = blk.sitofp(I32, &new_i32, DOUBLE);
                super::record_int_facts_for_update(ctx, *id, *op);
                return Ok(if *prefix { new } else { old });
            }
            let (storage, storage_is_root) = if let Some(slot) = ctx.locals.get(id).cloned() {
                (slot, false)
            } else if let Some(global_name) = ctx.module_globals.get(id).cloned() {
                (format!("@{}", global_name), true)
            } else {
                // Soft fallback: silently increment a throwaway value.
                return Ok(double_literal(0.0));
            };
            let blk = ctx.block();
            let old = blk.load(DOUBLE, &storage);
            let old = coerce_old(blk, &old);
            let new = step_new(blk, &old);
            if storage_is_root {
                // Module globals are registered mutable GC roots and route
                // through the root helper; the raw store below is stack-only.
                emit_root_nanbox_store_on_block(blk, &new, &storage);
            } else {
                // GC_STORE_AUDIT(STACK): update writes a function-local alloca;
                // module globals use the root helper.
                blk.store(DOUBLE, &new, &storage);
            }
            // Keep the parallel i32 counter slot in sync (if active).
            // This costs one `add i32, 1` per iteration but saves a
            // `fptosi double → i32` on every IndexGet/IndexSet use.
            if let Some(i32_slot) = ctx.i32_counter_slots.get(id).cloned() {
                let blk = ctx.block();
                let old_i32 = blk.load(I32, &i32_slot);
                let delta = match op {
                    UpdateOp::Increment => "1",
                    UpdateOp::Decrement => "-1",
                };
                let new_i32 = blk.add(I32, &old_i32, delta);
                blk.store(I32, &new_i32, &i32_slot);
            }
            super::record_int_facts_for_update(ctx, *id, *op);
            Ok(if *prefix { new } else { old })
        }

        // `Date.now()` — special HIR variant that lowers to a single FFI
        // call returning a `double` (milliseconds since UNIX epoch as
        // produced by `js_date_now` in `perry-runtime/src/date.rs`).
        Expr::DateNow => Ok(ctx.block().call(DOUBLE, "js_date_now", &[])),
        // #7760: one volatile `i8` load, the same shape the array index fast
        // path uses for its own invalidation flag. Emitted once at the entry of
        // a `for…of` over a proven array to choose between the index loop and
        // the lazy iterator-protocol loop, so the fast arm pays a load and a
        // predictable branch PER LOOP, never per iteration.
        Expr::ArrayIterationPatched => {
            let blk = ctx.block();
            let flag = blk.load_volatile(crate::types::I8, "@PERRY_ARRAY_PROTO_ITERATOR_PATCHED");
            let widened = blk.zext(crate::types::I8, &flag, crate::types::I32);
            Ok(super::i32_bool_to_nanbox(blk, &widened))
        }

        // -------- Arithmetic --------
        // String concatenation (Phase B): if Add receives operands where
        // either side is statically a string, route through string concat.
        // - both strings → `lower_string_concat` (inline bitcast+and unbox)
        // - one string + one non-string → `lower_string_coerce_concat`
        //   (the non-string side passes through `js_jsvalue_to_string`
        //   which dispatches on the NaN tag at runtime)
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
