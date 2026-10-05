//! `NATIVE_MODULE_TABLE` lookup + the generic
//! `lower_native_module_dispatch` driver that emits a call into a
//! native-stdlib runtime symbol from a `NativeModSig` row.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{nanbox_bigint_inline, nanbox_string_inline, unbox_to_i64, FnCtx};
use crate::nanbox::double_literal;
use crate::native_value::{
    materialize_native_handle_to_js_value, materialize_promise_boundary_to_js_value, LoweredValue,
    MaterializationReason,
};
use crate::types::{DOUBLE, I32, I64};

use super::{NativeArgKind, NativeModSig, NativeRetKind, NATIVE_MODULE_TABLE};

/// Look up a native module method in the static dispatch table.
/// Entries with `class_filter: Some("Pool")` only match when
/// `class_name == Some("Pool")`; entries with `class_filter: None`
/// match any class_name. More-specific entries (with class_filter)
/// are checked first.
#[allow(private_interfaces)]
pub fn native_module_lookup(
    module: &str,
    has_receiver: bool,
    method: &str,
    class_name: Option<&str>,
) -> Option<&'static NativeModSig> {
    // Mirror the well-known aliasing here so call-site lookups find the
    // right runtime fns regardless of which alias the user imported from.
    let normalized = match module {
        "sys" => "util",
        // #6563: @lydell/node-pty is an API-identical fork of node-pty
        // (opencode imports the fork, kimi-code the original); both route to
        // the one runtime pty implementation.
        "@lydell/node-pty" | "bun-pty" => "node-pty",
        // #11211: `perry/container-compose` is the internal specifier for the
        // same compose surface `perry/compose` exposes (stdlib_features.rs
        // maps both to the `container` feature); share its rows.
        "perry/container-compose" => "perry/compose",
        m => m,
    };
    // First pass: look for an exact class_filter match.
    let exact = NATIVE_MODULE_TABLE.iter().find(|sig| {
        sig.module == normalized
            && sig.has_receiver == has_receiver
            && sig.method == method
            && sig.class_filter.is_some()
            && sig.class_filter == class_name
    });
    if exact.is_some() {
        return exact;
    }
    // Second pass: generic (class_filter == None) entries.
    NATIVE_MODULE_TABLE.iter().find(|sig| {
        sig.module == normalized
            && sig.has_receiver == has_receiver
            && sig.method == method
            && sig.class_filter.is_none()
    })
}

