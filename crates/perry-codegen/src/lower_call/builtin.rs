//! Built-in `new C()` constructor lowering — `lower_builtin_new`.
//!
//! Tier 2.2 follow-up (v0.5.339) — extracts the 399-LOC dispatcher
//! that handles `new` calls against built-in classes (Date, Map, Set,
//! Buffer, fetch Headers / Request / Response, mongodb MongoClient,
//! redis Redis client, fastify App, ws WebSocketServer, pg Client /
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

use super::{build_headers_from_object, extract_options_fields, get_raw_string_ptr};

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

pub(super) fn lower_builtin_new<'a>(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    args: &'a [Expr],
    group: &mut RootedGroup<'a>,
) -> Result<Option<String>> {
    // Issue #602: ambiguously-named built-in constructors (Client / Pool /
    // Database / Redis / MongoClient / Decimal) collide with bindings from
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
        "Client" | "Pool" => Some(&["pg"]),
        "Database" => Some(&["better-sqlite3"]),
        "DatabaseSync" | "Session" | "StatementSync" => Some(&["sqlite", "node:sqlite"]),
        "Redis" => Some(&["ioredis", "redis", "iovalkey"]),
        "MongoClient" => Some(&["mongodb"]),
        "Decimal" => Some(&["decimal.js"]),
        "RateLimiterMemory" => Some(&["rate-limiter-flexible"]),
        "CronJob" => Some(&["cron", "node-cron"]),
        _ => None,
    };
    if let Some(sources) = required_sources {
        if !import_src.is_some_and(|src| sources.contains(&src)) {
            return Ok(None);
        }
    }
    match class_name {
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
            let msg_handle = unbox_to_i64(blk, &msg_box);
            let runtime = if class_name == "EvalError" {
                "js_evalerror_new"
            } else {
                "js_urierror_new"
            };
            let err_handle = blk.call(I64, runtime, &[(I64, &msg_handle)]);
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
        // commander Command — `new Command()` allocates a real CommanderHandle
        // via the runtime constructor so subsequent `.command(...).action(...)
        // .parse(...)` calls operate on a registered handle. Without this,
        // `lower_new` falls back to an empty placeholder ObjectHeader and the
        // entire fluent chain dispatches against junk (closes #187).
        "Command" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            let handle = blk.call(I64, "js_commander_new", &[]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // events.EventEmitter — `new EventEmitter()` produces a real
        // EventEmitterHandle so `.on(...)` / `.emit(...)` find their
        // registered handle (NATIVE_MODULE_TABLE wires those methods
        // through `js_event_emitter_*`). Same #187-shape bug — pre-fix
        // every .on/.emit call dispatched against a junk pointer and
        // silently registered nothing / fired nothing.
        "EventEmitter" => {
            // #6986: `opts` was held across the discard loop's lowering.
            let opts = adopt_leading_arg_discard_rest(ctx, args, group)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_event_emitter_new_with_options", &[(DOUBLE, &opts)]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
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
            let blk = ctx.block();
            let handle = blk.call(
                I64,
                "js_event_emitter_async_resource_new",
                &[(DOUBLE, &opts)],
            );
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
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
            let enc_handle = unbox_to_i64(blk, &enc_box);
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
        // lru-cache LRUCache — `new LRUCache({ max, ttl, updateAgeOnGet })`.
        // The runtime parses the whole NaN-boxed options object itself
        // (`js_lru_cache_new(options: f64)`), so we just lower the options
        // argument and hand it through — no static field extraction, which
        // means dynamic/variable options objects work too. A missing options
        // argument passes `undefined`, which the runtime rejects with the
        // same `TypeError` npm's constructor destructuring raises.
        "LRUCache" => {
            // npm's constructor ignores everything past the options object,
            // but the arguments are still evaluated — the tail is lowered
            // for its side effects so `new LRUCache(opts, f())` still calls
            // `f`. #6986: `opts_val` was held across that lowering.
            let opts_val = adopt_leading_arg_discard_rest(ctx, args, group)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_lru_cache_new", &[(DOUBLE, &opts_val)]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // (`WebSocketServer` is handled by an earlier branch lower in this
        // file — pre-existing from 2026-04-14. No new branch needed here.)
        // pg Client — `new Client(config)` matching npm pg's API: synchronous
        // constructor that stores the config; the user calls
        // `await client.connect()` separately to open the TCP connection.
        // Pre-fix `new Client(config)` fell into the empty-placeholder branch
        // and every chained method (.connect/.query/.end) dispatched against
        // junk. The runtime's older `js_pg_connect(config) -> Promise<Handle>`
        // (still wired as the receiver-less `pg.connect(config)` factory)
        // combines new+connect in one step; this branch maps the npm shape
        // through the new `js_pg_client_new` (sync, stores config) +
        // `js_pg_client_connect` (async, opens the connection) split.
        "Client" => {
            let config_val = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let handle = blk.call(I64, "js_pg_client_new", &[(DOUBLE, &config_val)]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // pg Pool — `new Pool(config)`. sqlx's `connect_lazy` makes this
        // synchronous (no actual connections opened until first `.query()`),
        // matching npm pg Pool's auto-connect-on-first-use semantics. The
        // older `js_pg_create_pool` factory (returns Promise<Handle>) stays
        // wired for `pg.Pool(config)` and similar patterns.
        "Pool" => {
            let config_val = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let handle = blk.call(I64, "js_pg_pool_new", &[(DOUBLE, &config_val)]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
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
        // mongodb MongoClient — `new MongoClient(uri)` matching npm mongodb's
        // API. URI is a string; runtime stores it and connects later via
        // `await client.connect()`.
        "MongoClient" => {
            let uri_ptr = if let Some(arg) = args.first() {
                get_raw_string_ptr(ctx, arg)?
            } else {
                "0".to_string()
            };
            let blk = ctx.block();
            let handle = blk.call(I64, "js_mongodb_client_new", &[(I64, &uri_ptr)]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // ioredis Redis — `new Redis()` or `new Redis(opts)`. The runtime's
        // `js_ioredis_new` reads connection settings from REDIS_HOST /
        // REDIS_PORT / REDIS_PASSWORD / REDIS_TLS env vars and ignores its
        // config arg; connection is lazy (the handle is registered immediately
        // and the actual TCP/TLS connect runs on the first `.get`/`.set`/etc.).
        // Pre-fix `new Redis()` fell into the empty-placeholder branch and
        // every chained method (set/get/del/exists/incr/decr/expire/quit)
        // dispatched against junk. The instance methods are wired in
        // NATIVE_MODULE_TABLE for module: "ioredis"; this branch makes the
        // ctor produce a real RedisClient handle so the dispatch lands on it.
        "Redis" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            // The runtime sig takes one i64 (currently *const c_void, ignored).
            // Pass 0 — semantically "use env-var defaults".
            let handle = blk.call(I64, "js_ioredis_new", &[(I64, "0")]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // rate-limiter-flexible `new RateLimiterMemory({ points, duration })`.
        // Gated on the import source above. The options object crosses as
        // raw NaN-box bits (i64) so the runtime parses `points`/`duration`
        // by name; missing arg → TAG_UNDEFINED → npm defaults (4 points /
        // 1 s). Pre-fix this fell to the js_object_alloc(0,0) placeholder
        // and every method call dispatched against `{}`. Instance methods
        // (consume/get/delete/block/penalty/reward) are wired in
        // NATIVE_MODULE_TABLE for module "rate-limiter-flexible".
        "RateLimiterMemory" => {
            // #6986: `options` was held across the discard loop's lowering.
            let options = adopt_leading_arg_discard_rest(ctx, args, group)?;
            let blk = ctx.block();
            let options_bits = blk.bitcast_double_to_i64(&options);
            let handle = blk.call(
                I64,
                "js_ratelimit_new_from_options",
                &[(I64, &options_bits)],
            );
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // npm `cron` package: `new CronJob(cronTime, onTick, onComplete?,
        // start?)`. Gated on the import source above. Unlike node-cron's
        // `schedule()` factory (which auto-starts), a CronJob only begins
        // firing when the 4th argument is truthy or `job.start()` is
        // called — `js_cron_job_new` implements that. The onTick closure
        // is UNBOXED to a raw ClosureHeader pointer (unbox_to_i64): the
        // cron tick calls it via js_closure_call0 on the raw pointer, so
        // tagged NaN-box bits would throw "value is not a function" on
        // the first fire. onComplete is lowered for side effects only.
        // start/stop/isRunning/nextDate dispatch via the existing
        // ("cron", true, …) NATIVE_MODULE_TABLE rows.
        "CronJob" => {
            // #6986: `expr_ptr` (via `get_raw_string_ptr`, itself a
            // lower_expr + immediate derive with no window of its own) and
            // `on_tick` were both held across every later argument's
            // lowering. Adopt each boxed operand into `group` as it is
            // produced — argument order preserved — then re-read right
            // before use. `expr_ptr`'s derivation (`js_get_string_pointer_unified`)
            // can itself allocate (SSO materialize, nanbox.rs), so it runs
            // FIRST among the final derivations: `on_tick`'s `unbox_to_i64`
            // is pure bitwise (no collect) and `start` needs no further
            // derivation, so re-reading them after is safe.
            let cron_time_idx = adopt_optional_arg(ctx, args, 0, group)?;
            let on_tick_idx = adopt_optional_arg(ctx, args, 1, group)?;
            if let Some(arg) = args.get(2) {
                let _ = lower_expr(ctx, arg)?;
            }
            let start_idx = adopt_optional_arg(ctx, args, 3, group)?;
            for arg in args.iter().skip(4) {
                let _ = lower_expr(ctx, arg)?;
            }
            let expr_ptr = match cron_time_idx {
                Some(i) => {
                    let cron_time = group.reread(ctx, i)?;
                    ctx.block().call(
                        I64,
                        "js_get_string_pointer_unified",
                        &[(DOUBLE, &cron_time)],
                    )
                }
                None => "0".to_string(),
            };
            let cb_ptr = match on_tick_idx {
                Some(i) => {
                    let on_tick = group.reread(ctx, i)?;
                    let blk = ctx.block();
                    unbox_to_i64(blk, &on_tick)
                }
                None => {
                    let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                    let blk = ctx.block();
                    unbox_to_i64(blk, &undef)
                }
            };
            let start = match start_idx {
                Some(i) => group.reread(ctx, i)?,
                None => double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
            };
            let blk = ctx.block();
            let handle = blk.call(
                I64,
                "js_cron_job_new",
                &[(I64, &expr_ptr), (I64, &cb_ptr), (DOUBLE, &start)],
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
        // decimal.js Decimal — `new Decimal(value)` where value is a number,
        // string, or another Decimal. Routes through `js_decimal_coerce_to_handle`
        // which NaN-decodes the JSValue and dispatches to `from_number` /
        // `from_string` / passthrough for an existing Decimal handle. Without
        // this, `new Decimal("0.1")` falls into the empty-placeholder branch
        // and every chained method dispatches against a junk receiver.
        "Decimal" => {
            let val = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                // `new Decimal()` with no args — coerce undefined → 0.
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let handle = blk.call(I64, "js_decimal_coerce_to_handle", &[(DOUBLE, &val)]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
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

            // Default init: status=200, statusText=null, headers=0
            let mut status_val = "200.0".to_string();
            let mut status_text_ptr = "0".to_string();
            let mut headers_handle = "0.0".to_string();

            if args.len() >= 2 {
                if let Some(props) = extract_options_fields(ctx, &args[1]) {
                    for (k, vexpr) in &props {
                        match k.as_str() {
                            "status" => {
                                status_val = lower_expr(ctx, vexpr)?;
                            }
                            "statusText" => {
                                status_text_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "headers" => {
                                // Inline object → build a Headers handle.
                                // Phase 3 anon-class → same via extract_options.
                                // Other expressions → use as-is (handle f64).
                                if let Some(hprops) = extract_options_fields(ctx, vexpr) {
                                    headers_handle = build_headers_from_object(ctx, &hprops)?;
                                } else {
                                    headers_handle = lower_expr(ctx, vexpr)?;
                                }
                            }
                            _ => {
                                let _ = lower_expr(ctx, vexpr)?;
                            }
                        }
                    }
                } else {
                    // Fix #421 (v0.5.575): the second arg is a runtime
                    // object (not an object literal) — happens when
                    // user code does `new Response(body, opts)` where
                    // `opts` is bound from elsewhere. Hono's
                    // `c.text(body, status)` path builds `{ status,
                    // headers }` inside `#newResponse` and passes it
                    // here. Previously perry just evaluated the arg
                    // for side effects and dropped status/headers,
                    // so every hono response had perry-default
                    // status (200) and no headers — `res.status`
                    // read undefined because the response never
                    // got a status to begin with. Now we extract
                    // `.status` / `.statusText` / `.headers` at
                    // runtime via `js_object_get_field_by_name_f64`
                    // and feed them to `js_response_new`.
                    let opts_val = lower_expr(ctx, &args[1])?;
                    let blk = ctx.block();
                    let opts_handle = crate::expr::unbox_to_i64(blk, &opts_val);

                    // Helper: intern a key, load its raw string ptr,
                    // call js_object_get_field_by_name_f64.
                    let get_field = |ctx_inner: &mut FnCtx<'_>, key: &str| -> Result<String> {
                        let key_idx = ctx_inner.strings.intern(key);
                        let key_global =
                            format!("@{}", ctx_inner.strings.entry(key_idx).handle_global);
                        let blk = ctx_inner.block();
                        let key_box = blk.load(DOUBLE, &key_global);
                        let key_bits = blk.bitcast_double_to_i64(&key_box);
                        let key_raw = blk.and(I64, &key_bits, crate::nanbox::POINTER_MASK_I64);
                        let opts_handle_local = opts_handle.clone();
                        Ok(blk.call(
                            DOUBLE,
                            "js_object_get_field_by_name_f64",
                            &[(I64, &opts_handle_local), (I64, &key_raw)],
                        ))
                    };

                    // status: NaN-boxed f64. The runtime treats NaN /
                    // 0 as "use default 200" so a missing field flows
                    // through cleanly.
                    status_val = get_field(ctx, "status")?;
                    // statusText: NaN-boxed string. Strip to raw ptr
                    // for the FFI signature.
                    let st_box = get_field(ctx, "statusText")?;
                    let blk = ctx.block();
                    status_text_ptr =
                        blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &st_box)]);
                    // headers: NaN-boxed Headers handle (an f64
                    // numeric id from `js_headers_new`). Pass through
                    // verbatim — js_response_new accepts the raw f64.
                    // Defensive: strip NaN-box tag if hono / user code
                    // wrapped it as a pointer.
                    headers_handle = get_field(ctx, "headers")?;
                }
            }

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
            let parts = if !args.is_empty() {
                lower_expr(ctx, &args[0])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let mut type_str = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            if args.len() >= 2 {
                if let Some(props) = extract_options_fields(ctx, &args[1]) {
                    for (k, vexpr) in &props {
                        if k == "type" {
                            type_str = lower_expr(ctx, vexpr)?;
                        } else {
                            let _ = lower_expr(ctx, vexpr)?;
                        }
                    }
                } else {
                    let _ = lower_expr(ctx, &args[1])?;
                }
            }
            let handle = ctx.block().call(
                DOUBLE,
                "js_blob_new",
                &[(DOUBLE, &parts), (DOUBLE, &type_str)],
            );
            Ok(Some(handle))
        }

        "File" => {
            let parts = if !args.is_empty() {
                lower_expr(ctx, &args[0])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let name = if args.len() >= 2 {
                lower_expr(ctx, &args[1])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let mut type_str = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            // NaN signals "use Date.now()" inside the runtime helper.
            let mut last_modified = double_literal(f64::NAN);
            if args.len() >= 3 {
                if let Some(props) = extract_options_fields(ctx, &args[2]) {
                    for (k, vexpr) in &props {
                        match k.as_str() {
                            "type" => type_str = lower_expr(ctx, vexpr)?,
                            "lastModified" => last_modified = lower_expr(ctx, vexpr)?,
                            _ => {
                                let _ = lower_expr(ctx, vexpr)?;
                            }
                        }
                    }
                } else {
                    let _ = lower_expr(ctx, &args[2])?;
                }
            }
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
                        let key_ptr = get_raw_string_ptr(ctx, &key_expr)?;
                        let value = lower_expr(ctx, vexpr)?;
                        let val_ptr =
                            ctx.block()
                                .call(I64, "js_jsvalue_to_string", &[(DOUBLE, &value)]);
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
            // new Request(url, init?) — init = Fetch RequestInit subset.
            // The spec runs ToString on `input` when it isn't already a Request,
            // so a URL object must stringify to its href. `get_raw_string_ptr`
            // (js_get_string_pointer_unified) only unwraps an actual string value
            // — handed a URL object (a heap ObjectHeader) it read the object
            // pointer as a string and produced "", so `new Request(new URL(u)).url`
            // was empty. Auth.js v5 builds its session request as `new
            // Request(fu("session", …))` where `fu` returns a URL object, so the
            // session lookup got an empty URL, returned 400 "Bad request.", and
            // `auth()` yielded that string instead of null — the authenticated-page
            // guard then never redirected and fell through to a DB-pool init that
            // threw. Route through `js_jsvalue_to_string`, which invokes `toString`
            // on an object (URL → href) and passes a real string straight through.
            let url_ptr = if !args.is_empty() {
                let v = lower_expr(ctx, &args[0])?;
                ctx.block()
                    .call(I64, "js_request_input_to_url", &[(DOUBLE, &v)])
            } else {
                "0".to_string()
            };

            let mut method_ptr = "0".to_string();
            let mut body_ptr = "0".to_string();
            let mut headers_handle = "0.0".to_string();
            let mut referrer_ptr = "0".to_string();
            let mut referrer_policy_ptr = "0".to_string();
            let mut mode_ptr = "0".to_string();
            let mut credentials_ptr = "0".to_string();
            let mut cache_ptr = "0".to_string();
            let mut redirect_ptr = "0".to_string();
            let mut integrity_ptr = "0".to_string();
            let mut keepalive = double_literal(f64::from_bits(crate::nanbox::TAG_FALSE));
            let mut duplex_ptr = "0".to_string();
            let mut signal = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));

            // #5458: when the init isn't a statically-extractable object
            // literal (e.g. a call-expression result `f()`, a spread `{...e}`,
            // or a dynamic object), read its fields at runtime instead of
            // discarding it. Dropping the init silently defaulted `method` back
            // to "GET", mis-dispatching POST requests to GET handlers in Hono.
            if args.len() >= 2 && extract_options_fields(ctx, &args[1]).is_none() {
                let init_val = lower_expr(ctx, &args[1])?;
                let handle = ctx.block().call(
                    DOUBLE,
                    "js_request_new_from_init",
                    &[(I64, &url_ptr), (DOUBLE, &init_val)],
                );
                return Ok(Some(handle));
            }

            if args.len() >= 2 {
                if let Some(props) = extract_options_fields(ctx, &args[1]) {
                    for (k, vexpr) in &props {
                        match k.as_str() {
                            "method" => {
                                method_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "body" => {
                                body_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "headers" => {
                                if let Some(hprops) = extract_options_fields(ctx, vexpr) {
                                    headers_handle = build_headers_from_object(ctx, &hprops)?;
                                } else {
                                    headers_handle = lower_expr(ctx, vexpr)?;
                                }
                            }
                            "referrer" => {
                                referrer_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "referrerPolicy" => {
                                referrer_policy_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "mode" => {
                                mode_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "credentials" => {
                                credentials_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "cache" => {
                                cache_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "redirect" => {
                                redirect_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "integrity" => {
                                integrity_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "keepalive" => {
                                keepalive = lower_expr(ctx, vexpr)?;
                            }
                            "duplex" => {
                                duplex_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "signal" => {
                                signal = lower_expr(ctx, vexpr)?;
                            }
                            _ => {
                                let _ = lower_expr(ctx, vexpr)?;
                            }
                        }
                    }
                } else {
                    let _ = lower_expr(ctx, &args[1])?;
                }
            }

            let handle = ctx.block().call(
                DOUBLE,
                "js_request_new",
                &[
                    (I64, &url_ptr),
                    (I64, &method_ptr),
                    (I64, &body_ptr),
                    (DOUBLE, &headers_handle),
                    (I64, &referrer_ptr),
                    (I64, &referrer_policy_ptr),
                    (I64, &mode_ptr),
                    (I64, &credentials_ptr),
                    (I64, &cache_ptr),
                    (I64, &redirect_ptr),
                    (I64, &integrity_ptr),
                    (DOUBLE, &keepalive),
                    (I64, &duplex_ptr),
                    (DOUBLE, &signal),
                ],
            );
            Ok(Some(handle))
        }

        // Issue #237: Web Streams API constructors. Source / sink / transform
        // objects accept `start` / `pull` / `cancel` / `write` / `close` /
        // `abort` / `transform` / `flush` callbacks; missing ones are passed
        // as TAG_UNDEFINED so the runtime can no-op cleanly.
        "ReadableStream" => {
            let mut start = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut pull = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut cancel = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut source_type = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut strategy = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut source_object = None;
            if !args.is_empty() {
                if let Some(props) = extract_options_fields(ctx, &args[0]) {
                    for (k, vexpr) in &props {
                        match k.as_str() {
                            "start" => {
                                start = lower_expr(ctx, vexpr)?;
                            }
                            "pull" => {
                                pull = lower_expr(ctx, vexpr)?;
                            }
                            "cancel" => {
                                cancel = lower_expr(ctx, vexpr)?;
                            }
                            "type" => {
                                source_type = lower_expr(ctx, vexpr)?;
                            }
                            _ => {
                                let _ = lower_expr(ctx, vexpr)?;
                            }
                        }
                    }
                } else {
                    source_object = Some(lower_expr(ctx, &args[0])?);
                }
            }
            if args.len() >= 2 {
                strategy = lower_expr(ctx, &args[1])?;
            }
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
            let mut start = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut write = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut close = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut abort = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut sink_type = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut hwm = double_literal(1.0);
            let mut sink_object = None;
            if !args.is_empty() {
                if let Some(props) = extract_options_fields(ctx, &args[0]) {
                    for (k, vexpr) in &props {
                        match k.as_str() {
                            "start" => {
                                start = lower_expr(ctx, vexpr)?;
                            }
                            "write" => {
                                write = lower_expr(ctx, vexpr)?;
                            }
                            "close" => {
                                close = lower_expr(ctx, vexpr)?;
                            }
                            "abort" => {
                                abort = lower_expr(ctx, vexpr)?;
                            }
                            "type" => {
                                sink_type = lower_expr(ctx, vexpr)?;
                            }
                            _ => {
                                let _ = lower_expr(ctx, vexpr)?;
                            }
                        }
                    }
                } else {
                    sink_object = Some(lower_expr(ctx, &args[0])?);
                }
            }
            if args.len() >= 2 {
                // #4915: pass the whole strategy value through — the runtime
                // accepts a plain highWaterMark number or a strategy object
                // (e.g. ByteLengthQueuingStrategy) and reads highWaterMark +
                // size() from it.
                hwm = lower_expr(ctx, &args[1])?;
            }
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
            let mut start = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut transform = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut flush = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let hwm = double_literal(1.0);
            let mut transformer_object = None;
            if !args.is_empty() {
                if let Some(props) = extract_options_fields(ctx, &args[0]) {
                    for (k, vexpr) in &props {
                        match k.as_str() {
                            "start" => {
                                start = lower_expr(ctx, vexpr)?;
                            }
                            "transform" => {
                                transform = lower_expr(ctx, vexpr)?;
                            }
                            "flush" => {
                                flush = lower_expr(ctx, vexpr)?;
                            }
                            _ => {
                                let _ = lower_expr(ctx, vexpr)?;
                            }
                        }
                    }
                } else {
                    transformer_object = Some(lower_expr(ctx, &args[0])?);
                }
            }
            // #4915: writableStrategy (arg 1) / readableStrategy (arg 2) —
            // each may be a plain highWaterMark number or a strategy object;
            // the runtime parses either form.
            let mut writable_strategy = hwm;
            let mut readable_strategy = double_literal(0.0);
            if args.len() >= 2 {
                writable_strategy = lower_expr(ctx, &args[1])?;
            }
            if args.len() >= 3 {
                readable_strategy = lower_expr(ctx, &args[2])?;
            }
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
