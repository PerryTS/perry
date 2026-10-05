//! Built-in `new C()` constructor lowering — `lower_builtin_new`.
//!
//! Tier 2.2 follow-up (v0.5.339) — extracts the 399-LOC dispatcher
//! that handles `new` calls against built-in classes (Date, Map, Set,
//! Buffer, fetch Headers / Request / Response,
//! fastify App, ws WebSocketServer, pg Client /
//! Pool, perry/plugin Decimal, AsyncLocalStorage, AbortController,
//! Command, …). Each match arm emits a runtime call to the
//! corresponding `js_<lib>_<class>_new(...)` C symbol.
//!
//! Pattern matches `ui_styling.rs` (the prior lower_call/ extraction):
//! `pub(super) fn` entry point, recursion through `super::lower_expr`,
//! shared `extract_options_fields` and `build_headers_from_object`
//! reach into the parent module.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_array_literal, lower_expr, nanbox_pointer_inline, unbox_to_i64, FnCtx};
use crate::nanbox::double_literal;
use crate::rooting::{self, RootedGroup};
use crate::types::{DOUBLE, I32, I64};

use super::{extract_options_fields, get_raw_string_ptr};

/// Lower `args[idx]` into `group`, rooted across everything from
/// `args[idx + 1..]` — the same #6969/#6986 discipline `new.rs`'s
/// `adopt_constructor_args` uses: adopt **as the value is produced**, not
/// after the fact (a group holding an already-dangling operand turns a
/// silent wrong answer into a SIGSEGV), and hand back only an index for
/// [`RootedGroup::reread`] at the point of use. `None` when the argument is
/// absent — callers supply their own default in that case, and it never
/// needs rooting: slice indexing guarantees every argument after an absent
/// one is absent too, so there is nothing left to hold it across.
fn adopt_optional_arg<'a>(
    ctx: &mut FnCtx<'_>,
    args: &'a [Expr],
    idx: usize,
    group: &mut RootedGroup<'a>,
) -> Result<Option<usize>> {
    let Some(a) = args.get(idx) else {
        return Ok(None);
    };
    let collects = rooting::any_operand_may_collect(ctx, args[idx + 1..].iter());
    Ok(Some(group.lower(ctx, a, collects)?))
}

/// The single-leading-options-argument shape repeated by more than a dozen
/// arms below: lower `args[0]` (defaulting to `undefined` when absent), then
/// lower every remaining argument for its side effects only, then hand back
/// `args[0]`'s value. Pre-#6986 the discard loop ran with `args[0]`'s value
/// sitting in a bare SSA register — any of those side-effect expressions can
/// allocate.
fn adopt_leading_arg_discard_rest<'a>(
    ctx: &mut FnCtx<'_>,
    args: &'a [Expr],
    group: &mut RootedGroup<'a>,
) -> Result<String> {
    let idx = adopt_optional_arg(ctx, args, 0, group)?;
    for a in args.iter().skip(1) {
        let _ = lower_expr(ctx, a)?;
    }
    Ok(match idx {
        Some(i) => group.reread(ctx, i)?,
        None => double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
    })
}

/// The two-leading-arguments shape (`Event`/`CustomEvent`/`DOMException`
/// below): lower `args[0]` then `args[1]` (each defaulting to `undefined`),
/// then lower every remaining argument for its side effects only, then hand
/// both values back in argument order. Pre-#6986 `args[0]`'s value sat in a
/// bare SSA register across `args[1]`'s lowering AND the discard loop.
fn adopt_two_leading_args_discard_rest<'a>(
    ctx: &mut FnCtx<'_>,
    args: &'a [Expr],
    group: &mut RootedGroup<'a>,
) -> Result<(String, String)> {
    let idx0 = adopt_optional_arg(ctx, args, 0, group)?;
    let idx1 = adopt_optional_arg(ctx, args, 1, group)?;
    for a in args.iter().skip(2) {
        let _ = lower_expr(ctx, a)?;
    }
    let undef = || double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let v0 = match idx0 {
        Some(i) => group.reread(ctx, i)?,
        None => undef(),
    };
    let v1 = match idx1 {
        Some(i) => group.reread(ctx, i)?,
        None => undef(),
    };
    Ok((v0, v1))
}

/// The shared argument shape of `new ReadableStream` / `WritableStream` /
/// `TransformStream`: a first argument that is either an inline literal whose
/// named members are callbacks (`start`, `pull`, ...) or any runtime value
/// standing for the underlying source/sink/transformer, followed by
/// `extra_count` strategy arguments.
///
/// #11789 sweep: every callback and strategy is lowered and rooted FIRST, in
/// source order, each across the ones after it (each callback is a closure
/// allocation), and they are read back together below the last. Returns, in
/// `names` order, each named callback (`undefined` when absent), the runtime
/// source object when the first argument was not a literal, and the strategy
/// arguments that were present. The literal is only ANALYSED before anything
/// is lowered.
fn lower_stream_ctor_args(
    ctx: &mut FnCtx<'_>,
    args: &[Expr],
    names: &[&str],
    extra_count: usize,
) -> Result<(Vec<String>, Option<String>, Vec<Option<String>>)> {
    let props_opt = match args.first() {
        Some(first) => extract_options_fields(ctx, first),
        None => None,
    };
    let mut group =
        rooting::open_rooted_group(props_opt.as_ref().map_or(1, Vec::len) + extra_count);
    // (name index | None for a discarded member, root)
    let mut member_roots: Vec<(Option<usize>, usize)> = Vec::new();
    let mut object_root: Option<usize> = None;
    let extras: Vec<&Expr> = args.iter().skip(1).take(extra_count).collect();
    match (&props_opt, args.first()) {
        (Some(props), _) => {
            for (i, (k, vexpr)) in props.iter().enumerate() {
                let collects = !extras.is_empty()
                    || rooting::any_operand_may_collect(
                        ctx,
                        props[i + 1..].iter().map(|(_, later)| later),
                    );
                let root = group.lower(ctx, vexpr, collects)?;
                member_roots.push((names.iter().position(|n| *n == k.as_str()), root));
            }
        }
        (None, Some(first)) => {
            object_root = Some(group.lower(ctx, first, !extras.is_empty())?);
        }
        (None, None) => {}
    }
    let mut extra_roots: Vec<usize> = Vec::with_capacity(extras.len());
    for (i, extra) in extras.iter().enumerate() {
        let collects = rooting::any_operand_may_collect(ctx, extras[i + 1..].iter().copied());
        extra_roots.push(group.lower(ctx, extra, collects)?);
    }
    let undef = || double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let mut named: Vec<String> = names.iter().map(|_| undef()).collect();
    for (slot, root) in member_roots {
        if let Some(slot) = slot {
            named[slot] = group.reread(ctx, root)?;
        }
    }
    let object = match object_root {
        Some(root) => Some(group.reread(ctx, root)?),
        None => None,
    };
    let mut extra_values: Vec<Option<String>> = vec![None; extra_count];
    for (i, root) in extra_roots.into_iter().enumerate() {
        extra_values[i] = Some(group.reread(ctx, root)?);
    }
    group.release(ctx);
    Ok((named, object, extra_values))
}