/// Lower a native module call through the dispatch table.
/// For receiver-less calls, `recv` should be None.
/// For instance method calls, `recv` is the receiver expression: it is
/// evaluated first and rooted across every argument with the arguments
/// themselves (#11789 sweep), and its handle is unboxed from the re-read.
#[allow(private_interfaces)]
pub fn lower_native_module_dispatch(
    ctx: &mut FnCtx<'_>,
    sig: &NativeModSig,
    recv: Option<&Expr>,
    args: &[Expr],
) -> Result<String> {
    // Native-table calls used to lower arguments into bare SSA registers one
    // at a time. If a later argument allocated or called user code, an earlier
    // object/closure could move before the FFI call (for example
    // `https.createServer(options, common.mustCall(handler))`), leaving the
    // runtime with a valid-looking pointer to an evacuated `{}`. Root the
    // complete operand window and coerce only the post-window re-reads.
    //
    // #11789 sweep: the receiver is the first operand of the same window — an
    // instance method's receiver was unboxed to a raw handle BEFORE the
    // arguments were lowered, so `conn.query(String(x), work())` kept a bare
    // handle across `work`. And a `VarArgsAsArray` tail allocates its array
    // AFTER the re-read, so every operand's window is "collects" there.
    let operands: Vec<&Expr> = recv.into_iter().chain(args.iter()).collect();
    let has_receiver = recv.is_some();
    let packs_varargs = sig.args.contains(&NativeArgKind::VarArgsAsArray);
    // #11830: coercing a `StrPtr` argument (`js_value_to_str_ptr_for_ffi`)
    // materialises a short string or `JSON.stringify`s an object, so it
    // allocates and can run a user `toJSON` / `toString`. Doing it argument by
    // argument left every earlier raw pointer one collection from stale. Each
    // such operand is therefore coerced, in order, into its own rooted slot
    // first, and the raw pointers are read back in one pass after the last
    // coercion. A string literal is already a heap string and keeps the
    // in-place conversion.
    let first_arg = usize::from(has_receiver);
    let packed_from = sig
        .args
        .iter()
        .position(|k| *k == NativeArgKind::VarArgsAsArray)
        .unwrap_or(usize::MAX);
    let coerces: Vec<bool> = operands
        .iter()
        .enumerate()
        .map(|(o, e)| {
            o >= first_arg
                && o - first_arg < packed_from
                && sig.args.get(o - first_arg) == Some(&NativeArgKind::StrPtr)
                && !matches!(e, Expr::String(_))
        })
        .collect();
    let body = |ctx: &mut FnCtx<'_>, all_values: &[String]| -> Result<String> {
        // Build the LLVM arg list: receiver handle (if any) + coerced args.
        let mut llvm_args: Vec<(crate::types::LlvmType, String)> = Vec::new();
        let mut arg_types: Vec<crate::types::LlvmType> = Vec::new();

        // Receiver handle
        let rooted_args = if has_receiver {
            let handle = unbox_to_i64(ctx.block(), &all_values[0]);
            llvm_args.push((I64, handle));
            arg_types.push(I64);
            &all_values[1..]
        } else {
            all_values
        };

        // Coerce each arg per the sig's coercion rules.
        // If more args are passed than the sig declares, pass extras as F64.
        let mut i = 0;
        while i < args.len() {
            let kind = sig.args.get(i).copied().unwrap_or(NativeArgKind::F64);
            if kind == NativeArgKind::VarArgsAsArray {
                // Pack args[i..] into a freshly allocated JS array and pass a
                // single i64 ArrayHeader pointer. VarArgsAsArray must be the
                // last entry in `sig.args`, so any further declared kinds
                // would be unreachable — break after consuming.
                let remaining = &rooted_args[i..];
                let cap = (remaining.len() as u32).to_string();
                let mut arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
                for v in remaining {
                    let blk = ctx.block();
                    arr = blk.call(I64, "js_array_push_f64", &[(I64, &arr), (DOUBLE, v)]);
                }
                llvm_args.push((I64, arr));
                arg_types.push(I64);
                i = args.len();
                break;
            }
            let lowered = rooted_args[i].clone();
            match kind {
                NativeArgKind::F64 => {
                    llvm_args.push((DOUBLE, lowered));
                    arg_types.push(DOUBLE);
                }
                // Already coerced to a rooted raw pointer (see `coerces`).
                NativeArgKind::StrPtr if coerces[first_arg + i] => {
                    llvm_args.push((I64, lowered));
                    arg_types.push(I64);
                }
                NativeArgKind::StrPtr => {
                    let blk = ctx.block();
                    let ptr = blk.call(I64, "js_value_to_str_ptr_for_ffi", &[(DOUBLE, &lowered)]);
                    llvm_args.push((I64, ptr));
                    arg_types.push(I64);
                }
                NativeArgKind::PtrI64 => {
                    let blk = ctx.block();
                    let handle = unbox_to_i64(blk, &lowered);
                    llvm_args.push((I64, handle));
                    arg_types.push(I64);
                }
                NativeArgKind::JsvalI64 => {
                    // Bitcast the NaN-boxed f64 to i64 without unboxing —
                    // the callee will interpret the raw bits.
                    let blk = ctx.block();
                    let bits = blk.bitcast_double_to_i64(&lowered);
                    llvm_args.push((I64, bits));
                    arg_types.push(I64);
                }
                NativeArgKind::VarArgsAsArray => unreachable!("handled above"),
            }
            i += 1;
        }
        // If fewer args than sig expects, pad with undefined / 0 / empty-array.
        for j in i..sig.args.len() {
            match sig.args[j] {
                NativeArgKind::F64 => {
                    llvm_args.push((
                        DOUBLE,
                        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
                    ));
                    arg_types.push(DOUBLE);
                }
                NativeArgKind::StrPtr | NativeArgKind::PtrI64 => {
                    llvm_args.push((I64, "0".to_string()));
                    arg_types.push(I64);
                }
                NativeArgKind::JsvalI64 => {
                    // A missing NA_JSV arg is JS `undefined`, not numeric 0.
                    // NA_JSV slots carry the *raw NaN-box bits* as i64, so a
                    // padded `0` would be read back as the f64 `0.0` (a number)
                    // — issue #1852: `socket.end()` with no args then
                    // stringified `0` and wrote a spurious "0" byte before FIN.
                    // Pad with the TAG_UNDEFINED bit pattern so the callee's
                    // value-probe sees `undefined`.
                    llvm_args.push((I64, (crate::nanbox::TAG_UNDEFINED as i64).to_string()));
                    arg_types.push(I64);
                }
                NativeArgKind::VarArgsAsArray => {
                    // No user args at this position — pass an empty array.
                    let arr = ctx.block().call(I64, "js_array_alloc", &[(I32, "0")]);
                    llvm_args.push((I64, arr));
                    arg_types.push(I64);
                }
            }
        }
        // `findPackageJSON()` distinguishes a missing first argument from an
        // explicitly supplied `undefined`, although both otherwise lower to the
        // same NaN-box value. Preserve the source call arity for the runtime.
        if sig.runtime == "js_module_find_package_json" {
            llvm_args.push((DOUBLE, double_literal(args.len() as f64)));
            arg_types.push(DOUBLE);
        }

        let thread_launch = matches!(
            sig.runtime,
            "js_thread_spawn" | "js_thread_parallel_map" | "js_thread_parallel_filter"
        );
        let runtime = if thread_launch {
            let prepare = format!(
                "__perry_prepare_thread_strings_{}",
                ctx.strings.thread_literal_callback_prefix()
            );
            ctx.pending_declares
                .push((prepare.clone(), crate::types::VOID, vec![]));
            llvm_args.push((I64, format!("ptrtoint (ptr @{} to i64)", prepare)));
            arg_types.push(I64);
            format!("{}_with_literals", sig.runtime)
        } else {
            sig.runtime.to_string()
        };
        let runtime = runtime.as_str();

        // Determine return type for the declare
        let ret_type = match sig.ret {
            NativeRetKind::GcPtr
            | NativeRetKind::NullableGcPtr
            | NativeRetKind::HandleId
            | NativeRetKind::JsValue
            | NativeRetKind::Promise
            | NativeRetKind::Str
            | NativeRetKind::ObjFromJsonStr
            | NativeRetKind::BigInt => I64,
            NativeRetKind::F64 => DOUBLE,
            NativeRetKind::BoolI1 => crate::types::I1,
            NativeRetKind::BoolI32 | NativeRetKind::I32Void => I32,
            NativeRetKind::Void => crate::types::VOID,
        };

        ctx.pending_declares
            .push((runtime.to_string(), ret_type, arg_types));

        let arg_slices: Vec<(crate::types::LlvmType, &str)> =
            llvm_args.iter().map(|(t, s)| (*t, s.as_str())).collect();

        match sig.ret {
            NativeRetKind::GcPtr
            | NativeRetKind::NullableGcPtr
            | NativeRetKind::HandleId
            | NativeRetKind::JsValue => {
                let blk = ctx.block();
                let raw = blk.call(I64, sig.runtime, &arg_slices);
                let lowered = LoweredValue::native_handle(raw.clone());
                ctx.record_lowered_value(
                    "NativeModuleReturn",
                    None,
                    "native_module.raw_handle",
                    &lowered,
                    None,
                    None,
                    None,
                    false,
                    false,
                    vec![format!("runtime={}", sig.runtime)],
                );
                Ok(materialize_native_handle_to_js_value(
                    ctx,
                    lowered,
                    MaterializationReason::ReturnAbi,
                ))
            }
            NativeRetKind::Promise => {
                let blk = ctx.block();
                let raw = blk.call(I64, sig.runtime, &arg_slices);
                let lowered = LoweredValue::promise_boundary(raw.clone());
                ctx.record_lowered_value(
                    "NativeModuleReturn",
                    None,
                    "native_module.raw_promise",
                    &lowered,
                    None,
                    None,
                    None,
                    false,
                    false,
                    vec![format!("runtime={}", sig.runtime)],
                );
                Ok(materialize_promise_boundary_to_js_value(
                    ctx,
                    lowered,
                    MaterializationReason::ReturnAbi,
                ))
            }
            NativeRetKind::Str => {
                // Returned raw *mut StringHeader — NaN-box with STRING_TAG so
                // downstream string ops (JSON.stringify, ===, .length) work.
                // Null pointer (header value 0) is returned as TAG_NULL so
                // `request.header('missing')` reads as `null` instead of a
                // dangling string pointer.
                let blk = ctx.block();
                let raw = blk.call(I64, sig.runtime, &arg_slices);
                let is_null = blk.icmp_eq(I64, &raw, "0");
                let boxed = nanbox_string_inline(blk, &raw);
                let null_val = double_literal(f64::from_bits(crate::nanbox::TAG_NULL));
                Ok(blk.select(crate::types::I1, &is_null, DOUBLE, &null_val, &boxed))
            }
            NativeRetKind::ObjFromJsonStr => {
                // Returned raw *mut StringHeader containing JSON — pipe
                // through `js_json_parse_or_null` so user code sees a real
                // object (e.g. `jwt.verify(...).sub` works). Symmetric
                // counterpart to the NA_JSON arg coercion landed in #915.
                // Null pointer (failure mode — e.g. `jwt.verify` on a bad
                // signature) is returned as TAG_NULL without throwing,
                // matching the previous NR_STR null-handling. #927.
                //
                // `js_json_parse_or_null` takes `*const StringHeader` (i64
                // on the FFI side) and returns the NaN-boxed JSValue bits
                // as i64. It returns TAG_NULL for null input (instead of
                // the throw that plain `js_json_parse` does). Declare
                // BEFORE grabbing `blk` so the mutable borrow on
                // pending_declares doesn't overlap the block borrow.
                ctx.pending_declares
                    .push(("js_json_parse_or_null".to_string(), I64, vec![I64]));
                let blk = ctx.block();
                let raw = blk.call(I64, sig.runtime, &arg_slices);
                let parsed_bits = blk.call(I64, "js_json_parse_or_null", &[(I64, &raw)]);
                Ok(blk.bitcast_i64_to_double(&parsed_bits))
            }
            NativeRetKind::BigInt => {
                // Returned raw *mut BigIntHeader — NaN-box with BIGINT_TAG (0x7FFA).
                let blk = ctx.block();
                let raw = blk.call(I64, sig.runtime, &arg_slices);
                Ok(nanbox_bigint_inline(blk, &raw))
            }
            NativeRetKind::F64 => Ok(ctx.block().call(DOUBLE, runtime, &arg_slices)),
            NativeRetKind::BoolI1 | NativeRetKind::BoolI32 => {
                let blk = ctx.block();
                let raw = blk.call(ret_type, sig.runtime, &arg_slices);
                let is_true = blk.icmp_ne(ret_type, &raw, "0");
                let true_val = double_literal(f64::from_bits(crate::nanbox::TAG_TRUE));
                let false_val = double_literal(f64::from_bits(crate::nanbox::TAG_FALSE));
                Ok(blk.select(crate::types::I1, &is_true, DOUBLE, &true_val, &false_val))
            }
            NativeRetKind::I32Void => {
                let _discard = ctx.block().call(I32, sig.runtime, &arg_slices);
                Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
            }
            NativeRetKind::Void => {
                ctx.block().call_void(sig.runtime, &arg_slices);
                Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
            }
        }
    };
    crate::rooting::with_rooted_group(ctx, operands.len(), |ctx, group| {
        // Incremental, one operand at a time: each is rooted BEFORE the next is
        // lowered. A `VarArgsAsArray` tail allocates its array after the
        // re-read, so every window is "collects" there; a later coercion is a
        // window too.
        for (i, operand) in operands.iter().enumerate() {
            let collects = packs_varargs
                || coerces[i + 1..].iter().any(|c| *c)
                || crate::rooting::any_operand_may_collect(ctx, operands[i + 1..].iter().copied());
            group.lower(ctx, operand, collects)?;
        }
        let mut pending = coerces.iter().filter(|c| **c).count();
        let mut coerced = Vec::with_capacity(operands.len());
        for (i, needs) in coerces.iter().enumerate() {
            coerced.push(if *needs {
                pending -= 1;
                Some(group.coerce_to_ptr(ctx, i, pending > 0, |ctx, v| {
                    ctx.block()
                        .call(I64, "js_value_to_str_ptr_for_ffi", &[(DOUBLE, v)])
                })?)
            } else {
                None
            });
        }
        // Below the last coercion: re-read everything, in operand order.
        let mut values = Vec::with_capacity(operands.len());
        for (i, emitted) in coerced.into_iter().enumerate() {
            values.push(match emitted {
                Some(e) => group.reread_emitted(ctx, e),
                None => group.reread(ctx, i)?,
            });
        }
        body(ctx, &values)
    })
}

