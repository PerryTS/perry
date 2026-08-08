//! ArrayIsArray..ProcessEnv (arrays + buffers + paths).
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.
//!
//! # Layer 1 migrated module (#7615, slice 1b)
//!
//! Nothing in here names `expr::temp_root`; every operand that is live across
//! the lowering of a sibling operand goes through
//! [`crate::rooting::with_operands_rooted`], which lowers the group left to
//! right with each already-evaluated value rooted across the ones that follow,
//! re-reads them below the last collection point, and owns the release on every
//! path out including `?`. `crate::rooting::migration_ledger` fails the build if
//! this module reaches back into the raw API.
//!
//! A single-operand arm keeps its plain `lower_expr` call, as the template
//! module (`expr/url_main.rs`, #7617) does: with nothing lowered after it there
//! is no window, `operand_protection` would answer `Reuse`, and wrapping it
//! would emit the same IR through more machinery.
//!
//! ## What the migration found
//!
//! `Expr::BufferSlice` is the one arm here that held a **raw, already-unboxed**
//! pointer across user code:
//!
//! ```text
//! let buf_box   = lower_expr(buffer)      // NaN-boxed BufferHeader
//! let buf_handle = unbox_to_i64(buf_box)  // RAW pointer, in a register
//! let start_box = lower_expr(start)       // arbitrary user code -- allocates
//! let end_box   = lower_expr(end)         // ditto
//! js_buffer_slice(buf_handle, ...)        // reads the PRE-MOVE address
//! ```
//!
//! That is #7453's shape with the extra twist that the value in flight is not
//! even NaN-boxed any more, so #7280's `root_reload` post-pass cannot help: it
//! re-reads a shadow slot into a `double`, and the consuming call reads an `i64`
//! derived above the window. The unbox now happens below the group's re-read,
//! which is the only place it can be correct.
//!
//! The other windows closed here are ordinary operand-to-operand ones —
//! `AggregateErrorNew`, `BufferConcatWithLength`, `ObjectCreate`, both
//! `FinalizationRegistry` mutators and the two `ErrorNew*` forms — where an
//! earlier NaN-boxed operand sat in a register while a later one ran.

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::double_literal;
use crate::rooting;
use crate::types::{DOUBLE, I32, I64};

