//! DynamicImport / ExternFuncRef / I18nString.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::{anyhow, bail, Result};
use perry_hir::Expr;
use perry_types::Type as HirType;

use crate::nanbox::{double_literal, POINTER_MASK_I64};
use crate::types::{DOUBLE, I32, I64, PTR};

use super::{
    emit_string_literal_global, import_origin_suffix, is_global_this_builtin_name, lower_expr,
    nanbox_pointer_inline, nanbox_string_inline, unbox_to_i64, FnCtx, I18nLowerCtx,
};

/// Build the namespace value for a resolved dynamic-import/require target prefix
/// on the current block: a native submodule (`__node_submod__<key>`), a native
/// builtin (`__native_mod__<name>`), or a compiled module (`<prefix>__init` +
/// `@__perry_ns_<prefix>`). Returns a NaN-boxed `DOUBLE` value id.
fn namespace_value_for_prefix(ctx: &mut FnCtx<'_>, prefix: &str) -> String {
    if let Some(key) = prefix.strip_prefix("__node_submod__") {
        let key = key.to_string();
        let submod_label = emit_string_literal_global(ctx, &key);
        let submod_len = key.len();
        let install_sym = crate::nm_install::nm_submod_install_symbol(&key);
        let blk = ctx.block();
        if let Some(s) = install_sym {
            blk.call_void(s, &[]);
        }
        blk.call(
            DOUBLE,
            "js_node_submodule_namespace",
            &[(PTR, &submod_label), (I32, &submod_len.to_string())],
        )
    } else if let Some(name) = prefix.strip_prefix("__native_mod__") {
        let name = name.to_string();
        let mod_label = emit_string_literal_global(ctx, &name);
        let mod_len = name.len();
        let blk = ctx.block();
        if let Some(s) = crate::nm_install::nm_install_symbol(&name) {
            blk.call_void(s, &[]);
        }
        blk.call(
            DOUBLE,
            "js_create_native_module_namespace",
            &[(PTR, &mod_label), (I64, &mod_len.to_string())],
        )
    } else {
        // Issue #753: trigger the target's init (idempotent for Eager targets;
        // the populating call for Deferred targets) before loading its namespace.
        let blk = ctx.block();
        blk.call_void(&format!("{}__init", prefix), &[]);
        blk.load(DOUBLE, &format!("@__perry_ns_{}", prefix))
    }
}

/// #5389 Tier 2: lower a synchronous CommonJS `require(expr)` in a compiled
/// external module. Mirrors the dynamic-`import()` dispatch in `lower`, but
/// returns the target **namespace value directly** (no Promise wrap) and uses the
/// ambient createRequire-backed require (`js_module_ambient_require_apply`) as the
/// unresolved / no-match fallthrough instead of a rejected promise — so builtins
/// keep resolving by string and unknown packages throw the descriptive
/// `ERR_PERRY_UNSUPPORTED_CREATE_REQUIRE`. `paths` is populated by the same
/// `collect_modules` resolver as `import()`.
fn lower_dynamic_require(ctx: &mut FnCtx<'_>, paths: &[String], arg: &Expr) -> Result<String> {
    // Empty `paths` → genuinely runtime-computed specifier (didn't const-fold).
    // Resolve it entirely through the ambient require.
    if paths.is_empty() {
        let spec_val = lower_expr(ctx, arg)?;
        return Ok(ctx.block().call(
            DOUBLE,
            "js_module_ambient_require_apply",
            &[(DOUBLE, &spec_val)],
        ));
    }

    // Single resolved target: the resolver proved this is the only possible
    // specifier. Evaluate the arg for side effects, then return the namespace
    // directly (or fall back to the ambient require if the driver didn't map the
    // path to a target prefix).
    if paths.len() == 1 {
        let spec_val = lower_expr(ctx, arg)?;
        let target_prefix = ctx.dynamic_import_path_to_prefix.get(&paths[0]).cloned();
        return Ok(match target_prefix {
            Some(prefix) => namespace_value_for_prefix(ctx, &prefix),
            None => ctx.block().call(
                DOUBLE,
                "js_module_ambient_require_apply",
                &[(DOUBLE, &spec_val)],
            ),
        });
    }

    // Multi-target: compare the runtime specifier against each resolved path via
    // `js_string_equals`; each match stores its namespace into the result slot.
    // The no-match fallthrough resolves via the ambient require (builtin-or-throw)
    // rather than rejecting.
    let spec_val = lower_expr(ctx, arg)?;
    let result_slot = ctx.block().alloca(DOUBLE);
    let join_block_idx = ctx.new_block("dynamic_require_join");
    let path_handle =
        ctx.block()
            .call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &spec_val)]);

    let resolved: Vec<(String, String)> = paths
        .iter()
        .filter_map(|p| {
            ctx.dynamic_import_path_to_prefix
                .get(p)
                .cloned()
                .map(|tgt| (p.clone(), tgt))
        })
        .collect();

    for (i, (path_str, target_prefix)) in resolved.iter().enumerate() {
        let key_idx = ctx.strings.intern(path_str);
        let key_entry = ctx.strings.entry(key_idx);
        let key_handle_global = format!("@{}", key_entry.handle_global);

        let blk = ctx.block();
        let key_box = blk.load(DOUBLE, &key_handle_global);
        let key_handle = blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &key_box)]);
        let eq_i32 = blk.call(
            I32,
            "js_string_equals",
            &[(I64, &path_handle), (I64, &key_handle)],
        );
        let cond = blk.icmp_ne(I32, &eq_i32, "0");

        let match_block_idx = ctx.new_block(&format!("dyn_require_match_{}", i));
        let next_label = if i + 1 < resolved.len() {
            ctx.new_block(&format!("dyn_require_next_{}", i))
        } else {
            ctx.new_block(&format!("dyn_require_fallback_{}", i))
        };
        let match_label = ctx.block_label(match_block_idx);
        let next_label_str = ctx.block_label(next_label);
        ctx.block().cond_br(&cond, &match_label, &next_label_str);

        ctx.current_block = match_block_idx;
        let join_label = ctx.block_label(join_block_idx);
        let ns_val = namespace_value_for_prefix(ctx, target_prefix);
        let blk = ctx.block();
        blk.store(DOUBLE, &ns_val, &result_slot);
        blk.br(&join_label);

        ctx.current_block = next_label;
    }

    // No-match fallthrough: resolve via the ambient require.
    let join_label = ctx.block_label(join_block_idx);
    let fallback = ctx.block().call(
        DOUBLE,
        "js_module_ambient_require_apply",
        &[(DOUBLE, &spec_val)],
    );
    let blk = ctx.block();
    blk.store(DOUBLE, &fallback, &result_slot);
    blk.br(&join_label);

    ctx.current_block = join_block_idx;
    Ok(ctx.block().load(DOUBLE, &result_slot))
}