#[cfg(test)]
mod ffi_return_type_tests {
    /// Verify that the `returns` manifest field values map to the correct
    /// dispatch flags. These tests guard against accidentally conflating
    /// "i64_str" with "i64" or "string" — the three are mutually exclusive.
    ///
    /// Related: issue #222 — explicit `returns: "i64_str"` for string-pointer
    /// detection when the Rust function is declared `-> i64`.
    fn parse_flags(manifest_ret: Option<&str>) -> (bool, bool, bool, bool) {
        // Mirror the manifest-driven arm of the flag computation in the
        // ExternFuncRef dispatch inside lower_call.  The name-based heuristic
        // and HIR-type fallback arms are omitted here; this only tests the
        // explicit manifest field.
        let returns_i64_str = matches!(manifest_ret, Some("i64_str"));
        let returns_string = matches!(manifest_ret, Some("string") | Some("ptr"));
        let returns_i64 = matches!(manifest_ret, Some("i64"));
        let returns_void = matches!(manifest_ret, Some("void"));
        (returns_i64_str, returns_string, returns_i64, returns_void)
    }

    #[test]
    fn i64_str_is_recognized() {
        let (i64_str, string, i64, void) = parse_flags(Some("i64_str"));
        assert!(i64_str, "returns_i64_str must be true for \"i64_str\"");
        assert!(!string, "returns_string must be false for \"i64_str\"");
        assert!(!i64, "returns_i64 must be false for \"i64_str\"");
        assert!(!void, "returns_void must be false for \"i64_str\"");
    }

