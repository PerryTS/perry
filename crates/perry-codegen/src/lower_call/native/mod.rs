//! Native-method-call dispatcher: `lower_native_method_call`.
//!
//! Tier 2.2 follow-up (v0.5.340). The 805-LOC dispatcher routes
//! `obj.method(args)` calls against native modules (mysql2, pg, redis,
//! mongo, ws, fastify, fetch, perry/ui, perry/system, perry/i18n,
//! perry/plugin, AbortController, …) to their runtime FFI symbols. It
//! also handles a handful of receiver-less perry/ui forms (`Text(...)`,
//! `Button(...)`) that previously routed here before the v0.5.10
//! perry-ui table extraction.
//!
//! 14 helper cross-references reach back into the parent module via
//! `super::` (perry_*_table_lookup family, native_module_lookup,
//! lower_perry_ui_table_call, lower_fetch_native_method,
//! lower_abort_controller_call, lower_notification_schedule, …).
//! All were bumped from private `fn` to `pub(super) fn` in this PR.
//!
//! Split into siblings:
//! - `box_style.rs` — `apply_box_style` + `emit_dim_setter` (perry/tui
//!   `Box(...)` inline-style destructure helpers).
//! - `jsonwebtoken.rs` — `lower_jsonwebtoken_sign` / `_verify` (#1074
//!   algorithm-aware routing).
//! The giant `lower_native_method_call` dispatcher itself stays here.

use anyhow::{bail, Result};
use perry_dispatch::{ArgKind as UiArgKind, ReturnKind as UiReturnKind};
use perry_hir::Expr;

use crate::expr::{lower_expr, nanbox_pointer_inline, unbox_to_i64, FnCtx};
use crate::nanbox::{double_literal, POINTER_MASK_I64};
use crate::types::{DOUBLE, I32, I64};

// Re-export parent items so siblings can pick them up via `use super::*;`.
// `pub(super)` is the visibility-tightest form that still lets the glob
// fire from children — `use super::name` (no `pub`) keeps the item
// scoped to mod.rs only and `super::*` from siblings can't see it.
pub(super) use super::{
    apply_inline_style, collect_closure_introduced_ids, extract_options_fields,
    find_outer_writes_stmt, get_raw_string_ptr, lower_fetch_native_method,
    lower_native_module_dispatch, lower_notification_schedule, lower_perry_ui_table_call,
    native_module_lookup, perry_i18n_table_lookup, perry_media_table_lookup,
    perry_plugin_instance_method_lookup, perry_plugin_table_lookup, perry_system_table_lookup,
    perry_ui_instance_method_lookup, perry_ui_table_lookup, perry_updater_table_lookup,
};

mod box_style;
mod jsonwebtoken;
mod perf_hooks;

use box_style::apply_box_style;
use jsonwebtoken::{lower_jsonwebtoken_sign, lower_jsonwebtoken_verify};