/// Emit one resolved i18n template (a single locale's translation row) and
/// return the NaN-boxed string result.
///
/// Builds a `(fragment, Option<param_name>)` plan from the template: each
/// `{name}` placeholder splits a fragment; text between/around placeholders
/// is a literal piece. `{{` / `}}` are tolerated as literal braces (matches
/// common i18n conventions and avoids quirks if a translation contains a
/// literal `{`).
///
/// - No placeholders → intern the template and load the static handle.
/// - Otherwise walk the plan and emit a `js_string_concat` chain,
///   accumulating an i64 string handle (NOT a NaN-boxed double — saves the
///   bitcast/mask cycle on every concat):
///     - Lit(s): intern via StringPool, load the handle, mask.
///     - Param(name): look up the pre-lowered value in `lowered_params`,
///       coerce via `js_string_coerce` (which already returns a handle). A
///       placeholder naming an unknown param falls back to the literal
///       `{name}` text so the user can see the bug.
///
/// Params must already be lowered by the caller (exactly once, in source
/// order) — this function only *references* them, so it is safe to call
/// once per locale branch without duplicating side effects.
fn emit_i18n_template(
    ctx: &mut FnCtx<'_>,
    template: &str,
    lowered_params: &std::collections::HashMap<String, String>,
) -> Result<String> {
    #[derive(Debug)]
    enum Part {
        Lit(String),
        Param(String),
    }
    let mut plan: Vec<Part> = Vec::new();
    {
        let bytes = template.as_bytes();
        let mut i = 0usize;
        // Buffer literal fragments as raw bytes and decode once per fragment.
        // Pushing `b as char` would re-encode each byte as its own Unicode
        // scalar, mangling any non-ASCII text in a placeholder-containing
        // template (e.g. `für {name}`). We only ever special-case the ASCII
        // bytes `{`/`}`, so multi-byte UTF-8 sequences stay intact.
        let mut buf: Vec<u8> = Vec::new();
        let flush = |buf: &mut Vec<u8>| String::from_utf8_lossy(&std::mem::take(buf)).into_owned();
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'{' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                    buf.push(b'{');
                    i += 2;
                    continue;
                }
                // Find the matching `}`.
                let end = bytes[i + 1..]
                    .iter()
                    .position(|&c| c == b'}')
                    .map(|p| i + 1 + p);
                match end {
                    Some(close) => {
                        if !buf.is_empty() {
                            plan.push(Part::Lit(flush(&mut buf)));
                        }
                        let name = std::str::from_utf8(&bytes[i + 1..close])
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        plan.push(Part::Param(name));
                        i = close + 1;
                    }
                    None => {
                        // Unterminated `{` — treat as literal.
                        buf.push(b);
                        i += 1;
                    }
                }
            } else if b == b'}' && i + 1 < bytes.len() && bytes[i + 1] == b'}' {
                buf.push(b'}');
                i += 2;
            } else {
                buf.push(b);
                i += 1;
            }
        }
        if !buf.is_empty() {
            plan.push(Part::Lit(flush(&mut buf)));
        }
    }

    // Fast path: no `{name}` placeholders → just emit the literal.
    let has_placeholders = plan.iter().any(|p| matches!(p, Part::Param(_)));
    if !has_placeholders {
        let key_idx = ctx.strings.intern(template);
        let handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
        return Ok(ctx.block().load(DOUBLE, &handle_global));
    }

    let mut acc_handle: Option<String> = None;
    for part in &plan {
        let part_handle: String = match part {
            Part::Lit(s) => {
                let key_idx = ctx.strings.intern(s);
                let handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
                let blk = ctx.block();
                let lit_box = blk.load(DOUBLE, &handle_global);
                unbox_to_i64(blk, &lit_box)
            }
            Part::Param(name) => {
                let v_box = match lowered_params.get(name) {
                    Some(v) => v.clone(),
                    None => {
                        let placeholder = format!("{{{}}}", name);
                        let key_idx = ctx.strings.intern(&placeholder);
                        let handle_global =
                            format!("@{}", ctx.strings.entry(key_idx).handle_global);
                        ctx.block().load(DOUBLE, &handle_global)
                    }
                };
                let blk = ctx.block();
                blk.call(I64, "js_string_coerce", &[(DOUBLE, &v_box)])
            }
        };
        acc_handle = Some(match acc_handle {
            None => part_handle,
            Some(prev) => {
                let blk = ctx.block();
                blk.call(
                    I64,
                    "js_string_concat",
                    &[(I64, &prev), (I64, &part_handle)],
                )
            }
        });
    }
    // `plan` had at least one placeholder so it can't be empty;
    // `acc_handle` is therefore Some. Box the final handle.
    let final_handle = acc_handle.expect("template plan was non-empty");
    Ok(nanbox_string_inline(ctx.block(), &final_handle))
}

/// Resolve every locale's template for one i18n string-table row. An empty
/// translation cell means the locale file is missing this key — fall back to
/// `fallback_text` (the base key's source text; for plural-form rows this is
/// the base key WITHOUT the `.one`/`.other` suffix, so an untranslated form
/// never leaks the suffixed registry key to the user).
///
/// Returns `(templates, default_idx, locale_codes)`; a `(vec![fallback],
/// 0, vec![])` triple when i18n isn't configured for this build.
fn resolve_i18n_templates(
    i18n: &Option<I18nLowerCtx>,
    fallback_text: &str,
    string_idx: u32,
) -> (Vec<String>, usize, Vec<String>) {
    let (mut templates, default_idx, locale_codes): (Vec<String>, usize, Vec<String>) =
        match i18n.as_ref() {
            Some(t) if t.key_count > 0 => {
                let locale_count = t.translations.len() / t.key_count;
                let rows = (0..locale_count)
                    .map(
                        |li| match t.translations.get(li * t.key_count + string_idx as usize) {
                            Some(s) if !s.is_empty() => s.clone(),
                            _ => fallback_text.to_string(),
                        },
                    )
                    .collect();
                (rows, t.default_locale_idx, t.locale_codes.clone())
            }
            _ => (Vec::new(), 0, Vec::new()),
        };
    if templates.is_empty() {
        templates.push(fallback_text.to_string());
    }
    let default_idx = default_idx.min(templates.len() - 1);
    (templates, default_idx, locale_codes)
}