    #[test]
    fn string_not_confused_with_i64_str() {
        let (i64_str, string, i64, void) = parse_flags(Some("string"));
        assert!(!i64_str, "returns_i64_str must be false for \"string\"");
        assert!(string, "returns_string must be true for \"string\"");
        assert!(!i64, "returns_i64 must be false for \"string\"");
        assert!(!void, "returns_void must be false for \"string\"");
    }

    #[test]
    fn ptr_alias_for_string() {
        let (i64_str, string, i64, void) = parse_flags(Some("ptr"));
        assert!(!i64_str, "returns_i64_str must be false for \"ptr\"");
        assert!(string, "returns_string must be true for \"ptr\"");
        assert!(!i64, "returns_i64 must be false for \"ptr\"");
        assert!(!void, "returns_void must be false for \"ptr\"");
    }

    #[test]
    fn i64_stays_numeric() {
        let (i64_str, string, i64, void) = parse_flags(Some("i64"));
        assert!(!i64_str, "returns_i64_str must be false for \"i64\"");
        assert!(!string, "returns_string must be false for \"i64\"");
        assert!(i64, "returns_i64 must be true for \"i64\"");
        assert!(!void, "returns_void must be false for \"i64\"");
    }

    #[test]
    fn void_recognized() {
        let (i64_str, string, i64, void) = parse_flags(Some("void"));
        assert!(!i64_str, "returns_i64_str must be false for \"void\"");
        assert!(!string, "returns_string must be false for \"void\"");
        assert!(!i64, "returns_i64 must be false for \"void\"");
        assert!(void, "returns_void must be true for \"void\"");
    }

    #[test]
    fn i64_str_dispatch_order() {
        // When manifest is "i64_str", it must take the i64_str path even
        // if the HIR type also says String (which would normally set
        // returns_string via the ext_return_type arm).
        let manifest_ret: Option<&str> = Some("i64_str");
        let returns_i64_str = matches!(manifest_ret, Some("i64_str"));
        // Simulate returns_string with HIR String type:
        let hir_string_arm = true; // ext_return_type == HirType::String
        let returns_string = matches!(manifest_ret, Some("string") | Some("ptr")) || hir_string_arm;
        // Both could be true simultaneously, but in the dispatch the
        // `returns_i64_str` branch is checked FIRST, so it wins.
        assert!(returns_i64_str);
        assert!(returns_string); // also true — but i64_str branch fires first
    }

    #[test]
    fn net_socket_write_and_end_match_their_float_ffi_abi() {
        for method in ["write", "end"] {
            let sig = super::native_module_lookup("net", true, method, Some("Socket"))
                .unwrap_or_else(|| panic!("net.Socket.{method} must resolve"));
            assert_eq!(
                sig.args,
                [super::NativeArgKind::F64; 3],
                "net.Socket.{method} takes three NaN-boxed f64 arguments"
            );
        }
    }
}