use super::{
    i32_bool_to_nanbox, lower_expr, lower_math_operand, nanbox_pointer_inline,
    nanbox_string_inline, unbox_str_handle, unbox_to_i64, FnCtx,
};

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::ArrayIsArray(o) => {
            // Fast path: static type is definitively array → emit
            // TAG_TRUE at compile time. Slow path: indeterminate
            // type (Any / Unknown / no annotation / Union including
            // a non-array variant) → emit runtime call to
            // `js_array_is_array`, which correctly handles
            // JSON.parse results, closure-captured values, function
            // returns typed `any`, and lazy arrays
            // (GC_TYPE_LAZY_ARRAY). Emitting TAG_FALSE as a compile-
            // time constant (the previous behavior) was wrong
            // whenever the operand's static type was Any: the user's
            // `Array.isArray(JSON.parse("[...]"))` would always
            // return false despite being a real array at runtime.
            //
            // The fast-path TRUE check used to delegate to
            // `is_array_expr`, but that helper deliberately treats a
            // Union as array-typed when ANY variant is Array — which
            // is correct for routing `.length` / `.push` dispatch on
            // `T[] | null` after a truthy narrow, but wrong for
            // `Array.isArray`: a parameter typed `number | number[]`
            // would constant-fold to TAG_TRUE for every call site,
            // making the if-guard always pick the array branch even
            // when the runtime value is a number (issue #324). Use a
            // strict match here instead — only pure `Array(_)` /
            // `Tuple(_)` types short-circuit; anything Union-shaped
            // falls through to the runtime.
            let v = lower_expr(ctx, o)?;
            if let Some(ty) = crate::type_analysis::static_type_of(ctx, o) {
                if matches!(
                    ty,
                    perry_hir::types::Type::Array(_) | perry_hir::types::Type::Tuple(_)
                ) {
                    return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_TRUE)));
                }
                // Definitively not an array: emit TAG_FALSE. Leaves
                // numeric / string / boolean literals and known
                // object-class instances on the fast path.
                let definitely_not_array = matches!(
                    ty,
                    perry_hir::types::Type::Number
                        | perry_hir::types::Type::Int32
                        | perry_hir::types::Type::String
                        | perry_hir::types::Type::Boolean
                        | perry_hir::types::Type::Null
                        | perry_hir::types::Type::Void
                        | perry_hir::types::Type::BigInt
                        | perry_hir::types::Type::Symbol
                );
                if definitely_not_array {
                    return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_FALSE)));
                }
            }
            // Indeterminate — dispatch to runtime.
            Ok(ctx
                .block()
                .call(DOUBLE, "js_array_is_array", &[(DOUBLE, &v)]))
        }

        // -------- new AggregateError(errors, message) --------
        // Calls real runtime `js_aggregateerror_new(errors_handle, msg_handle)`
        // which stores both the errors array and message in ErrorHeader.
        Expr::AggregateErrorNew {
            errors,
            message,
            options,
        } => {
            // #2838: `errors` must reach the runtime as a raw NaN-boxed value
            // (NOT an array pointer) so Sets / strings / generators / any
            // iterable can be consumed and non-iterables rejected with a
            // TypeError. #2836: apply the optional `{ cause }`.
            //
            // `errors` was live in a register across `message`'s lowering and
            // `message` across the options bag's — both arbitrary user code.
            let mut operands: Vec<&Expr> = vec![errors, message];
            if let Some(o) = options {
                operands.push(o);
            }
            rooting::with_operands_rooted(ctx, &operands, |ctx, vals| {
                let errors_box = vals[0].clone();
                let m = vals[1].clone();
                let options_box = vals.get(2).cloned().unwrap_or_else(|| {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                });
                let blk = ctx.block();
                let msg_handle = unbox_to_i64(blk, &m);
                let err_handle = blk.call(
                    I64,
                    "js_aggregateerror_new_full",
                    &[
                        (DOUBLE, &errors_box),
                        (I64, &msg_handle),
                        (DOUBLE, &options_box),
                    ],
                );
                Ok(nanbox_pointer_inline(blk, &err_handle))
            })
        }

        // -------- RegExpLastIndex — regex.lastIndex getter --------
        Expr::RegExpLastIndex(r) => {
            let r_box = lower_expr(ctx, r)?;
            let blk = ctx.block();
            let r_handle = unbox_to_i64(blk, &r_box);
            Ok(blk.call(DOUBLE, "js_regexp_get_last_index", &[(I64, &r_handle)]))
        }

        // -------- BufferConcat stub --------
        // -------- BufferConcat --------
        // `Buffer.concat([buf1, buf2, ...])`. Lower the array of buffer
        // pointers and pass to `js_buffer_concat`. The runtime walks the
        // array, summing lengths and copying bytes into a fresh buffer.
        Expr::BufferConcat(operand) => {
            let arr_box = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            // #2013: `list` must be an Array — validate before treating the
            // value as an ArrayHeader. Returns the (still NaN-boxed) bits,
            // which `js_buffer_concat` strips itself.
            let arr_handle = blk.call(I64, "js_buffer_validate_concat_list", &[(DOUBLE, &arr_box)]);
            let buf_handle = blk.call(I64, "js_buffer_concat", &[(I64, &arr_handle)]);
            Ok(nanbox_pointer_inline(blk, &buf_handle))
        }
        Expr::BufferConcatWithLength { list, total_length } => {
            // The list is live across `total_length`'s lowering.
            rooting::with_operands_rooted(ctx, &[list, total_length], |ctx, vals| {
                let arr_box = vals[0].clone();
                let total_box = vals[1].clone();
                let blk = ctx.block();
                // #2013: validate `list` is an Array (see BufferConcat above).
                let arr_handle =
                    blk.call(I64, "js_buffer_validate_concat_list", &[(DOUBLE, &arr_box)]);
                let buf_handle = blk.call(
                    I64,
                    "js_buffer_concat_with_length",
                    &[(I64, &arr_handle), (DOUBLE, &total_box)],
                );
                Ok(nanbox_pointer_inline(blk, &buf_handle))
            })
        }

        // #1177: `buf.slice(start?, end?)` on a statically buffer-producing
        // receiver — emitted by the HIR fold at `expr_call/mod.rs:5396` when
        // `.slice` is called on `BufferConcat` / `BufferFrom` / a chained
        // `BufferSlice`. Pre-fix the chained `Buffer.concat(c).slice(0,8)`
        // shape fell through to generic dynamic dispatch which routed
        // `.slice` through String.slice semantics on the NaN-boxed Buffer
        // pointer — producing a "string" with length=8 and all bytes empty.
        // Folding to `Expr::BufferSlice` here calls `js_buffer_slice` (which
        // ALWAYS copies bytes via `ptr::copy_nonoverlapping` into a freshly
        // allocated Buffer registered in BUFFER_REGISTRY) so the result has
        // its own backing storage independent of the parent's lifetime.
        Expr::BufferSlice { buffer, start, end } => {
            // The receiver used to be unboxed to a RAW BufferHeader pointer
            // before `start` and `end` were lowered, so the pointer the call
            // read was the pre-move address whenever either argument ran user
            // code. See the module header: this is the one shape in this file
            // that #7280's `root_reload` structurally cannot repair, because
            // what is stale is an `i64` derived above the window rather than
            // the `double` a slot re-read would produce. The unbox is now
            // emitted below the group's re-read.
            let mut operands: Vec<&Expr> = vec![buffer];
            if let Some(e) = start {
                operands.push(e);
            }
            if let Some(e) = end {
                operands.push(e);
            }
            rooting::with_operands_rooted(ctx, &operands, |ctx, vals| {
                let buf_box = vals[0].clone();
                // Default start=0, end=buf.length. `js_buffer_slice` itself
                // handles end-clamping via `.min(len)`, so we can pass i32::MAX
                // when end is omitted to mean "to the end" — matches how the
                // Node API treats `buf.slice(start)` (no end → to the end).
                let mut next = 1;
                let start_box = if start.is_some() {
                    next += 1;
                    vals[next - 1].clone()
                } else {
                    double_literal(0.0)
                };
                let end_box = if end.is_some() {
                    vals[next].clone()
                } else {
                    double_literal(i32::MAX as f64)
                };
                let blk = ctx.block();
                let buf_handle = unbox_to_i64(blk, &buf_box);
                let start_i32 = blk.fptosi(DOUBLE, &start_box, I32);
                let end_i32 = blk.fptosi(DOUBLE, &end_box, I32);
                let result = blk.call(
                    I64,
                    "js_buffer_slice",
                    &[(I64, &buf_handle), (I32, &start_i32), (I32, &end_i32)],
                );
                Ok(nanbox_pointer_inline(blk, &result))
            })
        }

        // -------- BufferIsBuffer --------
        // `Buffer.isBuffer(x)`. Runtime returns i32 (0/1); wrap as NaN-boxed
        // boolean. `js_buffer_is_buffer` already strips NaN-box tags and
        // checks the BUFFER_REGISTRY, so any value type is safe to pass.
        Expr::BufferIsBuffer(operand) => {
            let v_box = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            let v_handle = unbox_to_i64(blk, &v_box);
            let i32_result = blk.call(I32, "js_buffer_is_buffer", &[(I64, &v_handle)]);
            Ok(i32_bool_to_nanbox(blk, &i32_result))
        }

        // -------- BufferIsEncoding --------
        Expr::BufferIsEncoding(operand) => {
            let v_box = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            let i32_result = blk.call(I32, "js_buffer_is_encoding", &[(DOUBLE, &v_box)]);
            Ok(i32_bool_to_nanbox(blk, &i32_result))
        }

        // -------- StaticPluginResolve stub --------
        Expr::StaticPluginResolve(_) => Ok(double_literal(0.0)),

        // -------- More cheap stubs --------
        // #7621: `js_path_arg_header`, not `unbox_to_i64` — the plain mask hands
        // an SSO string's inline CHARACTERS to a `*StringHeader` consumer.
        Expr::PathNormalize(p) => {
            let p_box = lower_expr(ctx, p)?;
            let blk = ctx.block();
            let p_handle = blk.call(I64, "js_path_arg_header", &[(DOUBLE, &p_box)]);
            let result = blk.call(I64, "js_path_normalize", &[(I64, &p_handle)]);
            Ok(nanbox_string_inline(blk, &result))
        }
        Expr::PathResolve(p) => {
            let p_box = lower_expr(ctx, p)?;
            let blk = ctx.block();
            let p_handle = blk.call(I64, "js_path_arg_header", &[(DOUBLE, &p_box)]);
            let result = blk.call(I64, "js_path_resolve", &[(I64, &p_handle)]);
            Ok(nanbox_string_inline(blk, &result))
        }
        Expr::ObjectCreate(p, props) => {
            // #2816: route through `js_object_create_with_props` so prototype
            // validation + the optional descriptor bag are handled uniformly.
            // Pass `undefined` for the props arg when only one argument was
            // supplied. The prototype is live across the descriptor bag's
            // lowering, which for the usual `Object.create(proto, {…})` shape
            // is an object literal — i.e. an allocation.
            let mut operands: Vec<&Expr> = vec![p];
            if let Some(props_expr) = props {
                operands.push(props_expr);
            }
            rooting::with_operands_rooted(ctx, &operands, |ctx, vals| {
                let v = vals[0].clone();
                let props_val = vals.get(1).cloned().unwrap_or_else(|| {
                    crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                });
                Ok(ctx.block().call(
                    DOUBLE,
                    "js_object_create_with_props",
                    &[(DOUBLE, &v), (DOUBLE, &props_val)],
                ))
            })
        }
        Expr::MathClz32(o) => {
            let v = lower_math_operand(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_math_clz32", &[(DOUBLE, &v)]))
        }
        Expr::FsReadFileSync(p) => {
            // Phase H fs: call js_fs_read_file_sync which returns a
            // raw *mut StringHeader i64. NaN-box with STRING_TAG so
            // downstream `.length` / `===` paths can use it as a string.
            let path_box = lower_expr(ctx, p)?;
            let blk = ctx.block();
            let str_handle = blk.call(I64, "js_fs_read_file_sync", &[(DOUBLE, &path_box)]);
            Ok(nanbox_string_inline(blk, &str_handle))
        }
        Expr::FinalizationRegistryNew(callback) => {
            // `new FinalizationRegistry(cb)` — allocates a wrapper object
            // that stores the cleanup callback and an `entries` list for
            // later register/unregister lookups. Runtime returns a raw
            // *mut ObjectHeader (i64); NaN-box with POINTER_TAG so the
            // value can flow through subsequent dispatch sites.
            let cb = lower_expr(ctx, callback)?;
            let blk = ctx.block();
            let obj = blk.call(I64, "js_finreg_new", &[(DOUBLE, &cb)]);
            Ok(nanbox_pointer_inline(blk, &obj))
        }
        Expr::FinalizationRegistryRegister {
            registry,
            target,
            held,
            token,
        } => {
            // `reg.register(target, held, token?)` — always returns undefined.
            // Four operands, each live across every one that follows it.
            let mut operands: Vec<&Expr> = vec![registry, target, held];
            if let Some(token_expr) = token {
                operands.push(token_expr);
            }
            rooting::with_operands_rooted(ctx, &operands, |ctx, vals| {
                let reg = vals[0].clone();
                let tgt = vals[1].clone();
                let h = vals[2].clone();
                let tok = vals.get(3).cloned().unwrap_or_else(|| {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                });
                Ok(ctx.block().call(
                    DOUBLE,
                    "js_finreg_register",
                    &[(DOUBLE, &reg), (DOUBLE, &tgt), (DOUBLE, &h), (DOUBLE, &tok)],
                ))
            })
        }
        Expr::FinalizationRegistryUnregister { registry, token } => {
            // `reg.unregister(token)` — returns NaN-boxed boolean.
            rooting::with_operands_rooted(ctx, &[registry, token], |ctx, vals| {
                let reg = vals[0].clone();
                let tok = vals[1].clone();
                Ok(ctx.block().call(
                    DOUBLE,
                    "js_finreg_unregister",
                    &[(DOUBLE, &reg), (DOUBLE, &tok)],
                ))
            })
        }
        Expr::ErrorNewWithCause { message, cause } => {
            // new Error(msg, { cause }). Runtime stores the cause
            // on the ErrorHeader so `e.cause` returns it. The message string is
            // live across the cause's evaluation.
            rooting::with_operands_rooted(ctx, &[message, cause], |ctx, vals| {
                let msg = vals[0].clone();
                let c = vals[1].clone();
                let blk = ctx.block();
                let err_handle = blk.call(
                    I64,
                    "js_error_new_with_cause_from_value",
                    &[(DOUBLE, &msg), (DOUBLE, &c)],
                );
                Ok(nanbox_pointer_inline(blk, &err_handle))
            })
        }
        Expr::ErrorNewWithOptions {
            kind,
            message,
            options,
        } => {
            // #2836: new <Error-kind>(msg, options) where `options` is a
            // runtime value (variable or dynamic object). The runtime reads
            // the `cause` property off `options` and stamps the right
            // ERROR_KIND_* so `instanceof TypeError`/etc. still hold.
            let kind = *kind;
            rooting::with_operands_rooted(ctx, &[message, options], |ctx, vals| {
                let msg = vals[0].clone();
                let opts = vals[1].clone();
                let blk = ctx.block();
                let kind_lit = (kind as i64).to_string();
                let err_handle = blk.call(
                    I64,
                    "js_error_new_kind_with_options_from_value",
                    &[(I32, &kind_lit), (DOUBLE, &msg), (DOUBLE, &opts)],
                );
                Ok(nanbox_pointer_inline(blk, &err_handle))
            })
        }
        Expr::EnvGet(name) => {
            // process.env.HOME -> js_getenv("HOME") -> string handle
            let key_idx = ctx.strings.intern(name);
            let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
            let blk = ctx.block();
            let key_box = blk.load(DOUBLE, &key_handle_global);
            let key_handle = unbox_to_i64(blk, &key_box);
            // js_getenv_value returns `undefined` (nullish) for an unset
            // var, not a STRING_TAG'd null pointer — so `?? default`
            // applies and typeof/JSON.stringify agree (#1312).
            Ok(blk.call(DOUBLE, "js_getenv_value", &[(I64, &key_handle)]))
        }
        Expr::EnvGetDynamic(name_expr) => {
            let key_box = lower_expr(ctx, name_expr)?;
            let blk = ctx.block();
            // SSO-safe key unbox — name comes from a runtime expr (e.g.
            // `process.env[shortName]`); `js_getenv` dereferences it as
            // `*StringHeader`. #214 SSO bug class.
            let key_handle = unbox_str_handle(blk, &key_box);
            // `undefined` for unset vars — see EnvGet above (#1312).
            Ok(blk.call(DOUBLE, "js_getenv_value", &[(I64, &key_handle)]))
        }
        Expr::ProcessEnv => {
            // `process.env` (or `globalThis.process.env`) as a value.
            // The runtime returns an already-NaN-boxed f64 POINTER_TAG
            // to a cached object populated from the OS environment on
            // first call. Subsequent PropertyGet dispatch on it works
            // via the normal object field path.
            Ok(ctx.block().call(DOUBLE, "js_process_env", &[]))
        }
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