pub(super) fn lower_builtin_new<'a>(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    args: &'a [Expr],
    group: &mut RootedGroup<'a>,
) -> Result<Option<String>> {
    // Issue #602: ambiguously-named built-in constructors (Client / Pool /
    // Database / Decimal) collide with bindings from
    // unrelated packages — `import Client from "better-sqlite3"` would
    // otherwise dispatch through pg's Client arm and emit an undefined
    // `js_pg_client_new` reference at link time. None of these names is a
    // Node global, so the arm fires ONLY on positive evidence: a recorded
    // import binding whose source is the arm's package (the CJS wrap's
    // require-adoption records these too). Names without a matching import
    // source fall through to the generic path — this covers function-scoped
    // class expressions like undici's `var Client = class _Client …` inside
    // bundled vendor code (Next.js `@edge-runtime/primitives`), which are
    // invisible to `ctx.classes` and previously hit pg's arm, breaking the
    // link of any program that bundles undici without importing pg.
    let import_src = ctx
        .imported_class_sources
        .get(class_name)
        .map(|s| s.as_str());
    let required_sources: Option<&[&str]> = match class_name {
        "Database" => Some(&["better-sqlite3"]),
        "DatabaseSync" | "Session" | "StatementSync" => Some(&["sqlite", "node:sqlite"]),
        "Transpiler" => Some(&["bun"]),
        _ => None,
    };
    if let Some(sources) = required_sources {
        if !import_src.is_some_and(|src| sources.contains(&src)) {
            return Ok(None);
        }
    }
    match class_name {
        "Transpiler" => {
            let options = adopt_leading_arg_discard_rest(ctx, args, group)?;
            ctx.pending_declares
                .push(("js_bun_transpiler_new".to_string(), I64, vec![DOUBLE]));
            let block = ctx.block();
            let handle = block.call(I64, "js_bun_transpiler_new", &[(DOUBLE, &options)]);
            Ok(Some(nanbox_pointer_inline(block, &handle)))
        }
        "Resolver"
            if import_src.is_some_and(|source| {
                matches!(
                    source.strip_prefix("node:").unwrap_or(source),
                    "dns" | "dns/promises"
                )
            }) =>
        {
            // `new Resolver()` is a constructor expression, so it bypasses
            // the native-module call table used by `dns.Resolver()`. Route it
            // to the same runtime constructor and preserve evaluation of any
            // superfluous arguments.
            let options_idx = adopt_optional_arg(ctx, args, 0, group)?;
            for arg in args.iter().skip(1) {
                let _ = lower_expr(ctx, arg)?;
            }
            let options = match options_idx {
                Some(index) => group.reread(ctx, index)?,
                None => double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
            };
            let options = group.adopt_emitted(ctx, crate::rooting::Repr::Boxed, &options, true);
            let runtime = if import_src.is_some_and(|source| {
                source.strip_prefix("node:").unwrap_or(source) == "dns/promises"
            }) {
                "js_dns_promises_resolver_new"
            } else {
                "js_dns_resolver_new"
            };
            ctx.pending_declares
                .push((runtime.to_string(), DOUBLE, vec![I64]));
            let zero = "0".to_string();
            let args_array = group.begin_array(ctx, &zero);
            let options = group.reread_emitted(ctx, options);
            group.push_array(ctx, args_array, &options);
            let args_array = group.read_array(ctx, args_array);
            Ok(Some(ctx.block().call(
                DOUBLE,
                runtime,
                &[(I64, &args_array)],
            )))
        }
        "Utf8Stream"
            if import_src
                .map(|source| source.strip_prefix("node:").unwrap_or(source) == "fs")
                .unwrap_or(false) =>
        {
            // #6986: `options` was live across the discard loop below, which
            // lowers arbitrary user expressions for their side effects.
            let options_idx = adopt_optional_arg(ctx, args, 0, group)?;
            for arg in args.iter().skip(1) {
                let _ = lower_expr(ctx, arg)?;
            }
            let options = match options_idx {
                Some(i) => group.reread(ctx, i)?,
                None => double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
            };
            Ok(Some(ctx.block().call(
                DOUBLE,
                "js_fs_utf8_stream_new",
                &[(DOUBLE, &options)],
            )))
        }
        "EvalError" | "URIError" => {
            // #6986: `msg_box` was live across the discard loop below. The
            // `None` fallback needs no rooting — an absent first argument
            // means `args` is empty, so the loop is empty too.
            let msg_idx = adopt_optional_arg(ctx, args, 0, group)?;
            for arg in args.iter().skip(1) {
                let _ = lower_expr(ctx, arg)?;
            }
            let msg_box = match msg_idx {
                Some(i) => group.reread(ctx, i)?,
                None => lower_expr(ctx, &Expr::String(String::new()))?,
            };
            let blk = ctx.block();
            // The message goes over NaN-boxed and the runtime coerces it: a
            // masked inline SSO message (`new EvalError(String(n))`) was read
            // as a header address (#11519).
            let kind = if class_name == "EvalError" {
                "6" // ERROR_KIND_EVAL_ERROR
            } else {
                "7" // ERROR_KIND_URI_ERROR
            };
            let err_handle = blk.call(
                I64,
                "js_error_new_kind_from_value",
                &[(I32, kind), (DOUBLE, &msg_box)],
            );
            Ok(Some(nanbox_pointer_inline(blk, &err_handle)))
        }
        // `new RegExp(pattern)` / `new RegExp(pattern, flags)` — call
        // js_regexp_new directly so the resulting object is a real
        // RegExpHeader (registered in REGEX_POINTERS, .test/.exec/etc
        // dispatch correctly). Refs #486 — hono's `buildWildcardRegExp`
        // does `new RegExp(path === "*" ? "" : ...)`. Pre-fix, the
        // generic Expr::New path fell through to the placeholder
        // js_object_alloc(0,0) and the resulting "fake regex" never
        // actually matched anything (`.test("/")` returned false on every
        // input — caused middleware-vs-route lookup in
        // RegExpRouter.add's wildcard branch to skip every push, leaving
        // matchResult[0] missing the middleware entry). Compile-time
        // RegExp LITERALS (`/foo/g`) already lower through Expr::RegExp
        // at expr.rs:4964 — this arm covers the runtime `new RegExp(arg)`
        // form where the pattern argument is a non-literal expression.
        // `new ArrayBuffer(size)` — issue #579. Pre-fix this fell through
        // to the empty-ObjectHeader placeholder and `new Uint8Array(ab)`
        // views silently allocated independent storage (no aliasing). The
        // runtime's `js_array_buffer_new` allocates a real BufferHeader
        // that subsequent Uint8Array views share by pointer (see
        // `js_uint8array_new` in `crates/perry-runtime/src/buffer.rs`:
        // sources that ARE registered buffers but NOT marked as
        // Uint8Array — i.e. ArrayBuffers — are aliased rather than
        // copied). SharedArrayBuffer uses the same storage allocation with a
        // separate runtime registry so util.types can distinguish it.
        // #10873: `new ArrayBuffer(length, { maxByteLength })`. The options bag
        // used to be dropped here — never even evaluated — so a resizable
        // buffer silently came back fixed-length. The runtime reads
        // `maxByteLength` AFTER `ToIndex(length)`, per spec, so both operands
        // go over raw. `length` can be an object (its `valueOf` runs in the
        // runtime), and lowering the options literal allocates: root it.
        "ArrayBuffer" if args.len() >= 2 => {
            let size_collects = rooting::any_operand_may_collect(ctx, args[1..].iter());
            let size_idx = group.lower(ctx, &args[0], size_collects)?;
            let options_idx = adopt_optional_arg(ctx, args, 1, group)?;
            for arg in args.iter().skip(2) {
                let _ = lower_expr(ctx, arg)?;
            }
            let size_box = group.reread(ctx, size_idx)?;
            let options_box = match options_idx {
                Some(i) => group.reread(ctx, i)?,
                None => double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
            };
            let handle = ctx.block().call(
                I64,
                "js_array_buffer_new_with_options",
                &[(DOUBLE, &size_box), (DOUBLE, &options_box)],
            );
            Ok(Some(nanbox_pointer_inline(ctx.block(), &handle)))
        }
        "ArrayBuffer" | "SharedArrayBuffer" => {
            let size_box = if !args.is_empty() {
                lower_expr(ctx, &args[0])?
            } else {
                double_literal(0.0)
            };
            let blk = ctx.block();
            let runtime = if class_name == "SharedArrayBuffer" {
                "js_shared_array_buffer_new_value"
            } else {
                "js_array_buffer_new_value"
            };
            let handle = blk.call(I64, runtime, &[(DOUBLE, &size_box)]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        "Uint8Array" if args.len() >= 2 => {
            // Pass the raw NaN-boxed offset/length (undefined when absent),
            // same as the non-Uint8Array view kinds below: the runtime runs
            // ToIndex — which can execute user `valueOf` code — and applies
            // the spec's post-coercion detached/bounds checks. The old
            // `fptosi` cast silently turned object arguments into garbage
            // without ever running their coercion.
            // #6986: `source` (and `offset_box`) were held in bare SSA
            // registers across the later operands' lowering — each an
            // arbitrary expression (ToIndex can run user `valueOf`).
            // `args.len() >= 2` is this arm's guard, so args[0]/args[1] are
            // always present.
            let source_collects = rooting::any_operand_may_collect(ctx, args[1..].iter());
            let source_idx = group.lower(ctx, &args[0], source_collects)?;
            let offset_collects = rooting::any_operand_may_collect(ctx, args[2..].iter());
            let offset_idx = group.lower(ctx, &args[1], offset_collects)?;
            let length_idx = adopt_optional_arg(ctx, args, 2, group)?;
            let source = group.reread(ctx, source_idx)?;
            let offset_box = group.reread(ctx, offset_idx)?;
            let length_box = match length_idx {
                Some(i) => group.reread(ctx, i)?,
                None => double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
            };
            let handle = ctx.block().call(
                I64,
                "js_uint8array_view",
                &[
                    (DOUBLE, &source),
                    (DOUBLE, &offset_box),
                    (DOUBLE, &length_box),
                ],
            );
            Ok(Some(nanbox_pointer_inline(ctx.block(), &handle)))
        }
        // #4103: `new TA(buffer, byteOffset, length?)` view constructor for the
        // non-`Uint8Array` kinds. The runtime applies the spec offset/length
        // bounds + alignment checks (RangeError) — Uint8Array piggybacks on the
        // BufferHeader path above. Pass the raw NaN-boxed offset/length so
        // ToIndex (and the `undefined` length default) is honored in the runtime.
        name @ ("Int8Array" | "Uint16Array" | "Int16Array" | "Uint32Array" | "Int32Array"
        | "Float32Array" | "Float64Array" | "Uint8ClampedArray" | "BigInt64Array"
        | "BigUint64Array" | "Float16Array")
            if args.len() >= 2 =>
        {
            let kind = typed_array_view_kind(name);
            // #6986: same hazard as the Uint8Array view arm above.
            let source_collects = rooting::any_operand_may_collect(ctx, args[1..].iter());
            let source_idx = group.lower(ctx, &args[0], source_collects)?;
            let offset_collects = rooting::any_operand_may_collect(ctx, args[2..].iter());
            let offset_idx = group.lower(ctx, &args[1], offset_collects)?;
            let length_idx = adopt_optional_arg(ctx, args, 2, group)?;
            let source = group.reread(ctx, source_idx)?;
            let offset_box = group.reread(ctx, offset_idx)?;
            let length_box = match length_idx {
                Some(i) => group.reread(ctx, i)?,
                None => double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
            };
            let handle = ctx.block().call(
                I64,
                "js_typed_array_view",
                &[
                    (I32, &kind.to_string()),
                    (DOUBLE, &source),
                    (DOUBLE, &offset_box),
                    (DOUBLE, &length_box),
                ],
            );
            Ok(Some(nanbox_pointer_inline(ctx.block(), &handle)))
        }
        // Minimal DataView support for BufferSource consumers such as
        // StringDecoder: Perry models ArrayBuffer/Uint8Array storage as a
        // BufferHeader, so `new DataView(buffer)` can create a registered view
        // over that backing store for byte-extraction call sites.
        "DataView" => {
            // Pass the raw NaN-boxed arguments (undefined when absent) so the
            // runtime can apply the spec's ToIndex/range validation and throw
            // TypeError/RangeError where required (#3657).
            //
            // #6986: `view_box` (and `offset_box`) were held in bare SSA
            // registers across the later operands' lowering.
            let view_idx = adopt_optional_arg(ctx, args, 0, group)?;
            let offset_idx = adopt_optional_arg(ctx, args, 1, group)?;
            let length_idx = adopt_optional_arg(ctx, args, 2, group)?;
            let undef = || double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let view_box = match view_idx {
                Some(i) => group.reread(ctx, i)?,
                None => undef(),
            };
            let offset_box = match offset_idx {
                Some(i) => group.reread(ctx, i)?,
                None => undef(),
            };
            let length_box = match length_idx {
                Some(i) => group.reread(ctx, i)?,
                None => undef(),
            };
            Ok(Some(ctx.block().call(
                DOUBLE,
                "js_data_view_new",
                &[
                    (DOUBLE, &view_box),
                    (DOUBLE, &offset_box),
                    (DOUBLE, &length_box),
                ],
            )))
        }
        "RegExp" => {
            // Pass the NaN-boxed pattern/flags straight to the full constructor
            // so a RegExp pattern (flag override / copy), an `undefined` pattern,
            // or an object `flags` (ToString → SyntaxError) are all handled per
            // spec. Defaults are `undefined` (NOT 0.0) so `new RegExp()` builds
            // an empty source.
            // #6986: `pattern_box` was held in a bare SSA register across
            // `flags_box`'s lowering (an arbitrary user expression).
            let pattern_idx = adopt_optional_arg(ctx, args, 0, group)?;
            let flags_idx = adopt_optional_arg(ctx, args, 1, group)?;
            let pattern_box = match pattern_idx {
                Some(i) => group.reread(ctx, i)?,
                None => double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
            };
            let flags_box = match flags_idx {
                Some(i) => group.reread(ctx, i)?,
                None => double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
            };
            let blk = ctx.block();
            let handle = blk.call(
                I64,
                "js_regexp_construct",
                &[(DOUBLE, &pattern_box), (DOUBLE, &flags_box)],
            );
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // events.EventEmitter — `new EventEmitter(opts)` is an ordinary
        // object with the shared `EventEmitter.prototype` (#10508); its
        // methods are the prototype's, exactly as for a subclass instance.
        "EventEmitter" => {
            // #6986: `opts` was held across the discard loop's lowering.
            let opts = adopt_leading_arg_discard_rest(ctx, args, group)?;
            Ok(Some(ctx.block().call(
                DOUBLE,
                "js_event_emitter_object_new",
                &[(DOUBLE, &opts)],
            )))
        }
        // The public Node constructor creates an inert ChildProcess whose
        // low-level `.spawn(options)` validates its own option bag. Normal
        // callers use the dedicated spawn/fork lowering paths instead.
        "ChildProcess" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            Ok(Some(ctx.block().call(DOUBLE, "js_child_process_new", &[])))
        }
        "EventEmitterAsyncResource" => {
            // #6986: `opts` was held across the discard loop's lowering.
            let opts = adopt_leading_arg_discard_rest(ctx, args, group)?;
            Ok(Some(ctx.block().call(
                DOUBLE,
                "js_event_emitter_async_resource_object_new",
                &[(DOUBLE, &opts)],
            )))
        }
        "BlockList" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            let handle = blk.call(I64, "js_net_block_list_new", &[]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        "SocketAddress" => {
            // #6986: `options` was held across the discard loop's lowering.
            let options = adopt_leading_arg_discard_rest(ctx, args, group)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_net_socket_address_new", &[(DOUBLE, &options)]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        "EventTarget" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            let handle = blk.call(I64, "js_event_target_new", &[]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        "MessageChannel" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            Ok(Some(blk.call(DOUBLE, "js_message_channel_new", &[])))
        }
        "MessagePort" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            Ok(Some(blk.call(
                DOUBLE,
                "js_message_port_constructor_error",
                &[],
            )))
        }
        "BroadcastChannel" => {
            // #6986: `name` was held across the discard loop's lowering.
            let name = adopt_leading_arg_discard_rest(ctx, args, group)?;
            let blk = ctx.block();
            Ok(Some(blk.call(
                DOUBLE,
                "js_broadcast_channel_new",
                &[(DOUBLE, &name)],
            )))
        }
        "Event" => {
            // #6986: `event_type` was held across `options`' lowering AND the
            // discard loop.
            let (event_type, options) = adopt_two_leading_args_discard_rest(ctx, args, group)?;
            let blk = ctx.block();
            let argc = args.len().to_string();
            let handle = blk.call(
                I64,
                "js_event_new",
                &[(DOUBLE, &event_type), (DOUBLE, &options), (I32, &argc)],
            );
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        "CustomEvent" => {
            // #6986: `event_type` was held across `options`' lowering AND the
            // discard loop.
            let (event_type, options) = adopt_two_leading_args_discard_rest(ctx, args, group)?;
            let blk = ctx.block();
            let argc = args.len().to_string();
            let handle = blk.call(
                I64,
                "js_custom_event_new",
                &[(DOUBLE, &event_type), (DOUBLE, &options), (I32, &argc)],
            );
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        "DOMException" => {
            // #6986: `message` was held across `name`'s lowering AND the
            // discard loop.
            let (message, name) = adopt_two_leading_args_discard_rest(ctx, args, group)?;
            let blk = ctx.block();
            let handle = blk.call(
                I64,
                "js_dom_exception_new",
                &[(DOUBLE, &message), (DOUBLE, &name)],
            );
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        "Console" => {
            // #6986: `opts` was held across `stderr`'s lowering AND the
            // discard loop in the two-argument branch.
            let (opts, stderr) = adopt_two_leading_args_discard_rest(ctx, args, group)?;
            if args.get(1).is_some() {
                return Ok(Some(ctx.block().call(
                    DOUBLE,
                    "js_console_new2",
                    &[(DOUBLE, &opts), (DOUBLE, &stderr)],
                )));
            }
            Ok(Some(ctx.block().call(
                DOUBLE,
                "js_console_new",
                &[(DOUBLE, &opts)],
            )))
        }
        // node:perf_hooks — `new PerformanceObserver(cb)` registers the
        // observer and returns its `perf_observer` namespace object (already
        // NaN-boxed). `cb` is passed through so the runtime can invoke it on
        // flush; absent → undefined.
        "PerformanceObserver" => {
            let cb = if let Some(a) = args.first() {
                lower_expr(ctx, a)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            Ok(Some(ctx.block().call(
                DOUBLE,
                "js_perf_observer_new",
                &[(DOUBLE, &cb)],
            )))
        }
        "PerformanceMark" => {
            let (name, options) = adopt_two_leading_args_discard_rest(ctx, args, group)?;
            ctx.pending_declares.push((
                "js_perf_mark_constructor".to_string(),
                DOUBLE,
                vec![DOUBLE, DOUBLE],
            ));
            Ok(Some(ctx.block().call(
                DOUBLE,
                "js_perf_mark_constructor",
                &[(DOUBLE, &name), (DOUBLE, &options)],
            )))
        }
        "Performance"
        | "PerformanceEntry"
        | "PerformanceMeasure"
        | "PerformanceObserverEntryList"
        | "PerformanceResourceTiming" => {
            for arg in args {
                let _ = lower_expr(ctx, arg)?;
            }
            ctx.pending_declares
                .push(("js_perf_illegal_constructor".to_string(), DOUBLE, vec![]));
            Ok(Some(ctx.block().call(
                DOUBLE,
                "js_perf_illegal_constructor",
                &[],
            )))
        }
        // string_decoder.StringDecoder — issue #848. `new StringDecoder("utf8")`
        // pre-fix fell through to the generic `js_object_alloc(0, 0)` placeholder,
        // so `dec.write` / `dec.end` were `undefined`. Allocate a real handle
        // here; `common/dispatch.rs` dispatches the instance methods + getters
        // through HANDLE_METHOD_DISPATCH / HANDLE_PROPERTY_DISPATCH. Encoding
        // arg is passed through so future non-UTF-8 backends can switch on it;
        // the current impl only tracks the UTF-8 partial-codepoint state.
        "StringDecoder" => {
            // #6986: `enc_box` was held across the discard loop's lowering.
            let enc_box = adopt_leading_arg_discard_rest(ctx, args, group)?;
            let blk = ctx.block();
            // The raw NaN-box bits, not a mask: the runtime tells undefined, a
            // heap string and an inline SSO name (`"ut" + "f8"`) apart by tag
            // (#11519); a masked SSO value was read as a header address.
            let enc_handle = blk.bitcast_double_to_i64(&enc_box);
            let handle = blk.call(I64, "js_string_decoder_new", &[(I64, &enc_handle)]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // node:stream — `new Readable(opts)` / `new Writable(opts)` /
        // `new Duplex(opts)` / `new Transform(opts)` / `new PassThrough(opts)`.
        // Issue #631. Pre-fix the generic Expr::New path produced an empty
        // ObjectHeader, so `r.on`, `r.pipe`, `.read`, etc. were undefined and
        // any downstream call crashed. The runtime helpers in
        // `perry-runtime/src/node_stream.rs` build an ObjectHeader with each
        // method name keyed to a NaN-boxed closure pointer that captures the
        // host object — `typeof r.on === "function"` and chained
        // `.on(...).on(...).pipe(...)` calls return `this` so the chain
        // doesn't lose identity. Stub semantics only: no real data pump.
        "Readable" | "Writable" | "Duplex" | "Transform" | "PassThrough" => {
            // #6986: `opts_box` was held across the discard loop's lowering.
            let opts_box = adopt_leading_arg_discard_rest(ctx, args, group)?;
            let runtime_fn = match class_name {
                "Readable" => "js_node_stream_readable_new",
                "Writable" => "js_node_stream_writable_new",
                "Duplex" => "js_node_stream_duplex_new",
                "Transform" => "js_node_stream_transform_new",
                "PassThrough" => "js_node_stream_passthrough_new",
                _ => unreachable!(),
            };
            let result = ctx.block().call(DOUBLE, runtime_fn, &[(DOUBLE, &opts_box)]);
            Ok(Some(result))
        }
        // (`WebSocketServer` is handled by an earlier branch lower in this
        // file — pre-existing from 2026-04-14. No new branch needed here.)
        // bun:sqlite Database — distinct internal name avoids colliding with
        // better-sqlite3's exported `Database` while preserving full JS values
        // for Bun's optional filename and flags object.
        "BunSqliteDatabase" => {
            let path_idx = adopt_optional_arg(ctx, args, 0, group)?;
            let options_idx = adopt_optional_arg(ctx, args, 1, group)?;
            let undef = || double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let path_value = match path_idx {
                Some(i) => group.reread(ctx, i)?,
                None => undef(),
            };
            let options_value = match options_idx {
                Some(i) => group.reread(ctx, i)?,
                None => undef(),
            };
            let blk = ctx.block();
            let handle = blk.call(
                I64,
                "js_bun_sqlite_database_new",
                &[(DOUBLE, &path_value), (DOUBLE, &options_value)],
            );
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // better-sqlite3 Database — `new Database(filename)` opens a SQLite
        // connection. Without this, `new Database(...)` falls into lower_new's
        // empty-object placeholder, so `db` is a generic ObjectHeader pointer
        // instead of a real Handle from `js_sqlite_open`. `db.prepare(...)`
        // then unboxes that bogus pointer; `get_handle::<SqliteDbHandle>`
        // returns None; prepare returns -1; every chained `.run()`/`.get()`/
        // `.all()` dispatches against junk and silently produces undefined.
        "Database" => {
            let path_ptr = if let Some(arg) = args.first() {
                get_raw_string_ptr(ctx, arg)?
            } else {
                "0".to_string()
            };
            let blk = ctx.block();
            let handle = blk.call(I64, "js_sqlite_open", &[(I64, &path_ptr)]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // node:sqlite DatabaseSync — keep full NaN-boxed values for path and
        // options so the runtime can preserve Node-shaped validation errors.
        "DatabaseSync" => {
            // #6986: `path_value` was held in a bare SSA register across
            // `options_value`'s lowering (an arbitrary user expression).
            let path_idx = adopt_optional_arg(ctx, args, 0, group)?;
            let options_idx = adopt_optional_arg(ctx, args, 1, group)?;
            let undef = || double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let path_value = match path_idx {
                Some(i) => group.reread(ctx, i)?,
                None => undef(),
            };
            let options_value = match options_idx {
                Some(i) => group.reread(ctx, i)?,
                None => undef(),
            };
            let blk = ctx.block();
            let handle = blk.call(
                I64,
                "js_node_sqlite_database_sync_new",
                &[(DOUBLE, &path_value), (DOUBLE, &options_value)],
            );
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        "StatementSync" => {
            // #6986: `arg0` was held in a bare SSA register across `arg1`'s
            // lowering (an arbitrary user expression).
            let idx0 = adopt_optional_arg(ctx, args, 0, group)?;
            let idx1 = adopt_optional_arg(ctx, args, 1, group)?;
            let undef = || double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let arg0 = match idx0 {
                Some(i) => group.reread(ctx, i)?,
                None => undef(),
            };
            let arg1 = match idx1 {
                Some(i) => group.reread(ctx, i)?,
                None => undef(),
            };
            let blk = ctx.block();
            let handle = blk.call(
                I64,
                "js_node_sqlite_statement_sync_new",
                &[(DOUBLE, &arg0), (DOUBLE, &arg1)],
            );
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        "Session" => {
            // #6986: `arg0` was held in a bare SSA register across `arg1`'s
            // lowering (an arbitrary user expression).
            let idx0 = adopt_optional_arg(ctx, args, 0, group)?;
            let idx1 = adopt_optional_arg(ctx, args, 1, group)?;
            let undef = || double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let arg0 = match idx0 {
                Some(i) => group.reread(ctx, i)?,
                None => undef(),
            };
            let arg1 = match idx1 {
                Some(i) => group.reread(ctx, i)?,
                None => undef(),
            };
            let blk = ctx.block();
            let handle = blk.call(
                I64,
                "js_node_sqlite_session_new",
                &[(DOUBLE, &arg0), (DOUBLE, &arg1)],
            );
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // async_hooks.AsyncLocalStorage — `new AsyncLocalStorage()` produces a
        // real handle so `.run(store, cb)` / `.getStore()` / `.enterWith(store)`
        // / `.exit(cb)` / `.disable()` find their registered store stack.
        // Same #187-shape bug — pre-fix `new AsyncLocalStorage()` fell into the
        // empty-placeholder branch and `.run(store, cb)` dispatched against a
        // junk pointer (callback never fired, store never recorded).
        "AsyncLocalStorage" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            let handle = blk.call(I64, "js_async_local_storage_new", &[]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // #1367: `new crypto.X509Certificate(pem | der)` — parse the cert
        // into a handle exposing subject/issuer/validFrom/validTo/
        // serialNumber/fingerprint/fingerprint256/ca/raw. The arg is a PEM
        // string or DER Buffer; unbox to its raw pointer for the runtime
        // parser (`js_crypto_x509_new` returns an already-NaN-boxed handle).
        "X509Certificate" => {
            let input = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let input_handle = unbox_to_i64(blk, &input);
            Ok(Some(blk.call(
                DOUBLE,
                "js_crypto_x509_new",
                &[(I64, &input_handle)],
            )))
        }
        "AsyncResource" => {
            // #6986: `type_value` was held in a bare SSA register across
            // `options_value`'s lowering (an arbitrary user expression).
            let type_idx = adopt_optional_arg(ctx, args, 0, group)?;
            let options_idx = adopt_optional_arg(ctx, args, 1, group)?;
            let undef = || double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let type_value = match type_idx {
                Some(i) => group.reread(ctx, i)?,
                None => undef(),
            };
            let options_value = match options_idx {
                Some(i) => group.reread(ctx, i)?,
                None => undef(),
            };
            let blk = ctx.block();
            let handle = blk.call(
                I64,
                "js_async_resource_new",
                &[(DOUBLE, &type_value), (DOUBLE, &options_value)],
            );
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // #2875: TC39 explicit-resource-management stacks. `new
        // DisposableStack()` / `new AsyncDisposableStack()` allocate a
        // GC-managed stack object (NaN-boxed pointer) so the instance methods
        // (`use` / `adopt` / `defer` / `dispose` / `move` / `disposed`)
        // dispatch through the `__disposable__` rows in native_table.
        "DisposableStack" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            let handle = blk.call(I64, "js_disposable_stack_new", &[]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        "AsyncDisposableStack" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            let handle = blk.call(I64, "js_async_disposable_stack_new", &[]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // #2875: `new SuppressedError(error, suppressed, message?)` — an
        // Error-subclass object carrying `.error` / `.suppressed` /
        // `.message` / `.name`. The runtime ctor registers the class id as
        // extending Error (once) so `instanceof Error` holds; the property
        // reads flow through the ordinary by-name object getter.
        "SuppressedError" => {
            // #6986: `error` (and `suppressed`) were held in bare SSA
            // registers across the later operands' lowering.
            let error_idx = adopt_optional_arg(ctx, args, 0, group)?;
            let suppressed_idx = adopt_optional_arg(ctx, args, 1, group)?;
            let message_idx = adopt_optional_arg(ctx, args, 2, group)?;
            let undef = || double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let error = match error_idx {
                Some(i) => group.reread(ctx, i)?,
                None => undef(),
            };
            let suppressed = match suppressed_idx {
                Some(i) => group.reread(ctx, i)?,
                None => undef(),
            };
            let message = match message_idx {
                Some(i) => group.reread(ctx, i)?,
                None => undef(),
            };
            let blk = ctx.block();
            Ok(Some(blk.call(
                DOUBLE,
                "js_suppressed_error_new",
                &[(DOUBLE, &error), (DOUBLE, &suppressed), (DOUBLE, &message)],
            )))
        }
        "Array" => {
            // `new Array()` → empty array, `new Array(n)` → length-n sparse
            // array after runtime validation, and `new Array(value)` with a
            // non-number argument → one-element array.
            if args.is_empty() {
                let blk = ctx.block();
                let handle = blk.call(I64, "js_array_create", &[]);
                let blk = ctx.block();
                return Ok(Some(nanbox_pointer_inline(blk, &handle)));
            }
            if args.len() == 1 {
                let value = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let handle = blk.call(I64, "js_array_constructor_single", &[(DOUBLE, &value)]);
                let blk = ctx.block();
                return Ok(Some(nanbox_pointer_inline(blk, &handle)));
            }
            // #3985: `Array(a, b, c, ...)` / `new Array(a, b, ...)` with ≥2 args
            // is the element-list form — semantically identical to the
            // `[a, b, c, ...]` literal (only the single-number form means
            // "length"). Previously this returned `Ok(None)` and fell back to a
            // generic path that produced a length-0 array.
            let arr = lower_array_literal(ctx, args)?;
            Ok(Some(arr))
        }
        "Response" => {
            // new Response(body?, init?) — init = { status?, statusText?, headers? }
            // Clear BodyInit metadata before evaluating either argument: init
            // evaluation can throw after the body was converted, in which case
            // js_response_new never gets a chance to consume that metadata.
            ctx.block().call(DOUBLE, "js_response_body_init_reset", &[]);
            // Route the body through js_response_body_init_ptr (not the plain
            // string coercion) so a ReadableStream body — e.g. Hono's
            // `new Response(res.body, res)` header re-wrap — is drained to its
            // bytes instead of stringified to its numeric stream handle.
            // Non-stream bodies coerce exactly as get_raw_string_ptr did.
            let body_ptr = if !args.is_empty() {
                let v = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                blk.call(I64, "js_response_body_init_ptr", &[(DOUBLE, &v)])
            } else {
                "0".to_string()
            };
            // #11789 sweep: the body is converted BEFORE the init is evaluated
            // (the BodyInit metadata reset above depends on that order), so
            // its raw pointer is rooted across the init's evaluation and read
            // back below it.
            let body_root =
                group.adopt_emitted(ctx, rooting::Repr::Ptr, &body_ptr, args.len() >= 2);

            // Default init: status=200, statusText=null, headers=0
            let (status_val, status_text_ptr, headers_handle) = if args.len() >= 2 {
                super::options::lower_response_init(ctx, &args[1])?
            } else {
                ("200.0".to_string(), "0".to_string(), "0.0".to_string())
            };
            let body_ptr = group.reread_emitted(ctx, body_root);

            let handle = ctx.block().call(
                DOUBLE,
                "js_response_new",
                &[
                    (I64, &body_ptr),
                    (DOUBLE, &status_val),
                    (I64, &status_text_ptr),
                    (DOUBLE, &headers_handle),
                ],
            );
            // Response handle is a plain numeric f64 (response-registry id).
            // DO NOT NaN-box — method dispatch expects raw f64.
            Ok(Some(handle))
        }

        // Issue #1211: `new Blob(parts, opts)` / `new File(parts, name, opts)`.
        // `parts` is a JS array of mixed string/Buffer/Blob inputs — the
        // runtime helper (`js_blob_new` / `js_file_new`) walks the array
        // and concatenates the bytes. Locals bound by `const b = new Blob(...)`
        // are tagged `("blob", "Blob")` in `destructuring/var_decl.rs` so
        // subsequent property/method access dispatches through the
        // `module == "blob"` arm above.
        "Blob" => {
            // #11789 sweep: `parts` is held across the options' evaluation and
            // the `type` across the options after it; both are read back below
            // the last. The options literal is only ANALYSED here, so it can be
            // destructured before anything is lowered.
            let props_opt = if args.len() >= 2 {
                extract_options_fields(ctx, &args[1])
            } else {
                None
            };
            let mut blob_group =
                rooting::open_rooted_group(1 + props_opt.as_ref().map_or(1, Vec::len));
            let parts_root = match args.first() {
                Some(a) => Some(blob_group.lower(ctx, a, args.len() >= 2)?),
                None => None,
            };
            let mut type_root: Option<usize> = None;
            if args.len() >= 2 {
                if let Some(props) = &props_opt {
                    for (i, (k, vexpr)) in props.iter().enumerate() {
                        let collects = rooting::any_operand_may_collect(
                            ctx,
                            props[i + 1..].iter().map(|(_, later)| later),
                        );
                        let root = blob_group.lower(ctx, vexpr, collects)?;
                        if k == "type" {
                            type_root = Some(root);
                        }
                    }
                } else {
                    blob_group.lower(ctx, &args[1], false)?;
                }
            }
            let parts = match parts_root {
                Some(root) => blob_group.reread(ctx, root)?,
                None => double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
            };
            let type_str = match type_root {
                Some(root) => blob_group.reread(ctx, root)?,
                None => double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
            };
            let handle = ctx.block().call(
                DOUBLE,
                "js_blob_new",
                &[(DOUBLE, &parts), (DOUBLE, &type_str)],
            );
            blob_group.release(ctx);
            Ok(Some(handle))
        }

        "File" => {
            // #11789 sweep: as for `Blob` above, with the `name` argument and
            // the `lastModified` option in the same window.
            let props_opt = if args.len() >= 3 {
                extract_options_fields(ctx, &args[2])
            } else {
                None
            };
            let mut file_group =
                rooting::open_rooted_group(2 + props_opt.as_ref().map_or(1, Vec::len));
            let parts_root = match args.first() {
                Some(a) => Some(file_group.lower(ctx, a, args.len() >= 2)?),
                None => None,
            };
            let name_root = match args.get(1) {
                Some(a) => Some(file_group.lower(ctx, a, args.len() >= 3)?),
                None => None,
            };
            let mut type_root: Option<usize> = None;
            let mut last_modified_root: Option<usize> = None;
            if args.len() >= 3 {
                if let Some(props) = &props_opt {
                    for (i, (k, vexpr)) in props.iter().enumerate() {
                        let collects = rooting::any_operand_may_collect(
                            ctx,
                            props[i + 1..].iter().map(|(_, later)| later),
                        );
                        let root = file_group.lower(ctx, vexpr, collects)?;
                        match k.as_str() {
                            "type" => type_root = Some(root),
                            "lastModified" => last_modified_root = Some(root),
                            _ => {}
                        }
                    }
                } else {
                    file_group.lower(ctx, &args[2], false)?;
                }
            }
            let undef = || double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let parts = match parts_root {
                Some(root) => file_group.reread(ctx, root)?,
                None => undef(),
            };
            let name = match name_root {
                Some(root) => file_group.reread(ctx, root)?,
                None => undef(),
            };
            let type_str = match type_root {
                Some(root) => file_group.reread(ctx, root)?,
                None => undef(),
            };
            // NaN signals "use Date.now()" inside the runtime helper.
            let last_modified = match last_modified_root {
                Some(root) => file_group.reread(ctx, root)?,
                None => double_literal(f64::NAN),
            };
            let handle = ctx.block().call(
                DOUBLE,
                "js_file_new",
                &[
                    (DOUBLE, &parts),
                    (DOUBLE, &name),
                    (DOUBLE, &type_str),
                    (DOUBLE, &last_modified),
                ],
            );
            file_group.release(ctx);
            Ok(Some(handle))
        }

        "Headers" => {
            // new Headers(init?) — init can be an object literal or another
            // Headers/array iterable.
            let h = ctx.block().call(DOUBLE, "js_headers_new", &[]);
            // `js_headers_new` produces the only reference to the new handle.
            // Initializer evaluation and string coercion can collect, so keep
            // that emitted value in the surrounding constructor root group and
            // re-read it at every use below.  In particular, a dynamic header
            // value may run `toString()` before `js_headers_set` consumes the
            // handle (#8087's native statepoint corpus caught this window).
            let h_root = group.adopt_emitted(ctx, rooting::Repr::Boxed, &h, !args.is_empty());
            if !args.is_empty() {
                if let Some(props) = extract_options_fields(ctx, &args[0]) {
                    for (k, vexpr) in &props {
                        let key_expr = Expr::String(k.clone());
                        // #11789 sweep: the value is evaluated (and coerced --
                        // a `toString()` is user code) BEFORE the key's raw
                        // pointer is taken, so the key is never held across it.
                        let value = lower_expr(ctx, vexpr)?;
                        let val_ptr =
                            ctx.block()
                                .call(I64, "js_jsvalue_to_string", &[(DOUBLE, &value)]);
                        let key_ptr = get_raw_string_ptr(ctx, &key_expr)?;
                        let h = group.reread_emitted(ctx, h_root);
                        ctx.block().call(
                            DOUBLE,
                            "js_headers_set",
                            &[(DOUBLE, &h), (I64, &key_ptr), (I64, &val_ptr)],
                        );
                    }
                } else {
                    let init = lower_expr(ctx, &args[0])?;
                    let h = group.reread_emitted(ctx, h_root);
                    ctx.block().call(
                        DOUBLE,
                        "js_headers_init_from_value",
                        &[(DOUBLE, &h), (DOUBLE, &init)],
                    );
                }
            }
            Ok(Some(group.reread_emitted(ctx, h_root)))
        }

        "FormData" => {
            // new FormData() — Perry's current native registry stores string
            // values, which covers deterministic constructor/mutator parity
            // for append/set/delete/get/getAll/has/iteration.
            let h = ctx.block().call(DOUBLE, "js_form_data_new", &[]);
            Ok(Some(h))
        }

        "Request" => {
            // Preserve the input until runtime construction. Reducing it to a
            // URL loses Request fields before init overrides are applied (#10380).
            let (input, init) = adopt_two_leading_args_discard_rest(ctx, args, group)?;
            Ok(Some(ctx.block().call(
                DOUBLE,
                "js_request_new_from_input",
                &[(DOUBLE, &input), (DOUBLE, &init)],
            )))
        }

        // Issue #237: Web Streams API constructors. Source / sink / transform
        // objects accept `start` / `pull` / `cancel` / `write` / `close` /
        // `abort` / `transform` / `flush` callbacks; missing ones are passed
        // as TAG_UNDEFINED so the runtime can no-op cleanly.
        "ReadableStream" => {
            // #11789 sweep: the callbacks (each a closure allocation) and the
            // strategy are lowered and rooted in source order, each across the
            // ones after it, and read back together below the last.
            let (named, source_object, extra) =
                lower_stream_ctor_args(ctx, args, &["start", "pull", "cancel", "type"], 1)?;
            let [start, pull, cancel, source_type]: [String; 4] =
                named.try_into().expect("four named stream options");
            let strategy = extra[0]
                .clone()
                .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            if let Some(source) = source_object {
                let h = ctx.block().call(
                    DOUBLE,
                    "js_readable_stream_new_from_source_object",
                    &[(DOUBLE, &source), (DOUBLE, &strategy)],
                );
                return Ok(Some(h));
            }
            let h = ctx.block().call(
                DOUBLE,
                "js_readable_stream_new_with_strategy_and_source_type",
                &[
                    (DOUBLE, &start),
                    (DOUBLE, &pull),
                    (DOUBLE, &cancel),
                    (DOUBLE, &strategy),
                    (DOUBLE, &source_type),
                ],
            );
            Ok(Some(h))
        }

        "WritableStream" => {
            if matches!(args.first(), Some(Expr::Null)) {
                for arg in args {
                    let _ = lower_expr(ctx, arg)?;
                }
                let h = ctx
                    .block()
                    .call(DOUBLE, "js_writable_stream_throw_invalid_sink", &[]);
                return Ok(Some(h));
            }
            // #11789 sweep: as for `ReadableStream` above. #4915: the whole
            // strategy value passes through -- the runtime accepts a plain
            // highWaterMark number or a strategy object (e.g.
            // ByteLengthQueuingStrategy) and reads highWaterMark + size() from it.
            let (named, sink_object, extra) = lower_stream_ctor_args(
                ctx,
                args,
                &["start", "write", "close", "abort", "type"],
                1,
            )?;
            let [start, write, close, abort, sink_type]: [String; 5] =
                named.try_into().expect("five named stream options");
            let hwm = extra[0].clone().unwrap_or_else(|| double_literal(1.0));
            if let Some(sink) = sink_object {
                let h = ctx.block().call(
                    DOUBLE,
                    "js_writable_stream_new_from_sink_object",
                    &[(DOUBLE, &sink), (DOUBLE, &hwm)],
                );
                return Ok(Some(h));
            }
            let h = ctx.block().call(
                DOUBLE,
                "js_writable_stream_new_with_sink_type",
                &[
                    (DOUBLE, &start),
                    (DOUBLE, &write),
                    (DOUBLE, &close),
                    (DOUBLE, &abort),
                    (DOUBLE, &hwm),
                    (DOUBLE, &sink_type),
                ],
            );
            Ok(Some(h))
        }

        "TransformStream" => {
            // #11789 sweep: as for `ReadableStream` above. #4915:
            // writableStrategy (arg 1) / readableStrategy (arg 2) -- each may be
            // a plain highWaterMark number or a strategy object; the runtime
            // parses either form.
            let (named, transformer_object, extra) =
                lower_stream_ctor_args(ctx, args, &["start", "transform", "flush"], 2)?;
            let [start, transform, flush]: [String; 3] =
                named.try_into().expect("three named stream options");
            let writable_strategy = extra[0].clone().unwrap_or_else(|| double_literal(1.0));
            let readable_strategy = extra[1].clone().unwrap_or_else(|| double_literal(0.0));
            if let Some(transformer) = transformer_object {
                let h = ctx.block().call(
                    DOUBLE,
                    "js_transform_stream_new_from_transformer_object",
                    &[
                        (DOUBLE, &transformer),
                        (DOUBLE, &writable_strategy),
                        (DOUBLE, &readable_strategy),
                    ],
                );
                return Ok(Some(h));
            }
            let h = ctx.block().call(
                DOUBLE,
                "js_transform_stream_new",
                &[
                    (DOUBLE, &start),
                    (DOUBLE, &transform),
                    (DOUBLE, &flush),
                    (DOUBLE, &writable_strategy),
                    (DOUBLE, &readable_strategy),
                ],
            );
            Ok(Some(h))
        }

        "TextEncoderStream" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let h = ctx
                .block()
                .call(DOUBLE, "js_stream_web_text_encoder_stream_new", &[]);
            Ok(Some(h))
        }

        "TextDecoderStream" => {
            // #6986: `label` was held across `options`' lowering AND the
            // discard loop.
            let (label, options) = adopt_two_leading_args_discard_rest(ctx, args, group)?;
            let h = ctx.block().call(
                DOUBLE,
                "js_stream_web_text_decoder_stream_new",
                &[(DOUBLE, &label), (DOUBLE, &options)],
            );
            Ok(Some(h))
        }

        "CompressionStream" | "DecompressionStream" => {
            // #6986: `format` was held across the discard loop's lowering.
            let format = adopt_leading_arg_discard_rest(ctx, args, group)?;
            let runtime = if class_name == "CompressionStream" {
                "js_stream_web_compression_stream_new"
            } else {
                "js_stream_web_decompression_stream_new"
            };
            let h = ctx.block().call(DOUBLE, runtime, &[(DOUBLE, &format)]);
            Ok(Some(h))
        }

        // #4915: `new ReadableStreamBYOBReader(stream)` — equivalent to
        // `stream.getReader({ mode: "byob" })`. The runtime validates that
        // the argument is a byte stream (TypeError otherwise) and returns a
        // reader handle whose `read(view)` fills the caller's buffer.
        "ReadableStreamBYOBReader" => {
            // #6986: `stream` was held across the discard loop's lowering.
            let stream = adopt_leading_arg_discard_rest(ctx, args, group)?;
            let h = ctx.block().call(
                DOUBLE,
                "js_readable_stream_get_byob_reader",
                &[(DOUBLE, &stream)],
            );
            Ok(Some(h))
        }

        // node:stream/web QueuingStrategy classes (#1545). Both take a single
        // `{ highWaterMark }` options object; the runtime reads
        // `opts.highWaterMark` and builds a `{ highWaterMark, size }` object.
        // CountQueuingStrategy.size() returns 1, ByteLengthQueuingStrategy.size(chunk)
        // returns chunk.byteLength. Pass the whole options expression through so
        // both literal (`{ highWaterMark: 5 }`) and dynamic option objects work.
        "CountQueuingStrategy" | "ByteLengthQueuingStrategy" => {
            // #6986: `opts` was held across the discard loop's lowering.
            let opts = adopt_leading_arg_discard_rest(ctx, args, group)?;
            let func = if class_name == "CountQueuingStrategy" {
                "js_count_queuing_strategy_new"
            } else {
                "js_byte_length_queuing_strategy_new"
            };
            let h = ctx.block().call(DOUBLE, func, &[(DOUBLE, &opts)]);
            Ok(Some(h))
        }

        "Promise" => {
            // `new Promise((resolve, reject) => { ... })` — the runtime's
            // `js_promise_new_with_executor` takes the closure, allocates
            // the resolve/reject helper closures, and invokes the executor
            // synchronously. The executor must actually run to honor
            // imperative patterns like `new Promise(r => { setTimeout(r,1) })`
            // that are common in the tests.
            if args.is_empty() {
                let p = ctx.block().call(I64, "js_promise_new", &[]);
                return Ok(Some(nanbox_pointer_inline(ctx.block(), &p)));
            }
            let exec_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let exec_handle = unbox_to_i64(blk, &exec_box);
            let p = blk.call(I64, "js_promise_new_with_executor", &[(I64, &exec_handle)]);
            Ok(Some(nanbox_pointer_inline(blk, &p)))
        }
        "WeakMap" => {
            // #6986: the iterable was lowered into a bare SSA register and
            // held across `js_weakmap_new`'s allocation — the eager
            // `.map(lower_expr)` computed every argument up front, then the
            // allocating call ran, then `lowered_args.first()` was read back
            // from that now-possibly-stale register. `collects: true`
            // unconditionally — the allocation always follows, regardless of
            // how many extra arguments there are.
            let iterable_idx = match args.first() {
                Some(a) => Some(group.lower(ctx, a, true)?),
                None => None,
            };
            for a in args.iter().skip(1) {
                let _ = lower_expr(ctx, a)?;
            }
            let handle = ctx.block().call(I64, "js_weakmap_new", &[]);
            // js_weakmap_new returns a raw `*mut ObjectHeader` — NaN-box
            // with POINTER_TAG so subsequent `js_weakmap_*` calls can
            // `js_nanbox_get_pointer` on the f64.
            let boxed = nanbox_pointer_inline(ctx.block(), &handle);
            match iterable_idx {
                Some(i) => {
                    let iterable = group.reread(ctx, i)?;
                    Ok(Some(ctx.block().call(
                        DOUBLE,
                        "js_weakmap_init_iterable",
                        &[(DOUBLE, &boxed), (DOUBLE, &iterable)],
                    )))
                }
                None => Ok(Some(boxed)),
            }
        }
        "WeakSet" => {
            // #6986: same hazard as WeakMap above.
            let iterable_idx = match args.first() {
                Some(a) => Some(group.lower(ctx, a, true)?),
                None => None,
            };
            for a in args.iter().skip(1) {
                let _ = lower_expr(ctx, a)?;
            }
            let handle = ctx.block().call(I64, "js_weakset_new", &[]);
            let boxed = nanbox_pointer_inline(ctx.block(), &handle);
            match iterable_idx {
                Some(i) => {
                    let iterable = group.reread(ctx, i)?;
                    Ok(Some(ctx.block().call(
                        DOUBLE,
                        "js_weakset_init_iterable",
                        &[(DOUBLE, &boxed), (DOUBLE, &iterable)],
                    )))
                }
                None => Ok(Some(boxed)),
            }
        }
        "AbortController" => {
            // Lower any incidental args for side effects (shouldn't have any).
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let handle = ctx.block().call(I64, "js_abort_controller_new", &[]);
            // The runtime returns a raw *mut ObjectHeader — NaN-box with
            // POINTER_TAG so regular property get (`controller.signal`,
            // `controller.aborted`) works via js_object_get_field_by_name_f64.
            let boxed = nanbox_pointer_inline(ctx.block(), &handle);
            Ok(Some(boxed))
        }

        // new WebSocketServer({ port: N }) → js_ws_server_new(opts_f64)
        "WebSocketServer" => {
            // Lower the options object (first arg) as a NaN-boxed f64.
            let opts = if !args.is_empty() {
                lower_expr(ctx, &args[0])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            ctx.pending_declares
                .push(("js_ws_server_new".to_string(), I64, vec![DOUBLE]));
            let blk = ctx.block();
            let handle = blk.call(I64, "js_ws_server_new", &[(DOUBLE, &opts)]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // Issue #606 — `new WebSocket(url)` from `import { WebSocket } from
        // "ws"`. npm ws's API is sync-ctor: returns the client handle
        // immediately and connects in the background; the user's
        // `client.on("open", cb)` then registers a listener that fires
        // once the connect completes. The previous lower path treated
        // `new WebSocket(...)` as a no-op `Expr::New` and let the
        // method-dispatch tower invoke `js_ws_connect` (which returns a
        // Promise, not a handle), so `client.on(...)` was being called
        // against a promise pointer and silently no-op'd. Routing
        // through `js_ws_connect_start` returns the handle synchronously
        // and the connect runs as a sibling tokio task that pushes an
        // Open / Error event when complete.
        "WebSocket" => {
            let url_box = if !args.is_empty() {
                lower_expr(ctx, &args[0])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            ctx.pending_declares
                .push(("js_ws_connect_start".to_string(), DOUBLE, vec![DOUBLE]));
            let blk = ctx.block();
            // js_ws_connect_start returns the ws_id as a plain f64
            // (1.0, 2.0, …). Convert to i64 then NaN-box with
            // POINTER_TAG so the standard `unbox_to_i64` receiver
            // contract recovers the right ws_id at every method call
            // site (`client.on(...)`, `.send(...)`, `.close()`).
            let raw_f64 = blk.call(DOUBLE, "js_ws_connect_start", &[(DOUBLE, &url_box)]);
            let raw_i64 = blk.fptosi(DOUBLE, &raw_f64, I64);
            Ok(Some(nanbox_pointer_inline(blk, &raw_i64)))
        }

        _ => Ok(None),
    }
}

/// #10359: construct the global intrinsic `class_name` through the builtin
/// table alone, for a `new globalThis.<name>()` whose name a module class,
/// class alias or import shadows. `lower_new` resolves those bindings first,
/// so it would construct the binding. `None` means no builtin arm owns the name
/// and no argument was lowered; the caller then constructs the global
/// property's runtime value.
pub(crate) fn lower_global_intrinsic_new(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    args: &[Expr],
) -> Result<Option<String>> {
    let mut group = rooting::open_rooted_group(args.len() + 1);
    let result = lower_builtin_new(ctx, class_name, args, &mut group);
    group.release(ctx);
    result
}

/// Map a typed-array constructor name to its runtime `KIND_*` integer (mirrors
/// `perry_runtime::typedarray::KIND_*`). Used by the `#4103` view-constructor
/// arm to tell `js_typed_array_view` which element type to build.
fn typed_array_view_kind(name: &str) -> i32 {
    match name {
        "Int8Array" => 0,
        "Uint8Array" => 1,
        "Int16Array" => 2,
        "Uint16Array" => 3,
        "Int32Array" => 4,
        "Uint32Array" => 5,
        "Float32Array" => 6,
        "Float64Array" => 7,
        "Uint8ClampedArray" => 8,
        "BigInt64Array" => 9,
        "BigUint64Array" => 10,
        "Float16Array" => 11,
        _ => 7,
    }
}