/// Emit the runtime locale-row index for this site as an i32 SSA value.
/// `perry_i18n_locale_index_for` lazily detects the system locale on first
/// call (the entry `main` prelude's `perry_i18n_init` normally resolved it
/// already — then this is a cached atomic load), matches it against the
/// configured locale list (one interned "en,de,fr" literal), and returns the
/// row index; an explicit `perry_i18n_set_locale_index` always wins.
fn emit_locale_index(ctx: &mut FnCtx<'_>, locale_codes: &[String], default_idx: usize) -> String {
    let locales_joined = locale_codes.join(",");
    let locales_key_idx = ctx.strings.intern(&locales_joined);
    let locales_handle_global = format!("@{}", ctx.strings.entry(locales_key_idx).handle_global);
    let blk = ctx.block();
    let locales_box = blk.load(DOUBLE, &locales_handle_global);
    let locales_handle = unbox_to_i64(blk, &locales_box);
    blk.call(
        I32,
        "perry_i18n_locale_index_for",
        &[(I64, &locales_handle), (I32, &default_idx.to_string())],
    )
}

/// Emit the value for one resolved i18n row (a `templates` vector in locale
/// order): the compile-time fast path when every locale's template is
/// identical (or the build is single-locale / lacks a runtime locale index),
/// otherwise a branch chain on `locale_idx_val` with the default locale's row
/// as the fallthrough.
fn emit_i18n_row_value(
    ctx: &mut FnCtx<'_>,
    templates: &[String],
    default_idx: usize,
    locale_idx_val: Option<&str>,
    lowered_params: &std::collections::HashMap<String, String>,
) -> Result<String> {
    let all_same = templates.iter().all(|t| t == &templates[default_idx]);
    let locale_idx = match locale_idx_val {
        Some(v) if !all_same => v,
        _ => return emit_i18n_template(ctx, &templates[default_idx], lowered_params),
    };

    let result_slot = ctx.block().alloca(DOUBLE);
    let join_block_idx = ctx.new_block("i18n_locale_join");

    for (li, template) in templates.iter().enumerate() {
        if li == default_idx {
            continue; // default row is the fallthrough below
        }
        let blk = ctx.block();
        let cond = blk.icmp_eq(I32, locale_idx, &li.to_string());
        let match_block_idx = ctx.new_block(&format!("i18n_locale_{}", li));
        let next_block_idx = ctx.new_block(&format!("i18n_locale_next_{}", li));
        let match_label = ctx.block_label(match_block_idx);
        let next_label = ctx.block_label(next_block_idx);
        ctx.block().cond_br(&cond, &match_label, &next_label);

        ctx.current_block = match_block_idx;
        let val = emit_i18n_template(ctx, template, lowered_params)?;
        let join_label = ctx.block_label(join_block_idx);
        let blk = ctx.block();
        blk.store(DOUBLE, &val, &result_slot);
        blk.br(&join_label);

        ctx.current_block = next_block_idx;
    }

    // Fallthrough: the default locale's row (also covers a stale
    // out-of-range index from perry_i18n_set_locale_index).
    let val = emit_i18n_template(ctx, &templates[default_idx], lowered_params)?;
    let join_label = ctx.block_label(join_block_idx);
    let blk = ctx.block();
    blk.store(DOUBLE, &val, &result_slot);
    blk.br(&join_label);

    ctx.current_block = join_block_idx;
    Ok(ctx.block().load(DOUBLE, &result_slot))
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::WorkerNew {
            paths,
            filename,
            options,
            is_eval: _,
        } => {
            let _ = lower_expr(ctx, filename)?;
            let options_val = if let Some(options) = options {
                lower_expr(ctx, options)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            // An empty `paths` list means collect_modules could not resolve
            // the filename statically (it warned at compile time). Many real
            // packages construct Workers only on cold paths (e.g. Next.js
            // build-time worker pools) — throw if one is actually reached at
            // runtime instead of failing the whole compile.
            if paths.is_empty() {
                let msg = "worker_threads Worker filename was not statically \
                           resolvable at compile time; constructing this Worker \
                           is unsupported in the compiled binary";
                let msg_idx = ctx.strings.intern(msg);
                let msg_entry = ctx.strings.entry(msg_idx);
                let msg_bytes_global = format!("@{}", msg_entry.bytes_global);
                let msg_len_str = msg_entry.byte_len.to_string();
                let blk = ctx.block();
                blk.call_void(
                    "js_throw_error_with_code",
                    &[
                        (PTR, &msg_bytes_global),
                        (I64, &msg_len_str),
                        (PTR, "null"),
                        (I64, "0"),
                        (I32, "0"),
                    ],
                );
                blk.unreachable();
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            if paths.len() != 1 {
                bail!(
                    "worker_threads Worker requires exactly one compile-time-resolved filename, got {}",
                    paths.len()
                );
            }
            let path = &paths[0];
            let target_prefix = ctx
                .dynamic_import_path_to_prefix
                .get(path)
                .cloned()
                .ok_or_else(|| anyhow!("worker_threads Worker target was not compiled: {path}"))?;
            if target_prefix.starts_with("__node_submod__")
                || target_prefix.starts_with("__native_mod__")
            {
                bail!("worker_threads Worker target must be a compiled source file: {path}");
            }
            // Call the module's unguarded `__init_body`, NOT the guarded
            // `__init` wrapper. The wrapper's process-global
            // `__perry_init_done_*` flag is set by the first worker (or by
            // main-thread import init) and would make every later worker's
            // entry a no-op — leaving the spawned thread idle and the parent
            // waiting forever. The body re-runs the module top-level on each
            // worker thread (each has its own thread-local arena), so every
            // worker actually executes its entry and posts its result back.
            let init_name = format!("{}__init_body", target_prefix);
            ctx.pending_declares
                .push((init_name.clone(), crate::types::VOID, vec![]));
            let entry_ptr = ctx.block().ptrtoint(&format!("@{}", init_name), I64);
            Ok(ctx.block().call(
                DOUBLE,
                "js_worker_threads_worker_new",
                &[(I64, &entry_ptr), (DOUBLE, &options_val)],
            ))
        }
        Expr::DynamicImport {
            paths,
            arg,
            deferred_error,
            synchronous,
            ..
        } => {
            // #5389 Tier 2: a synchronous CommonJS `require(expr)` returns the
            // target namespace value directly (no Promise) and falls back to the
            // ambient createRequire-backed `require` for unresolved / no-match
            // specifiers. Resolution (`paths`) is identical to `import()`.
            if *synchronous {
                return lower_dynamic_require(ctx, paths, arg);
            }
            // #5230: a non-resolvable (runtime-computed) specifier was
            // *deferred* (the default, non-strict policy — analog of #5206's
            // eval deferral). Evaluate the arg, then hand the runtime value to
            // the deferred-fallback helper (#6660): a specifier that names a
            // node BUILTIN at runtime (`imp("node:os")` through a helper the
            // resolver couldn't fold) resolves to the builtin namespace like
            // Node; anything else rejects with the descriptive deferral
            // `Error` so `await import(spec)` throws only if this site is
            // actually reached, instead of failing the whole build.
            if let Some(msg) = deferred_error {
                let spec_val = lower_expr(ctx, arg)?;
                let msg_val = lower_expr(ctx, &Expr::String(msg.clone()))?;
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_module_dynamic_import_deferred",
                    &[(DOUBLE, &spec_val), (DOUBLE, &msg_val)],
                ));
            }

            // Defensive: an empty `paths` list means the resolver pass
            // failed to populate this node, which `collect_modules`
            // should have raised as a compile error. Fall through to the
            // runtime fallback (#6660: builtin-or-`ERR_MODULE_NOT_FOUND`
            // rejection — historically this arm rejected with literal
            // `undefined`, which surfaced as a reasonless
            // `Uncaught (in promise) undefined`) rather than crashing the IR.
            if paths.is_empty() {
                let spec_val = lower_expr(ctx, arg)?;
                let hooked = ctx.block().call(
                    DOUBLE,
                    "js_module_dynamic_import_apply_hooks",
                    &[(DOUBLE, &spec_val)],
                );
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_module_dynamic_import_fallback",
                    &[(DOUBLE, &hooked)],
                ));
            }

            // Single-target fast path. Skip the runtime string compare
            // — the static resolver already proved this is the only
            // possible target.
            if paths.len() == 1 {
                // Evaluate the arg for side effects (most are pure but
                // a template literal with computed parts can have e.g.
                // function calls) and let registered module loader hooks
                // observe/delegate the import. The statically known target
                // still determines the namespace in Perry's compile-time graph.
                let path_val = lower_expr(ctx, arg)?;
                let _ = ctx.block().call(
                    DOUBLE,
                    "js_module_dynamic_import_apply_hooks",
                    &[(DOUBLE, &path_val)],
                );
                let path = &paths[0];
                let target_prefix = ctx.dynamic_import_path_to_prefix.get(path).cloned();
                // #1671: a dynamic import of a known node-submodule
                // (`await import('hono/jsx/server')`) carries the sentinel
                // prefix `__node_submod__<key>` instead of a compiled-module
                // prefix. Build its namespace via `js_node_submodule_namespace`
                // and resolve the promise with it (mirrors the static
                // namespace-import path).
                if let Some(prefix) = &target_prefix {
                    if let Some(key) = prefix.strip_prefix("__node_submod__") {
                        let key = key.to_string();
                        let submod_label = emit_string_literal_global(ctx, &key);
                        let submod_len = key.len();
                        let install_sym = crate::nm_install::nm_submod_install_symbol(&key);
                        let blk = ctx.block();
                        if let Some(s) = install_sym {
                            blk.call_void(s, &[]);
                        }
                        let ns_val = blk.call(
                            DOUBLE,
                            "js_node_submodule_namespace",
                            &[(PTR, &submod_label), (I32, &submod_len.to_string())],
                        );
                        let promise = blk.call(I64, "js_promise_resolved", &[(DOUBLE, &ns_val)]);
                        return Ok(nanbox_pointer_inline(blk, &promise));
                    }
                    // #1673: a dynamic import of a general native builtin
                    // (`await import('node:crypto')`) carries the sentinel
                    // prefix `__native_mod__<name>`. Build its namespace via
                    // `js_create_native_module_namespace` — the same
                    // NATIVE_MODULE_CLASS_ID object `require('node:crypto')`
                    // produces, whose member access dispatches natively at
                    // runtime — and resolve the promise with it.
                    if let Some(name) = prefix.strip_prefix("__native_mod__") {
                        let name = name.to_string();
                        let mod_label = emit_string_literal_global(ctx, &name);
                        let mod_len = name.len();
                        let blk = ctx.block();
                        if let Some(s) = crate::nm_install::nm_install_symbol(&name) {
                            blk.call_void(s, &[]);
                        }
                        let ns_val = blk.call(
                            DOUBLE,
                            "js_create_native_module_namespace",
                            &[(PTR, &mod_label), (I64, &mod_len.to_string())],
                        );
                        let promise = blk.call(I64, "js_promise_resolved", &[(DOUBLE, &ns_val)]);
                        return Ok(nanbox_pointer_inline(blk, &promise));
                    }
                }
                let blk = ctx.block();
                let ns_val = match target_prefix {
                    Some(prefix) => {
                        // Issue #753: trigger the target's init before
                        // loading its namespace. For Eager targets the
                        // guard short-circuits; for Deferred targets
                        // this is the only invocation that populates
                        // `@__perry_ns_<prefix>`.
                        blk.call_void(&format!("{}__init", prefix), &[]);
                        blk.load(DOUBLE, &format!("@__perry_ns_{}", prefix))
                    }
                    None => {
                        // Driver didn't resolve this path to a target module —
                        // route through the runtime fallback (#6660: builtin
                        // specifiers resolve like Node, everything else rejects
                        // with `ERR_MODULE_NOT_FOUND` instead of the old
                        // literal-`undefined` rejection).
                        return Ok(blk.call(
                            DOUBLE,
                            "js_module_dynamic_import_fallback",
                            &[(DOUBLE, &path_val)],
                        ));
                    }
                };
                let promise = blk.call(I64, "js_promise_resolved", &[(DOUBLE, &ns_val)]);
                return Ok(nanbox_pointer_inline(blk, &promise));
            }

            // Multi-target: evaluate the runtime path string, then
            // emit a chain of `js_string_equals` compares. Each
            // successful compare resolves to its corresponding
            // namespace global. The final fallback emits a rejected
            // promise.
            let raw_path_val = lower_expr(ctx, arg)?;
            let path_val = ctx.block().call(
                DOUBLE,
                "js_module_dynamic_import_apply_hooks",
                &[(DOUBLE, &raw_path_val)],
            );
            // Result phi slot: every successful match stores the
            // promise (NaN-boxed POINTER_TAG f64) here, then jumps to
            // a join block which loads and returns. Using an alloca
            // keeps the IR straightforward without proper phi nodes.
            let result_slot = ctx.block().alloca(DOUBLE);
            let join_block_idx = ctx.new_block("dynamic_import_join");

            // Unbox the path argument once into an i64 StringHeader*.
            let path_handle =
                ctx.block()
                    .call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &path_val)]);

            // Pre-resolve target prefixes so we can skip paths that
            // don't have a known target (driver dropped them).
            let resolved: Vec<(String, String)> = paths
                .iter()
                .filter_map(|p| {
                    ctx.dynamic_import_path_to_prefix
                        .get(p)
                        .cloned()
                        .map(|tgt| (p.clone(), tgt))
                })
                .collect();

            for (i, (path_str, target_prefix)) in resolved.iter().enumerate() {
                // Intern the path string so the compare against the
                // runtime arg works on real StringHeader pointers.
                let key_idx = ctx.strings.intern(path_str);
                let key_entry = ctx.strings.entry(key_idx);
                let key_handle_global = format!("@{}", key_entry.handle_global);

                let blk = ctx.block();
                let key_box = blk.load(DOUBLE, &key_handle_global);
                let key_handle =
                    blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &key_box)]);
                let eq_i32 = blk.call(
                    I32,
                    "js_string_equals",
                    &[(I64, &path_handle), (I64, &key_handle)],
                );
                let cond = blk.icmp_ne(I32, &eq_i32, "0");

                let match_block_idx = ctx.new_block(&format!("dyn_import_match_{}", i));
                let next_label = if i + 1 < resolved.len() {
                    ctx.new_block(&format!("dyn_import_next_{}", i))
                } else {
                    ctx.new_block(&format!("dyn_import_reject_{}", i))
                };
                let match_label = ctx.block_label(match_block_idx);
                let next_label_str = ctx.block_label(next_label);
                ctx.block().cond_br(&cond, &match_label, &next_label_str);

                // Match arm — call target's __init (idempotent), load
                // namespace, wrap in promise, store into result_slot,
                // branch to join. Issue #753: the init call is the
                // only thing that triggers a Deferred target's body
                // and namespace populator; for Eager targets the
                // guard short-circuits.
                ctx.current_block = match_block_idx;
                let join_label = ctx.block_label(join_block_idx);
                // #1671: known node-submodule target (sentinel prefix) →
                // build its namespace via the runtime helper rather than a
                // compiled-module init + namespace global.
                let ns_val = if let Some(key) = target_prefix.strip_prefix("__node_submod__") {
                    let key = key.to_string();
                    let submod_label = emit_string_literal_global(ctx, &key);
                    let submod_len = key.len();
                    let install_sym = crate::nm_install::nm_submod_install_symbol(&key);
                    let blk = ctx.block();
                    if let Some(s) = install_sym {
                        blk.call_void(s, &[]);
                    }
                    blk.call(
                        DOUBLE,
                        "js_node_submodule_namespace",
                        &[(PTR, &submod_label), (I32, &submod_len.to_string())],
                    )
                } else if let Some(name) = target_prefix.strip_prefix("__native_mod__") {
                    // #1673: general native builtin target in a multi-path
                    // (`import(cond ? 'node:crypto' : './local.ts')`) chain.
                    let name = name.to_string();
                    let mod_label = emit_string_literal_global(ctx, &name);
                    let mod_len = name.len();
                    let blk = ctx.block();
                    if let Some(s) = crate::nm_install::nm_install_symbol(&name) {
                        blk.call_void(s, &[]);
                    }
                    blk.call(
                        DOUBLE,
                        "js_create_native_module_namespace",
                        &[(PTR, &mod_label), (I64, &mod_len.to_string())],
                    )
                } else {
                    let blk = ctx.block();
                    blk.call_void(&format!("{}__init", target_prefix), &[]);
                    blk.load(DOUBLE, &format!("@__perry_ns_{}", target_prefix))
                };
                let blk = ctx.block();
                let promise = blk.call(I64, "js_promise_resolved", &[(DOUBLE, &ns_val)]);
                let boxed = nanbox_pointer_inline(blk, &promise);
                blk.store(DOUBLE, &boxed, &result_slot);
                blk.br(&join_label);

                // Move to the next compare block (or fallthrough to
                // rejection on the last iteration).
                ctx.current_block = next_label;
            }

            // No-match fallthrough: runtime fallback (#6660) — a builtin
            // specifier resolves like Node, everything else rejects with
            // `ERR_MODULE_NOT_FOUND` (this arm used to reject with literal
            // `undefined`).
            let join_label = ctx.block_label(join_block_idx);
            let blk = ctx.block();
            let fallback = blk.call(
                DOUBLE,
                "js_module_dynamic_import_fallback",
                &[(DOUBLE, &path_val)],
            );
            blk.store(DOUBLE, &fallback, &result_slot);
            blk.br(&join_label);

            // Join: load result and return.
            ctx.current_block = join_block_idx;
            Ok(ctx.block().load(DOUBLE, &result_slot))
        }

        // -------- ExternFuncRef as a value --------
        // The Call path in `lower_call.rs` dispatches `Expr::Call { callee:
        // ExternFuncRef, .. }` directly to the cross-module symbol. When
        // an imported function appears as a STANDALONE value — `if
        // (this.ffi.setCursors)` truthiness check, `someFn === otherFn`
        // equality comparison, or being passed as a callback — we route
        // to the static `__perry_extern_closure_<src>__<name>` global
        // emitted by `compile_module` for every imported function (see the
        // wrapper-emit block right after the user-function `__perry_wrap_*`
        // loop). The global is a `ClosureHeader` with `func_ptr` pointing
        // at a thin `__perry_wrap_extern_<src>__<name>` thunk and
        // `type_tag = CLOSURE_MAGIC`, so the runtime's `js_closure_callN`
        // sees a valid closure and dispatches correctly. We just take the
        // address and NaN-box it as POINTER.
        //
        // For namespaces / built-ins that aren't in `import_function_prefixes`
        // (e.g. setTimeout / clearTimeout / Math / Date), we still don't
        // have a wrapper to point at. Fall back to TAG_TRUE so truthiness
        // checks work; calling those values via stored references would
        // need a separate runtime path that this commit doesn't add.
        Expr::ExternFuncRef { name, .. } => {
            // Imported class references (refs #420 / drizzle): when `name`
            // resolves to a class registered in `ctx.class_ids` (populated
            // from `opts.imported_classes` for imported classes too), emit
            // the same INT32-tagged class-id NaN-box that local `Expr::ClassRef`
            // produces. This is what `js_object_has_own`'s Symbol-key branch
            // looks for to consult `CLASS_STATIC_SYMBOLS`. Without this,
            // `Object.prototype.hasOwnProperty.call(ImportedClass, sym)`
            // always returned false because the receiver was a closure-pointer
            // NaN-box (POINTER_TAG) rather than a class-ref (INT32_TAG).
            if let Some(&cid) = ctx.class_ids.get(name) {
                let bits = crate::nanbox::INT32_TAG | (cid as u64 & 0xFFFF_FFFF);
                return Ok(double_literal(f64::from_bits(bits)));
            }
            // Issue #841: named imports from Node submodules Perry recognizes
            // as runtime-backed values must win over the generic native-module
            // closure wrapper. Default imports use this map too; returning the
            // runtime export here keeps `import consumers from
            // "node:stream/consumers"` equal to the namespace's `.default`
            // object instead of the namespace object itself.
            if let Some((submod_key, exported_name)) = ctx.import_function_node_submodule.get(name)
            {
                let install_sym = crate::nm_install::nm_submod_install_symbol(submod_key);
                let submod_label = emit_string_literal_global(ctx, submod_key);
                let name_label = emit_string_literal_global(ctx, exported_name);
                let submod_len = submod_key.len();
                let name_len = exported_name.len();
                let blk = ctx.block();
                if let Some(s) = install_sym {
                    blk.call_void(s, &[]);
                }
                return Ok(blk.call(
                    DOUBLE,
                    "js_node_submodule_export_as_function",
                    &[
                        (PTR, &submod_label),
                        (I32, &submod_len.to_string()),
                        (PTR, &name_label),
                        (I32, &name_len.to_string()),
                    ],
                ));
            }
            if let Some(source_prefix) = ctx.import_function_prefixes.get(name).cloned() {
                // Next.js lazy-require: a `_lazyreq_N` binding is the CJS require
                // shim's handle to a FUNCTION-LOCAL `require('S')`. S is
                // `Deferred` (never eager-initialized), so before reading its
                // default-export getter, fire `<S>__init()` — idempotent, so
                // re-reads cost a guard check. This is the moment Node would run
                // S's module body: when `require('S')` is actually called.
                if name.starts_with("_lazyreq_") {
                    let init_fn = format!("{}__init", source_prefix);
                    ctx.pending_declares
                        .push((init_fn.clone(), crate::types::VOID, vec![]));
                    ctx.block().call_void(&init_fn, &[]);
                }
                // Issue #678 followup: a V8-fallback import used as a value
                // (rather than called directly) has no native singleton
                // wrapper to point at — the `__perry_wrap_extern_*` for V8
                // imports is the same no-op stub the imported-class branch
                // emits (returns undefined). NaN-box `undefined` so any
                // truthiness check fails closed; equality compares against
                // `undefined`; a call through this value fast-paths through
                // the closure-call's invalid-magic check.
                if ctx.import_function_v8_specifiers.contains_key(name) {
                    return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                }
                // Issue #678: re-export renames mean the origin's symbol uses
                // the *origin* name as the suffix, not the consumer-visible one.
                let origin_suffix = import_origin_suffix(ctx.import_function_origin_names, name);
                // Imported VARIABLES (exported consts/lets) need to be
                // called through their getter to fetch the value, not
                // wrapped as closures. Without this, `let v = HONE_VERSION`
                // creates a closure wrapper instead of the actual string.
                if ctx.imported_vars.contains(name) {
                    let fname = format!("perry_fn_{}__{}", source_prefix, origin_suffix);
                    ctx.pending_declares.push((fname.clone(), DOUBLE, vec![]));
                    return Ok(ctx.block().call(DOUBLE, &fname, &[]));
                }
                // Drizzle / cross-module IIFE-pattern static-property fix:
                // route through the SOURCE module's wrapper symbol +
                // `js_closure_alloc_singleton` (same path the source
                // module uses for its own `Expr::FuncRef(id)` value
                // reads) so the consumer gets the same ClosureHeader
                // pointer the source sees. Pre-fix each consumer emitted
                // its own `__perry_extern_closure_<src>__<name>` global
                // with LLVM `internal` linkage, so the consumer-side
                // pointer differed from the source-side singleton. The
                // IIFE pattern `((fn2) => { fn2.X = Y; })(fn)` writes to
                // CLOSURE_DYNAMIC_PROPS keyed by the source-local closure
                // pointer; consumer reads keyed by their own pointer
                // missed every entry. Drizzle hit this on every
                // `sql.raw(...)` / `sql.identifier(...)` / `sql.fromList`
                // call — the `((sql2) => { sql2.raw = ...; })(sql)`
                // pattern in `drizzle-orm/sql/sql.js`. The fix unifies
                // function-declaration imports with the same architecture
                // const-bound closures already use: ONE module-level
                // singleton observed via stable address from every
                // module. `js_closure_alloc_singleton` keys its
                // pool by func_ptr, so calling it with the source's
                // wrapper symbol from any module returns the same
                // ClosureHeader the source returned for its
                // `Expr::FuncRef(id)` value-reads. The pre-fix
                // `__perry_extern_closure_<src>__<name>` global is no
                // longer referenced and link-time DCE strips it.
                // Refs #645 deeper followup / #488 drizzle-sqlite.
                let target_name = format!("perry_fn_{}__{}", source_prefix, origin_suffix);
                let wrap_name = format!("__perry_wrap_{}", target_name);
                // Declare the source's wrapper so LLVM accepts the
                // `@<wrap_name>` reference. Signature mirrors the
                // emission in `compile_module`'s wrapper-loop (i64
                // closure_ptr + N doubles, arity capped at 5). The
                // signature is for LLVM-IR well-formedness only — the
                // runtime closure-call machinery casts the func_ptr to
                // its own ABI at dispatch time, so size of the param
                // count here is informational. Defaults to arity 0 if
                // the import metadata didn't carry a count; the runtime
                // path still works because singleton lookup uses the
                // address, not the signature.
                let param_count = ctx
                    .imported_func_param_counts
                    .get(name)
                    .copied()
                    .unwrap_or(0)
                    .min(5);
                let mut wrap_param_types: Vec<crate::types::LlvmType> = vec![I64];
                for _ in 0..param_count {
                    wrap_param_types.push(DOUBLE);
                }
                ctx.pending_declares
                    .push((wrap_name.clone(), DOUBLE, wrap_param_types));
                let blk = ctx.block();
                let wrap_ptr = format!("@{}", wrap_name);
                let closure_handle =
                    blk.call(I64, "js_closure_alloc_singleton", &[(PTR, &wrap_ptr)]);
                return Ok(nanbox_pointer_inline(blk, &closure_handle));
            }
            // Issue #841 companion: namespace imports for the same five
            // submodules. The `collect_modules.rs` rejection skips
            // these so the namespace binding flows through HIR and
            // lands here. Emit a call to `js_node_submodule_namespace`
            // which returns a per-submodule stub object whose properties
            // are the function singletons named imports produce.
            if let Some(submod_key) = ctx.namespace_node_submodules.get(name) {
                let submod_label = emit_string_literal_global(ctx, submod_key);
                let submod_len = submod_key.len();
                let install_sym = crate::nm_install::nm_submod_install_symbol(submod_key);
                let blk = ctx.block();
                if let Some(s) = install_sym {
                    blk.call_void(s, &[]);
                }
                return Ok(blk.call(
                    DOUBLE,
                    "js_node_submodule_namespace",
                    &[(PTR, &submod_label), (I32, &submod_len.to_string())],
                ));
            }
            // Issue #629: namespace imports for unresolved modules
            // (`import * as fsp from "node:fs/promises"`) — when the
            // module isn't backed by perry-stdlib bindings or compiled
            // sources, the binding lands here. Pre-fix the catch-all
            // returned TAG_TRUE so `typeof fsp === "boolean"` and every
            // property access produced the confusing "(boolean).X is
            // not a function" error. Route to the runtime stub which
            // returns an empty-object pointer (typeof "object", every
            // property reads undefined). Namespace bindings registered
            // in `ctx.namespace_imports` already short-circuit via
            // dedicated arms above; this catch-all only fires for
            // names with no resolution at all.
            if ctx.namespace_imports.contains(name) {
                // A namespace import used as a whole VALUE (passed to a
                // function, iterated by `Object.keys`/`for…in`/`Object.entries`,
                // spread, …) must be a real object whose OWN ENUMERABLE
                // properties are the source module's exports — not the empty
                // `js_unresolved_namespace_stub`. Drizzle's
                // `drizzle(pool, { schema })` (with `import * as schema`) and
                // Stripe's `_prepResources` (`for (const name in resources)`
                // over `import * as resources`) both enumerate the namespace and
                // silently saw zero members otherwise. Materialize the object by
                // resolving each exported member through the SAME per-member
                // `ns.member` PropertyGet lowering (functions → closure
                // singletons, consts → getters, classes → class refs).
                let mut members: Vec<String> = ctx
                    .namespace_member_prefixes
                    .keys()
                    .filter(|(ns, _)| ns == name)
                    .map(|(_, m)| m.clone())
                    .collect();
                if !members.is_empty() {
                    members.sort();
                    members.dedup();
                    let n_str = (members.len() as u32).to_string();
                    let zero_str = "0".to_string();
                    let handle = ctx.block().call(
                        I64,
                        "js_object_alloc",
                        &[(I32, &zero_str), (I32, &n_str)],
                    );
                    for member in &members {
                        let member_get = Expr::PropertyGet {
                            byte_offset: 0,
                            object: Box::new(Expr::ExternFuncRef {
                                name: name.clone(),
                                param_types: Vec::new(),
                                return_type: HirType::Any,
                            }),
                            property: member.clone(),
                        };
                        let v_box = lower_expr(ctx, &member_get)?;
                        let key_idx = ctx.strings.intern(member);
                        let key_handle_global =
                            format!("@{}", ctx.strings.entry(key_idx).handle_global);
                        let blk = ctx.block();
                        let key_box = blk.load(DOUBLE, &key_handle_global);
                        let key_bits = blk.bitcast_double_to_i64(&key_box);
                        let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                        blk.call_void(
                            "js_object_set_field_by_name",
                            &[(I64, &handle), (I64, &key_raw), (DOUBLE, &v_box)],
                        );
                    }
                    let blk = ctx.block();
                    return Ok(nanbox_pointer_inline(blk, &handle));
                }
                return Ok(ctx
                    .block()
                    .call(DOUBLE, "js_unresolved_namespace_stub", &[]));
            }
            // #4950: built-in globals that HIR lowers to `ExternFuncRef` even
            // in VALUE position (the `is_builtin_function` timer set —
            // `setTimeout`, `setImmediate`, …) used to fall through to the
            // TAG_TRUE sentinel, so `var localSetImmediate = setImmediate`
            // read a boolean and calling through it threw `value is not a
            // function` (scheduler's host-callback setup, → react-reconciler
            // → every React renderer). Resolve them to the same
            // `populate_global_this_builtins` closure `globalThis.<name>`
            // reads, which is callable through the dynamic dispatch path.
            if is_global_this_builtin_name(name) {
                let name_idx = ctx.strings.intern(name);
                let name_bytes_global = format!("@{}", ctx.strings.entry(name_idx).bytes_global);
                let name_len = name.len().to_string();
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_get_global_this_builtin_value",
                    &[(PTR, &name_bytes_global), (I64, &name_len)],
                ));
            }
            // A default-import alias of a Node builtin module used as a VALUE
            // (`const nodeTimers = require('node:timers')`, adopted to an
            // import by the CJS wrap) — materialize the real native-module
            // namespace object so member reads, monkey-patch writes, and
            // enumeration behave. Previously fell through to TAG_TRUE:
            // `typeof nodeTimers === "boolean"` and Next.js's
            // fast-set-immediate extension threw on
            // `nodeTimers.setImmediate = patched` at startup.
            if let Some(source) = ctx.imported_class_sources.get(name) {
                let bare = source.strip_prefix("node:").unwrap_or(source).to_string();
                if perry_hir::is_node_builtin_module(&bare) {
                    let module_label = emit_string_literal_global(ctx, &bare);
                    let module_len = bare.len();
                    let blk = ctx.block();
                    if let Some(s) = crate::nm_install::nm_install_symbol(&bare) {
                        blk.call_void(s, &[]);
                    }
                    return Ok(blk.call(
                        DOUBLE,
                        "js_create_native_module_namespace",
                        &[(PTR, &module_label), (I64, &module_len.to_string())],
                    ));
                }
            }
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_TRUE)))
        }

        // -------- I18nString — compile-time resolution + runtime interpolation --------
        // Two cases:
        //
        //  (a) `ctx.i18n` is `None` — the project doesn't configure i18n,
        //      or this build doesn't have a snapshot threaded through.
        //      Fall back to emitting the verbatim key string. Lower
        //      params for side effects (closure collection, string
        //      literal interning) so they don't get dropped.
        //
        //  (b) `ctx.i18n` is `Some(I18nLowerCtx { translations,
        //      key_count, default_locale_idx })` — pull the right cell
        //      from the flat 2D table at compile time using the entry's
        //      `string_idx`, then:
        //
        //      - If the resolved string has no `{name}` placeholders,
        //        intern it as a string literal and load the handle.
        //      - Otherwise, parse the placeholders, lower each param's
        //        value, `js_string_coerce` to a handle, and chain
        //        `js_string_concat` calls to build the final string at
        //        runtime. Fragments are interned via the StringPool so
        //        identical templates share storage.
        //
        // Plurals: when the key carries `.zero`/`.one`/…/`.other` variants
        // in the locale files (registered by the perry-transform pass as
        // `plural_forms: (category, string_idx)` rows) AND the call site
        // passes the detected plural parameter, the lowering selects the
        // CLDR plural category at runtime via `perry_i18n_plural_category`
        // and branches to the matching form's row — each form then runs the
        // same per-locale selection as a non-plural key. Sites without a
        // count param (or keys without plural variants) use the canonical
        // `string_idx` row directly.
        Expr::I18nString {
            key,
            string_idx,
            params,
            plural_forms,
            plural_param,
        } => {
            // Resolve every locale's template for the base row (and later,
            // each plural form's row) from the flat 2D table.
            let (templates, default_idx, locale_codes) =
                resolve_i18n_templates(ctx.i18n, key, *string_idx);

            // Lower each param exactly once, up front, so closures and
            // side effects in arg expressions fire in source order — no
            // matter which locale/plural branch runs (or whether the
            // template references the param at all).
            let mut lowered_params: std::collections::HashMap<String, String> =
                std::collections::HashMap::with_capacity(params.len());
            for (name, v) in params {
                let v_box = lower_expr(ctx, v)?;
                lowered_params.insert(name.clone(), v_box);
            }

            // A runtime locale index is only meaningful when the table
            // metadata is consistent and there is more than one locale.
            let multi_locale = locale_codes.len() == templates.len() && templates.len() > 1;

            // Plural selection applies when the key has plural variants and
            // this site actually passes the plural parameter.
            let count_val = plural_param
                .as_ref()
                .and_then(|p| lowered_params.get(p))
                .cloned()
                .filter(|_| !plural_forms.is_empty());

            let Some(count_val) = count_val else {
                // Non-plural path: base row only.
                let locale_idx = if multi_locale {
                    Some(emit_locale_index(ctx, &locale_codes, default_idx))
                } else {
                    None
                };
                return emit_i18n_row_value(
                    ctx,
                    &templates,
                    default_idx,
                    locale_idx.as_deref(),
                    &lowered_params,
                );
            };

            // Plural path. The locale index feeds both the CLDR category
            // lookup and each form's per-locale template selection.
            let locale_idx = if multi_locale {
                emit_locale_index(ctx, &locale_codes, default_idx)
            } else {
                // Single-locale build: row 0 is the only (= default) row.
                default_idx.to_string()
            };
            let category = ctx.block().call(
                I32,
                "perry_i18n_plural_category",
                &[(I32, &locale_idx), (DOUBLE, &count_val)],
            );

            // `other` (category 5) is the fallthrough when present; a key
            // with no `.other` variant falls back to the base row.
            let fallback_row: u32 = plural_forms
                .iter()
                .find(|(cat, _)| *cat == 5)
                .map(|(_, idx)| *idx)
                .unwrap_or(*string_idx);

            let result_slot = ctx.block().alloca(DOUBLE);
            let join_block_idx = ctx.new_block("i18n_plural_join");

            for (cat, form_idx) in plural_forms.iter().filter(|(cat, _)| *cat != 5) {
                let blk = ctx.block();
                let cond = blk.icmp_eq(I32, &category, &cat.to_string());
                let match_block_idx = ctx.new_block(&format!("i18n_plural_{}", cat));
                let next_block_idx = ctx.new_block(&format!("i18n_plural_next_{}", cat));
                let match_label = ctx.block_label(match_block_idx);
                let next_label = ctx.block_label(next_block_idx);
                ctx.block().cond_br(&cond, &match_label, &next_label);

                ctx.current_block = match_block_idx;
                let (form_templates, form_default_idx, _) =
                    resolve_i18n_templates(ctx.i18n, key, *form_idx);
                let locale_idx_ref = multi_locale.then_some(locale_idx.as_str());
                let val = emit_i18n_row_value(
                    ctx,
                    &form_templates,
                    form_default_idx,
                    locale_idx_ref,
                    &lowered_params,
                )?;
                let join_label = ctx.block_label(join_block_idx);
                let blk = ctx.block();
                blk.store(DOUBLE, &val, &result_slot);
                blk.br(&join_label);

                ctx.current_block = next_block_idx;
            }

            // Fallthrough: the `other` form (or the base row).
            let (fb_templates, fb_default_idx, _) =
                resolve_i18n_templates(ctx.i18n, key, fallback_row);
            let locale_idx_ref = multi_locale.then_some(locale_idx.as_str());
            let val = emit_i18n_row_value(
                ctx,
                &fb_templates,
                fb_default_idx,
                locale_idx_ref,
                &lowered_params,
            )?;
            let join_label = ctx.block_label(join_block_idx);
            let blk = ctx.block();
            blk.store(DOUBLE, &val, &result_slot);
            blk.br(&join_label);

            ctx.current_block = join_block_idx;
            Ok(ctx.block().load(DOUBLE, &result_slot))
        }

        // -------- Child Process --------
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
