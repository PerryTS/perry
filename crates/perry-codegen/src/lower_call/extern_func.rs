//! Cross-module function call via `Expr::ExternFuncRef` — covers
//! built-in extern names (setTimeout, setInterval, gc, jsx, …),
//! perry/system + perry/updater + perry/background dispatch via the
//! `lower_perry_ui_table_call` machinery, V8-fallback bridge calls,
//! and the generic `perry_fn_<src>__<name>` consumer-prefix path.

use anyhow::Result;
use perry_hir::Expr;
use perry_types::Type as HirType;

use crate::expr::{lower_expr, nanbox_pointer_inline, nanbox_string_inline, unbox_to_i64, FnCtx};
use crate::nanbox::{double_literal, POINTER_MASK_I64};
use crate::type_analysis::{is_array_expr, is_string_expr};
use crate::types::{DOUBLE, I32, I64, I8, PTR};

use super::{
    lower_perry_ui_table_call, perry_background_table_lookup, perry_system_table_lookup,
    perry_updater_table_lookup, try_rewrite_perry_tui_jsx_intrinsic,
};

pub fn try_lower_extern_func_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    // Cross-module function call via ExternFuncRef. The HIR carries the
    // function name; we look up the source module's prefix in
    // `import_function_prefixes` (built by the CLI from hir.imports) and
    // generate `perry_fn_<source_prefix>__<name>`. The function is
    // declared in the OTHER module's compilation; here we just emit a
    // direct LLVM call to its scoped name and the system linker
    // resolves the symbol when the .o files are linked together.
    let Expr::ExternFuncRef {
        name,
        return_type: ext_return_type,
        ..
    } = callee
    else {
        return Ok(None);
    };
    // Issue #1317: when `name` is bound to a named import from a Node
    // submodule Perry recognizes but doesn't yet back with a real impl
    // (`node:timers/promises`, `node:readline/promises`,
    // `node:stream/promises`, `node:stream/consumers`, `node:sys`), the
    // shadowing import collides with global names like `setTimeout`/
    // `setInterval`/`setImmediate`. Without this guard, e.g.
    // `import { setTimeout } from "node:timers/promises";
    //  await setTimeout(1, { a: 1 }, { ref: false })`
    // would lower to the global `setTimeout(fn, delay, ...args)` fast
    // path below — handing `1` to `js_set_timeout_callback_args` as if
    // it were a callback handle and segfaulting on the next dispatch.
    // Skip the fast-path table so the call falls through to the regular
    // closure-dispatch lowering, which invokes the named-import
    // singleton's thunk (raising the "not yet implemented" error).
    if ctx.import_function_node_submodule.contains_key(name) {
        return Ok(None);
    }
    match name.as_str() {
        "setTimeout" if args.len() == 2 => {
            let cb_box = lower_expr(ctx, &args[0])?;
            let delay_box = lower_expr(ctx, &args[1])?;
            let blk = ctx.block();
            let cb_handle = unbox_to_i64(blk, &cb_box);
            let id = blk.call(
                I64,
                "js_set_timeout_callback",
                &[(I64, &cb_handle), (DOUBLE, &delay_box)],
            );
            return Ok(Some(nanbox_pointer_inline(blk, &id)));
        }
        "setImmediate" if !args.is_empty() => {
            let cb_box = lower_expr(ctx, &args[0])?;
            if args.len() == 1 {
                let blk = ctx.block();
                let cb_handle = unbox_to_i64(blk, &cb_box);
                let id = blk.call(I64, "js_set_immediate_callback", &[(I64, &cb_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &id)));
            }

            let n = args.len() - 1;
            let buf = ctx.func.alloca_entry_array(DOUBLE, n);
            for (i, a) in args.iter().skip(1).enumerate() {
                let v = lower_expr(ctx, a)?;
                let blk = ctx.block();
                let slot = blk.gep(DOUBLE, &buf, &[(I64, &format!("{}", i))]);
                blk.store(DOUBLE, &v, &slot);
            }
            let ptr_reg = ctx.block().next_reg();
            ctx.block().emit_raw(format!(
                "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                ptr_reg, n, buf
            ));
            let blk = ctx.block();
            let cb_handle = unbox_to_i64(blk, &cb_box);
            let id = blk.call(
                I64,
                "js_set_immediate_callback_args",
                &[(I64, &cb_handle), (PTR, &ptr_reg), (I32, &n.to_string())],
            );
            return Ok(Some(nanbox_pointer_inline(blk, &id)));
        }
        // Refs #665: `setTimeout(fn, delay, ...args)` — JS spec forwards
        // the trailing args to `fn` when the timer fires. Pack them into
        // a stack buffer of doubles and hand off to the varargs runtime
        // entry. Used by Promise-executor patterns like
        // `setTimeout(resolve, delay, res)` (rate-limiter-flexible's
        // `RateLimiterMemory.consume` is the discovering call site).
        "setTimeout" if args.len() >= 3 => {
            let cb_box = lower_expr(ctx, &args[0])?;
            let delay_box = lower_expr(ctx, &args[1])?;
            let n = args.len() - 2;
            let buf = ctx.func.alloca_entry_array(DOUBLE, n);
            for (i, a) in args.iter().skip(2).enumerate() {
                let v = lower_expr(ctx, a)?;
                let blk = ctx.block();
                let slot = blk.gep(DOUBLE, &buf, &[(I64, &format!("{}", i))]);
                blk.store(DOUBLE, &v, &slot);
            }
            let ptr_reg = ctx.block().next_reg();
            ctx.block().emit_raw(format!(
                "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                ptr_reg, n, buf
            ));
            let blk = ctx.block();
            let cb_handle = unbox_to_i64(blk, &cb_box);
            let id = blk.call(
                I64,
                "js_set_timeout_callback_args",
                &[
                    (I64, &cb_handle),
                    (DOUBLE, &delay_box),
                    (crate::types::PTR, &ptr_reg),
                    (I32, &n.to_string()),
                ],
            );
            return Ok(Some(nanbox_pointer_inline(blk, &id)));
        }
        "setInterval" if args.len() == 2 => {
            let cb_box = lower_expr(ctx, &args[0])?;
            let delay_box = lower_expr(ctx, &args[1])?;
            let blk = ctx.block();
            let cb_handle = unbox_to_i64(blk, &cb_box);
            let id = blk.call(
                I64,
                "setInterval",
                &[(I64, &cb_handle), (DOUBLE, &delay_box)],
            );
            return Ok(Some(nanbox_pointer_inline(blk, &id)));
        }
        "clearTimeout" if args.len() == 1 => {
            // Pass the raw NaN-boxed arg so the runtime accepts both the
            // handle and its primitive numeric id (`clearTimeout(+t)`, #1213).
            let id_box = lower_expr(ctx, &args[0])?;
            ctx.block()
                .call_void("js_clear_timeout_value", &[(DOUBLE, &id_box)]);
            return Ok(Some(double_literal(f64::from_bits(
                crate::nanbox::TAG_UNDEFINED,
            ))));
        }
        "clearInterval" if args.len() == 1 => {
            let id_box = lower_expr(ctx, &args[0])?;
            ctx.block()
                .call_void("js_clear_interval_value", &[(DOUBLE, &id_box)]);
            return Ok(Some(double_literal(f64::from_bits(
                crate::nanbox::TAG_UNDEFINED,
            ))));
        }
        "gc" => {
            ctx.block().call_void("js_gc_collect", &[]);
            return Ok(Some(double_literal(f64::from_bits(
                crate::nanbox::TAG_UNDEFINED,
            ))));
        }
        "getAppVersion" if args.is_empty() => {
            let version = ctx.app_metadata.version.clone();
            let idx = ctx.strings.intern(&version);
            let handle_global = format!("@{}", ctx.strings.entry(idx).handle_global);
            return Ok(Some(ctx.block().load(DOUBLE, &handle_global)));
        }
        "getAppBuildNumber" if args.is_empty() => {
            return Ok(Some(double_literal(ctx.app_metadata.build_number as f64)));
        }
        "getBundleId" if args.is_empty() => {
            let bundle_id = ctx.app_metadata.bundle_id.clone();
            let idx = ctx.strings.intern(&bundle_id);
            let handle_global = format!("@{}", ctx.strings.entry(idx).handle_global);
            return Ok(Some(ctx.block().load(DOUBLE, &handle_global)));
        }
        // JSX runtime calls: `jsx(type, props)` and `jsxs(type, props)`.
        // The HIR lowers <div>…</div> to ExternFuncRef { name: "jsx" } and
        // <div><a/><b/></div> (multiple children) to "jsxs".  The first arg
        // is the element type (a string literal for HTML tags, or a NaN-boxed
        // function/class reference for components); the second arg is a
        // NaN-boxed props object (or TAG_NULL).  Both are passed as DOUBLE so
        // the ABI is uniform regardless of whether the type arg is a string or
        // a component reference — avoiding the PTR vs DOUBLE divergence that
        // the generic ExternFuncRef path would otherwise produce for string
        // literals.  The runtime stubs `js_jsx`/`js_jsxs` are no-op link
        // stubs that return TAG_UNDEFINED; real JSX rendering should be
        // implemented by importing a JSX runtime package (e.g. react or
        // preact) via the `perry.compilePackages` mechanism.
        //
        // perry/tui JSX intrinsic rewriter (#689). When the first arg
        // is `ExternFuncRef { name: "__perry_jsx_intrinsic::<mod>::<method>__" }`
        // (the HIR's marker for `<Box>` / `<Text>` resolved against a
        // native module — see `crates/perry-hir/src/jsx.rs`), bypass
        // the runtime `js_jsx` adapter entirely and route the call
        // through `lower_native_method_call` so the JSX form lowers
        // to the same widget builder the function-call form would.
        // Today this covers Box + Text from `perry/tui`; other
        // intrinsics (Spacer / Input / Spinner / List / Select /
        // ProgressBar / Table / Tabs / TextArea) are listed as
        // follow-up scope in #689 and continue to fall through to
        // `js_jsx` (returns TAG_UNDEFINED until the rewriter is
        // extended).
        "jsx" | "jsxs" => {
            if let Some(call) = try_rewrite_perry_tui_jsx_intrinsic(ctx, name == "jsxs", args)? {
                return Ok(Some(call));
            }
            let runtime_fn = if name == "jsx" { "js_jsx" } else { "js_jsxs" };
            let mut lowered: Vec<String> = Vec::with_capacity(args.len());
            for a in args {
                lowered.push(lower_expr(ctx, a)?);
            }
            let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
            return Ok(Some(ctx.block().call(DOUBLE, runtime_fn, &arg_slices)));
        }
        _ => {}
    }
    // Issue #841: direct call against a named import from one of the
    // five recognized Node submodules (`import { pipeline } from
    // "node:stream/promises"; pipeline()`). The HIR registers
    // `pipeline` as an imported func; without this routing the
    // catch-all below tries to emit a bare LLVM call to `@pipeline`
    // and the linker errors with `Undefined symbols: _pipeline`.
    //
    // Route to the value-form singleton getter and then dispatch
    // through the closure-call machinery — the singleton's thunk
    // throws an "is not yet implemented" Error. Real impls are
    // tracked separately under #793.
    if let Some((submod_key, exported_name)) = ctx.import_function_node_submodule.get(name).cloned()
    {
        let mut lowered_args = Vec::with_capacity(args.len());
        for a in args {
            lowered_args.push(crate::expr::lower_expr(ctx, a)?);
        }
        let submod_label = crate::expr::emit_string_literal_global(ctx, &submod_key);
        let name_label = crate::expr::emit_string_literal_global(ctx, &exported_name);
        let submod_len = submod_key.len();
        let name_len = exported_name.len();
        ctx.pending_declares.push((
            "js_node_submodule_export_as_function".to_string(),
            DOUBLE,
            vec![PTR, I32, PTR, I32],
        ));
        let blk = ctx.block();
        let closure_value = blk.call(
            DOUBLE,
            "js_node_submodule_export_as_function",
            &[
                (PTR, &submod_label),
                (I32, &submod_len.to_string()),
                (PTR, &name_label),
                (I32, &name_len.to_string()),
            ],
        );
        // Drive through the closure-call machinery and preserve the user's
        // arguments. The original #841 surface-only fix discarded args because
        // all known submodule thunks threw/no-op'd. `node:diagnostics_channel`
        // now implements real `channel(name)`, `subscribe(name, cb)`, etc., so
        // named imports must receive their actual argument list.
        let blk = ctx.block();
        let closure_bits = blk.bitcast_double_to_i64(&closure_value);
        let closure_handle = blk.and(I64, &closure_bits, POINTER_MASK_I64);
        let call_name = format!("js_closure_call{}", lowered_args.len().min(16));
        let mut decl_types = vec![I64];
        decl_types.extend(std::iter::repeat(DOUBLE).take(lowered_args.len().min(16)));
        ctx.pending_declares
            .push((call_name.clone(), DOUBLE, decl_types));
        let mut call_args: Vec<(crate::types::LlvmType, String)> = vec![(I64, closure_handle)];
        for arg in lowered_args.into_iter().take(16) {
            call_args.push((DOUBLE, arg));
        }
        let arg_refs: Vec<(crate::types::LlvmType, &str)> =
            call_args.iter().map(|(t, s)| (*t, s.as_str())).collect();
        return Ok(Some(ctx.block().call(DOUBLE, &call_name, &arg_refs)));
    }
    // perry/system dispatch: map JS names (isDarkMode, getDeviceIdiom,
    // keychainSave, etc.) to their perry_system_* / perry_* C symbols.
    // These arrive as ExternFuncRef because perry/system imports aren't
    // lowered to NativeMethodCall in the HIR.
    if let Some(sig) = perry_system_table_lookup(name) {
        return Ok(Some(lower_perry_ui_table_call(ctx, sig, args)?));
    }
    // perry/updater dispatch: same shape as perry/system. Imports from
    // `perry/updater` arrive as ExternFuncRef; route by name to the
    // perry_updater_* runtime symbols in `perry-updater`.
    if let Some(sig) = perry_updater_table_lookup(name) {
        return Ok(Some(lower_perry_ui_table_call(ctx, sig, args)?));
    }
    // perry/background dispatch (issue #538): registerTask / schedule /
    // cancel from `perry/background`. Backed by perry_background_* in
    // libperry_ui_*.a (real impls on iOS + Android, no-op stubs
    // elsewhere). Same calling convention as perry/system.
    if let Some(sig) = perry_background_table_lookup(name) {
        return Ok(Some(lower_perry_ui_table_call(ctx, sig, args)?));
    }
    // Built-in runtime extern functions (`js_weakmap_set`,
    // `js_regexp_exec`, etc.) that start with `js_` are resolved
    // directly against the runtime library — bypass the import-
    // map lookup and emit a direct LLVM call with an f64/f64 ABI.
    // (The declarations are added centrally in runtime_decls.rs.)
    //
    // External `perry.nativeLibrary` packages commonly export their
    // symbols with the same `js_*` prefix. If the manifest declares
    // this name, let the native-library path below emit the call and
    // declaration from `ffi_signatures` instead of treating it as a
    // runtime builtin.
    if name.starts_with("js_") && !ctx.ffi_signatures.contains_key(name) {
        let mut lowered: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered.push(lower_expr(ctx, a)?);
        }
        let arg_slices: Vec<(crate::types::LlvmType, &str)> =
            lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
        return Ok(Some(ctx.block().call(DOUBLE, name, &arg_slices)));
    }
    // Issue #692: default-import call against an unresolved module.
    // `import sanitizeHtml from "sanitize-html"` (when sanitize-html
    // didn't resolve to a NativeCompiled module / perry-stdlib
    // binding) lowers `sanitizeHtml(x)` to `Call { callee:
    // ExternFuncRef { name: "default" } }` — the HIR's
    // register_imported_func uses the literal `"default"` as the
    // exported-name marker for default imports (lower.rs:3727).
    // Without a source_prefix, the catch-all below emitted a direct
    // LLVM call to the bare symbol `default`, and the system linker
    // failed with `undefined reference to 'default'`. Route to the
    // runtime stub instead: lower args for side effects (so closure
    // collection / string interning still happens), then call
    // `js_unresolved_default_call` which returns NaN-boxed undefined
    // and prints a one-shot diagnostic at runtime. The program now
    // links; the user gets a clear runtime signal rather than a
    // cryptic linker error.
    if name == "default" && !ctx.import_function_prefixes.contains_key(name) {
        for a in args {
            let _ = lower_expr(ctx, a)?;
        }
        return Ok(Some(ctx.block().call(
            DOUBLE,
            "js_unresolved_default_call",
            &[],
        )));
    }
    // Native library functions (bloom_draw_rect, bloom_init_window,
    // etc.) that aren't in the import map — emit a direct call so
    // the linker resolves them against the linked native .a library.
    // Previously these were silently dropped (returned 0.0), which
    // caused Bloom Engine games to render blank windows.
    //
    // #1110 (follow-up to #1085): a symbol declared in the source
    // package's `perry.nativeLibrary.functions` manifest is always
    // resolved against the linked static library, never via the
    // `perry_fn_<src>__<name>` wrapper (the source `.ts` is ambient
    // and emits no wrapper). Force the FFI-manifest path whenever
    // `ffi_signatures` knows the name, even if some other code path
    // accidentally registered an entry in `import_function_prefixes`
    // (re-export chains, namespace re-exports, etc. — anything that
    // doesn't go through the #1085 per-specifier skip ends up there).
    let force_ffi_path = ctx.ffi_signatures.contains_key(name);
    let prefix_lookup = if force_ffi_path {
        None
    } else {
        ctx.import_function_prefixes.get(name).cloned()
    };
    let Some(source_prefix) = prefix_lookup else {
        // Determine per-arg types: string args need to be unboxed
        // to raw `*const u8` pointers and passed as `ptr` so the
        // ARM64 ABI puts them in x-registers (not d-registers).
        // Without this, bloom_draw_text(text, x, y, ...) passes
        // the NaN-boxed string in d0 but the native function reads
        // x0 as a *const u8 → SIGSEGV.
        // Extern C functions use the platform C ABI. Perry stores
        // all values as `double`, but native C/Rust functions may
        // take a mix of i64 (pointers/handles) and f64 (floats).
        //
        // The LLVM IR declaration type determines ARM64 register
        // placement: i64 → x-register, double → d-register.
        //
        // When the FFI manifest (`ffi_signatures`) declares a param
        // as `"i64"`, lower it via `fptosi` to put the value in an
        // x-register. This is required for handle-typed params like
        // `view: *mut EditorView` — without it the C ABI reads a
        // garbage value out of x0/x1 since Perry put the handle in
        // d-registers.
        let manifest_sig = ctx.ffi_signatures.get(name).cloned();
        let mut lowered: Vec<String> = Vec::with_capacity(args.len());
        let mut arg_types: Vec<crate::types::LlvmType> = Vec::with_capacity(args.len());
        for (idx, a) in args.iter().enumerate() {
            let val = lower_expr(ctx, a)?;
            let manifest_kind: Option<&str> = manifest_sig
                .as_ref()
                .and_then(|(p, _)| p.get(idx).map(|s| s.as_str()));
            if is_string_expr(ctx, a) {
                let blk = ctx.block();
                let raw_ptr = blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &val)]);
                let ptr_val = blk.inttoptr(I64, &raw_ptr);
                lowered.push(ptr_val);
                arg_types.push(PTR);
            } else if is_array_expr(ctx, a) {
                let blk = ctx.block();
                let bits = blk.bitcast_double_to_i64(&val);
                let header_handle = blk.and(I64, &bits, POINTER_MASK_I64);
                let header_ptr = blk.inttoptr(I64, &header_handle);
                // Skip 8-byte ArrayHeader (u32 length + u32 capacity)
                // to reach the inline f64 data.
                let eight = "8".to_string();
                let data_ptr = blk.gep(I8, &header_ptr, &[(I64, &eight)]);
                lowered.push(data_ptr);
                arg_types.push(PTR);
            } else if matches!(manifest_kind, Some("i64")) {
                // Manifest declares this param as i64 → place in
                // x-register. JS numbers are stored as f64 directly
                // (a handle of `0x305b42a0c00` is the f64 value
                // 13190580238336.0, not a NaN-box payload), so
                // truncate via `fptosi` to recover the integer.
                let blk = ctx.block();
                let i = blk.fptosi(DOUBLE, &val, I64);
                lowered.push(i);
                arg_types.push(I64);
            } else {
                lowered.push(val);
                arg_types.push(DOUBLE);
            }
        }
        let arg_slices: Vec<(crate::types::LlvmType, &str)> = arg_types
            .iter()
            .zip(lowered.iter())
            .map(|(t, v)| (*t, v.as_str()))
            .collect();
        // Determine return type.
        //
        // Manifest `returns` field takes precedence over HIR heuristics:
        //
        //   "string" / "ptr"  → PTR return (*const u8 / *const StringHeader);
        //                       ptrtoint + NaN-box STRING_TAG. Use when the
        //                       Rust function is declared `-> *const u8`.
        //   "i64_str"         → I64 return (raw integer that IS a *StringHeader
        //                       address). NaN-box directly with STRING_TAG; no
        //                       sitofp. Use when the Rust function is declared
        //                       `-> i64` but the value is a string pointer.
        //   "i64"             → I64 return; sitofp → JS number. Use for opaque
        //                       handles / integers (`*mut View`, counts, etc.).
        //   "void"            → no return value.
        //   (absent)          → fall back to HIR ExternFuncRef.return_type and
        //                       the name-pattern heuristic below.
        let has_string_args = arg_types.contains(&PTR);
        let manifest_ret: Option<&str> = manifest_sig.as_ref().map(|(_, r)| r.as_str());
        // "i64_str": explicit opt-in for FFI functions that return a raw i64
        // which is actually a *StringHeader pointer — distinct from "string"
        // (which declares the function as returning `ptr` in LLVM IR) and
        // from "i64" (which sitofp-converts the integer to a JS number).
        let returns_i64_str = matches!(manifest_ret, Some("i64_str"));
        let returns_string = matches!(manifest_ret, Some("string") | Some("ptr"))
            || matches!(ext_return_type, HirType::String)
            || (manifest_ret.is_none()
                && has_string_args
                && (name.contains("read_file")
                    || name.contains("clipboard_text")
                    || name.contains("file_dialog")));
        let returns_void = matches!(manifest_ret, Some("void"))
            || (manifest_ret.is_none() && matches!(ext_return_type, HirType::Void));
        let returns_i64 = matches!(manifest_ret, Some("i64"));
        if returns_void {
            ctx.pending_declares
                .push((name.clone(), crate::types::VOID, arg_types));
            ctx.block().call_void(name, &arg_slices);
            return Ok(Some(double_literal(f64::from_bits(
                crate::nanbox::TAG_UNDEFINED,
            ))));
        } else if returns_i64_str {
            // C function returns a raw i64 that is a *StringHeader address.
            // Declare as I64 (matching the C ABI — x0 on ARM64, rax on
            // x86_64), call it, and NaN-box the result directly with
            // STRING_TAG. No sitofp (which would corrupt the pointer
            // bits) and no ptrtoint (already an integer, not a ptr).
            ctx.pending_declares.push((name.clone(), I64, arg_types));
            let raw = ctx.block().call(I64, name, &arg_slices);
            let blk = ctx.block();
            return Ok(Some(nanbox_string_inline(blk, &raw)));
        } else if returns_string {
            ctx.pending_declares.push((name.clone(), PTR, arg_types));
            let raw_ptr = ctx.block().call(PTR, name, &arg_slices);
            // Convert raw *const u8 back to a NaN-boxed string.
            let blk = ctx.block();
            let ptr_i64 = blk.ptrtoint(&raw_ptr, I64);
            return Ok(Some(nanbox_string_inline(blk, &ptr_i64)));
        } else if returns_i64 {
            // C function returns i64 in x0 (e.g. `*mut View`
            // handles). Declare as I64; the value comes back as a
            // raw integer. Convert via `sitofp` so callers see a
            // normal JS number; subsequent FFI calls that pass it
            // back as an i64 param will truncate via `fptosi`.
            ctx.pending_declares.push((name.clone(), I64, arg_types));
            let raw = ctx.block().call(I64, name, &arg_slices);
            let blk = ctx.block();
            return Ok(Some(blk.sitofp(I64, &raw, DOUBLE)));
        } else {
            // Native library functions (Bloom, etc.) return f64 in
            // the d0 register — they use the Perry double-based ABI,
            // not a C integer ABI. Declare as DOUBLE and use the
            // return value directly (no sitofp needed).
            ctx.pending_declares.push((name.clone(), DOUBLE, arg_types));
            return Ok(Some(ctx.block().call(DOUBLE, name, &arg_slices)));
        }
    };
    // Issue #678 followup: if the consumer-visible name resolves to a
    // V8-fallback module, there is no `perry_fn_<src>__<name>` symbol
    // (the origin was demoted to V8 and never emitted a native one).
    // Route the call through the runtime V8 bridge.
    if let Some(specifier) = ctx.import_function_v8_specifiers.get(name).cloned() {
        let mut lowered: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered.push(lower_expr(ctx, a)?);
        }
        return Ok(Some(crate::expr::emit_v8_export_call(
            ctx, &specifier, name, &lowered,
        )));
    }
    // Issue #678: re-export rename (`export { default as render } from
    // './render.js'`) means the origin module emits the symbol under
    // the *origin* name (`default`), not the consumer-visible name
    // (`render`). Look up the actual origin suffix before forming the
    // extern.
    let origin_suffix = crate::expr::import_origin_suffix(ctx.import_function_origin_names, name);
    let fname = format!("perry_fn_{}__{}", source_prefix, origin_suffix);
    // Issue #493 followup: when the imported binding is a VARIABLE
    // holding a closure value (e.g. `var mergePath = (b, s, ...r) => …`
    // exported from another module), `perry_fn_<src>__<name>` is the
    // ZERO-arg GETTER that returns the closure pointer (set up at
    // crates/perry/src/commands/compile.rs's `imported_vars` registration
    // and emitted by the source module's value-getter loop). Calling
    // the getter with N args puts garbage in the registers and discards
    // the actual call — `mergePath('/', '/foo')` returned the closure
    // itself instead of the merged path. The fix is to call the getter
    // first, treating its return as a closure value, then dispatch
    // through `js_closure_callN`. The runtime's closure-rest registry
    // (issue #493) bundles trailing args correctly when the closure
    // has `...rest`. Before this branch, ExternFuncRef-as-call for
    // imported-VAR bindings silently broke any code path that imports
    // an arrow-bound exported value (hono's `mergePath` from utils/url.js,
    // any `export const foo = () => …` cross-module use).
    if ctx.imported_vars.contains(name) {
        ctx.pending_declares.push((fname.clone(), DOUBLE, vec![]));
        let closure_box = ctx.block().call(DOUBLE, &fname, &[]);
        let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered_args.push(lower_expr(ctx, a)?);
        }
        if lowered_args.len() > 16 {
            anyhow::bail!(
                "perry-codegen Phase D.1: closure call with {} args (max 16)",
                lowered_args.len()
            );
        }
        let blk = ctx.block();
        let closure_handle = unbox_to_i64(blk, &closure_box);
        let runtime_fn = format!("js_closure_call{}", lowered_args.len());
        let mut call_args: Vec<(crate::types::LlvmType, &str)> = vec![(I64, &closure_handle)];
        for v in &lowered_args {
            call_args.push((DOUBLE, v.as_str()));
        }
        return Ok(Some(blk.call(DOUBLE, &runtime_fn, &call_args)));
    }
    // Record the cross-module call so the caller can add a `declare`
    // line for it after the &mut LlFunction borrow is released. The
    // module dedupes by name, so duplicates are harmless. Without
    // this, clang errors with `use of undefined value @perry_fn_*`
    // for any cross-module call hidden inside a closure body, try
    // block, switch, etc. — the old pre-walker missed those shapes.
    //
    // Determine the actual param count from the imported function
    // signature. Calls that pass fewer args than the function declares
    // (because the trailing params have defaults) need to be padded
    // with `undefined` so the function body sees defined values for
    // the missing args (and can apply its defaults). Without this,
    // the d-registers for the missing params hold stale data and
    // the function reads garbage (e.g. alpha = -3e-5 instead of 1).
    let declared_count = ctx
        .imported_func_param_counts
        .get(name)
        .copied()
        .unwrap_or(args.len());
    let has_rest = ctx.imported_func_has_rest.contains(name);
    // Issue #608: when the imported callee declares a trailing
    // `...rest` parameter, the LLVM signature has exactly
    // `declared_count` doubles (rest counts as one slot — a
    // NaN-boxed array pointer). Bundle every arg at and beyond the
    // rest position into a single `js_array_alloc` array; that
    // array is what the callee's rest binding sees. Without this
    // bundling, `tag\`hello ${x}\`` lowers to `tag([…], x)` and
    // the cross-module callee reads `params` as `x` directly
    // (`undefined` when no interp args, or the raw arg value
    // when one).
    let target_arity = if has_rest {
        declared_count.max(1)
    } else {
        declared_count.max(args.len())
    };
    let param_types: Vec<crate::types::LlvmType> =
        std::iter::repeat_n(DOUBLE, target_arity).collect();
    ctx.pending_declares
        .push((fname.clone(), DOUBLE, param_types));
    let mut lowered: Vec<String> = Vec::with_capacity(target_arity);
    if has_rest {
        // Fixed (non-rest) params: pass through.
        let fixed_count = declared_count.saturating_sub(1);
        for a in args.iter().take(fixed_count) {
            lowered.push(lower_expr(ctx, a)?);
        }
        // Pad fixed params if the caller passed too few.
        let undefined_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        while lowered.len() < fixed_count {
            lowered.push(undefined_lit.clone());
        }
        // Materialize the rest array (always — even when zero
        // trailing args, the callee's rest binding must be `[]`).
        let rest_count = args.len().saturating_sub(fixed_count);
        let cap = (rest_count as u32).to_string();
        let mut current = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
        for a in args.iter().skip(fixed_count) {
            let v = lower_expr(ctx, a)?;
            let blk = ctx.block();
            current = blk.call(I64, "js_array_push_f64", &[(I64, &current), (DOUBLE, &v)]);
        }
        let rest_box = nanbox_pointer_inline(ctx.block(), &current);
        lowered.push(rest_box);
    } else {
        for a in args {
            lowered.push(lower_expr(ctx, a)?);
        }
        // Pad with TAG_UNDEFINED for the missing trailing args.
        let undefined_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        while lowered.len() < target_arity {
            lowered.push(undefined_lit.clone());
        }
    }
    let arg_slices: Vec<(crate::types::LlvmType, &str)> =
        lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
    Ok(Some(ctx.block().call(DOUBLE, &fname, &arg_slices)))
}