pub(crate) fn lower_native_method_call(
    ctx: &mut FnCtx<'_>,
    module: &str,
    class_name: Option<&str>,
    method: &str,
    object: Option<&Expr>,
    args: &[Expr],
) -> Result<String> {
    // Web Fetch API dispatch — Response / Headers / Request / static
    // factories. Handled before the receiver-less early-out so that
    // `Response.json(v)` (object.is_none()) finds its runtime function.
    if let Some(val) = lower_fetch_native_method(ctx, module, method, object, args)? {
        return Ok(val);
    }

    // `perry/i18n.t(key, params?)` is the i18n entry point. The
    // perry-transform i18n pass already replaced the first arg with
    // an `Expr::I18nString { key, string_idx, params, ... }` containing
    // all the metadata the codegen needs to resolve the translation
    // at compile time. The wrapping `t()` call is therefore identity:
    // we just lower `args[0]` (the I18nString) and return its value.
    // Without this case, the receiver-less early-out below would
    // discard the I18nString and return `double 0.0`, which prints
    // as `0` instead of the translated text — the symptom that broke
    // the v0.5.7 i18n test before this fix landed.
    if module == "perry/i18n" && method == "t" && object.is_none() {
        if let Some(first) = args.first() {
            return lower_expr(ctx, first);
        }
    }

    // Node util.types predicate calls lower to the canonical receiver-less
    // NativeMethodCall { module: "util/types", class_name: None, ... }.
    if module == "util/types" && class_name.is_none() && object.is_none() {
        let runtime = match method {
            "isPromise" => Some("js_util_types_is_promise"),
            "isArrayBuffer" | "isAnyArrayBuffer" => Some("js_util_types_is_array_buffer"),
            "isArrayBufferView" => Some("js_util_types_is_array_buffer_view"),
            "isTypedArray" => Some("js_util_types_is_typed_array"),
            "isUint8Array" => Some("js_util_types_is_uint8_array"),
            "isUint16Array" => Some("js_util_types_is_uint16_array"),
            "isInt32Array" => Some("js_util_types_is_int32_array"),
            "isFloat64Array" => Some("js_util_types_is_float64_array"),
            "isMap" => Some("js_util_types_is_map"),
            "isSet" => Some("js_util_types_is_set"),
            "isDate" => Some("js_util_types_is_date"),
            "isRegExp" => Some("js_util_types_is_reg_exp"),
            _ => None,
        };
        if let Some(runtime) = runtime {
            let value = if let Some(first) = args.first() {
                lower_expr(ctx, first)?
            } else {
                crate::nanbox::double_literal(0.0)
            };
            return Ok(ctx.block().call(DOUBLE, runtime, &[(DOUBLE, &value)]));
        }
    }

    if module == "jsonwebtoken" && method == "sign" && object.is_none() {
        return lower_jsonwebtoken_sign(ctx, args);
    }
    if module == "jsonwebtoken" && method == "verify" && object.is_none() {
        return lower_jsonwebtoken_verify(ctx, args);
    }

    // node:perf_hooks → native/perf_hooks.rs (performance.* + PerformanceObserver).
    if let Some(v) = perf_hooks::lower_perf_hooks_method(ctx, module, method, object, args)? {
        return Ok(v);
    }

    if module == "crypto"
        && class_name == Some("ECDH")
        && method == "convertKey"
        && object.is_none()
    {
        let mut lowered = Vec::with_capacity(5);
        for i in 0..5 {
            lowered.push(if let Some(arg) = args.get(i) {
                lower_expr(ctx, arg)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            });
        }
        let blk = ctx.block();
        return Ok(blk.call(
            DOUBLE,
            "js_crypto_ecdh_convert_key",
            &[
                (DOUBLE, &lowered[0]),
                (DOUBLE, &lowered[1]),
                (DOUBLE, &lowered[2]),
                (DOUBLE, &lowered[3]),
                (DOUBLE, &lowered[4]),
            ],
        ));
    }

    // `perry/ui.App({ title, width, height, body, icon? })` — minimum-viable
    // dispatch so a perry/ui app actually launches an NSApplication and
    // shows a window. Pre-v0.5.10 this fell into the receiver-less early-
    // out below and returned `double 0.0`, so the program completed
    // without entering the AppKit run loop — mango compiled cleanly but
    // exited immediately on launch with no output. This is the smallest
    // dispatch that proves the linking + runtime + Mach-O code path works
    // end to end. Other perry/ui constructors (Text, Button, VStack,
    // HStack, etc.) are NOT dispatched yet so the body is the
    // zero-sentinel — the window appears with the right title/size but
    // no widget tree. Full widget dispatch is a separate followup.
    // perry/tui Text(content, { fg, bg, bold, italic, underline, reverse }) —
    // the second-arg options form for #405 Phase 3.5 styling. Dispatches to
    // `js_perry_tui_text_styled` with the four-color/style args; the bare
    // 1-arg `Text(content)` form keeps falling through to the regular
    // PERRY_UI_TABLE dispatch which routes to `js_perry_tui_text`. Object
    // literals reach this point as `Expr::New { class_name: __AnonShape_… }`
    // — use `extract_options_fields` to pull the fields out either way.
    if module == "perry/tui" && method == "Text" && object.is_none() && args.len() >= 2 {
        if let Some(props) = extract_options_fields(ctx, &args[1]) {
            let content_ptr = get_raw_string_ptr(ctx, &args[0])?;
            let mut fg_str = Expr::String(String::new());
            let mut bg_str = Expr::String(String::new());
            let mut style_bits: u8 = 0;
            for (key, val) in &props {
                match key.as_str() {
                    "fg" | "color" => fg_str = val.clone(),
                    "bg" | "backgroundColor" => bg_str = val.clone(),
                    "bold" => {
                        if matches!(val, Expr::Bool(true)) {
                            style_bits |= 0b0001;
                        }
                    }
                    "italic" => {
                        if matches!(val, Expr::Bool(true)) {
                            style_bits |= 0b0010;
                        }
                    }
                    "underline" => {
                        if matches!(val, Expr::Bool(true)) {
                            style_bits |= 0b0100;
                        }
                    }
                    // ink uses "inverse"; #358 used "reverse". Accept both.
                    "reverse" | "inverse" => {
                        if matches!(val, Expr::Bool(true)) {
                            style_bits |= 0b0000_1000;
                        }
                    }
                    // ink-shape parity (#679 Phase 5): dimColor + strikethrough.
                    "dimColor" | "dim" => {
                        if matches!(val, Expr::Bool(true)) {
                            style_bits |= 0b0001_0000;
                        }
                    }
                    "strikethrough" => {
                        if matches!(val, Expr::Bool(true)) {
                            style_bits |= 0b0010_0000;
                        }
                    }
                    _ => {}
                }
            }
            let fg_ptr = get_raw_string_ptr(ctx, &fg_str)?;
            let bg_ptr = get_raw_string_ptr(ctx, &bg_str)?;
            let bits_lit = double_literal(style_bits as f64);
            ctx.pending_declares.push((
                "js_perry_tui_text_styled".to_string(),
                I64,
                vec![I64, I64, I64, DOUBLE],
            ));
            let handle = ctx.block().call(
                I64,
                "js_perry_tui_text_styled",
                &[
                    (I64, &content_ptr),
                    (I64, &fg_ptr),
                    (I64, &bg_ptr),
                    (DOUBLE, &bits_lit),
                ],
            );
            return Ok(nanbox_pointer_inline(ctx.block(), &handle));
        }
    }

    // perry/tui Input(value, cursor) — 2-arg form for arbitrary-position
    // cursor. The runtime decomposes into a row Box of [before, cursor,
    // after] Text widgets so the cursor character draws with reverse
    // video at the right offset. The 1-arg `Input(value)` form falls
    // through to the regular dispatch table. (#404.)
    if module == "perry/tui" && method == "Input" && object.is_none() && args.len() >= 2 {
        let content_ptr = get_raw_string_ptr(ctx, &args[0])?;
        let cursor = lower_expr(ctx, &args[1])?;
        ctx.pending_declares
            .push(("js_perry_tui_input_at".to_string(), I64, vec![I64, DOUBLE]));
        let handle = ctx.block().call(
            I64,
            "js_perry_tui_input_at",
            &[(I64, &content_ptr), (DOUBLE, &cursor)],
        );
        return Ok(nanbox_pointer_inline(ctx.block(), &handle));
    }

    // perry/tui AnimatedSpinner({ interval, frames }) — unpacks the
    // options object and dispatches to `js_perry_tui_animated_spinner`.
    // Both opts are optional; the runtime falls back to 100 ms /
    // ['-', '\\', '|', '/']. Handles 0-arg, 1-arg-options, and 1-arg-
    // non-options (treated as default) call shapes here so bare
    // `AnimatedSpinner()` doesn't trip over the dispatch table's
    // 2-arg arity expectation. (#403.)
    if module == "perry/tui" && method == "AnimatedSpinner" && object.is_none() {
        let mut interval_expr: Expr = Expr::Number(0.0);
        let mut frames_expr: Option<Expr> = None;
        if let Some(first) = args.first() {
            if let Some(props) = extract_options_fields(ctx, first) {
                for (k, v) in &props {
                    match k.as_str() {
                        "interval" => interval_expr = v.clone(),
                        "frames" => frames_expr = Some(v.clone()),
                        _ => {}
                    }
                }
            }
        }
        let interval = lower_expr(ctx, &interval_expr)?;
        let frames = match frames_expr {
            Some(e) => lower_expr(ctx, &e)?,
            None => double_literal(0.0),
        };
        let frames_h = unbox_to_i64(ctx.block(), &frames);
        ctx.pending_declares.push((
            "js_perry_tui_animated_spinner".to_string(),
            I64,
            vec![DOUBLE, I64],
        ));
        let handle = ctx.block().call(
            I64,
            "js_perry_tui_animated_spinner",
            &[(DOUBLE, &interval), (I64, &frames_h)],
        );
        return Ok(nanbox_pointer_inline(ctx.block(), &handle));
    }

    // perry/tui Table({ headers, rows, selected }) — unpacks the options
    // object and dispatches to `js_perry_tui_table(headers_ptr, rows_ptr,
    // selected_idx)`. The 2D `rows` array is passed through unchanged;
    // the runtime walks it via `read_string_2d_array`. (#402.)
    if module == "perry/tui" && method == "Table" && object.is_none() && !args.is_empty() {
        if let Some(props) = extract_options_fields(ctx, &args[0]) {
            let mut headers_expr: Option<Expr> = None;
            let mut rows_expr: Option<Expr> = None;
            let mut selected_expr: Expr = Expr::Number(-1.0);
            for (k, v) in &props {
                match k.as_str() {
                    "headers" => headers_expr = Some(v.clone()),
                    "rows" => rows_expr = Some(v.clone()),
                    "selected" => selected_expr = v.clone(),
                    _ => {}
                }
            }
            let headers = match headers_expr {
                Some(e) => lower_expr(ctx, &e)?,
                None => double_literal(0.0),
            };
            let rows = match rows_expr {
                Some(e) => lower_expr(ctx, &e)?,
                None => double_literal(0.0),
            };
            let selected = lower_expr(ctx, &selected_expr)?;
            // Unbox the array pointers (NaN-boxed POINTER) into raw i64.
            let blk = ctx.block();
            let headers_h = unbox_to_i64(blk, &headers);
            let rows_h = unbox_to_i64(blk, &rows);
            ctx.pending_declares.push((
                "js_perry_tui_table".to_string(),
                I64,
                vec![I64, I64, DOUBLE],
            ));
            let handle = ctx.block().call(
                I64,
                "js_perry_tui_table",
                &[(I64, &headers_h), (I64, &rows_h), (DOUBLE, &selected)],
            );
            return Ok(nanbox_pointer_inline(ctx.block(), &handle));
        }
    }

    // perry/tui Tabs({ tabs, active, body }) — unpacks the options
    // object and dispatches to `js_perry_tui_tabs(tabs_ptr, active,
    // body_ptr)`. `body` is an array of widget handles; only the
    // active tab's body is mounted. (#402.)
    if module == "perry/tui" && method == "Tabs" && object.is_none() && !args.is_empty() {
        if let Some(props) = extract_options_fields(ctx, &args[0]) {
            let mut tabs_expr: Option<Expr> = None;
            let mut active_expr: Expr = Expr::Number(0.0);
            let mut body_expr: Option<Expr> = None;
            for (k, v) in &props {
                match k.as_str() {
                    "tabs" => tabs_expr = Some(v.clone()),
                    "active" => active_expr = v.clone(),
                    "body" => body_expr = Some(v.clone()),
                    _ => {}
                }
            }
            let tabs = match tabs_expr {
                Some(e) => lower_expr(ctx, &e)?,
                None => double_literal(0.0),
            };
            let active = lower_expr(ctx, &active_expr)?;
            let body = match body_expr {
                Some(e) => lower_expr(ctx, &e)?,
                None => double_literal(0.0),
            };
            let blk = ctx.block();
            let tabs_h = unbox_to_i64(blk, &tabs);
            let body_h = unbox_to_i64(blk, &body);
            ctx.pending_declares.push((
                "js_perry_tui_tabs".to_string(),
                I64,
                vec![I64, DOUBLE, I64],
            ));
            let handle = ctx.block().call(
                I64,
                "js_perry_tui_tabs",
                &[(I64, &tabs_h), (DOUBLE, &active), (I64, &body_h)],
            );
            return Ok(nanbox_pointer_inline(ctx.block(), &handle));
        }
    }

    // perry/tui Box — TS shapes:
    //   Box()                                — empty container
    //   Box([child, …])                      — children array (Phase 1)
    //   Box({ flexDirection, gap, … }, [child, …])  — style + children (Phase 3)
    //   Box({ flexDirection, gap, … })       — style, no children
    //
    // Detect which by examining args[0]: an array → children-only;
    // an object/object-shape → style; followed by an array → children.
    // Mirrors the perry/ui VStack pattern: create handle, optionally
    // emit per-style-field setter calls, then iterate the children
    // array calling add_child per element. Bare `Box()` falls through
    // to the regular PERRY_UI_TABLE dispatch (just emits js_perry_tui_box).
    // (#358 Phases 1 + 3.)
    if module == "perry/tui" && method == "Box" && object.is_none() && !args.is_empty() {
        // Note: js_perry_tui_box returns I64 (raw handle); the
        // dispatch table's NR_PTR contract NaN-boxes it for the
        // outer call. The special-case path here mirrors that — call
        // returns I64, store in an I64 slot, NaN-box at the very end
        // when handing off to the caller.
        ctx.pending_declares
            .push(("js_perry_tui_box".to_string(), I64, vec![]));
        ctx.pending_declares.push((
            "js_perry_tui_box_add_child".to_string(),
            DOUBLE,
            vec![I64, I64],
        ));
        let blk = ctx.block();
        let parent_handle = blk.call(I64, "js_perry_tui_box", &[]);
        let parent_slot = ctx.func.alloca_entry(I64);
        ctx.block().store(I64, &parent_handle, &parent_slot);

        // Determine which arg is the style-options object and which
        // is the children array.
        //
        // 2-arg shape `Box(opts, children)` — first is always style,
        // second is always children, regardless of whether `children`
        // is a literal array or a runtime value like `msgs.map(...)`.
        // The old structural classifier only recognised `Expr::Array`
        // as children, so `Box(opts, runtimeArr)` silently dropped the
        // children. (#679 follow-up.)
        //
        // 1-arg shape: classify structurally — an Object-shaped
        // expression is style, anything else is children.
        let mut style_arg: Option<&Expr> = None;
        let mut children_arg: Option<&Expr> = None;
        if args.len() >= 2 {
            style_arg = Some(&args[0]);
            children_arg = Some(&args[1]);
        } else if let Some(arg) = args.first() {
            match arg {
                Expr::Array(_) | Expr::ArraySpread(_) => children_arg = Some(arg),
                Expr::Object(_) | Expr::New { .. } => style_arg = Some(arg),
                // Bare identifier / call / etc. — most TS programs
                // use this for children, e.g. `Box(rows)` where
                // `rows = messages.map(…)`. Treat as children.
                _ => children_arg = Some(arg),
            }
        }

        // Emit per-field style setter calls if a style object was
        // recognized. Each known field maps to one js_perry_tui_box_set_*
        // FFI; unknown fields are silently dropped (forward-compat
        // for future style props).
        if let Some(style) = style_arg {
            apply_box_style(ctx, &parent_slot, style)?;
        }

        if let Some(children_expr) = children_arg {
            let elements_owned: Option<Vec<Expr>> = match children_expr {
                Expr::Array(elems) => Some(elems.clone()),
                _ => None,
            };
            if let Some(elements) = elements_owned {
                for child in &elements {
                    let child_box = lower_expr(ctx, child)?;
                    let blk = ctx.block();
                    let child_handle = unbox_to_i64(blk, &child_box);
                    let parent_reload = blk.load(I64, &parent_slot);
                    blk.call_void(
                        "js_perry_tui_box_add_child",
                        &[(I64, &parent_reload), (I64, &child_handle)],
                    );
                }
            } else {
                // Non-literal children (e.g. `Box(messages.map(m => Text(m)))`)
                // — lower to a runtime array pointer + delegate iteration
                // to `js_perry_tui_box_add_children_array`. Pre-#679-follow-up
                // this branch dropped the result and the Box ended up empty.
                let children_box = lower_expr(ctx, children_expr)?;
                let blk = ctx.block();
                let children_handle = unbox_to_i64(blk, &children_box);
                ctx.pending_declares.push((
                    "js_perry_tui_box_add_children_array".to_string(),
                    DOUBLE,
                    vec![I64, I64],
                ));
                let blk = ctx.block();
                let parent_reload = blk.load(I64, &parent_slot);
                blk.call(
                    DOUBLE,
                    "js_perry_tui_box_add_children_array",
                    &[(I64, &parent_reload), (I64, &children_handle)],
                );
            }
        }

        let blk = ctx.block();
        let parent_final = blk.load(I64, &parent_slot);
        // NaN-box the handle into a POINTER-tagged f64 — same as the
        // dispatch table's NR_PTR contract.
        return Ok(nanbox_pointer_inline(blk, &parent_final));
    }

    // perry/ui VStack/HStack — special-case because the TS shape is
    // `VStack(spacing, [child1, child2, ...])` (or just `VStack([...])`),
    // but the runtime takes only `(spacing) -> handle` and children get
    // added one by one via `perry_ui_widget_add_child`. We can't express
    // this with the per-method table because it's variadic in arg shape
    // *and* needs sequential calls per child.
    if module == "perry/ui" && (method == "VStack" || method == "HStack") && object.is_none() {
        let runtime_create = if method == "VStack" {
            "perry_ui_vstack_create"
        } else {
            "perry_ui_hstack_create"
        };
        // First arg may be the spacing number OR the children array
        // (when the user calls `VStack([children])` without an explicit
        // spacing). Detect which by checking the type.
        let (spacing_d, children_idx) = match args.first() {
            Some(Expr::Array(_)) | Some(Expr::ArraySpread(_)) => ("8.0".to_string(), 0),
            Some(other) => {
                // Could be a number (spacing) — lower it. The children
                // are then in args[1] (if present).
                let v = lower_expr(ctx, other)?;
                (v, 1)
            }
            None => ("8.0".to_string(), 0),
        };
        ctx.pending_declares
            .push((runtime_create.to_string(), I64, vec![DOUBLE]));
        let blk = ctx.block();
        let parent_handle = blk.call(I64, runtime_create, &[(DOUBLE, &spacing_d)]);
        // Stash so add_child has it; we'll need to reload later because
        // calls between here and the loop may invalidate `parent_handle`'s
        // SSA name in subsequent blocks.
        let parent_slot = ctx.func.alloca_entry(I64);
        ctx.block().store(I64, &parent_handle, &parent_slot);

        // Walk the children array (if present). For each element, lower
        // to a JSValue, unbox to widget handle, call
        // `perry_ui_widget_add_child(parent, child)`.
        ctx.pending_declares.push((
            "perry_ui_widget_add_child".to_string(),
            crate::types::VOID,
            vec![I64, I64],
        ));
        if let Some(children_expr) = args.get(children_idx) {
            let elements_owned: Option<Vec<Expr>> = match children_expr {
                Expr::Array(elems) => Some(elems.clone()),
                _ => None,
            };
            if let Some(elements) = elements_owned {
                for child in &elements {
                    let child_box = lower_expr(ctx, child)?;
                    let blk = ctx.block();
                    let child_handle = unbox_to_i64(blk, &child_box);
                    let parent_reload = blk.load(I64, &parent_slot);
                    blk.call_void(
                        "perry_ui_widget_add_child",
                        &[(I64, &parent_reload), (I64, &child_handle)],
                    );
                }
            } else {
                // Children expression isn't a literal array — emit an
                // inline LLVM loop that walks the runtime array and calls
                // `perry_ui_widget_add_child` for each element. Without
                // this, `for (const x of xs) ys.push(chip(x));
                // HStack(8, ys)` and similar patterns silently dropped
                // every loop-built widget (#634); only the literal-array
                // shape produced render output.
                let arr_d = lower_expr(ctx, children_expr)?;
                let arr_ptr = {
                    let blk = ctx.block();
                    unbox_to_i64(blk, &arr_d)
                };
                ctx.pending_declares
                    .push(("js_array_get_length".to_string(), I64, vec![I64]));
                let len = ctx
                    .block()
                    .call(I64, "js_array_get_length", &[(I64, &arr_ptr)]);

                let i_slot = ctx.func.alloca_entry(I64);
                ctx.block().store(I64, "0", &i_slot);

                let header_idx = ctx.new_block("ui_addch.header");
                let body_idx = ctx.new_block("ui_addch.body");
                let exit_idx = ctx.new_block("ui_addch.exit");
                let header_label = ctx.block_label(header_idx);
                let body_label = ctx.block_label(body_idx);
                let exit_label = ctx.block_label(exit_idx);
                ctx.block().br(&header_label);

                ctx.current_block = header_idx;
                let i_h = ctx.block().load(I64, &i_slot);
                let cmp = ctx.block().icmp_slt(I64, &i_h, &len);
                ctx.block().cond_br(&cmp, &body_label, &exit_label);

                ctx.current_block = body_idx;
                ctx.pending_declares.push((
                    "js_array_get_element".to_string(),
                    DOUBLE,
                    vec![I64, I64],
                ));
                let i_b = ctx.block().load(I64, &i_slot);
                let elem_d = ctx.block().call(
                    DOUBLE,
                    "js_array_get_element",
                    &[(I64, &arr_ptr), (I64, &i_b)],
                );
                let child_handle = {
                    let blk = ctx.block();
                    unbox_to_i64(blk, &elem_d)
                };
                let parent_reload = ctx.block().load(I64, &parent_slot);
                ctx.block().call_void(
                    "perry_ui_widget_add_child",
                    &[(I64, &parent_reload), (I64, &child_handle)],
                );
                let one_l = "1".to_string();
                let i_next = ctx.block().add(I64, &i_b, &one_l);
                ctx.block().store(I64, &i_next, &i_slot);
                ctx.block().br(&header_label);

                ctx.current_block = exit_idx;
            }
        }

        // Issue #185 Phase C step 5: optional inline `style: { ... }`
        // arg AFTER the children array. Position depends on whether
        // spacing was passed first:
        //   VStack(children, style?)              children_idx=0, style at args[1]
        //   VStack(spacing, children, style?)     children_idx=1, style at args[2]
        // `apply_inline_style` no-ops on non-object trailing args, so
        // the call is safe even when it's accidentally something else.
        let style_idx = children_idx + 1;
        if let Some(style_arg) = args.get(style_idx).cloned() {
            let parent_handle_str = ctx.block().load(I64, &parent_slot);
            apply_inline_style(ctx, &parent_handle_str, &style_arg)?;
        }

        let blk = ctx.block();
        let parent_final = blk.load(I64, &parent_slot);
        return Ok(nanbox_pointer_inline(blk, &parent_final));
    }

    // perry/ui ForEach — TS shape is `ForEach(state, (i) => Widget)`. The
    // runtime's `perry_ui_for_each_init` wants `(container, state, closure)`,
    // so we synthesize a VStack container, call for_each_init with it, and
    // return the container handle. Without this special case the call falls
    // through to the generic dispatch which emits the "method 'ForEach' not
    // in dispatch table" warning and returns 0/undefined — the outer VStack
    // then tries to add_child with an invalid handle, AppKit silently fails
    // to attach the window body, and the process runs but no window shows.
    if module == "perry/ui" && method == "ForEach" && object.is_none() && args.len() == 2 {
        ctx.pending_declares
            .push(("perry_ui_vstack_create".to_string(), I64, vec![DOUBLE]));
        ctx.pending_declares.push((
            "perry_ui_for_each_init".to_string(),
            crate::types::VOID,
            vec![I64, I64, DOUBLE],
        ));

        let spacing = "8.0".to_string();
        let blk = ctx.block();
        let container = blk.call(I64, "perry_ui_vstack_create", &[(DOUBLE, &spacing)]);
        let container_slot = ctx.func.alloca_entry(I64);
        ctx.block().store(I64, &container, &container_slot);

        // args[0]: State handle — NaN-boxed pointer, unbox to i64.
        let state_box = lower_expr(ctx, &args[0])?;
        let blk = ctx.block();
        let state_handle = unbox_to_i64(blk, &state_box);

        // args[1]: render closure — stays as a NaN-boxed f64.
        let closure_d = lower_expr(ctx, &args[1])?;

        let blk = ctx.block();
        let container_reload = blk.load(I64, &container_slot);
        blk.call_void(
            "perry_ui_for_each_init",
            &[
                (I64, &container_reload),
                (I64, &state_handle),
                (DOUBLE, &closure_d),
            ],
        );

        let blk = ctx.block();
        let container_final = blk.load(I64, &container_slot);
        return Ok(nanbox_pointer_inline(blk, &container_final));
    }

    // perry/ui Text(content, id) — 2-arg form registers the widget in the
    // per-platform text registry so setText(id, val) can update it later.
    // The 1-arg form `Text(content)` routes through the PERRY_UI_TABLE entry
    // (perry_ui_text_create) as normal; only the 2-arg form is intercepted here.
    if module == "perry/ui" && method == "Text" && object.is_none() && args.len() == 2 {
        let text_ptr = get_raw_string_ptr(ctx, &args[0])?;
        let id_ptr = get_raw_string_ptr(ctx, &args[1])?;
        ctx.pending_declares.push((
            "perry_ui_text_create_with_id".to_string(),
            I64,
            vec![I64, I64],
        ));
        let blk = ctx.block();
        let handle = blk.call(
            I64,
            "perry_ui_text_create_with_id",
            &[(I64, &text_ptr), (I64, &id_ptr)],
        );
        // Optional trailing style arg (position 2) — same pattern as Button.
        if let Some(style_arg) = args.get(2).cloned() {
            apply_inline_style(ctx, &handle, &style_arg)?;
        }
        let blk = ctx.block();
        return Ok(nanbox_pointer_inline(blk, &handle));
    }

    // perry/ui Button — TS shape is `Button(label, handler)` where
    // handler is a closure. The simple positional form is what mango
    // uses. The Object-config form (`Button(label, { onPress: cb })`)
    // is a followup.
    if module == "perry/ui" && method == "Button" && object.is_none() {
        let label_ptr = if let Some(label) = args.first() {
            get_raw_string_ptr(ctx, label)?
        } else {
            "0".to_string()
        };
        let handler_d = if let Some(handler) = args.get(1) {
            lower_expr(ctx, handler)?
        } else {
            "0.0".to_string()
        };
        ctx.pending_declares
            .push(("perry_ui_button_create".to_string(), I64, vec![I64, DOUBLE]));
        // Scope `blk` so the mutable borrow on `ctx` is released before
        // we call `apply_inline_style(ctx, ...)`, which re-borrows.
        let handle = {
            let blk = ctx.block();
            blk.call(
                I64,
                "perry_ui_button_create",
                &[(I64, &label_ptr), (DOUBLE, &handler_d)],
            )
        };

        // Issue #185 Phase C step 2: optional trailing `style` arg.
        // `Button(label, onPress, { borderRadius, opacity, ... })`
        // destructures the StyleProps object at HIR time and emits a
        // sequence of setter calls against the just-created handle.
        // Mirrors the v0.5.x `App({ title, width, height, body })` HIR
        // pass — same `extract_options_fields` helper, same per-key
        // routing. Step 2 covers single-value scalar props; colors /
        // padding / shadow / gradient need multi-arg destructure and
        // land in step 3.
        if let Some(style_arg) = args.get(2) {
            apply_inline_style(ctx, &handle, style_arg)?;
        }

        let blk = ctx.block();
        return Ok(nanbox_pointer_inline(blk, &handle));
    }

    // Generic perry/ui receiver-less dispatch via a per-method table.
    // Constructors and setters that don't need special arg shape handling
    // (object literals, children arrays, closures stored in side tables)
    // route through here. Each entry declares the runtime function name
    // plus the arg coercion + return boxing rules.
    //
    // The table covers ~80% of mango's perry/ui surface. Special cases
    // (App with object literal, VStack/HStack with children array,
    // Button with optional Object config) are handled in dedicated
    // arms BELOW so they short-circuit before this table is consulted.
    //
    // Extending: add a row to PERRY_UI_TABLE matching the TS method name
    // to the perry_ui_* runtime function and arg shape. Most setters
    // follow `(widget, …number args)` and most constructors return a
    // widget handle that gets NaN-boxed as POINTER on the way out.
    // perry/ui.showToast(msg) — Phase 2 v3 Option 1. Enqueues `msg`
    // into the runtime's drain queue; the auto-emitted .ets onClick
    // pumps the queue into ArkUI's `promptAction.showToast` after the
    // closure body returns. On non-harmonyos targets the runtime FFI
    // is still defined (just with empty queue + no consumer) so
    // cross-platform code compiles, but only harmonyos shows visual
    // feedback. Future v3 follow-up: route to NSAlert/UIAlertController/
    // GtkPopover on the desktop UI backends.
    if module == "perry/ui" && method == "showToast" && object.is_none() {
        if args.is_empty() {
            return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
        }
        let msg_d = lower_expr(ctx, &args[0])?;
        ctx.pending_declares.push((
            "perry_arkts_show_toast".to_string(),
            crate::types::VOID,
            vec![DOUBLE],
        ));
        let blk = ctx.block();
        blk.call_void("perry_arkts_show_toast", &[(DOUBLE, &msg_d)]);
        return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
    }

    // perry/ui.setText(id, value) — Phase 2 v3 Option 2 reactive Text.
    // Enqueues a (id, value) update; the auto-emitted .ets onClick
    // pumps the queue into the matching `@State text_<id>` after the
    // closure body returns. Same drain-pattern shape as showToast.
    if module == "perry/ui" && method == "setText" && object.is_none() {
        if args.len() < 2 {
            return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
        }
        let id_d = lower_expr(ctx, &args[0])?;
        let val_d = lower_expr(ctx, &args[1])?;
        ctx.pending_declares.push((
            "perry_arkts_set_text".to_string(),
            crate::types::VOID,
            vec![DOUBLE, DOUBLE],
        ));
        let blk = ctx.block();
        blk.call_void("perry_arkts_set_text", &[(DOUBLE, &id_d), (DOUBLE, &val_d)]);
        return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
    }

    // Issue #535 — perry/ui `state<T>` desugar trio. Synthetic methods
    // emitted only by `crates/perry-transform/src/state_desugar.rs`.
    if module == "perry/ui"
        && (method == "__state_init" || method == "__state_set")
        && object.is_none()
    {
        if args.len() != 2 {
            return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
        }
        let id_d = lower_expr(ctx, &args[0])?;
        let val_d = lower_expr(ctx, &args[1])?;
        let runtime_fn = if method == "__state_init" {
            "js_state_init"
        } else {
            "js_state_set"
        };
        ctx.pending_declares.push((
            runtime_fn.to_string(),
            crate::types::VOID,
            vec![DOUBLE, DOUBLE],
        ));
        let blk = ctx.block();
        blk.call_void(runtime_fn, &[(DOUBLE, &id_d), (DOUBLE, &val_d)]);
        return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
    }
    if module == "perry/ui" && method == "__state_get" && object.is_none() {
        if args.len() != 1 {
            return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
        }
        let id_d = lower_expr(ctx, &args[0])?;
        ctx.pending_declares
            .push(("js_state_get".to_string(), DOUBLE, vec![DOUBLE]));
        let blk = ctx.block();
        let result = blk.call(DOUBLE, "js_state_get", &[(DOUBLE, &id_d)]);
        return Ok(result);
    }

    // Issue #610 — `__foreach_register(synth_id, host, render_closure)`
    // synthetic method emitted by state_desugar's `ForEach(stateBinding,
    // render)` rewrite. Forwards (synth_id, host_handle, render_closure)
    // to the runtime registry. The runtime walks this map on every
    // js_state_set for the matching synth id, calling the platform's
    // foreach-render handler with the new count value — the platform
    // crate (perry-ui-macos / perry-ui-gtk4 / etc.) clears the host's
    // children, calls render_closure(i) for each i in [0..count), and
    // adds each returned widget.
    if module == "perry/ui" && method == "__foreach_register" && object.is_none() {
        if args.len() != 3 {
            return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
        }
        let synth_id_d = lower_expr(ctx, &args[0])?;
        let host_d = lower_expr(ctx, &args[1])?;
        let host_i64 = unbox_to_i64(ctx.block(), &host_d);
        let render_d = lower_expr(ctx, &args[2])?;
        ctx.pending_declares.push((
            "js_foreach_register".to_string(),
            crate::types::VOID,
            vec![DOUBLE, I64, DOUBLE],
        ));
        ctx.block().call_void(
            "js_foreach_register",
            &[(DOUBLE, &synth_id_d), (I64, &host_i64), (DOUBLE, &render_d)],
        );
        return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
    }

    // Issue #535 Layer 2 — `__navstack_register_route(synth_id, name, body)`
    // synthetic method emitted by state_desugar's NavStack(state, routes)
    // rewrite. Lowers `body` to a widget handle (NaN-boxed pointer →
    // unbox to i64) and forwards (synth_id, name, handle) to the runtime
    // registry. The runtime walks this map on every js_state_set for the
    // matching synth id, toggling each route's NSView.isHidden via the
    // platform handler registered by perry-ui-macos at app startup.
    if module == "perry/ui" && method == "__navstack_register_route" && object.is_none() {
        if args.len() != 3 {
            return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
        }
        let synth_id_d = lower_expr(ctx, &args[0])?;
        let name_d = lower_expr(ctx, &args[1])?;
        let body_d = lower_expr(ctx, &args[2])?;
        let body_i64 = unbox_to_i64(ctx.block(), &body_d);
        ctx.pending_declares.push((
            "js_navstack_register_route".to_string(),
            crate::types::VOID,
            vec![DOUBLE, DOUBLE, I64],
        ));
        ctx.block().call_void(
            "js_navstack_register_route",
            &[(DOUBLE, &synth_id_d), (DOUBLE, &name_d), (I64, &body_i64)],
        );
        // Return the body handle (already NaN-boxed) so the rewrite can
        // chain by binding the result as the route's host child.
        return Ok(body_d);
    }

    // perry/arkts: HarmonyOS Phase 2 v2 callback bridge. Synthetic module
    // injected by the harvest pass (`compile.rs::emit_index_ets`) — never
    // user-authored. `registerCallback(idx, closure)` lowers to a call to
    // the runtime FFI `perry_arkts_register_callback(i64, f64)` which
    // stores the closure pointer in a slot table that NAPI's
    // `invokeCallback(idx)` dispatches against on ArkUI tap events.
    if module == "perry/arkts" && method == "registerCallback" && object.is_none() {
        if args.len() != 2 {
            bail!(
                "perry/arkts.registerCallback expects (idx, closure), got {} args",
                args.len()
            );
        }
        let idx_d = lower_expr(ctx, &args[0])?;
        let closure_d = lower_expr(ctx, &args[1])?;
        ctx.pending_declares.push((
            "perry_arkts_register_callback".to_string(),
            crate::types::VOID,
            vec![I64, DOUBLE],
        ));
        let blk = ctx.block();
        let idx_i64 = blk.fptosi(DOUBLE, &idx_d, I64);
        blk.call_void(
            "perry_arkts_register_callback",
            &[(I64, &idx_i64), (DOUBLE, &closure_d)],
        );
        return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
    }

    // perry/system dispatch: audioStart, audioGetLevel, getDeviceModel, etc.
    if module == "perry/system" && object.is_none() {
        if method == "notificationSchedule" {
            return lower_notification_schedule(ctx, args);
        }
        if args.is_empty() {
            match method {
                "getAppVersion" => {
                    let version = ctx.app_metadata.version.clone();
                    let idx = ctx.strings.intern(&version);
                    let handle_global = format!("@{}", ctx.strings.entry(idx).handle_global);
                    return Ok(ctx.block().load(DOUBLE, &handle_global));
                }
                "getAppBuildNumber" => {
                    return Ok(double_literal(ctx.app_metadata.build_number as f64));
                }
                "getBundleId" => {
                    let bundle_id = ctx.app_metadata.bundle_id.clone();
                    let idx = ctx.strings.intern(&bundle_id);
                    let handle_global = format!("@{}", ctx.strings.entry(idx).handle_global);
                    return Ok(ctx.block().load(DOUBLE, &handle_global));
                }
                _ => {}
            }
        }
        if let Some(sig) = perry_system_table_lookup(method) {
            return lower_perry_ui_table_call(ctx, sig, args);
        }
    }

    // perry/media dispatch: createPlayer, play, pause, seek, setVolume,
    // onStateChange, onTimeUpdate, setNowPlaying, destroy. Streaming
    // media playback backed by AVPlayer (Apple), MediaPlayer/JNI
    // (Android), GStreamer (GTK4/Linux), Media Foundation (Windows).
    if module == "perry/media" && object.is_none() {
        if let Some(sig) = perry_media_table_lookup(method) {
            return lower_perry_ui_table_call(ctx, sig, args);
        }
        bail!(
            "perry/media: '{}' is not a known function (args: {}). \
             Check types/perry/media/index.d.ts for the supported API surface.",
            method,
            args.len()
        );
    }

    // perry/i18n format wrappers: Currency, Percent, FormatNumber, ShortDate,
    // LongDate, FormatTime, Raw. Without this, the call falls through to the
    // receiver-less early-out and returns NaN-boxed `undefined` (issue #188).
    // `t()` is dispatched separately near the top of this function.
    if module == "perry/i18n" && object.is_none() {
        if let Some(sig) = perry_i18n_table_lookup(method) {
            return lower_perry_ui_table_call(ctx, sig, args);
        }
    }

    // perry/plugin dispatch: loadPlugin, listPlugins, emitHook, etc.
    if module == "perry/plugin" && object.is_none() {
        if let Some(sig) = perry_plugin_table_lookup(method) {
            return lower_perry_ui_table_call(ctx, sig, args);
        }
        bail!(
            "perry/plugin: '{}' is not a known function (args: {}). \
             Check types/perry/plugin/index.d.ts for the supported API surface.",
            method,
            args.len()
        );
    }

    // perry/updater dispatch: compareVersions, verifyHash, verifySignature,
    // sentinel state helpers, install, relaunch.
    if module == "perry/updater" && object.is_none() {
        if let Some(sig) = perry_updater_table_lookup(method) {
            return lower_perry_ui_table_call(ctx, sig, args);
        }
        bail!(
            "perry/updater: '{}' is not a known function (args: {}). \
             Check types/perry/updater/index.d.ts for the supported API surface.",
            method,
            args.len()
        );
    }

    // Phase 2 v3.3: `Text(content, id)` reactive form. The 1-arg
    // `Text(content)` row in PERRY_UI_TABLE doesn't know about the
    // optional `id` second arg — pre-fix the table-call's "if args.len()
    // == sig.args.len() + 1 ⇒ inline_style_arg" path absorbed it as a
    // would-be style object, then `apply_inline_style` silently no-op'd
    // because strings aren't object literals. Effect: id was dropped on
    // the floor and `setText("counter", ...)` had nothing to look up.
    //
    // Fix: detect Text-with-id BEFORE the table lookup, lower the
    // create call manually (mirroring the table-call shape), then
    // emit `perry_arkts_register_text_id(handle, id)` so the platform
    // UI lib can map id → widget handle. On harmonyos, codegen-arkts
    // emits `@State text_<id>` directly into the .ets and the
    // register_text_id call is a runtime no-op (see
    // perry-runtime/src/ui_text_registry.rs).
    if module == "perry/ui" && method == "Text" && object.is_none() && args.len() == 2 {
        let content_ptr = get_raw_string_ptr(ctx, &args[0])?;
        ctx.pending_declares
            .push(("perry_ui_text_create".to_string(), I64, vec![I64]));
        let handle = {
            let blk = ctx.block();
            blk.call(I64, "perry_ui_text_create", &[(I64, &content_ptr)])
        };
        // Lower the id arg as a regular NaN-boxed JS value so the
        // runtime's `decode_jsvalue_string` can read it through the
        // standard StringHeader path (handles SSO + heap strings the
        // same way, and matches the harmonyos drain-queue contract).
        let id_d = lower_expr(ctx, &args[1])?;
        ctx.pending_declares.push((
            "perry_arkts_register_text_id".to_string(),
            crate::types::VOID,
            vec![I64, DOUBLE],
        ));
        let blk = ctx.block();
        blk.call_void(
            "perry_arkts_register_text_id",
            &[(I64, &handle), (DOUBLE, &id_d)],
        );
        return Ok(nanbox_pointer_inline(blk, &handle));
    }

    if module == "perry/ui"
        && object.is_none()
        && method != "App"
        && method != "VStack"
        && method != "HStack"
        // Image + WebView have option-bag handlers further down that
        // do their own arg destructuring; they're not in perry_ui_table
        // so they must skip this catch-all bail.
        && method != "Image"
        && method != "WebView"
    {
        if let Some(sig) = perry_ui_table_lookup(method) {
            return lower_perry_ui_table_call(ctx, sig, args);
        }
        // Fail fast at compile time so a missing/misspelled method
        // surfaces as an error instead of silently returning 0.0 —
        // which used to compile, link, and run with a zero widget
        // handle (no window, or null-pointer crash at the caller).
        bail!(
            "perry/ui: '{}' is not a known function (args: {}). \
             Check the spelling and consult types/perry/ui/index.d.ts \
             for the supported API surface.",
            method,
            args.len()
        );
    }

    // perry/ui Image({ url, alt? }) — issue #635. The positional form
    // `Image(url, alt?)` is picked up by the perry_ui table below; the
    // object-literal form is destructured here into the same call shape
    // by extracting the `url` and `alt` fields and forwarding to the
    // table. Anything else on the object (placeholder / contentMode in
    // the documented surface) is silently dropped — those fields are
    // post-v1.
    if module == "perry/ui" && method == "Image" && object.is_none() && args.len() == 1 {
        if let Some(props) = extract_options_fields(ctx, &args[0]) {
            let mut url_arg: Option<Expr> = None;
            let mut alt_arg: Option<Expr> = None;
            for (key, val) in &props {
                match key.as_str() {
                    "url" => url_arg = Some(val.clone()),
                    "alt" => alt_arg = Some(val.clone()),
                    _ => {
                        // Lower for side effects so any nested closures
                        // are still collected.
                        let _ = lower_expr(ctx, val)?;
                    }
                }
            }
            if let Some(u) = url_arg {
                let positional = vec![u, alt_arg.unwrap_or_else(|| Expr::String(String::new()))];
                if let Some(sig) = perry_ui_table_lookup("Image") {
                    return lower_perry_ui_table_call(ctx, sig, &positional);
                }
            }
        }
    }

    // perry/ui WebView({ url, allowedDomains?, userAgent?, ephemeral?,
    //                    onShouldNavigate?, onLoaded?, onError?,
    //                    width?, height? }) — issue #658 Phase 1.
    //
    // Single object-literal form. Codegen calls
    // `perry_ui_webview_create(url, w, h)` then for every other present
    // key emits a corresponding `perry_ui_webview_set_*` call against
    // the returned handle. Same shape as the App({...}) destructure
    // above. There's no positional `WebView(url, w, h)` overload —
    // option-bag is the only TS surface (every parameter is optional
    // except url, and named is much more readable for ~9 fields).
    if module == "perry/ui" && method == "WebView" && object.is_none() && args.len() == 1 {
        let Some(props) = extract_options_fields(ctx, &args[0]) else {
            bail!(
                "perry/ui: WebView(...) requires a config object literal. Use \
                 `WebView({{ url: ..., onShouldNavigate: (u) => ..., onLoaded: (u) => ... }})` \
                 (see types/perry/ui/index.d.ts)."
            );
        };

        let mut url_ptr: String = "0".to_string();
        let mut width_d: String = "0.0".to_string();
        let mut height_d: String = "0.0".to_string();
        let mut user_agent_ptr: Option<String> = None;
        let mut allowed_domains_handle: Option<String> = None;
        let mut ephemeral_d: Option<String> = None;
        let mut on_should_navigate_d: Option<String> = None;
        let mut on_loaded_d: Option<String> = None;
        let mut on_error_d: Option<String> = None;

        for (key, val) in &props {
            match key.as_str() {
                "url" => {
                    let v = lower_expr(ctx, val)?;
                    let blk = ctx.block();
                    url_ptr = unbox_to_i64(blk, &v);
                }
                "width" => {
                    width_d = lower_expr(ctx, val)?;
                }
                "height" => {
                    height_d = lower_expr(ctx, val)?;
                }
                "userAgent" => {
                    let v = lower_expr(ctx, val)?;
                    let blk = ctx.block();
                    user_agent_ptr = Some(unbox_to_i64(blk, &v));
                }
                "allowedDomains" => {
                    // The user passes a JS array of strings; we treat it as a
                    // generic widget-like handle (i64 unbox of POINTER) and
                    // the runtime walks it via js_array_get_length / element.
                    let v = lower_expr(ctx, val)?;
                    let blk = ctx.block();
                    allowed_domains_handle = Some(unbox_to_i64(blk, &v));
                }
                "ephemeral" => {
                    // Boolean → JS truthy → f64 → i64 (1 = ephemeral).
                    let v = lower_expr(ctx, val)?;
                    let blk = ctx.block();
                    let truthy = blk.call(I64, "js_is_truthy", &[(DOUBLE, &v)]);
                    ephemeral_d = Some(truthy);
                }
                "onShouldNavigate" => {
                    on_should_navigate_d = Some(lower_expr(ctx, val)?);
                }
                "onLoaded" => {
                    on_loaded_d = Some(lower_expr(ctx, val)?);
                }
                "onError" => {
                    on_error_d = Some(lower_expr(ctx, val)?);
                }
                _ => {
                    // Unknown key — lower for side effects so any nested
                    // closures still get collected by the closure-conversion
                    // pass.
                    let _ = lower_expr(ctx, val)?;
                }
            }
        }

        ctx.pending_declares.push((
            "perry_ui_webview_create".to_string(),
            I64,
            // v2-B: 4th arg is `ephemeral_hint` (1.0 ephemeral / 0.0 persistent).
            vec![I64, DOUBLE, DOUBLE, DOUBLE],
        ));
        ctx.pending_declares.push((
            "perry_ui_webview_set_user_agent".to_string(),
            crate::types::VOID,
            vec![I64, I64],
        ));
        ctx.pending_declares.push((
            "perry_ui_webview_set_allowed_domains".to_string(),
            crate::types::VOID,
            vec![I64, I64],
        ));
        ctx.pending_declares.push((
            "perry_ui_webview_set_ephemeral".to_string(),
            crate::types::VOID,
            vec![I64, I64],
        ));
        ctx.pending_declares.push((
            "perry_ui_webview_set_on_should_navigate".to_string(),
            crate::types::VOID,
            vec![I64, DOUBLE],
        ));
        ctx.pending_declares.push((
            "perry_ui_webview_set_on_loaded".to_string(),
            crate::types::VOID,
            vec![I64, DOUBLE],
        ));
        ctx.pending_declares.push((
            "perry_ui_webview_set_on_error".to_string(),
            crate::types::VOID,
            vec![I64, DOUBLE],
        ));
        ctx.pending_declares
            .push(("js_is_truthy".to_string(), I64, vec![DOUBLE]));

        // v2-B: pass ephemeral as a creation-time arg so backends with
        // construction-time data-store choices (WebView2 userDataFolder,
        // WebKitGTK NetworkSession::new_ephemeral) honor it before the
        // first navigation. Default 1.0 = ephemeral when the user omits
        // the field. The truthy lowering above produces an i64 (0 / 1);
        // bitcast to a double via sitofp so the FFI sees an f64 hint.
        let blk = ctx.block();
        let eph_hint = if let Some(eph) = &ephemeral_d {
            blk.sitofp(I64, eph, DOUBLE)
        } else {
            double_literal(1.0)
        };

        let handle = blk.call(
            I64,
            "perry_ui_webview_create",
            &[
                (I64, &url_ptr),
                (DOUBLE, &width_d),
                (DOUBLE, &height_d),
                (DOUBLE, &eph_hint),
            ],
        );
        if let Some(ua) = &user_agent_ptr {
            blk.call_void(
                "perry_ui_webview_set_user_agent",
                &[(I64, &handle), (I64, ua)],
            );
        }
        if let Some(dom) = &allowed_domains_handle {
            blk.call_void(
                "perry_ui_webview_set_allowed_domains",
                &[(I64, &handle), (I64, dom)],
            );
        }
        if let Some(cb) = &on_should_navigate_d {
            blk.call_void(
                "perry_ui_webview_set_on_should_navigate",
                &[(I64, &handle), (DOUBLE, cb)],
            );
        }
        if let Some(cb) = &on_loaded_d {
            blk.call_void(
                "perry_ui_webview_set_on_loaded",
                &[(I64, &handle), (DOUBLE, cb)],
            );
        }
        if let Some(cb) = &on_error_d {
            blk.call_void(
                "perry_ui_webview_set_on_error",
                &[(I64, &handle), (DOUBLE, cb)],
            );
        }

        // Return as a NaN-boxed widget handle (POINTER tag).
        return Ok(nanbox_pointer_inline(blk, &handle));
    }

    if module == "perry/ui" && method == "App" && object.is_none() {
        if args.len() != 1 {
            bail!(
                "perry/ui: App(...) takes a single config object literal like \
                 `App({{ title, width, height, body }})`, got {} argument(s). \
                 There is no `App(title, builder)` callback form.",
                args.len()
            );
        }
        let Some(props) = extract_options_fields(ctx, &args[0]) else {
            bail!(
                "perry/ui: App(...) requires a config object literal. Use \
                 `App({{ title: ..., width: ..., height: ..., body: ... }})` \
                 (see types/perry/ui/index.d.ts)."
            );
        };
        let mut title_ptr: String = "0".to_string();
        let mut width_d: String = "1024.0".to_string();
        let mut height_d: String = "768.0".to_string();
        let mut body_handle: String = "0".to_string();
        let mut icon_ptr: Option<String> = None;
        let mut window_state_ptr: Option<String> = None;
        for (key, val) in &props {
            match key.as_str() {
                "title" => {
                    let v = lower_expr(ctx, val)?;
                    let blk = ctx.block();
                    title_ptr = unbox_to_i64(blk, &v);
                }
                "width" => {
                    width_d = lower_expr(ctx, val)?;
                }
                "height" => {
                    height_d = lower_expr(ctx, val)?;
                }
                "body" => {
                    let v = lower_expr(ctx, val)?;
                    let blk = ctx.block();
                    body_handle = unbox_to_i64(blk, &v);
                }
                "icon" => {
                    let v = lower_expr(ctx, val)?;
                    let blk = ctx.block();
                    icon_ptr = Some(unbox_to_i64(blk, &v));
                }
                // Issue #1280 — `windowState: "normal" | "maximized" | "fullscreen"`.
                // Forwarded to perry_ui_app_set_window_state; each platform
                // backend applies the state at app_run time.
                "windowState" => {
                    let v = lower_expr(ctx, val)?;
                    let blk = ctx.block();
                    window_state_ptr = Some(unbox_to_i64(blk, &v));
                }
                _ => {
                    let _ = lower_expr(ctx, val)?;
                }
            }
        }
        ctx.pending_declares.push((
            "perry_ui_app_create".to_string(),
            I64,
            vec![I64, DOUBLE, DOUBLE],
        ));
        ctx.pending_declares.push((
            "perry_ui_app_set_icon".to_string(),
            crate::types::VOID,
            vec![I64],
        ));
        ctx.pending_declares.push((
            "perry_ui_app_set_window_state".to_string(),
            crate::types::VOID,
            vec![I64, I64],
        ));
        ctx.pending_declares.push((
            "perry_ui_app_set_body".to_string(),
            crate::types::VOID,
            vec![I64, I64],
        ));
        ctx.pending_declares.push((
            "perry_ui_app_run".to_string(),
            crate::types::VOID,
            vec![I64],
        ));
        let blk = ctx.block();
        let app_handle = blk.call(
            I64,
            "perry_ui_app_create",
            &[(I64, &title_ptr), (DOUBLE, &width_d), (DOUBLE, &height_d)],
        );
        if let Some(icon) = icon_ptr {
            blk.call_void("perry_ui_app_set_icon", &[(I64, &icon)]);
        }
        if let Some(state_ptr) = window_state_ptr {
            blk.call_void(
                "perry_ui_app_set_window_state",
                &[(I64, &app_handle), (I64, &state_ptr)],
            );
        }
        blk.call_void(
            "perry_ui_app_set_body",
            &[(I64, &app_handle), (I64, &body_handle)],
        );
        blk.call_void("perry_ui_app_run", &[(I64, &app_handle)]);
        return Ok(double_literal(0.0));
    }
    include!("native_fs_branch.rs");

    // process module functions: cwd / uptime / memoryUsage / versions
    // accessed as destructured imports. `import { cwd } from 'node:process'`
    // → NativeMethodCall { module: "process", method: "cwd", object: None }.
    // The implicit-global form `process.cwd()` is already lowered to
    // dedicated HIR variants (Expr::ProcessCwd etc) in
    // perry-hir/src/lower/expr_call.rs:262, so the runtime helpers
    // (js_process_cwd / js_process_uptime / js_process_versions /
    // js_process_memory_usage) already exist — this arm just routes the
    // destructured-import shape to the same helpers. Closes #360 item #2's
    // dispatch gap (the warning fix alone would link cwd() but return
    // undefined silently — worse UX than the original "Could not resolve").
    if module == "process" && object.is_none() {
        match method {
            "cwd" => {
                let blk = ctx.block();
                let h = blk.call(I64, "js_process_cwd", &[]);
                return Ok(crate::expr::nanbox_string_inline(blk, &h));
            }
            "uptime" => {
                return Ok(ctx.block().call(DOUBLE, "js_process_uptime", &[]));
            }
            "memoryUsage" => {
                return Ok(ctx.block().call(DOUBLE, "js_process_memory_usage", &[]));
            }
            _ => {
                // Unknown process method — fall through to the generic
                // dispatch which will emit a diagnostic if no signature
                // matches. Likely candidates not wired here: nextTick
                // (needs a callback arg), exit (takes a code), kill,
                // hrtime. Each is its own follow-up under #360.
            }
        }
    }

    // Generic native module dispatch (receiver-less): fastify, mysql2,
    // ws, pg, ioredis, mongodb, better-sqlite3, etc. These were in the
    // old Cranelift codegen's dispatch table but lost in the v0.5.0
    // LLVM cutover.
    if object.is_none() {
        if let Some(sig) = native_module_lookup(module, false, method, class_name) {
            // perry/thread thread-safety check: the closure passed to
            // parallelMap / parallelFilter / spawn must not write to any
            // variable declared outside its own body. Each worker thread
            // gets its own deep-copied snapshot of ordinary captures, and
            // module-level variables live in global slots that would race
            // across workers — either way, writes are silently lost or
            // corrupted relative to user expectations. Enforce at compile
            // time so the docs' promise is real.
            //
            // Note we can't rely on the closure's `mutable_captures` field
            // alone: the HIR filters module-level IDs out of `captures`
            // via `filter_module_level_captures` (see lower.rs:457), so a
            // top-level `let counter = 0; parallelMap(data, () => counter++)`
            // ends up with `captures: [], mutable_captures: []` even though
            // the body obviously writes to `counter`. Instead, walk the
            // body ourselves and flag any LocalSet/Update whose target
            // isn't a parameter or a `let` introduced inside the body.
            if module == "perry/thread" {
                let closure_arg = match method {
                    "parallelMap" | "parallelFilter" => args.get(1),
                    "spawn" => args.first(),
                    _ => None,
                };
                if let Some(callback) = closure_arg {
                    match callback {
                        Expr::Closure { params, body, .. } => {
                            let mut inner_ids: std::collections::HashSet<perry_types::LocalId> =
                                params.iter().map(|p| p.id).collect();
                            for stmt in body {
                                collect_closure_introduced_ids(stmt, &mut inner_ids);
                            }
                            let mut outer_writes: Vec<perry_types::LocalId> = Vec::new();
                            for stmt in body {
                                find_outer_writes_stmt(stmt, &inner_ids, &mut outer_writes);
                            }
                            if let Some(&first_outer) = outer_writes.first() {
                                anyhow::bail!(
                                    "perry/thread: closure passed to `{}` writes to outer variable (LocalId {}) — \
                                     this is not allowed because each worker thread receives a deep-copied \
                                     snapshot of captured values (and module-level slots are not shared across \
                                     workers in the way ordinary TS globals appear to be), so writes would be \
                                     silently lost or corrupted relative to user expectations. Return values \
                                     from the closure and aggregate them on the main thread instead. \
                                     See docs/src/threading/overview.md#no-shared-mutable-state.",
                                    method, first_outer,
                                );
                            }
                        }
                        // Named-function callback bypass: `function worker(n) { counter++; }
                        // parallelMap(xs, worker)` is semantically identical to the inline-
                        // closure form we check above, but we don't have the callee's HIR
                        // body accessible from FnCtx (only `func_names: FuncId -> String`,
                        // not the full function table). Bail with a helpful diagnostic
                        // pointing the user at the inline-closure workaround. Pure
                        // function workers work fine when wrapped (`(x) => worker(x)`);
                        // this just closes the compile-time safety bypass that silently
                        // let outer-writing named functions through.
                        Expr::FuncRef(_) | Expr::LocalGet(_) | Expr::ExternFuncRef { .. } => {
                            anyhow::bail!(
                                "perry/thread: `{}` callback must be an inline arrow/closure, not a \
                                 named function reference. Compile-time thread-safety analysis can only \
                                 inspect inline closures today; a named function could write to outer \
                                 variables which would be silently lost on the deep-copy worker boundary. \
                                 Workaround: wrap the named function in an inline closure — \
                                 `{}(xs, (x) => myFn(x))`. See docs/src/threading/overview.md#no-shared-mutable-state.",
                                method, method,
                            );
                        }
                        _ => {}
                    }
                }
            }
            return lower_native_module_dispatch(ctx, sig, None, args);
        }
    }

    // #1002: native `util.format` / `util.formatWithOptions`. The HIR
    // surfaces these as receiver-less `NativeMethodCall { module:
    // "util", method: "format" | "formatWithOptions", args }`. Before
    // this arm, both fell into the receiver-less fall-through below
    // and returned `TAG_UNDEFINED` — `console.log(util.format("hi %s",
    // "x"))` printed `undefined` and `test_util_format_with_options`
    // failed the parity gate on every PR.
    //
    // Bundle the substitution args into a heap array (same shape
    // `js_console_log_spread` uses) and call `js_util_format`. For
    // `formatWithOptions(opts, fmt, ...args)`, drop arg[0] (the
    // `util.inspect` options bag) — the runtime stub ignores it; full
    // options-passthrough (real `%o` / `%O` styling) is a follow-up.
    if module == "util" && object.is_none() && (method == "format" || method == "formatWithOptions")
    {
        let skip = if method == "formatWithOptions" { 1 } else { 0 };
        // Lower any dropped args (just the options bag for now) for
        // side effects so closures inside them still register.
        for a in args.iter().take(skip) {
            let _ = lower_expr(ctx, a)?;
        }
        let payload: Vec<&Expr> = args.iter().skip(skip).collect();
        let cap = (payload.len() as u32).to_string();
        let mut current_arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
        for arg in &payload {
            let v = lower_expr(ctx, arg)?;
            let blk = ctx.block();
            current_arr = blk.call(
                I64,
                "js_array_push_f64",
                &[(I64, &current_arr), (DOUBLE, &v)],
            );
        }
        let blk = ctx.block();
        let result = blk.call(DOUBLE, "js_util_format", &[(I64, &current_arr)]);
        return Ok(result);
    }

    // Receiver-less native method calls (e.g. plugin::setConfig(...)
    // as a static module function): lower args for side effects and
    // return TAG_UNDEFINED. Using TAG_UNDEFINED (not 0.0) so that
    // downstream .length reads return 0 instead of crashing (the
    // inline .length guard checks ptr < 4096, and TAG_UNDEFINED's
    // lower 48 bits = 1).
    let Some(recv) = object else {
        for a in args {
            let _ = lower_expr(ctx, a)?;
        }
        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
    };
    let _ = (module, method); // shut up unused warnings on the early-out path

    // perry/ui instance method calls: `windowHandle.show()`, `windowHandle.setBody(w)`, etc.
    // The HIR produces these with `object: Some(handle)` and `module: "perry/ui"`.
    // Lower the receiver to get the widget/window handle, then dispatch.
    if module == "perry/ui" {
        let recv_val = lower_expr(ctx, recv)?;
        let blk = ctx.block();
        let handle = unbox_to_i64(blk, &recv_val);
        if let Some(sig) = perry_ui_instance_method_lookup(method) {
            // Build args: handle is the first arg, then the call args.
            let mut llvm_args: Vec<(crate::types::LlvmType, String)> =
                Vec::with_capacity(1 + args.len());
            let mut runtime_param_types: Vec<crate::types::LlvmType> =
                Vec::with_capacity(1 + args.len());
            llvm_args.push((I64, handle));
            runtime_param_types.push(I64);
            for (kind, arg) in sig.args.iter().zip(args.iter()) {
                match kind {
                    UiArgKind::Widget => {
                        let v = lower_expr(ctx, arg)?;
                        let blk = ctx.block();
                        let h = unbox_to_i64(blk, &v);
                        llvm_args.push((I64, h));
                        runtime_param_types.push(I64);
                    }
                    UiArgKind::Str => {
                        let h = get_raw_string_ptr(ctx, arg)?;
                        llvm_args.push((I64, h));
                        runtime_param_types.push(I64);
                    }
                    UiArgKind::F64 => {
                        let v = lower_expr(ctx, arg)?;
                        llvm_args.push((DOUBLE, v));
                        runtime_param_types.push(DOUBLE);
                    }
                    UiArgKind::Closure => {
                        let v = lower_expr(ctx, arg)?;
                        llvm_args.push((DOUBLE, v));
                        runtime_param_types.push(DOUBLE);
                    }
                    UiArgKind::I64Raw => {
                        let v = lower_expr(ctx, arg)?;
                        let blk = ctx.block();
                        let i = blk.fptosi(DOUBLE, &v, I64);
                        llvm_args.push((I64, i));
                        runtime_param_types.push(I64);
                    }
                }
            }
            let return_type = match sig.ret {
                UiReturnKind::Widget | UiReturnKind::I64AsF64 => I64,
                UiReturnKind::F64 => DOUBLE,
                UiReturnKind::Void => crate::types::VOID,
                UiReturnKind::Str => I64,
            };
            ctx.pending_declares
                .push((sig.runtime.to_string(), return_type, runtime_param_types));
            let ref_args: Vec<(crate::types::LlvmType, &str)> =
                llvm_args.iter().map(|(t, s)| (*t, s.as_str())).collect();
            let blk = ctx.block();
            return match sig.ret {
                UiReturnKind::Void => {
                    blk.call_void(sig.runtime, &ref_args);
                    Ok(double_literal(0.0))
                }
                UiReturnKind::Widget => {
                    let raw = blk.call(I64, sig.runtime, &ref_args);
                    Ok(crate::expr::nanbox_pointer_inline(blk, &raw))
                }
                UiReturnKind::F64 => Ok(blk.call(DOUBLE, sig.runtime, &ref_args)),
                UiReturnKind::Str => {
                    let raw = blk.call(I64, sig.runtime, &ref_args);
                    Ok(crate::expr::nanbox_string_inline(blk, &raw))
                }
                UiReturnKind::I64AsF64 => {
                    let raw = blk.call(I64, sig.runtime, &ref_args);
                    Ok(blk.sitofp(I64, &raw, DOUBLE))
                }
            };
        }
        // Unknown instance method — fail the compile. Previously this
        // lowered the args for side effects and returned TAG_UNDEFINED,
        // which silently swallowed styling calls like `label.setColor(...)`
        // and `btn.setCornerRadius(...)` (see types/perry/ui/index.d.ts
        // for the real method surface — styling uses the free-function
        // `textSetColor(widget, r, g, b, a)` / `setCornerRadius(widget, r)`
        // forms, not instance methods on the widget handle).
        bail!(
            "perry/ui: '.{}(...)' is not a known instance method (args: {}). \
             See types/perry/ui/index.d.ts — widget styling uses free functions \
             like `textSetFontSize(label, 24)` and `widgetSetBackgroundColor(btn, r, g, b, a)`, \
             not instance-method setters.",
            method,
            args.len()
        );
    }

    // perry/plugin PluginApi instance methods: `api.registerHook(...)`, `api.emit(...)`, etc.
    // The HIR produces these with `object: Some(handle)` and `module: "perry/plugin"`.
    if module == "perry/plugin" {
        let recv_val = lower_expr(ctx, recv)?;
        let blk = ctx.block();
        let handle = unbox_to_i64(blk, &recv_val);
        if let Some(sig) = perry_plugin_instance_method_lookup(method) {
            let mut llvm_args: Vec<(crate::types::LlvmType, String)> =
                Vec::with_capacity(1 + args.len());
            let mut runtime_param_types: Vec<crate::types::LlvmType> =
                Vec::with_capacity(1 + args.len());
            llvm_args.push((I64, handle));
            runtime_param_types.push(I64);
            for (kind, arg) in sig.args.iter().zip(args.iter()) {
                match kind {
                    UiArgKind::Widget => {
                        let v = lower_expr(ctx, arg)?;
                        let blk = ctx.block();
                        let h = unbox_to_i64(blk, &v);
                        llvm_args.push((I64, h));
                        runtime_param_types.push(I64);
                    }
                    UiArgKind::Str => {
                        let h = get_raw_string_ptr(ctx, arg)?;
                        llvm_args.push((I64, h));
                        runtime_param_types.push(I64);
                    }
                    UiArgKind::F64 | UiArgKind::Closure => {
                        let v = lower_expr(ctx, arg)?;
                        llvm_args.push((DOUBLE, v));
                        runtime_param_types.push(DOUBLE);
                    }
                    UiArgKind::I64Raw => {
                        let v = lower_expr(ctx, arg)?;
                        let blk = ctx.block();
                        let i = blk.fptosi(DOUBLE, &v, I64);
                        llvm_args.push((I64, i));
                        runtime_param_types.push(I64);
                    }
                }
            }
            let return_type = match sig.ret {
                UiReturnKind::Widget | UiReturnKind::I64AsF64 | UiReturnKind::Str => I64,
                UiReturnKind::F64 => DOUBLE,
                UiReturnKind::Void => crate::types::VOID,
            };
            ctx.pending_declares
                .push((sig.runtime.to_string(), return_type, runtime_param_types));
            let ref_args: Vec<(crate::types::LlvmType, &str)> =
                llvm_args.iter().map(|(t, s)| (*t, s.as_str())).collect();
            let blk = ctx.block();
            return match sig.ret {
                UiReturnKind::Void => {
                    blk.call_void(sig.runtime, &ref_args);
                    Ok(double_literal(0.0))
                }
                UiReturnKind::Widget => {
                    let raw = blk.call(I64, sig.runtime, &ref_args);
                    Ok(crate::expr::nanbox_pointer_inline(blk, &raw))
                }
                UiReturnKind::F64 => Ok(blk.call(DOUBLE, sig.runtime, &ref_args)),
                UiReturnKind::I64AsF64 => {
                    let raw = blk.call(I64, sig.runtime, &ref_args);
                    Ok(blk.sitofp(I64, &raw, DOUBLE))
                }
                UiReturnKind::Str => {
                    let raw = blk.call(I64, sig.runtime, &ref_args);
                    Ok(crate::expr::nanbox_string_inline(blk, &raw))
                }
            };
        }
        bail!(
            "perry/plugin: '.{}(...)' is not a known PluginApi method (args: {}). \
             See types/perry/plugin/index.d.ts for the supported API surface.",
            method,
            args.len()
        );
    }

    if module == "array" && method == "push_spread" {
        // Refs #488 drizzle-sqlite: `arr.push(...src)` shape. Pre-fix
        // this had no codegen arm — the catch-all at the end of this
        // function silently lowered receiver + args for side effects and
        // returned `0.0`. drizzle's `mergeQueries` does
        // `result.params.push(...query.params)` so SQL queries went out
        // with empty params and INSERT silently inserted nothing.
        //
        // The HIR shape from `expr_call.rs:4810` packs the spread arg as
        // `args[0]` (the inner spread expression), so we expect exactly
        // one arg with the source array.
        if args.len() != 1 {
            bail!(
                "array.push_spread expects exactly 1 arg, got {}",
                args.len()
            );
        }
        let src_box = lower_expr(ctx, &args[0])?;
        let arr_box = lower_expr(ctx, recv)?;
        let blk = ctx.block();
        let arr_handle = unbox_to_i64(blk, &arr_box);
        let orig_handle = arr_handle.clone();
        let src_handle = unbox_to_i64(blk, &src_box);
        let blk = ctx.block();
        let new_handle = blk.call(
            I64,
            "js_array_push_spread_f64",
            &[(I64, &arr_handle), (I64, &src_handle)],
        );
        let blk = ctx.block();
        let new_box = nanbox_pointer_inline(blk, &new_handle);
        // Same write-back-only-if-realloc'd pattern as push_single.
        let needs_writeback = matches!(recv, Expr::LocalGet(_) | Expr::PropertyGet { .. });
        if needs_writeback {
            let blk = ctx.block();
            let changed = blk.icmp_ne(I64, &new_handle, &orig_handle);
            let wb_idx = ctx.new_block("arr.push_spread.wb");
            let merge_idx = ctx.new_block("arr.push_spread.merge");
            let wb_label = ctx.block_label(wb_idx);
            let merge_label = ctx.block_label(merge_idx);
            ctx.block().cond_br(&changed, &wb_label, &merge_label);

            ctx.current_block = wb_idx;
            match recv {
                Expr::LocalGet(id) => {
                    if let Some(slot) = ctx.locals.get(id).cloned() {
                        ctx.block().store(DOUBLE, &new_box, &slot);
                    } else if let Some(global_name) = ctx.module_globals.get(id).cloned() {
                        let g_ref = format!("@{}", global_name);
                        ctx.block().store(DOUBLE, &new_box, &g_ref);
                    }
                }
                Expr::PropertyGet {
                    object: obj_expr,
                    property,
                } => {
                    let obj_box = lower_expr(ctx, obj_expr)?;
                    let key_idx = ctx.strings.intern(property);
                    let key_handle_global =
                        format!("@{}", ctx.strings.entry(key_idx).handle_global);
                    let blk = ctx.block();
                    let obj_bits = blk.bitcast_double_to_i64(&obj_box);
                    let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
                    let key_box = blk.load(DOUBLE, &key_handle_global);
                    let key_bits = blk.bitcast_double_to_i64(&key_box);
                    let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                    blk.call_void(
                        "js_object_set_field_by_name",
                        &[(I64, &obj_handle), (I64, &key_raw), (DOUBLE, &new_box)],
                    );
                }
                _ => unreachable!(),
            }
            ctx.block().br(&merge_label);

            ctx.current_block = merge_idx;
        }
        // Spec: push returns the new length; statement context discards.
        return Ok(new_box);
    }

    if module == "array" && (method == "push_single" || method == "push") {
        if args.is_empty() {
            bail!("array.push expects ≥1 arg, got 0");
        }
        // Lower every argument first so closures and string literals get
        // collected, then lower the receiver once. js_array_push_f64 may
        // realloc on each call, so we thread the returned pointer through
        // and write the final pointer back to the receiver — but ONLY
        // if it actually changed. The runtime returns the same pointer
        // when capacity was sufficient (no grow); the writeback is a
        // no-op in that case but still costs a `js_object_set_field_by_name`
        // call (~50-100 cycles) per push. With amortized doubling, real
        // reallocs are O(log N) of the total pushes — guarding the
        // writeback elides the overhead on the 99.9% no-realloc path.
        let mut lowered: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered.push(lower_expr(ctx, a)?);
        }
        let arr_box = lower_expr(ctx, recv)?;
        let blk = ctx.block();
        let mut arr_handle = unbox_to_i64(blk, &arr_box);
        let orig_handle = arr_handle.clone();
        for v in &lowered {
            let blk = ctx.block();
            arr_handle = blk.call(I64, "js_array_push_f64", &[(I64, &arr_handle), (DOUBLE, v)]);
        }
        let blk = ctx.block();
        let new_handle = arr_handle;
        let new_box = nanbox_pointer_inline(blk, &new_handle);
        // Compare the (possibly-realloc'd) pointer against the original
        // and only run the writeback when it actually differs. Setup
        // wb / merge basic blocks so the write-back path is cold.
        // Match arms decide the writeback shape:
        //   1. recv = LocalGet(id)  → store back to the local's slot
        //   2. recv = PropertyGet { obj, prop } → set obj.prop = new_box
        //   3. anything else → no writeback (array may dangle on realloc,
        //      but we don't crash at codegen — same trade-off as before).
        let needs_writeback = matches!(recv, Expr::LocalGet(_) | Expr::PropertyGet { .. });
        if needs_writeback {
            let blk = ctx.block();
            let changed = blk.icmp_ne(I64, &new_handle, &orig_handle);
            let wb_idx = ctx.new_block("arr.push.wb");
            let merge_idx = ctx.new_block("arr.push.merge");
            let wb_label = ctx.block_label(wb_idx);
            let merge_label = ctx.block_label(merge_idx);
            ctx.block().cond_br(&changed, &wb_label, &merge_label);

            ctx.current_block = wb_idx;
            match recv {
                Expr::LocalGet(id) => {
                    if let Some(slot) = ctx.locals.get(id).cloned() {
                        ctx.block().store(DOUBLE, &new_box, &slot);
                    } else if let Some(global_name) = ctx.module_globals.get(id).cloned() {
                        let g_ref = format!("@{}", global_name);
                        ctx.block().store(DOUBLE, &new_box, &g_ref);
                    }
                }
                Expr::PropertyGet {
                    object: obj_expr,
                    property,
                } => {
                    let obj_box = lower_expr(ctx, obj_expr)?;
                    let key_idx = ctx.strings.intern(property);
                    let key_handle_global =
                        format!("@{}", ctx.strings.entry(key_idx).handle_global);
                    let blk = ctx.block();
                    let obj_bits = blk.bitcast_double_to_i64(&obj_box);
                    let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
                    let key_box = blk.load(DOUBLE, &key_handle_global);
                    let key_bits = blk.bitcast_double_to_i64(&key_box);
                    let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                    blk.call_void(
                        "js_object_set_field_by_name",
                        &[(I64, &obj_handle), (I64, &key_raw), (DOUBLE, &new_box)],
                    );
                }
                _ => unreachable!(),
            }
            ctx.block().br(&merge_label);

            ctx.current_block = merge_idx;
        }
        // push returns the new length in JS spec; for now we return
        // the new boxed pointer (statement context discards it).
        return Ok(new_box);
    }

    if module == "array" && (method == "pop_back" || method == "pop") {
        if !args.is_empty() {
            bail!("array.pop expects 0 args, got {}", args.len());
        }
        let arr_box = lower_expr(ctx, recv)?;
        let blk = ctx.block();
        let arr_handle = unbox_to_i64(blk, &arr_box);
        return Ok(blk.call(DOUBLE, "js_array_pop_f64", &[(I64, &arr_handle)]));
    }

    // Generic native module dispatch (with receiver): fastify instance
    // methods (app.get, app.listen, conn.query, etc.), mysql2, ws, pg,
    // ioredis, mongodb, better-sqlite3, etc.
    if let Some(sig) = native_module_lookup(module, true, method, class_name) {
        let recv_val = lower_expr(ctx, recv)?;
        let blk = ctx.block();
        let handle = unbox_to_i64(blk, &recv_val);
        return lower_native_module_dispatch(ctx, sig, Some(&handle), args);
    }

    // Unknown native method: lower the receiver and args for side
    // effects (so closures inside them get auto-collected and any
    // string literals get interned), then return a sentinel. This
    // unblocks compilation of programs that touch native modules
    // we haven't wired up yet — they'll produce garbage at runtime
    // but won't fail at codegen time.
    let _ = lower_expr(ctx, recv)?;
    for a in args {
        let _ = lower_expr(ctx, a)?;
    }
    Ok(double_literal(0.0))
}
