//! Call, new, and native method call lowering.
//!
//! Contains `lower_call`, `lower_new`, and `lower_native_method_call`.

use anyhow::{bail, Result};
use perry_hir::Expr;
use perry_types::Type as HirType;

use crate::expr::{lower_expr, nanbox_pointer_inline, nanbox_string_inline, unbox_to_i64, variant_name, FnCtx};
use crate::lower_array_method::lower_array_method;

/// Heuristic: is this expression likely an integer handle (pointer value
/// stored as a number) rather than a real float? Used for extern C FFI
/// calls to decide whether to pass the arg in an x-register (i64) or
/// d-register (double).
///
/// Returns true for variables, property accesses, casts, function calls —
/// anything that's likely a handle value obtained from a prior FFI call.
/// Returns false for number/integer literals and arithmetic — likely
/// actual float values (width, height, color components, etc.).
fn is_integer_handle_arg(expr: &Expr) -> bool {
    match expr {
        // Literal numbers are real floats (width, height, color, etc.)
        Expr::Integer(_) | Expr::Number(_) => false,
        // Unary minus on a literal (e.g. -1) — still a real number
        Expr::Unary { operand, .. } => !matches!(operand.as_ref(), Expr::Integer(_) | Expr::Number(_)),
        // Variables, property access — likely handles
        Expr::LocalGet(_) | Expr::PropertyGet { .. } => true,
        // Arithmetic on handles (handle + offset) — still integer
        Expr::Binary { .. } => true,
        // Function call results — likely handles from other FFI calls
        Expr::Call { .. } => true,
        // Everything else — default to double (safer for floats)
        _ => false,
    }
}
use crate::lower_string_method::lower_string_method;
use crate::nanbox::{double_literal, POINTER_MASK_I64};
use crate::type_analysis::{is_array_expr, is_map_expr, is_promise_expr, is_set_expr, is_string_expr, receiver_class_name};
use crate::types::{DOUBLE, I32, I64, I8, PTR, VOID};

/// Issue #92: inline Buffer numeric reads (`buf.readInt32BE(offset)` etc.)
/// as LLVM load + bswap + convert instead of a runtime dispatch through
/// `js_native_call_method`. Called from the PropertyGet branch below when
/// the receiver is a Buffer / Uint8Array and the method name matches one
/// of the Node-style numeric read accessors. Returns `Ok(None)` when
/// intrinsification isn't possible (the generic path then catches it) —
/// currently that's any receiver that isn't a tracked `buffer_data_slot`.
struct BufferNumericReadSpec {
    width_bytes: u32,
    swap: bool,     // BE → emit @llvm.bswap; LE → skip
    signed: bool,   // sitofp vs uitofp (ignored for float/double)
    is_float: bool, // true for readFloat*/readDouble*
}

fn classify_buffer_numeric_read(method: &str) -> Option<BufferNumericReadSpec> {
    use BufferNumericReadSpec as S;
    Some(match method {
        "readUInt8" | "readUint8" => S { width_bytes: 1, swap: false, signed: false, is_float: false },
        "readInt8"                => S { width_bytes: 1, swap: false, signed: true,  is_float: false },
        "readUInt16BE" | "readUint16BE" => S { width_bytes: 2, swap: true,  signed: false, is_float: false },
        "readUInt16LE" | "readUint16LE" => S { width_bytes: 2, swap: false, signed: false, is_float: false },
        "readInt16BE"                   => S { width_bytes: 2, swap: true,  signed: true,  is_float: false },
        "readInt16LE"                   => S { width_bytes: 2, swap: false, signed: true,  is_float: false },
        "readUInt32BE" | "readUint32BE" => S { width_bytes: 4, swap: true,  signed: false, is_float: false },
        "readUInt32LE" | "readUint32LE" => S { width_bytes: 4, swap: false, signed: false, is_float: false },
        "readInt32BE"                   => S { width_bytes: 4, swap: true,  signed: true,  is_float: false },
        "readInt32LE"                   => S { width_bytes: 4, swap: false, signed: true,  is_float: false },
        "readFloatBE"                   => S { width_bytes: 4, swap: true,  signed: true,  is_float: true },
        "readFloatLE"                   => S { width_bytes: 4, swap: false, signed: true,  is_float: true },
        "readDoubleBE"                  => S { width_bytes: 8, swap: true,  signed: true,  is_float: true },
        "readDoubleLE"                  => S { width_bytes: 8, swap: false, signed: true,  is_float: true },
        _ => return None,
    })
}

fn try_emit_buffer_read_intrinsic(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    method: &str,
    args: &[Expr],
) -> Result<Option<String>> {
    let spec = match classify_buffer_numeric_read(method) {
        Some(s) => s,
        None => return Ok(None),
    };
    // Node-style readers take exactly one `offset` arg. `readUInt8(offset)`
    // allows omitted offset but the compiler sees that as 0-arg; not our
    // concern here — fall through to runtime which handles the default.
    if args.len() != 1 {
        return Ok(None);
    }
    // Fast path only when the receiver is a `const buf = Buffer.alloc(N)`-style
    // local that's been registered in `buffer_data_slots` (see stmt.rs:472).
    // Arbitrary Buffer values (function args, fields) still go through runtime.
    let (ptr_slot, scope_idx) = match object {
        Expr::LocalGet(id) => match ctx.buffer_data_slots.get(id).cloned() {
            Some(s) => s,
            None => return Ok(None),
        },
        _ => return Ok(None),
    };
    // Offset as i32 (prefer the existing i32 slot if the expr qualifies,
    // otherwise fptosi from double).
    let offset_is_i32 = crate::expr::can_lower_expr_as_i32(
        &args[0],
        &ctx.i32_counter_slots,
        ctx.flat_const_arrays,
        &ctx.array_row_aliases,
        ctx.integer_locals,
        ctx.clamp3_functions,
        ctx.clamp_u8_functions,
    );
    let offset_i32 = if offset_is_i32 {
        crate::expr::lower_expr_as_i32(ctx, &args[0])?
    } else {
        let d = lower_expr(ctx, &args[0])?;
        ctx.block().fptosi(DOUBLE, &d, I32)
    };
    let blk = ctx.block();
    let data_ptr = blk.load(PTR, &ptr_slot);
    // BufferHeader {length: u32, capacity: u32} lives 8 bytes before the data.
    let header_ptr = blk.gep(I8, &data_ptr, &[(I32, "-8")]);
    let len_i32 = blk.load_invariant(I32, &header_ptr);
    // Bounds check: offset + width_bytes <= length, via @llvm.assume so the
    // branch doesn't block the LoopVectorizer (same trick as Uint8ArrayGet).
    let end_i32 = blk.add(I32, &offset_i32, &spec.width_bytes.to_string());
    let in_bounds = blk.icmp_ule(I32, &end_i32, &len_i32);
    blk.emit_raw(format!("call void @llvm.assume(i1 {})", in_bounds));
    let meta = crate::expr::buffer_alias_metadata_suffix(scope_idx);
    let elem_ptr = blk.gep_inbounds(I8, &data_ptr, &[(I32, &offset_i32)]);
    // Load raw bytes at the correct width.
    let (load_ty, swap_intrinsic) = match spec.width_bytes {
        1 => ("i8",  None),
        2 => ("i16", Some("llvm.bswap.i16")),
        4 => ("i32", Some("llvm.bswap.i32")),
        8 => ("i64", Some("llvm.bswap.i64")),
        _ => unreachable!(),
    };
    let raw = blk.fresh_reg();
    blk.emit_raw(format!("{} = load {}, ptr {}{}", raw, load_ty, elem_ptr, meta));
    // Byte-swap for BE on multi-byte widths (swap.i8 doesn't exist; width=1
    // never has `swap=true` in the spec table anyway).
    let swapped = match (spec.swap, swap_intrinsic) {
        (true, Some(intr)) => {
            let r = blk.fresh_reg();
            blk.emit_raw(format!("{} = call {} @{}({} {})", r, load_ty, intr, load_ty, raw));
            r
        }
        _ => raw,
    };
    // Convert to f64.
    let result = if spec.is_float {
        // Float/double: bitcast int bits → float bits, then fpext f32→f64 if needed.
        let float_ty = if spec.width_bytes == 4 { "float" } else { "double" };
        let as_float = blk.fresh_reg();
        blk.emit_raw(format!(
            "{} = bitcast {} {} to {}",
            as_float, load_ty, swapped, float_ty
        ));
        if spec.width_bytes == 4 {
            let extended = blk.fresh_reg();
            blk.emit_raw(format!("{} = fpext float {} to double", extended, as_float));
            extended
        } else {
            as_float
        }
    } else {
        // Integer: sitofp or uitofp through at least i32. The 1- and 2-byte
        // loads need a zext/sext to i32 first so the final fptoXi picks the
        // right sign semantics.
        let i32_val = match spec.width_bytes {
            1 | 2 => {
                if spec.signed {
                    blk.sext(load_ty, &swapped, I32)
                } else {
                    blk.zext(load_ty, &swapped, I32)
                }
            }
            4 => swapped,
            8 => {
                // Signed 8-byte reads (BigInt64) would need BigInt allocation;
                // only reach here for width_bytes==8 when is_float, which already
                // returned above. Defensive early-out.
                return Ok(None);
            }
            _ => unreachable!(),
        };
        if spec.signed {
            blk.sitofp(I32, &i32_val, DOUBLE)
        } else {
            blk.uitofp(I32, &i32_val, DOUBLE)
        }
    };
    Ok(Some(result))
}

/// Lower a `Call` expression. Two shapes are supported:
/// 1. `FuncRef(id)(args...)` — direct call to a user function by HIR id.
/// 2. `console.log(expr)` where `expr` lowers to a double — emits a
///    `js_console_log_number` call and returns `0.0` as the statement value.
pub(crate) fn lower_call(ctx: &mut FnCtx<'_>, callee: &Expr, args: &[Expr]) -> Result<String> {
    // Closure-typed local call: `counter()` where `counter` is a
    // local of `Type::Function(...)`. Dispatch through the runtime
    // `js_closure_call<N>` family — the runtime extracts the function
    // pointer from the closure header and invokes it with the closure
    // as the first arg followed by the user args.
    if let Expr::LocalGet(id) = callee {
        if matches!(ctx.local_types.get(id), Some(HirType::Function(_))) {
            let recv_box = lower_expr(ctx, callee)?;
            let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
            for a in args {
                lowered_args.push(lower_expr(ctx, a)?);
            }

            // Check if this closure has rest params — if so, bundle
            // trailing args into an array (same pattern as FuncRef).
            let rest_idx = ctx
                .local_closure_func_ids
                .get(id)
                .and_then(|cfid| ctx.closure_rest_params.get(cfid))
                .copied();

            let effective_args: Vec<String> = if let Some(ri) = rest_idx {
                let fixed_count = ri;
                let mut result: Vec<String> =
                    lowered_args[..fixed_count.min(lowered_args.len())].to_vec();
                // Materialize the rest array from trailing args.
                let rest_slice = if fixed_count < lowered_args.len() {
                    &lowered_args[fixed_count..]
                } else {
                    &[]
                };
                let rest_count = rest_slice.len() as u32;
                let cap = rest_count.to_string();
                let mut arr = ctx
                    .block()
                    .call(I64, "js_array_alloc", &[(I32, &cap)]);
                for v in rest_slice {
                    let blk = ctx.block();
                    arr = blk.call(
                        I64,
                        "js_array_push_f64",
                        &[(I64, &arr), (DOUBLE, v)],
                    );
                }
                let rest_box = nanbox_pointer_inline(ctx.block(), &arr);
                result.push(rest_box);
                result
            } else {
                lowered_args
            };

            if effective_args.len() > 5 {
                bail!(
                    "perry-codegen Phase D.1: closure call with {} args (max 5)",
                    effective_args.len()
                );
            }
            let blk = ctx.block();
            let closure_handle = unbox_to_i64(blk, &recv_box);
            let runtime_fn = format!("js_closure_call{}", effective_args.len());
            let mut call_args: Vec<(crate::types::LlvmType, &str)> =
                vec![(I64, &closure_handle)];
            for v in &effective_args {
                call_args.push((DOUBLE, v.as_str()));
            }
            return Ok(blk.call(DOUBLE, &runtime_fn, &call_args));
        }
    }

    // User function call via FuncRef.
    if let Expr::FuncRef(fid) = callee {
        let Some(fname) = ctx.func_names.get(fid).cloned() else {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            return Ok(double_literal(0.0));
        };

        // Rest parameter handling: if the called function has a
        // rest parameter, bundle all trailing args (those at and
        // beyond the rest position) into an array literal and
        // pass that as a single argument.
        let sig = ctx.func_signatures.get(fid).copied();
        let (declared_count, has_rest, _) = sig.unwrap_or((args.len(), false, false));
        let mut lowered: Vec<String> = Vec::with_capacity(declared_count);
        if has_rest {
            // Rest is always the LAST declared param. Pass the
            // first (declared_count - 1) args as-is, then bundle
            // the rest into an array.
            let fixed_count = declared_count.saturating_sub(1);
            for a in args.iter().take(fixed_count) {
                lowered.push(lower_expr(ctx, a)?);
            }
            // Materialize the rest array.
            let rest_count = args.len().saturating_sub(fixed_count);
            let cap = (rest_count as u32).to_string();
            let mut current = ctx
                .block()
                .call(I64, "js_array_alloc", &[(I32, &cap)]);
            for a in args.iter().skip(fixed_count) {
                let v = lower_expr(ctx, a)?;
                let blk = ctx.block();
                current = blk.call(
                    I64,
                    "js_array_push_f64",
                    &[(I64, &current), (DOUBLE, &v)],
                );
            }
            let rest_box = nanbox_pointer_inline(ctx.block(), &current);
            lowered.push(rest_box);
        } else {
            for a in args {
                lowered.push(lower_expr(ctx, a)?);
            }
        }
        let arg_slices: Vec<(crate::types::LlvmType, &str)> =
            lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();

        return Ok(ctx.block().call(DOUBLE, &fname, &arg_slices));
    }

    // Cross-module function call via ExternFuncRef. The HIR carries the
    // function name; we look up the source module's prefix in
    // `import_function_prefixes` (built by the CLI from hir.imports) and
    // generate `perry_fn_<source_prefix>__<name>`. The function is
    // declared in the OTHER module's compilation; here we just emit a
    // direct LLVM call to its scoped name and the system linker
    // resolves the symbol when the .o files are linked together.
    if let Expr::ExternFuncRef { name, return_type: ext_return_type, .. } = callee {
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
                return Ok(nanbox_pointer_inline(blk, &id));
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
                return Ok(nanbox_pointer_inline(blk, &id));
            }
            "clearTimeout" if args.len() == 1 => {
                let id_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let id_handle = unbox_to_i64(blk, &id_box);
                blk.call_void("clearTimeout", &[(I64, &id_handle)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "clearInterval" if args.len() == 1 => {
                let id_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let id_handle = unbox_to_i64(blk, &id_box);
                blk.call_void("clearInterval", &[(I64, &id_handle)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "gc" => {
                ctx.block().call_void("js_gc_collect", &[]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            _ => {}
        }
        // perry/system dispatch: map JS names (isDarkMode, getDeviceIdiom,
        // keychainSave, etc.) to their perry_system_* / perry_* C symbols.
        // These arrive as ExternFuncRef because perry/system imports aren't
        // lowered to NativeMethodCall in the HIR.
        if let Some(sig) = perry_system_table_lookup(name) {
            return lower_perry_ui_table_call(ctx, sig, args);
        }
        // Built-in runtime extern functions (`js_weakmap_set`,
        // `js_regexp_exec`, etc.) that start with `js_` are resolved
        // directly against the runtime library — bypass the import-
        // map lookup and emit a direct LLVM call with an f64/f64 ABI.
        // (The declarations are added centrally in runtime_decls.rs.)
        if name.starts_with("js_") {
            let mut lowered: Vec<String> = Vec::with_capacity(args.len());
            for a in args {
                lowered.push(lower_expr(ctx, a)?);
            }
            let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
            return Ok(ctx.block().call(DOUBLE, name, &arg_slices));
        }
        // Native library functions (bloom_draw_rect, bloom_init_window,
        // etc.) that aren't in the import map — emit a direct call so
        // the linker resolves them against the linked native .a library.
        // Previously these were silently dropped (returned 0.0), which
        // caused Bloom Engine games to render blank windows.
        let Some(source_prefix) = ctx.import_function_prefixes.get(name).cloned() else {
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
            // placement: i64 → x-register, double → d-register. Since
            // Perry can't know the actual C signature, we use a
            // heuristic: if the arg expression is a VARIABLE (LocalGet,
            // PropertyGet, etc.) that's not a literal number, assume
            // it's an integer handle → pass as i64 via fptosi. If it's
            // a number literal, keep as double (likely a real float
            // like width/height/color).
            let mut lowered: Vec<String> = Vec::with_capacity(args.len());
            let mut arg_types: Vec<crate::types::LlvmType> = Vec::with_capacity(args.len());
            // Native library functions (Bloom, etc.) pass numbers as f64
            // (d-registers) but need raw pointers for string/array args
            // (x-registers). Strings are unboxed to *const u8; arrays are
            // unboxed and offset past the 8-byte ArrayHeader to the
            // inline f64 data so the C/Rust side gets a valid *const f64.
            for a in args.iter() {
                let val = lower_expr(ctx, a)?;
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
                } else {
                    lowered.push(val);
                    arg_types.push(DOUBLE);
                }
            }
            let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                arg_types.iter().zip(lowered.iter()).map(|(t, v)| (*t, v.as_str())).collect();
            // Determine return type. If the ExternFuncRef declares
            // return_type: String, the native function returns
            // *const u8 (ptr in x0). If return_type: Void, no return.
            // Otherwise (Number/Any), assume f64 (d0).
            //
            // Heuristic fallback: even if declared as Number, if the
            // function name matches a known "returns-string" pattern
            // AND has string args, treat as ptr return. This covers
            // native libraries like Bloom that declare string-returning
            // functions as `number` for NaN-boxing compat.
            let has_string_args = arg_types.iter().any(|t| *t == PTR);
            let returns_string = matches!(ext_return_type, HirType::String)
                || (has_string_args && (
                    name.contains("read_file")
                    || name.contains("clipboard_text")
                    || name.contains("file_dialog")
                ));
            let returns_void = matches!(ext_return_type, HirType::Void);
            if returns_void {
                ctx.pending_declares
                    .push((name.clone(), crate::types::VOID, arg_types));
                ctx.block().call_void(name, &arg_slices);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            } else if returns_string {
                ctx.pending_declares
                    .push((name.clone(), PTR, arg_types));
                let raw_ptr = ctx.block().call(PTR, name, &arg_slices);
                // Convert raw *const u8 back to a NaN-boxed string.
                let blk = ctx.block();
                let ptr_i64 = blk.ptrtoint(&raw_ptr, I64);
                return Ok(nanbox_string_inline(blk, &ptr_i64));
            } else {
                // Native library functions (Bloom, etc.) return f64 in
                // the d0 register — they use the Perry double-based ABI,
                // not a C integer ABI. Declare as DOUBLE and use the
                // return value directly (no sitofp needed).
                ctx.pending_declares
                    .push((name.clone(), DOUBLE, arg_types));
                return Ok(ctx.block().call(DOUBLE, name, &arg_slices));
            }
        };
        let fname = format!("perry_fn_{}__{}", source_prefix, name);
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
        let target_arity = ctx
            .imported_func_param_counts
            .get(name)
            .copied()
            .unwrap_or(args.len())
            .max(args.len());
        let param_types: Vec<crate::types::LlvmType> =
            std::iter::repeat(DOUBLE).take(target_arity).collect();
        ctx.pending_declares
            .push((fname.clone(), DOUBLE, param_types));
        let mut lowered: Vec<String> = Vec::with_capacity(target_arity);
        for a in args {
            lowered.push(lower_expr(ctx, a)?);
        }
        // Pad with TAG_UNDEFINED for the missing trailing args.
        let undefined_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        while lowered.len() < target_arity {
            lowered.push(undefined_lit.clone());
        }
        let arg_slices: Vec<(crate::types::LlvmType, &str)> =
            lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
        return Ok(ctx.block().call(DOUBLE, &fname, &arg_slices));
    }

    // String/array method dispatch (Phase B.12) and class method
    // dispatch (Phase C.2). For PropertyGet receivers, dispatch based
    // on the receiver's static type.
    if let Expr::PropertyGet { object, property } = callee {
        // Number.prototype.toFixed(decimals) — call js_number_to_fixed.
        // Receiver is any number-typed value; we don't gate on
        // is_numeric_expr because tests often call it on Any locals.
        if property == "toFixed"
            && args.len() == 1
            && !is_string_expr(ctx, object)
            && !is_array_expr(ctx, object)
        {
            let v = lower_expr(ctx, object)?;
            let dec = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let handle =
                blk.call(I64, "js_number_to_fixed", &[(DOUBLE, &v), (DOUBLE, &dec)]);
            return Ok(nanbox_string_inline(blk, &handle));
        }
        // Number.prototype.toPrecision(digits)
        if property == "toPrecision"
            && args.len() == 1
            && !is_string_expr(ctx, object)
            && !is_array_expr(ctx, object)
        {
            let v = lower_expr(ctx, object)?;
            let prec = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let handle =
                blk.call(I64, "js_number_to_precision", &[(DOUBLE, &v), (DOUBLE, &prec)]);
            return Ok(nanbox_string_inline(blk, &handle));
        }
        // Number.prototype.toExponential(decimals)
        if property == "toExponential"
            && args.len() <= 1
            && !is_string_expr(ctx, object)
            && !is_array_expr(ctx, object)
        {
            let v = lower_expr(ctx, object)?;
            let dec = if args.is_empty() {
                "0.0".to_string()
            } else {
                lower_expr(ctx, &args[0])?
            };
            let blk = ctx.block();
            let handle =
                blk.call(I64, "js_number_to_exponential", &[(DOUBLE, &v), (DOUBLE, &dec)]);
            return Ok(nanbox_string_inline(blk, &handle));
        }
        // Buffer.prototype.toString(encoding) — handled BEFORE the radix
        // path because the encoding arg is a STRING ('utf8'/'hex'/'base64'),
        // not a number. Routing a string arg through `fptosi` produces
        // garbage and the runtime defaults to UTF-8 (the original v0.4.131
        // bug that this test pins). We dispatch via the runtime helper
        // `js_value_to_string_with_encoding` which checks BUFFER_REGISTRY
        // at runtime and falls back to `js_jsvalue_to_string` for
        // non-buffer values.
        if property == "toString"
            && args.len() == 1
            && !is_string_expr(ctx, object)
            && !is_array_expr(ctx, object)
            && is_string_expr(ctx, &args[0])
        {
            let has_user_toString = receiver_class_name(ctx, object)
                .map(|cls| {
                    let mut cur = Some(cls);
                    while let Some(c) = cur {
                        if ctx.methods.contains_key(&(c.clone(), "toString".to_string())) {
                            return true;
                        }
                        cur = ctx.classes.get(&c).and_then(|cd| cd.extends_name.clone());
                    }
                    false
                })
                .unwrap_or(false);
            if !has_user_toString {
                let v = lower_expr(ctx, object)?;
                let enc_tag_i32 = if let Expr::String(s) = &args[0] {
                    let lower = s.to_ascii_lowercase();
                    let tag: i32 = match lower.as_str() {
                        "utf8" | "utf-8" | "ascii" | "latin1" | "binary" => 0,
                        "hex" => 1,
                        "base64" | "base64url" => 2,
                        _ => 0,
                    };
                    tag.to_string()
                } else {
                    let enc_box = lower_expr(ctx, &args[0])?;
                    let blk = ctx.block();
                    blk.call(I32, "js_encoding_tag_from_value", &[(DOUBLE, &enc_box)])
                };
                let blk = ctx.block();
                let handle = blk.call(
                    I64,
                    "js_value_to_string_with_encoding",
                    &[(DOUBLE, &v), (I32, &enc_tag_i32)],
                );
                return Ok(nanbox_string_inline(blk, &handle));
            }
        }
        // Number.prototype.toString(radix) — special case where the
        // single arg is the radix (2..36). Routes through
        // js_jsvalue_to_string_radix so `(255).toString(16)` returns
        // "ff" instead of "255".
        if property == "toString"
            && args.len() == 1
            && !is_string_expr(ctx, object)
            && !is_array_expr(ctx, object)
        {
            // Only treat as radix call if class doesn't have toString.
            let has_user_toString = receiver_class_name(ctx, object)
                .map(|cls| {
                    let mut cur = Some(cls);
                    while let Some(c) = cur {
                        if ctx.methods.contains_key(&(c.clone(), "toString".to_string())) {
                            return true;
                        }
                        cur = ctx.classes.get(&c).and_then(|cd| cd.extends_name.clone());
                    }
                    false
                })
                .unwrap_or(false);
            if !has_user_toString {
                let v = lower_expr(ctx, object)?;
                let radix_d = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let radix_i32 = blk.fptosi(DOUBLE, &radix_d, I32);
                let handle = blk.call(
                    I64,
                    "js_jsvalue_to_string_radix",
                    &[(DOUBLE, &v), (I32, &radix_i32)],
                );
                return Ok(nanbox_string_inline(blk, &handle));
            }
        }
        // Universal `.toString()` — works for any JS value via the
        // runtime's js_jsvalue_to_string dispatch (numbers print as
        // their decimal form, strings as themselves, objects as
        // [object Object], etc.). Only intercepts if NO class
        // method dispatch can win (i.e. the receiver isn't a known
        // class with its own toString) — otherwise the user's
        // override wouldn't run.
        if property == "toString"
            && args.len() <= 1
            && !is_string_expr(ctx, object)
            && !is_array_expr(ctx, object)
        {
            // Check whether the receiver class (if any) defines
            // toString itself or via inheritance.
            let has_user_toString = receiver_class_name(ctx, object)
                .map(|cls| {
                    let mut cur = Some(cls);
                    while let Some(c) = cur {
                        if ctx.methods.contains_key(&(c.clone(), "toString".to_string())) {
                            return true;
                        }
                        cur = ctx.classes.get(&c).and_then(|cd| cd.extends_name.clone());
                    }
                    false
                })
                .unwrap_or(false);
            if !has_user_toString {
                let v = lower_expr(ctx, object)?;
                for a in args {
                    let _ = lower_expr(ctx, a)?;
                }
                let blk = ctx.block();
                let handle = blk.call(I64, "js_jsvalue_to_string", &[(DOUBLE, &v)]);
                return Ok(nanbox_string_inline(blk, &handle));
            }
        }
        if is_string_expr(ctx, object) {
            return lower_string_method(ctx, object, property, args);
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
            // array. Also exclude multi-arg variants that the string
            // dispatcher doesn't support (startsWith 2-arg, etc.).
            let is_string_only_method = match property.as_str() {
                "split" | "charCodeAt" | "charAt"
                | "trim" | "trimStart" | "trimEnd" | "substring"
                | "substr" | "toLowerCase" | "toUpperCase"
                | "replaceAll" | "padStart" | "padEnd" | "repeat"
                | "normalize" | "codePointAt"
                | "localeCompare" => true,
                // slice/indexOf/includes/startsWith/endsWith exist on both
                // strings and arrays. Route to string path only when args
                // rule out the array variant (e.g., slice(0) is ambiguous
                // but slice() with 0 args is always array.slice to copy).
                "slice" if args.len() >= 1 => true,
                "indexOf" | "includes" if args.len() == 1 => true,
                "startsWith" | "endsWith" if args.len() == 1 => true,
                "lastIndexOf" if args.len() == 1 => true,
                _ => false,
            };
            // Don't route buffer/Uint8Array methods through the string path —
            // buffers have a different header layout and their indexOf/includes
            // go through dispatch_buffer_method via js_native_call_method.
            let is_buffer = matches!(
                crate::type_analysis::static_type_of(ctx, object),
                Some(perry_types::Type::Named(ref n)) if n == "Uint8Array" || n == "Buffer"
            );
            if is_string_only_method && !is_array_expr(ctx, object) && !is_buffer {
                return lower_string_method(ctx, object, property, args);
            }
        }
        if is_array_expr(ctx, object) {
            return lower_array_method(ctx, object, property, args);
        }

        // -------- Promise.then / .catch / .finally --------
        // Promise pointers are NaN-boxed with POINTER_TAG. We unbox
        // to get the raw i64 promise handle, then call the runtime
        // `js_promise_then(promise, on_fulfilled, on_rejected)` which
        // returns a new promise handle that we re-box with POINTER_TAG.
        //
        // `.catch(cb)` is sugar for `.then(undefined, cb)`.
        if matches!(property.as_str(), "then" | "catch" | "finally")
            && is_promise_expr(ctx, object)
        {
            match property.as_str() {
                "then" => {
                    if !args.is_empty() {
                        let promise_box = lower_expr(ctx, object)?;
                        let on_fulfilled_box = lower_expr(ctx, &args[0])?;
                        let on_rejected_box = if args.len() >= 2 {
                            lower_expr(ctx, &args[1])?
                        } else {
                            "0".to_string() // null → no rejection handler
                        };
                        let blk = ctx.block();
                        let promise_handle = unbox_to_i64(blk, &promise_box);
                        let on_fulfilled_handle = unbox_to_i64(blk, &on_fulfilled_box);
                        let on_rejected_i64 = if args.len() >= 2 {
                            unbox_to_i64(blk, &on_rejected_box)
                        } else {
                            "0".to_string() // null i64
                        };
                        let new_promise = blk.call(
                            I64,
                            "js_promise_then",
                            &[
                                (I64, &promise_handle),
                                (I64, &on_fulfilled_handle),
                                (I64, &on_rejected_i64),
                            ],
                        );
                        return Ok(nanbox_pointer_inline(blk, &new_promise));
                    }
                }
                "catch" => {
                    if !args.is_empty() {
                        let promise_box = lower_expr(ctx, object)?;
                        let on_rejected_box = lower_expr(ctx, &args[0])?;
                        let blk = ctx.block();
                        let promise_handle = unbox_to_i64(blk, &promise_box);
                        let on_rejected_handle = unbox_to_i64(blk, &on_rejected_box);
                        let null_i64 = "0".to_string();
                        let new_promise = blk.call(
                            I64,
                            "js_promise_then",
                            &[
                                (I64, &promise_handle),
                                (I64, &null_i64),
                                (I64, &on_rejected_handle),
                            ],
                        );
                        return Ok(nanbox_pointer_inline(blk, &new_promise));
                    }
                }
                "finally" => {
                    // .finally(cb) — the callback takes no args and its
                    // return value is ignored. We pass it as on_fulfilled
                    // and on_rejected both set to the same closure; the
                    // runtime handles the "ignore return" semantics.
                    if !args.is_empty() {
                        let promise_box = lower_expr(ctx, object)?;
                        let on_finally_box = lower_expr(ctx, &args[0])?;
                        let blk = ctx.block();
                        let promise_handle = unbox_to_i64(blk, &promise_box);
                        let on_finally_handle = unbox_to_i64(blk, &on_finally_box);
                        let new_promise = blk.call(
                            I64,
                            "js_promise_then",
                            &[
                                (I64, &promise_handle),
                                (I64, &on_finally_handle),
                                (I64, &on_finally_handle),
                            ],
                        );
                        return Ok(nanbox_pointer_inline(blk, &new_promise));
                    }
                }
                _ => {}
            }
        }

        // -------- Map/Set methods on PropertyGet receivers --------
        // The HIR only folds `m.set(...)`/`m.get(...)` to MapSet/MapGet
        // when `m` is an Ident receiver (plain local). When the receiver
        // is `this.field` (class method accessing a Map-typed field),
        // the generic Call reaches here and needs an explicit dispatch
        // to the Map runtime helpers. Without this branch,
        // `this.handlers.get(event)` falls through to js_native_call_method
        // which doesn't know about Maps and returns undefined.
        if is_map_expr(ctx, object) {
            match property.as_str() {
                "set" if args.len() == 2 => {
                    let m_box = lower_expr(ctx, object)?;
                    let k_box = lower_expr(ctx, &args[0])?;
                    let v_box = lower_expr(ctx, &args[1])?;
                    let blk = ctx.block();
                    let m_handle = unbox_to_i64(blk, &m_box);
                    blk.call_void(
                        "js_map_set",
                        &[(I64, &m_handle), (DOUBLE, &k_box), (DOUBLE, &v_box)],
                    );
                    return Ok(m_box);
                }
                "get" if args.len() == 1 => {
                    let m_box = lower_expr(ctx, object)?;
                    let k_box = lower_expr(ctx, &args[0])?;
                    let blk = ctx.block();
                    let m_handle = unbox_to_i64(blk, &m_box);
                    return Ok(blk.call(
                        DOUBLE,
                        "js_map_get",
                        &[(I64, &m_handle), (DOUBLE, &k_box)],
                    ));
                }
                "has" if args.len() == 1 => {
                    let m_box = lower_expr(ctx, object)?;
                    let k_box = lower_expr(ctx, &args[0])?;
                    let blk = ctx.block();
                    let m_handle = unbox_to_i64(blk, &m_box);
                    let i32_v = blk.call(
                        crate::types::I32,
                        "js_map_has",
                        &[(I64, &m_handle), (DOUBLE, &k_box)],
                    );
                    return Ok(crate::expr::i32_bool_to_nanbox(blk, &i32_v));
                }
                "delete" if args.len() == 1 => {
                    let m_box = lower_expr(ctx, object)?;
                    let k_box = lower_expr(ctx, &args[0])?;
                    let blk = ctx.block();
                    let m_handle = unbox_to_i64(blk, &m_box);
                    let i32_v = blk.call(
                        crate::types::I32,
                        "js_map_delete",
                        &[(I64, &m_handle), (DOUBLE, &k_box)],
                    );
                    return Ok(crate::expr::i32_bool_to_nanbox(blk, &i32_v));
                }
                "clear" if args.is_empty() => {
                    let m_box = lower_expr(ctx, object)?;
                    let blk = ctx.block();
                    let m_handle = unbox_to_i64(blk, &m_box);
                    blk.call_void("js_map_clear", &[(I64, &m_handle)]);
                    return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                }
                _ => {}
            }
        }
        if is_set_expr(ctx, object) {
            match property.as_str() {
                "add" if args.len() == 1 => {
                    let s_box = lower_expr(ctx, object)?;
                    let v_box = lower_expr(ctx, &args[0])?;
                    let blk = ctx.block();
                    let s_handle = unbox_to_i64(blk, &s_box);
                    blk.call_void("js_set_add", &[(I64, &s_handle), (DOUBLE, &v_box)]);
                    return Ok(s_box);
                }
                "has" if args.len() == 1 => {
                    let s_box = lower_expr(ctx, object)?;
                    let v_box = lower_expr(ctx, &args[0])?;
                    let blk = ctx.block();
                    let s_handle = unbox_to_i64(blk, &s_box);
                    let i32_v = blk.call(
                        crate::types::I32,
                        "js_set_has",
                        &[(I64, &s_handle), (DOUBLE, &v_box)],
                    );
                    return Ok(crate::expr::i32_bool_to_nanbox(blk, &i32_v));
                }
                "delete" if args.len() == 1 => {
                    let s_box = lower_expr(ctx, object)?;
                    let v_box = lower_expr(ctx, &args[0])?;
                    let blk = ctx.block();
                    let s_handle = unbox_to_i64(blk, &s_box);
                    let i32_v = blk.call(
                        crate::types::I32,
                        "js_set_delete",
                        &[(I64, &s_handle), (DOUBLE, &v_box)],
                    );
                    return Ok(crate::expr::i32_bool_to_nanbox(blk, &i32_v));
                }
                "clear" if args.is_empty() => {
                    let s_box = lower_expr(ctx, object)?;
                    let blk = ctx.block();
                    let s_handle = unbox_to_i64(blk, &s_box);
                    blk.call_void("js_set_clear", &[(I64, &s_handle)]);
                    return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                }
                _ => {}
            }
        }

        // -------- Map.forEach / Set.forEach --------
        // The HIR emits these as generic Call { callee: PropertyGet }
        // because it skips ArrayForEach when the receiver is Map/Set.
        // Route to the runtime forEach implementations which iterate
        // entries and call the callback via js_closure_call2.
        if property == "forEach" && args.len() >= 1 {
            if is_map_expr(ctx, object) {
                let m_box = lower_expr(ctx, object)?;
                let cb_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let m_handle = unbox_to_i64(blk, &m_box);
                blk.call_void("js_map_foreach", &[(I64, &m_handle), (DOUBLE, &cb_box)]);
                return Ok(double_literal(0.0));
            }
            if is_set_expr(ctx, object) {
                let s_box = lower_expr(ctx, object)?;
                let cb_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let s_handle = unbox_to_i64(blk, &s_box);
                blk.call_void("js_set_foreach", &[(I64, &s_handle), (DOUBLE, &cb_box)]);
                return Ok(double_literal(0.0));
            }
        }

        // ── AbortController / AbortSignal dispatch ──
        // `new AbortController()` returns a NaN-boxed pointer
        // (refined to `Named("AbortController")`). The runtime's
        // ObjectHeader carries `signal` / `aborted` fields that the
        // generic property-get path reads. Method calls need explicit
        // interception because the class isn't in `ctx.classes`.
        if let Some(val) = lower_abort_controller_call(ctx, object, property, args)? {
            return Ok(val);
        }

        // ── Chained Web Fetch dispatch ──
        // `r.headers.get(k)` — the inner `r.headers` lowered to a
        // NativeMethodCall that returns an f64 Headers handle; route
        // the outer `.get(...)` (and friends) through the Headers FFI.
        // `r.clone().status` / `.text()` / etc — the inner clone call
        // returns an f64 Response handle; route the outer call through
        // the fetch dispatch.
        //
        // `new Response(...).text()` — likewise, when the receiver is
        // a direct `Expr::New { class_name: "Response"|"Headers"|"Request" }`
        // (no intermediate let binding).
        if let Expr::NativeMethodCall {
            module: chain_mod,
            method: chain_method,
            ..
        } = object.as_ref()
        {
            // Chain `<Response>.headers.<method>(...)` where chain_method == "headers".
            if chain_mod == "fetch" && chain_method == "headers" {
                if let Some(val) =
                    lower_fetch_native_method(ctx, "Headers", property.as_str(), Some(object), args)?
                {
                    return Ok(val);
                }
            }
            // Chain `<Response>.clone().<method>(...)` — dispatch as a
            // fetch method on the cloned handle.
            if chain_mod == "fetch" && chain_method == "clone" {
                if let Some(val) =
                    lower_fetch_native_method(ctx, "fetch", property.as_str(), Some(object), args)?
                {
                    return Ok(val);
                }
            }
        }
        // Chain `new Response(...).text()` / `.json()` etc.
        if let Expr::New { class_name: nc, .. } = object.as_ref() {
            let fetch_dispatch = matches!(
                nc.as_str(),
                "Response" | "Headers" | "Request"
            );
            if fetch_dispatch {
                let module = match nc.as_str() {
                    "Response" => "fetch",
                    "Headers" => "Headers",
                    "Request" => "Request",
                    _ => unreachable!(),
                };
                if let Some(val) =
                    lower_fetch_native_method(ctx, module, property.as_str(), Some(object), args)?
                {
                    return Ok(val);
                }
            }
        }

        // Class instance method call. The receiver's static type is
        // `Type::Named(<class>)` for typed instances.
        //
        // Resolution strategy:
        //   1. Walk the receiver's class + parent chain to find a
        //      method named `property`. The first match (most-derived
        //      that defines the method) is the static fallback.
        //   2. Find every subclass of the receiver's class that ALSO
        //      defines the same method — those are the virtual
        //      override candidates.
        //   3. If there are no overrides, emit a direct call to the
        //      static fallback (fast path, no runtime cost).
        //   4. If there ARE overrides, emit a switch on the object's
        //      runtime class_id: each override gets its own case
        //      calling its concrete method, default falls through to
        //      the static fallback.
        // Interface / dynamic dispatch fallback: when the static
        // class is unknown OR resolves to an interface name not in
        // the class registry, BUT the property name corresponds to
        // a method defined on at least one class in the registry,
        // emit a switch on class_id over all classes that have that
        // method.
        // Skip dynamic dispatch when the receiver is GlobalGet (e.g.
        // `console.log`). GlobalGet is a module-level global object
        // (console, Math, JSON, etc.), not a class instance. Without
        // this guard, `console.log()` gets hijacked by the interface
        // dispatch tower when a user class happens to have a method
        // with the same name (like `SimpleLogger.log()`).
        let is_global = matches!(object.as_ref(), Expr::GlobalGet(_));
        // If the receiver's static type is a well-known built-in with its own
        // runtime method family (Buffer byte readers, Array, Map, Set, …),
        // don't enter the user-class dispatch tower. Otherwise an imported
        // user class that happens to declare the same method name (e.g. a
        // BufferCursor with `readUInt8`) would be enumerated as an
        // implementor and `buf.readUInt8(i)` would fall through to the
        // default 0.0 case when the Buffer's class id doesn't match any
        // tower entry.
        let is_builtin_receiver = match receiver_class_name(ctx, object) {
            Some(name) => matches!(
                name.as_str(),
                "Buffer" | "Uint8Array" | "Uint8ClampedArray"
                    | "Int8Array" | "Int16Array" | "Uint16Array"
                    | "Int32Array" | "Uint32Array"
                    | "Float32Array" | "Float64Array"
                    | "BigInt64Array" | "BigUint64Array"
                    | "Array" | "ReadonlyArray"
                    | "Map" | "Set" | "WeakMap" | "WeakSet"
                    | "Promise" | "RegExp" | "Date"
            ),
            None => false,
        };
        let needs_dynamic_dispatch = !is_global && !is_builtin_receiver && match receiver_class_name(ctx, object) {
            None => true,
            Some(name) => !ctx.classes.contains_key(&name),
        };
        if needs_dynamic_dispatch {
            // Find all (class, method_name → fn_name) where the
            // method is defined directly on a class.
            let mut implementors: Vec<(u32, String)> = Vec::new();
            for ((cls, mname), fname) in ctx.methods.iter() {
                if mname != property {
                    continue;
                }
                if let Some(cid) = ctx.class_ids.get(cls).copied() {
                    implementors.push((cid, fname.clone()));
                }
            }
            if !implementors.is_empty() {
                let recv_box = lower_expr(ctx, object)?;
                let mut lowered_args: Vec<String> = Vec::with_capacity(args.len() + 1);
                lowered_args.push(recv_box.clone());
                for a in args {
                    lowered_args.push(lower_expr(ctx, a)?);
                }
                let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                    lowered_args.iter().map(|s| (DOUBLE, s.as_str())).collect();

                let blk = ctx.block();
                let recv_handle = unbox_to_i64(blk, &recv_box);
                let cid = blk.call(I32, "js_object_get_class_id", &[(I64, &recv_handle)]);

                // Tower of icmp+br: each implementor's case calls
                // its concrete method, default returns 0.0 (the
                // closure-call fallback would also handle this but
                // returning a sentinel is cheaper).
                let mut case_idxs: Vec<usize> = Vec::with_capacity(implementors.len());
                for (i, _) in implementors.iter().enumerate() {
                    case_idxs.push(ctx.new_block(&format!("idispatch.case{}", i)));
                }
                let default_idx = ctx.new_block("idispatch.default");
                let merge_idx = ctx.new_block("idispatch.merge");
                let merge_label = ctx.block_label(merge_idx);

                for (i, (case_cid, _)) in implementors.iter().enumerate() {
                    let case_label = ctx.block_label(case_idxs[i]);
                    let cmp = ctx.block().icmp_eq(I32, &cid, &case_cid.to_string());
                    if i + 1 < implementors.len() {
                        let next_idx = ctx.new_block(&format!("idispatch.test{}", i + 1));
                        let next_lbl = ctx.block_label(next_idx);
                        ctx.block().cond_br(&cmp, &case_label, &next_lbl);
                        ctx.current_block = next_idx;
                    } else {
                        let default_label = ctx.block_label(default_idx);
                        ctx.block().cond_br(&cmp, &case_label, &default_label);
                    }
                }

                let mut phi_inputs: Vec<(String, String)> = Vec::new();
                for ((_, fname), &case_idx) in implementors.iter().zip(case_idxs.iter()) {
                    ctx.current_block = case_idx;
                    let v = ctx.block().call(DOUBLE, fname, &arg_slices);
                    let after_label = ctx.block().label.clone();
                    if !ctx.block().is_terminated() {
                        ctx.block().br(&merge_label);
                    }
                    phi_inputs.push((v, after_label));
                }
                // Default branch: receiver's class id didn't match any user
                // class implementing `property`. Rather than returning 0.0,
                // fall through to the runtime's `js_native_call_method` so
                // same-named built-in methods (Buffer.readUInt8, Array.push,
                // Map.get, …) still reach their native dispatch. Without
                // this, a `buf.readUInt8(i)` call site ends up in the
                // default branch and returns 0, silently corrupting reads
                // any time a user class in scope happens to declare a
                // method of the same name.
                ctx.current_block = default_idx;
                let key_idx = ctx.strings.intern(property);
                let entry = ctx.strings.entry(key_idx);
                let bytes_global = format!("@{}", entry.bytes_global);
                let name_len_str = entry.byte_len.to_string();
                let (fb_args_ptr, fb_args_len) = if args.is_empty() {
                    ("null".to_string(), "0".to_string())
                } else {
                    let n = args.len();
                    let buf_reg = ctx.block().next_reg();
                    ctx.block().emit_raw(format!("{} = alloca [{} x double]", buf_reg, n));
                    for (i, a_val) in lowered_args.iter().skip(1).enumerate() {
                        let slot = ctx.block().gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                        ctx.block().store(DOUBLE, a_val, &slot);
                    }
                    let ptr_reg = ctx.block().next_reg();
                    ctx.block().emit_raw(format!(
                        "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                        ptr_reg, n, buf_reg
                    ));
                    (ptr_reg, n.to_string())
                };
                let v_def = ctx.block().call(
                    DOUBLE,
                    "js_native_call_method",
                    &[
                        (DOUBLE, &recv_box),
                        (crate::types::PTR, &bytes_global),
                        (I64, &name_len_str),
                        (crate::types::PTR, &fb_args_ptr),
                        (I64, &fb_args_len),
                    ],
                );
                let def_label = ctx.block().label.clone();
                ctx.block().br(&merge_label);
                phi_inputs.push((v_def, def_label));

                ctx.current_block = merge_idx;
                let phi_args: Vec<(&str, &str)> =
                    phi_inputs.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
                return Ok(ctx.block().phi(DOUBLE, &phi_args));
            }
        }

        if let Some(class_name) = receiver_class_name(ctx, object) {
            // Step 1: walk parent chain for the static method name.
            let mut static_fn: Option<String> = None;
            let mut current_class = Some(class_name.clone());
            while let Some(cur) = current_class {
                let key = (cur.clone(), property.clone());
                if let Some(fname) = ctx.methods.get(&key).cloned() {
                    static_fn = Some(fname);
                    break;
                }
                current_class = ctx
                    .classes
                    .get(&cur)
                    .and_then(|c| c.extends_name.clone());
            }

            if let Some(fallback_fn) = static_fn {
                // Step 2: collect overriding subclasses. For each
                // subclass C transitively extending class_name, look
                // up which method C uses for `property` (walking C's
                // parent chain). If that resolves to a different
                // function than the static fallback, C needs an
                // explicit case in the dispatch table.
                let mut overrides: Vec<(u32, String)> = Vec::new();
                for (sub_name, &sub_id) in ctx.class_ids.iter() {
                    if *sub_name == class_name {
                        continue;
                    }
                    // Is sub_name transitively a subclass of class_name?
                    let mut parent =
                        ctx.classes.get(sub_name).and_then(|c| c.extends_name.clone());
                    let mut is_subclass = false;
                    while let Some(p) = parent {
                        if p == class_name {
                            is_subclass = true;
                            break;
                        }
                        parent = ctx.classes.get(&p).and_then(|c| c.extends_name.clone());
                    }
                    if !is_subclass {
                        continue;
                    }
                    // Resolve the method for sub_name by walking its
                    // own parent chain (NOT class_name's chain).
                    let mut cur = Some(sub_name.clone());
                    let mut sub_fn: Option<String> = None;
                    while let Some(c) = cur {
                        let key = (c.clone(), property.clone());
                        if let Some(fname) = ctx.methods.get(&key).cloned() {
                            sub_fn = Some(fname);
                            break;
                        }
                        cur = ctx.classes.get(&c).and_then(|c| c.extends_name.clone());
                    }
                    if let Some(sub_fn) = sub_fn {
                        if sub_fn != fallback_fn {
                            overrides.push((sub_id, sub_fn));
                        }
                    }
                }

                let recv_box = lower_expr(ctx, object)?;
                let mut lowered_args: Vec<String> = Vec::with_capacity(args.len() + 1);
                lowered_args.push(recv_box.clone());
                for a in args {
                    lowered_args.push(lower_expr(ctx, a)?);
                }
                let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                    lowered_args.iter().map(|s| (DOUBLE, s.as_str())).collect();

                if overrides.is_empty() {
                    // Fast path: no virtual dispatch needed.
                    return Ok(ctx.block().call(DOUBLE, &fallback_fn, &arg_slices));
                }

                // Step 4: virtual dispatch via class_id switch.
                // Read class_id from the object header, then branch
                // to the right concrete method block.
                let blk = ctx.block();
                let recv_handle = unbox_to_i64(blk, &recv_box);
                let cid = blk.call(I32, "js_object_get_class_id", &[(I64, &recv_handle)]);

                // Pre-create blocks: one per override + default + merge.
                let mut case_idxs: Vec<usize> = Vec::with_capacity(overrides.len());
                for (i, _) in overrides.iter().enumerate() {
                    case_idxs.push(ctx.new_block(&format!("vdispatch.case{}", i)));
                }
                let default_idx = ctx.new_block("vdispatch.default");
                let merge_idx = ctx.new_block("vdispatch.merge");

                // Default → fallback. We use a tower of icmp+br rather
                // than the LLVM `switch` instruction (which the IR
                // builder doesn't expose generically) — same shape,
                // slightly more verbose.
                let mut current_label = ctx.block().label.clone();
                for (i, (case_cid, _)) in overrides.iter().enumerate() {
                    let next_label = if i + 1 < overrides.len() {
                        // We'll start the next test in this same block
                        // — actually use a fresh block for the test.
                        format!("vdispatch.test{}", i + 1)
                    } else {
                        ctx.block_label(default_idx)
                    };
                    let case_label = ctx.block_label(case_idxs[i]);
                    // Make sure ctx.current_block points at the
                    // current test block.
                    let _ = current_label;
                    let cmp = ctx.block().icmp_eq(I32, &cid, &case_cid.to_string());
                    if i + 1 < overrides.len() {
                        // Create the next test block as a fresh block
                        // and branch into it on the false arm.
                        let next_idx = ctx.new_block(&format!("vdispatch.test{}", i + 1));
                        let next_lbl = ctx.block_label(next_idx);
                        ctx.block().cond_br(&cmp, &case_label, &next_lbl);
                        ctx.current_block = next_idx;
                        current_label = next_lbl;
                    } else {
                        ctx.block().cond_br(&cmp, &case_label, &next_label);
                    }
                }

                // Each case block: call the override and branch to merge.
                let merge_label = ctx.block_label(merge_idx);
                let mut phi_inputs: Vec<(String, String)> = Vec::new();
                for ((_, fname), &case_idx) in overrides.iter().zip(case_idxs.iter()) {
                    ctx.current_block = case_idx;
                    let v = ctx.block().call(DOUBLE, fname, &arg_slices);
                    let after_label = ctx.block().label.clone();
                    if !ctx.block().is_terminated() {
                        ctx.block().br(&merge_label);
                    }
                    phi_inputs.push((v, after_label));
                }

                // Default block: call the static fallback.
                ctx.current_block = default_idx;
                let v_def = ctx.block().call(DOUBLE, &fallback_fn, &arg_slices);
                let def_label = ctx.block().label.clone();
                if !ctx.block().is_terminated() {
                    ctx.block().br(&merge_label);
                }
                phi_inputs.push((v_def, def_label));

                // Merge: phi over all incoming case results.
                ctx.current_block = merge_idx;
                let phi_args: Vec<(&str, &str)> =
                    phi_inputs.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
                return Ok(ctx.block().phi(DOUBLE, &phi_args));
            }
        }
    }

    // console.log(<args...>) sink.
    //
    // JS spec: console.log can take any number of args, separated by
    // single spaces. We approximate by emitting a separate dispatch
    // call per arg with a literal " " in between, then a final "\n".
    // The runtime functions take a NaN-boxed double and print it
    // followed by a single trailing space (for the inter-arg form)
    // or newline (for the final/single-arg form). For now we use the
    // existing js_console_log_dynamic for every arg — the runtime
    // already adds a newline, so multi-arg console.log will be
    // separated by newlines instead of spaces. Spec-compliant
    // separator handling lives in a future Phase I tweak.
    if let Expr::PropertyGet { object, property } = callee {
        if matches!(object.as_ref(), Expr::GlobalGet(_))
            && matches!(
                property.as_str(),
                "log" | "info" | "warn" | "error" | "debug"
                    | "dir" | "table" | "trace"
                    | "group" | "groupEnd" | "groupCollapsed"
                    | "time" | "timeEnd" | "timeLog"
                    | "count" | "countReset" | "clear" | "assert"
            )
        {
            // Catch-all for the entire console.* surface. Most of
            // them are best-effort: we route the args through
            // js_console_log_dynamic so the user at least sees the
            // values, then return undefined-as-double. Spec-compliant
            // dispatch (separate stderr for warn/error, dir's depth
            // option, table's tabular layout) is a future improvement.
            // Zero-arg console.* calls — handle the truly nullary
            // methods (groupEnd, clear) and the dataless variants of
            // log/info/warn/error/debug (which print nothing). Methods
            // with meaningful zero-arg semantics (count, countReset,
            // time, timeEnd, timeLog with the implicit "default" label)
            // intentionally fall through to the dedicated handler below.
            if args.is_empty() {
                match property.as_str() {
                    "groupEnd" => {
                        ctx.block().call_void("js_console_group_end", &[]);
                        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                    }
                    "clear" => {
                        ctx.block().call_void("js_console_clear", &[]);
                        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                    }
                    "count" | "countReset" | "time" | "timeEnd" | "timeLog" => {
                        // Fall through to the dedicated handler below
                        // which calls the runtime with the implicit
                        // "default" label.
                    }
                    _ => {
                        // log/info/warn/error/debug/etc. — print nothing.
                        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                    }
                }
            }
            // console.group / groupCollapsed with a label — push
            // indent level and print the label.
            if matches!(property.as_str(), "group" | "groupCollapsed") {
                for a in args {
                    let v = lower_expr(ctx, a)?;
                    ctx.block()
                        .call_void("js_console_log_dynamic", &[(DOUBLE, &v)]);
                }
                ctx.block().call_void("js_console_group_begin", &[]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            // console.trace([msg]) — `js_console_trace` formats the
            // optional message and emits a native backtrace to stderr
            // (issue #20).
            if property == "trace" {
                let val: String = if args.is_empty() {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                } else {
                    lower_expr(ctx, &args[0])?
                };
                ctx.block().call_void("js_console_trace", &[(DOUBLE, &val)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            // console.table(data) — dedicated table renderer.
            if property == "table" && args.len() == 1 {
                let v = lower_expr(ctx, &args[0])?;
                ctx.block().call_void("js_console_table", &[(DOUBLE, &v)]);
                return Ok("0.0".to_string());
            }
            // console.time(label) / timeEnd(label) / timeLog(label) —
            // dedicated timer functions that track per-label Instants
            // in a thread-local HashMap. Without this dispatch the
            // label got routed through js_console_log_dynamic and just
            // printed the string, losing the elapsed-time output.
            if matches!(property.as_str(), "time" | "timeEnd" | "timeLog" | "count" | "countReset")
                && args.len() == 1
            {
                let v = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let handle = blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &v)]);
                let runtime_fn = match property.as_str() {
                    "time" => "js_console_time",
                    "timeEnd" => "js_console_time_end",
                    "timeLog" => "js_console_time_log",
                    "count" => "js_console_count",
                    "countReset" => "js_console_count_reset",
                    _ => unreachable!(),
                };
                blk.call_void(runtime_fn, &[(I64, &handle)]);
                return Ok("0.0".to_string());
            }
            // Zero-arg time* / count* use the default label "default".
            if matches!(property.as_str(), "time" | "timeEnd" | "timeLog" | "count" | "countReset")
                && args.is_empty()
            {
                let sp_idx = ctx.strings.intern("default");
                let sp_global = format!("@{}", ctx.strings.entry(sp_idx).handle_global);
                let blk = ctx.block();
                let sp_box = blk.load(DOUBLE, &sp_global);
                let handle = blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &sp_box)]);
                let runtime_fn = match property.as_str() {
                    "time" => "js_console_time",
                    "timeEnd" => "js_console_time_end",
                    "timeLog" => "js_console_time_log",
                    "count" => "js_console_count",
                    "countReset" => "js_console_count_reset",
                    _ => unreachable!(),
                };
                blk.call_void(runtime_fn, &[(I64, &handle)]);
                return Ok("0.0".to_string());
            }
            // console.assert(cond[, ...messages]) — runtime helper
            // checks the condition and only prints "Assertion failed: msg"
            // when cond is falsy. Without this dedicated dispatch, the call
            // fell through to the multi-arg console.log path which
            // printed both cond and messages unconditionally ("true should
            // not appear" / "false assertion failed message").
            //
            // Two shapes:
            //   1. 0–1 message args → js_console_assert(cond, msg_ptr)
            //   2. 2+ message args  → bundle into array, call
            //      js_console_assert_spread(cond, arr_ptr) which formats
            //      each element with format_jsvalue and joins with spaces.
            if property == "assert" {
                let cond_v = if args.is_empty() {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                } else {
                    lower_expr(ctx, &args[0])?
                };
                if args.len() <= 2 {
                    let msg_handle = if args.len() == 2 {
                        let msg_v = lower_expr(ctx, &args[1])?;
                        let blk = ctx.block();
                        blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &msg_v)])
                    } else {
                        "0".to_string()
                    };
                    ctx.block()
                        .call_void("js_console_assert", &[(DOUBLE, &cond_v), (I64, &msg_handle)]);
                } else {
                    // Multi-arg messages: bundle args[1..] into a heap
                    // array and call the spread variant.
                    let cap = ((args.len() - 1) as u32).to_string();
                    let mut current_arr = ctx
                        .block()
                        .call(I64, "js_array_alloc", &[(I32, &cap)]);
                    for arg in args.iter().skip(1) {
                        let v = lower_expr(ctx, arg)?;
                        let blk = ctx.block();
                        current_arr = blk.call(
                            I64,
                            "js_array_push_f64",
                            &[(I64, &current_arr), (DOUBLE, &v)],
                        );
                    }
                    ctx.block().call_void(
                        "js_console_assert_spread",
                        &[(DOUBLE, &cond_v), (I64, &current_arr)],
                    );
                }
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            // console.dir(obj[, options]) — Node prints just the formatted
            // object, ignoring the options arg (Perry doesn't honor depth /
            // colors / showHidden yet). Without this, the multi-arg dispatch
            // would print both the obj and the options object side by side.
            if property == "dir" && !args.is_empty() {
                let v = lower_expr(ctx, &args[0])?;
                ctx.block().call_void("js_console_log_dynamic", &[(DOUBLE, &v)]);
                // Lower remaining args for side effects only.
                for a in args.iter().skip(1) {
                    let _ = lower_expr(ctx, a)?;
                }
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            // Single-arg fast path: just print directly.
            if args.len() == 1 {
                let arg = &args[0];
                let is_number_literal = matches!(arg, Expr::Integer(_) | Expr::Number(_));
                let v = lower_expr(ctx, arg)?;
                let runtime_fn = if is_number_literal {
                    "js_console_log_number"
                } else {
                    "js_console_log_dynamic"
                };
                ctx.block().call_void(runtime_fn, &[(DOUBLE, &v)]);
                return Ok("0.0".to_string());
            }
            // Multi-arg: bundle all args into a heap array and call
            // js_console_log_spread, which uses the runtime's
            // format_jsvalue (Node-style util.inspect output for
            // objects/arrays). This is more accurate than
            // js_jsvalue_to_string which only does the JS toString
            // protocol (returns "[object Object]" for plain objects).
            let cap = (args.len() as u32).to_string();
            let mut current_arr = ctx
                .block()
                .call(I64, "js_array_alloc", &[(I32, &cap)]);
            for arg in args.iter() {
                let v = lower_expr(ctx, arg)?;
                let blk = ctx.block();
                current_arr = blk.call(
                    I64,
                    "js_array_push_f64",
                    &[(I64, &current_arr), (DOUBLE, &v)],
                );
            }
            let runtime_fn = match property.as_str() {
                "warn" => "js_console_warn_spread",
                "error" => "js_console_error_spread",
                _ => "js_console_log_spread",
            };
            ctx.block().call_void(runtime_fn, &[(I64, &current_arr)]);
            return Ok("0.0".to_string());
        }
    }

    // -------- Promise.resolve / reject / all / race / allSettled --------
    //
    // The HIR doesn't have dedicated PromiseResolve/Reject variants —
    // they appear as Call { callee: PropertyGet { GlobalGet(0), "resolve" } }.
    // We assume any
    // GlobalGet receiver with a Promise-shaped property name is the
    // Promise constructor. (This conflicts with `console.resolve` etc.
    // — but those don't exist in JS.)
    if let Expr::PropertyGet { object, property } = callee {
        if matches!(object.as_ref(), Expr::GlobalGet(_)) {
            match property.as_str() {
                "resolve" => {
                    let value = if args.is_empty() {
                        double_literal(0.0)
                    } else {
                        lower_expr(ctx, &args[0])?
                    };
                    let blk = ctx.block();
                    let handle = blk.call(I64, "js_promise_resolved", &[(DOUBLE, &value)]);
                    return Ok(nanbox_pointer_inline(blk, &handle));
                }
                "reject" => {
                    let reason = if args.is_empty() {
                        double_literal(0.0)
                    } else {
                        lower_expr(ctx, &args[0])?
                    };
                    let blk = ctx.block();
                    let handle = blk.call(I64, "js_promise_rejected", &[(DOUBLE, &reason)]);
                    return Ok(nanbox_pointer_inline(blk, &handle));
                }
                "all" | "race" | "allSettled" | "any" => {
                    if args.is_empty() {
                        return Ok(double_literal(0.0));
                    }
                    let arr_box = lower_expr(ctx, &args[0])?;
                    let blk = ctx.block();
                    let arr_handle = unbox_to_i64(blk, &arr_box);
                    let runtime_fn = match property.as_str() {
                        "all" => "js_promise_all",
                        "race" => "js_promise_race",
                        "any" => "js_promise_any",
                        _ => "js_promise_all_settled",
                    };
                    let handle = blk.call(I64, runtime_fn, &[(I64, &arr_handle)]);
                    return Ok(nanbox_pointer_inline(blk, &handle));
                }
                "withResolvers" => {
                    // Promise.withResolvers<T>() returns { promise, resolve, reject }.
                    // We create a pending promise and return an object with
                    // the promise + resolve/reject closures.
                    let blk = ctx.block();
                    let handle = blk.call(I64, "js_promise_with_resolvers", &[]);
                    return Ok(nanbox_pointer_inline(blk, &handle));
                }
                // `Array.fromAsync(input)` — Node 22+ static method.
                // Dispatched here because the receiver is a GlobalGet
                // (matches the same pattern as Promise.all). The property
                // name `fromAsync` is unique to Array so there's no
                // conflict with Promise.
                "fromAsync" => {
                    if args.is_empty() {
                        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                    }
                    let input = lower_expr(ctx, &args[0])?;
                    let blk = ctx.block();
                    return Ok(blk.call(DOUBLE, "js_array_from_async", &[(DOUBLE, &input)]));
                }
                _ => {}
            }
        }
    }

    // -------- PropertyGet method dispatch via js_native_call_method --------
    //
    // For `recv.method(args)` where the static dispatch above didn't fire
    // and the receiver isn't a known class instance, route through the
    // runtime's universal `js_native_call_method` dispatcher. This is the
    // path that catches Map/Set/RegExp methods on plain object fields
    // (e.g. `wrap.m.get(k)` where `wrap: { m: Map }`) — the runtime
    // detects the registry and dispatches to `js_map_get` etc. directly.
    //
    // The signature is `js_native_call_method(obj: f64, name_ptr: ptr,
    // name_len: i64, args_ptr: ptr, args_len: i64) -> f64`. We pass the
    // method name as a raw rodata byte pointer (the StringPool already
    // emits the bytes as `[N+1 x i8]` for every interned string), and
    // materialize the args into a stack `[N x double]` slot.
    if let Expr::PropertyGet { object, property } = callee {
        // Skip when the receiver is a global module access (e.g. `console.log`,
        // `JSON.parse`) — those are handled by the spread/closure paths above
        // or have dedicated lowerings. Skip when the receiver is a known class
        // instance — those have static method dispatch handled earlier.
        //
        // Exception: `Uint8Array`/`Buffer` typed receivers must NOT be skipped.
        // They aren't real classes (no vtable) — the runtime's
        // `js_native_call_method` detects them via `is_registered_buffer` and
        // routes through `dispatch_buffer_method` which handles the full
        // Node-style numeric read/write/swap/indexOf method family.
        let class_name_opt = receiver_class_name(ctx, object);
        let is_buffer_class = matches!(
            class_name_opt.as_deref(),
            Some("Uint8Array") | Some("Buffer") | Some("Uint8ClampedArray")
        );
        let skip_native = matches!(object.as_ref(), Expr::GlobalGet(_))
            || (class_name_opt.is_some() && !is_buffer_class);
        if !skip_native {
            // Issue #92 fast path: intrinsify Buffer numeric reads
            // (`buf.readInt32BE(off)` etc.) when the receiver is a tracked
            // `const buf = Buffer.alloc(N)` local. Returns Ok(Some(reg)) on
            // success; falls through to the runtime dispatch for all other
            // Buffer methods or untracked receivers.
            if is_buffer_class {
                if let Some(reg) = try_emit_buffer_read_intrinsic(ctx, object, property, args)? {
                    return Ok(reg);
                }
            }
            let recv_box = lower_expr(ctx, object)?;
            let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
            for a in args {
                lowered_args.push(lower_expr(ctx, a)?);
            }
            // Intern the method name and reference its rodata byte global.
            let key_idx = ctx.strings.intern(property);
            let entry = ctx.strings.entry(key_idx);
            let bytes_global = format!("@{}", entry.bytes_global);
            let name_len_str = entry.byte_len.to_string();
            let blk = ctx.block();
            // Stack-allocate the args array if any.
            let (args_ptr, args_len_str) = if lowered_args.is_empty() {
                ("null".to_string(), "0".to_string())
            } else {
                let n = lowered_args.len();
                let buf_reg = blk.next_reg();
                blk.emit_raw(format!("{} = alloca [{} x double]", buf_reg, n));
                for (i, v) in lowered_args.iter().enumerate() {
                    let slot = blk.gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                    blk.store(DOUBLE, v, &slot);
                }
                (buf_reg, n.to_string())
            };
            return Ok(blk.call(
                DOUBLE,
                "js_native_call_method",
                &[
                    (DOUBLE, &recv_box),
                    (PTR, &bytes_global),
                    (I64, &name_len_str),
                    (PTR, &args_ptr),
                    (I64, &args_len_str),
                ],
            ));
        }
    }

    // Fallthrough: assume the callee evaluates to a closure value at
    // runtime and dispatch through `js_closure_call<N>`. This catches:
    //   - LocalGet of an `: any`-typed local that the static check missed
    //   - Nested calls like `curry(1)(2)(3)` where the callee is itself
    //     a Call returning a function
    //   - PropertyGet on a class instance whose property is a closure
    //
    // The runtime checks the closure header on its own — if the value
    // isn't actually a closure, js_closure_call<N> handles the error.
    if args.len() <= 16 {
        let recv_box = lower_expr(ctx, callee)?;
        let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered_args.push(lower_expr(ctx, a)?);
        }
        let blk = ctx.block();
        let closure_handle = unbox_to_i64(blk, &recv_box);
        let runtime_fn = format!("js_closure_call{}", args.len());
        let mut call_args: Vec<(crate::types::LlvmType, &str)> = vec![(I64, &closure_handle)];
        for v in &lowered_args {
            call_args.push((DOUBLE, v.as_str()));
        }
        return Ok(blk.call(DOUBLE, &runtime_fn, &call_args));
    }

    bail!(
        "perry-codegen: Call callee shape not supported ({}) with {} args",
        variant_name(callee),
        args.len()
    )
}

/// Lower `new ClassName(args…)` — Phase C.1.
///
/// Strategy: allocate an anonymous object via `js_object_alloc(0, N)`
/// where N is the field count, NaN-box the pointer, then inline the
/// constructor body with:
/// - a fresh local-id-keyed alloca slot for each constructor parameter
///   (pre-populated with the lowered argument value)
/// - a `this_stack` entry pointing at a slot holding the new object
///
/// `Expr::This` then loads from the top of `this_stack`. `this.x = v`
/// goes through the existing `Expr::PropertySet` path which targets
/// `js_object_set_field_by_name`.
///
/// Limitations of this first slice:
/// - No inheritance (parent classes ignored)
/// - No method calls on instances (just field reads/writes via the
///   existing PropertyGet/PropertySet paths)
/// - Constructor cannot use `return <expr>` (would terminate the
///   enclosing function, not the constructor body)
/// - No method dispatch or vtables — those land in Phase C.2/C.3
pub(crate) fn lower_new(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    args: &[Expr],
) -> Result<String> {
    // Built-in Web classes that the runtime provides constructors for.
    // These are checked BEFORE the ctx.classes lookup because the user
    // code may shadow the name — if they do, the class lookup below
    // wins.
    if !ctx.classes.contains_key(class_name) {
        if let Some(val) = lower_builtin_new(ctx, class_name, args)? {
            return Ok(val);
        }
    }

    // Local class alias rerouting: `let C = SomeClass; new C()` lowers
    // as `Expr::New { class_name: "C" }` because the parser sees an
    // Ident callee. The HIR doesn't statically resolve "C" to the
    // underlying class, so without this rerouting we'd fall through to
    // the empty-object placeholder. The Stmt::Let lowering populates
    // `ctx.local_class_aliases[let_name] = class_name` whenever a
    // `let` is initialized from `Expr::ClassRef(class_name)`. We
    // resolve the class name to its underlying real class here and
    // shadow the parameter so the rest of the function uses the
    // resolved name (alloc, ctor lookup, field offsets, etc).
    // Shadow `class_name` with the alias-resolved version. The
    // `resolved_owned` binding outlives the shadowed `&str` because it's
    // declared in the same scope. After this point everything in
    // `lower_new` (alloc, ctor lookup, field offsets, this_stack push)
    // sees the resolved class name and the rest of the function is
    // identical to the direct `new SomeClass()` path.
    let resolved_owned: String;
    let class_name: &str = if !ctx.classes.contains_key(class_name) {
        if let Some(resolved) = ctx.local_class_aliases.get(class_name).cloned() {
            if resolved != class_name {
                resolved_owned = resolved;
                &resolved_owned
            } else {
                class_name
            }
        } else {
            class_name
        }
    } else {
        class_name
    };

    let class = match ctx.classes.get(class_name).copied() {
        Some(c) => c,
        None => {
            // Built-in / native class (Promise, Error, Date, etc.) with
            // no dedicated lower_builtin_new handler — lower args for
            // side effects (closures, string literal interning) and
            // return a sentinel. Real dispatch happens via later
            // NativeMethodCall / PropertyGet paths.
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            // Allocate an empty object as the placeholder.
            let class_id = "0".to_string();
            let count = "0".to_string();
            let handle = ctx
                .block()
                .call(I64, "js_object_alloc", &[(I32, &class_id), (I32, &count)]);
            return Ok(nanbox_pointer_inline(ctx.block(), &handle));
        }
    };

    // Lower the args first (constructor params).
    let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
    for a in args {
        lowered_args.push(lower_expr(ctx, a)?);
    }

    // Compute total field count including inherited parent fields.
    // The runtime allocates at least 8 inline slots regardless, so this
    // mostly matters for shapes >8 fields.
    let mut field_count = class.fields.len() as u32;
    // Imported classes now carry their real field_names from the source
    // module. If the field count is still 0 (no fields info available),
    // use a generous default as a safety net.
    if field_count == 0 && class.constructor.is_none() {
        field_count = 32;
    }
    let mut parent = class.extends_name.as_deref();
    while let Some(parent_name) = parent {
        if let Some(p) = ctx.classes.get(parent_name).copied() {
            field_count += p.fields.len() as u32;
            parent = p.extends_name.as_deref();
        } else {
            break;
        }
    }

    // Allocate the object with the per-class id and (if applicable)
    // parent class id, so the runtime registers the inheritance
    // chain for instanceof / virtual dispatch lookups.
    //
    // Use `js_object_alloc_class_with_keys`, which pre-populates the
    // `keys_array` with the class's field names in declaration order
    // (parent fields first, walking from the deepest ancestor down,
    // then own fields). This is REQUIRED so the LLVM PropertyGet/Set
    // fast path's slot indices match the runtime's by-name dispatch
    // (which walks `keys_array`). Mixing the two access patterns on
    // the same object — e.g. constructor writes via the fast path,
    // PropertyUpdate reads via the runtime helper — only produces
    // consistent results when both agree on the slot mapping.
    //
    // The packed-keys constant is interned via the StringPool. Two
    // classes with the same field-name set + order share one constant.
    let cid = ctx.class_ids.get(class_name).copied().unwrap_or(0);
    let parent_cid = class
        .extends_name
        .as_deref()
        .and_then(|p| ctx.class_ids.get(p).copied())
        .unwrap_or(0);
    let cid_str = cid.to_string();
    let parent_cid_str = parent_cid.to_string();
    let n_str = field_count.to_string();

    // Fast path: if the class has a per-class keys global (built once
    // at module init via `js_build_class_keys_array`), emit INLINE
    // bump-allocator IR — no function call into the runtime at all on
    // the hot path. The runtime exposes a `InlineArenaState` struct
    // (data ptr at offset 0, current bump offset at offset 8, current
    // block size at offset 16) via `js_inline_arena_state()`. We call
    // that ONCE per JS function entry (cached in `arena_state_slot`)
    // and then emit a 5-instruction bump check + GcHeader/ObjectHeader
    // store sequence at every `new ClassName()` site. The slow path
    // (block overflow) calls `js_inline_arena_slow_alloc` which syncs
    // the inline state back to the underlying arena, allocates a new
    // block, and updates the inline state.
    //
    // Cycles per inlined alloc on the M-series fast path:
    //    load offset       (1)
    //    add+and align     (2)
    //    add new_offset    (1)
    //    load size + cmp   (2)
    //    cond br           (predicted, 0)
    //    store offset      (1)
    //    load data + gep   (2)
    //    write GcHeader    (1)  — packed i64 store
    //    write ObjectHeader×2 (2) — packed i64 stores
    //    write keys_ptr    (1)
    //  total: ~13 cycles vs ~140 cycles for the function-call path.
    //
    // Layout assumption: GcHeader is 8 bytes
    //    {obj_type:u8, gc_flags:u8, _reserved:u16, size:u32}
    // and ObjectHeader is 24 bytes
    //    {object_type:u32, class_id:u32, parent_class_id:u32,
    //     field_count:u32, keys_array:*ptr}
    // followed by `max(field_count, 8)` 8-byte field slots. The user
    // pointer the rest of the codegen sees is `raw + 8` (i.e. the
    // ObjectHeader address) — same as what
    // `js_object_alloc_class_inline_keys` returns.
    //
    // Layout constants are duplicated here from the runtime; if
    // `GcHeader` or `ObjectHeader` ever change in
    // `crates/perry-runtime/src/{gc,object}.rs`, update both sides.
    let obj_handle = if let Some(keys_global_name) = ctx.class_keys_globals.get(class_name).cloned() {
        // Compile-time layout constants.
        const GC_HEADER_SIZE: u64 = 8;
        const OBJECT_HEADER_SIZE: u64 = 24;
        const FIELD_SLOT_SIZE: u64 = 8;
        const MIN_FIELD_SLOTS: u64 = 8;
        const GC_TYPE_OBJECT: u64 = 2;
        const GC_FLAG_ARENA: u64 = 0x02;
        const OBJECT_TYPE_REGULAR: u64 = 1;

        let alloc_field_count = std::cmp::max(field_count as u64, MIN_FIELD_SLOTS);
        let payload_size = OBJECT_HEADER_SIZE + alloc_field_count * FIELD_SLOT_SIZE;
        let total_size = GC_HEADER_SIZE + payload_size; // e.g. 96 for any class with ≤8 fields
        let total_size_str = total_size.to_string();

        // Lazy: allocate the per-function arena-state slot on the
        // first `new` we see. The slot init (`call @js_inline_arena_state`
        // + store) lives in the entry block via `entry_init_call_ptr`,
        // so it dominates every reachable use.
        let arena_state_slot = if let Some(slot) = ctx.arena_state_slot.clone() {
            slot
        } else {
            let slot = ctx.func.entry_init_call_ptr("js_inline_arena_state");
            ctx.arena_state_slot = Some(slot.clone());
            slot
        };

        // Hoist the per-class `keys_array` global load to the function
        // entry block (cached in a stack slot per class). Without this
        // hoisting, LLVM would reload `@perry_class_keys_<class>` on
        // every loop iteration, because the loop body's `call
        // @js_inline_arena_slow_alloc` blocks LICM — LLVM can't prove
        // the call doesn't modify the global.
        let keys_slot = if let Some(s) = ctx.class_keys_slots.get(class_name).cloned() {
            s
        } else {
            let s = ctx.func.entry_init_load_global(&keys_global_name, I64);
            ctx.class_keys_slots.insert(class_name.to_string(), s.clone());
            s
        };
        let keys_ptr = ctx.block().load(I64, &keys_slot);

        // Inline bump-allocator IR.
        let blk = ctx.block();
        let state_ptr = blk.load(PTR, &arena_state_slot);

        // offset = state.offset (at byte offset 8 in InlineArenaState).
        // The offset is invariant 8-aligned: arena blocks start at offset 0
        // (8-aligned), every allocation is a multiple of 8 (`total_size`
        // includes the 8-byte GcHeader and `MIN_FIELD_SLOTS=8` slots ×
        // 8 bytes), and `js_inline_arena_slow_alloc` only ever swings the
        // state to `block.offset` which is also always 8-aligned. So we
        // skip the `(offset + 7) & -8` align-up step entirely — saves
        // 2 instructions per iter on the hot path.
        let offset_field_ptr = blk.gep(I8, &state_ptr, &[(I64, "8")]);
        let offset_val = blk.load(I64, &offset_field_ptr);
        let aligned_off = offset_val.clone();

        // new_offset = aligned + total_size
        let new_offset = blk.add(I64, &aligned_off, &total_size_str);

        // size = state.size (at byte offset 16)
        let size_field_ptr = blk.gep(I8, &state_ptr, &[(I64, "16")]);
        let size_val = blk.load(I64, &size_field_ptr);

        // fits = new_offset <= size
        let fits = blk.icmp_ule(I64, &new_offset, &size_val);

        // Set up fast/slow/merge basic blocks.
        let fast_idx = ctx.new_block("alloc.fast");
        let slow_idx = ctx.new_block("alloc.slow");
        let merge_idx = ctx.new_block("alloc.merge");
        let fast_label = ctx.block_label(fast_idx);
        let slow_label = ctx.block_label(slow_idx);
        let merge_label = ctx.block_label(merge_idx);

        ctx.block().cond_br(&fits, &fast_label, &slow_label);

        // ---- Fast path: bump and return data + aligned ----
        ctx.current_block = fast_idx;
        let blk = ctx.block();
        blk.store(I64, &new_offset, &offset_field_ptr);
        // data ptr is at byte offset 0 in InlineArenaState
        let data_ptr = blk.load(PTR, &state_ptr);
        let raw_fast = blk.gep(I8, &data_ptr, &[(I64, &aligned_off)]);
        let fast_pred_label = blk.label.clone();
        blk.br(&merge_label);

        // ---- Slow path: call into the runtime ----
        ctx.current_block = slow_idx;
        let raw_slow = ctx.block().call(
            PTR,
            "js_inline_arena_slow_alloc",
            &[
                (PTR, &state_ptr),
                (I64, &total_size_str),
                (I64, "8"),
            ],
        );
        let slow_pred_label = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        // ---- Merge: phi the raw pointer, write headers, NaN-box ----
        ctx.current_block = merge_idx;
        let blk = ctx.block();
        let raw = blk.phi(
            PTR,
            &[
                (&raw_fast, &fast_pred_label),
                (&raw_slow, &slow_pred_label),
            ],
        );

        // Write GcHeader (8 bytes) as a single i64 store. Field
        // packing (little-endian):
        //   bits  0..7   = obj_type (u8)
        //   bits  8..15  = gc_flags (u8)
        //   bits 16..31  = _reserved (u16)
        //   bits 32..63  = size (u32)
        let gc_packed: u64 = GC_TYPE_OBJECT
            | (GC_FLAG_ARENA << 8)
            | ((total_size as u64) << 32);
        blk.store(I64, &gc_packed.to_string(), &raw);

        // Write ObjectHeader at raw + 8.
        // First 8 bytes: object_type (u32, low) | class_id (u32, high)
        let oh_addr_1 = blk.gep(I8, &raw, &[(I64, "8")]);
        let oh_word_1: u64 = OBJECT_TYPE_REGULAR | ((cid as u64) << 32);
        blk.store(I64, &oh_word_1.to_string(), &oh_addr_1);

        // Second 8 bytes: parent_class_id (u32, low) | field_count (u32, high)
        let oh_addr_2 = blk.gep(I8, &raw, &[(I64, "16")]);
        let oh_word_2: u64 = (parent_cid as u64) | ((field_count as u64) << 32);
        blk.store(I64, &oh_word_2.to_string(), &oh_addr_2);

        // Third 8 bytes: keys_array pointer. The keys_ptr we loaded
        // above is an i64 (carries the ArrayHeader address); store as
        // i64 since the underlying memory is 8 bytes either way.
        let oh_addr_3 = blk.gep(I8, &raw, &[(I64, "24")]);
        blk.store(I64, &keys_ptr, &oh_addr_3);

        // User pointer = raw + 8 (the ObjectHeader address — what the
        // function-call path returned). Convert to i64 to match what
        // the existing nanbox_pointer_inline expects.
        let user_ptr = blk.gep(I8, &raw, &[(I64, "8")]);
        blk.ptrtoint(&user_ptr, I64)
    } else {
        // Fallback: build the packed-keys string at this site and
        // call the slower SHAPE_CACHE-aware allocator. Used when the
        // class isn't in `class_keys_globals` (e.g. anonymous /
        // synthetic classes that compile_module doesn't pre-emit a
        // global for).
        let mut packed_keys = String::new();
        let mut parent_chain: Vec<&perry_hir::Class> = Vec::new();
        let mut p = class.extends_name.as_deref();
        while let Some(parent_name) = p {
            if let Some(pc) = ctx.classes.get(parent_name).copied() {
                parent_chain.push(pc);
                p = pc.extends_name.as_deref();
            } else {
                break;
            }
        }
        for pc in parent_chain.iter().rev() {
            for f in &pc.fields {
                packed_keys.push_str(&f.name);
                packed_keys.push('\0');
            }
        }
        for f in &class.fields {
            packed_keys.push_str(&f.name);
            packed_keys.push('\0');
        }
        let keys_idx = ctx.strings.intern(&packed_keys);
        let keys_entry = ctx.strings.entry(keys_idx);
        let keys_global = format!("@{}", keys_entry.bytes_global);
        let keys_len_str = keys_entry.byte_len.to_string();

        ctx.block().call(
            I64,
            "js_object_alloc_class_with_keys",
            &[
                (I32, &cid_str),
                (I32, &parent_cid_str),
                (I32, &n_str),
                (PTR, &keys_global),
                (I32, &keys_len_str),
            ],
        )
    };
    let obj_box = nanbox_pointer_inline(ctx.block(), &obj_handle);

    // Allocate a `this` slot and store the new object there. The
    // slot lives on this_stack for the duration of the inlined ctor
    // body (which may span many basic blocks and contain nested
    // closures that capture `this`), so hoist to the entry block for
    // dominance safety.
    let this_slot = ctx.func.alloca_entry(DOUBLE);
    ctx.block().store(DOUBLE, &obj_box, &this_slot);
    ctx.this_stack.push(this_slot);
    ctx.class_stack.push(class_name.to_string());

    // Apply field initializers FIRST — TypeScript / ES2022 semantics:
    // class field initializers run at the start of the constructor body
    // (after super() for derived classes, before any user ctor code).
    // Walk the parent chain from the root down so parent fields are
    // initialized before the child's fields.
    apply_field_initializers_recursive(ctx, class_name)?;

    // If there's a constructor, inline its body. We allocate slots for
    // each constructor parameter and pre-populate them with the lowered
    // argument values. Locals/local_types are saved and restored to keep
    // the constructor's bindings scoped to its body — they don't leak
    // back into the enclosing function.
    if let Some(ctor) = &class.constructor {
        let saved_locals = ctx.locals.clone();
        let saved_local_types = ctx.local_types.clone();

        for (param, arg_val) in ctor.params.iter().zip(lowered_args.iter()) {
            // Ctor params become ctx.locals for the inlined body;
            // closures inside the ctor may capture them, so hoist
            // to the entry block.
            let slot = ctx.func.alloca_entry(DOUBLE);
            ctx.block().store(DOUBLE, arg_val, &slot);
            ctx.locals.insert(param.id, slot);
            ctx.local_types.insert(param.id, param.ty.clone());
        }

        // Lower the constructor body. Errors propagate.
        crate::stmt::lower_stmts(ctx, &ctor.body)?;

        // Restore the enclosing function's local scope.
        ctx.locals = saved_locals;
        ctx.local_types = saved_local_types;
    } else {
        // No own constructor — walk the parent chain to find an
        // inherited constructor and inline it. TypeScript semantics:
        // `class Child extends Parent {}` auto-forwards constructor
        // arguments to the parent constructor.
        let mut parent_name = class.extends_name.as_deref();
        while let Some(pname) = parent_name {
            if let Some(parent_class) = ctx.classes.get(pname).copied() {
                if let Some(parent_ctor) = &parent_class.constructor {
                    let saved_locals = ctx.locals.clone();
                    let saved_local_types = ctx.local_types.clone();

                    // Map constructor params from the parent's ctor to
                    // the supplied args. If caller passed fewer args
                    // than the parent expects, extra params get
                    // undefined.
                    for (i, param) in parent_ctor.params.iter().enumerate() {
                        // Parent-ctor params become ctx.locals for the
                        // inlined body; capturable by nested closures,
                        // so hoist to the entry block.
                        let slot = ctx.func.alloca_entry(DOUBLE);
                        if i < lowered_args.len() {
                            ctx.block().store(DOUBLE, &lowered_args[i], &slot);
                        } else {
                            let undef = crate::nanbox::double_literal(
                                f64::from_bits(crate::nanbox::TAG_UNDEFINED),
                            );
                            ctx.block().store(DOUBLE, &undef, &slot);
                        }
                        ctx.locals.insert(param.id, slot);
                        ctx.local_types.insert(param.id, param.ty.clone());
                    }

                    // Push the parent class name so `this` inside the
                    // parent ctor body resolves field names via the
                    // parent's field list.
                    ctx.class_stack.pop();
                    ctx.class_stack.push(pname.to_string());

                    crate::stmt::lower_stmts(ctx, &parent_ctor.body)?;

                    // Restore class_stack to the child.
                    ctx.class_stack.pop();
                    ctx.class_stack.push(class_name.to_string());

                    ctx.locals = saved_locals;
                    ctx.local_types = saved_local_types;
                    break; // Found and inlined the parent ctor.
                }
                parent_name = parent_class.extends_name.as_deref();
            } else {
                break;
            }
        }
        // If no parent constructor was found (imported class with no
        // inlineable constructor body), call the cross-module constructor.
        if let Some((ctor_name, param_count)) = ctx.imported_class_ctors.get(class_name).cloned() {
            // Pad missing optional args with TAG_UNDEFINED so the constructor
            // doesn't read garbage from stale registers.
            let undef_lit = crate::nanbox::double_literal(f64::from_bits(
                crate::nanbox::TAG_UNDEFINED,
            ));
            while lowered_args.len() < param_count {
                lowered_args.push(undef_lit.clone());
            }
            // Pass `this` as NaN-boxed double (same as compile_method's this_arg).
            let mut ctor_args: Vec<(crate::types::LlvmType, &str)> = Vec::with_capacity(1 + lowered_args.len());
            ctor_args.push((DOUBLE, &obj_box));
            let ctor_param_types: Vec<crate::types::LlvmType> = std::iter::once(DOUBLE)
                .chain(lowered_args.iter().map(|_| DOUBLE))
                .collect();
            for la in &lowered_args {
                ctor_args.push((DOUBLE, la.as_str()));
            }
            ctx.pending_declares.push((ctor_name.clone(), crate::types::VOID, ctor_param_types));
            ctx.block().call_void(&ctor_name, &ctor_args);
        }
    }

    ctx.this_stack.pop();
    ctx.class_stack.pop();
    Ok(obj_box)
}

/// Walk the inheritance chain from the root down and apply each class's
/// field initializers to `this`. Call this inside `lower_new` after the
/// `this` slot is pushed but before the constructor body is inlined.
///
/// Initializers run in declaration order: root parent first, then each
/// child, matching JavaScript / TypeScript class semantics where fields
/// are initialized before user-written constructor code executes (field
/// initializers are conceptually prepended to the constructor body).
/// Public entry point for scalar-replacement path in stmt.rs.
pub(crate) fn apply_field_initializers_recursive_pub(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
) -> Result<()> {
    apply_field_initializers_recursive(ctx, class_name)
}

fn apply_field_initializers_recursive(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
) -> Result<()> {
    // Collect the inheritance chain from root down.
    let mut chain: Vec<String> = Vec::new();
    let mut cur = Some(class_name.to_string());
    while let Some(c) = cur {
        let Some(class) = ctx.classes.get(&c).copied() else { break };
        chain.push(c.clone());
        cur = class.extends_name.clone();
    }
    chain.reverse();

    for class_name_in_chain in chain {
        let class = match ctx.classes.get(&class_name_in_chain).copied() {
            Some(c) => c,
            None => continue,
        };
        // Collect (property_name, init_expr) pairs up-front to avoid
        // holding an immutable borrow of ctx.classes across lower_expr.
        let mut init_pairs: Vec<(String, Expr)> = Vec::new();
        for field in &class.fields {
            if let Some(init) = &field.init {
                init_pairs.push((field.name.clone(), init.clone()));
            }
        }
        if init_pairs.is_empty() {
            continue;
        }

        // Temporarily swap class_stack so `this.field` in the init
        // resolves against the correct class.
        ctx.class_stack.push(class_name_in_chain.clone());
        for (prop, init_expr) in init_pairs {
            // Build a PropertySet { this, prop, init_expr } and lower.
            let set_expr = Expr::PropertySet {
                object: Box::new(Expr::This),
                property: prop,
                value: Box::new(init_expr),
            };
            let _ = lower_expr(ctx, &set_expr)?;
        }
        ctx.class_stack.pop();
    }
    Ok(())
}

/// Lower a `NativeMethodCall { module, method, object, args }` (Phase H.1).
///
/// Currently supports:
/// - `array.push_single` / `array.push` (single-arg push) on typed arrays
/// - `array.pop_back` / `array.pop` on typed arrays
///
/// The receiver is either a `PropertyGet { object, property }` (the
/// `this.items.push(x)` case) or a `LocalGet` (the `arr.push(x)` case).
/// For both shapes we chain a get + push + write-back so reallocations
/// are reflected in the source storage.
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
    // perry/ui VStack/HStack — special-case because the TS shape is
    // `VStack(spacing, [child1, child2, ...])` (or just `VStack([...])`),
    // but the runtime takes only `(spacing) -> handle` and children get
    // added one by one via `perry_ui_widget_add_child`. We can't express
    // this with the per-method table because it's variadic in arg shape
    // *and* needs sequential calls per child.
    if module == "perry/ui"
        && (method == "VStack" || method == "HStack")
        && object.is_none()
    {
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
        ctx.pending_declares.push((
            runtime_create.to_string(),
            I64,
            vec![DOUBLE],
        ));
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
                // Children expression isn't a literal array — lower for
                // side effects so closures still get collected.
                let _ = lower_expr(ctx, children_expr)?;
            }
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
        ctx.pending_declares.push((
            "perry_ui_vstack_create".to_string(),
            I64,
            vec![DOUBLE],
        ));
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
            &[(I64, &container_reload), (I64, &state_handle), (DOUBLE, &closure_d)],
        );

        let blk = ctx.block();
        let container_final = blk.load(I64, &container_slot);
        return Ok(nanbox_pointer_inline(blk, &container_final));
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
        ctx.pending_declares.push((
            "perry_ui_button_create".to_string(),
            I64,
            vec![I64, DOUBLE],
        ));
        let blk = ctx.block();
        let handle = blk.call(
            I64,
            "perry_ui_button_create",
            &[(I64, &label_ptr), (DOUBLE, &handler_d)],
        );
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
    // perry/system dispatch: audioStart, audioGetLevel, getDeviceModel, etc.
    if module == "perry/system" && object.is_none() {
        if let Some(sig) = perry_system_table_lookup(method) {
            return lower_perry_ui_table_call(ctx, sig, args);
        }
    }

    if module == "perry/ui"
        && object.is_none()
        && method != "App"
        && method != "VStack"
        && method != "HStack"
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
        blk.call_void(
            "perry_ui_app_set_body",
            &[(I64, &app_handle), (I64, &body_handle)],
        );
        blk.call_void("perry_ui_app_run", &[(I64, &app_handle)]);
        return Ok(double_literal(0.0));
    }

    // fs module functions: readdirSync, statSync, mkdirSync, etc.
    // These are receiver-less NativeMethodCalls (`import { readdirSync }
    // from 'fs'` → `NativeMethodCall { module: "fs", object: None }`).
    // Dispatch before the catch-all so they call the runtime instead of
    // returning TAG_UNDEFINED.
    if module == "fs" && object.is_none() {
        match method {
            "readdirSync" if args.len() >= 1 => {
                let p = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let raw = blk.call(DOUBLE, "js_fs_readdir_sync", &[(DOUBLE, &p)]);
                let raw_bits = blk.bitcast_double_to_i64(&raw);
                return Ok(nanbox_pointer_inline(blk, &raw_bits));
            }
            "statSync" if args.len() >= 1 => {
                let p = lower_expr(ctx, &args[0])?;
                return Ok(ctx.block().call(DOUBLE, "js_fs_stat_sync", &[(DOUBLE, &p)]));
            }
            "renameSync" if args.len() >= 2 => {
                let from = lower_expr(ctx, &args[0])?;
                let to = lower_expr(ctx, &args[1])?;
                ctx.block().call_void("js_fs_rename_sync", &[(DOUBLE, &from), (DOUBLE, &to)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "unlinkSync" if args.len() >= 1 => {
                let p = lower_expr(ctx, &args[0])?;
                ctx.block().call_void("js_fs_unlink_sync", &[(DOUBLE, &p)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "mkdirSync" if args.len() >= 1 => {
                let p = lower_expr(ctx, &args[0])?;
                ctx.block().call_void("js_fs_mkdir_sync", &[(DOUBLE, &p)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "rmdirSync" if args.len() >= 1 => {
                let p = lower_expr(ctx, &args[0])?;
                ctx.block().call_void("js_fs_rmdir_sync", &[(DOUBLE, &p)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "copyFileSync" if args.len() >= 2 => {
                let src = lower_expr(ctx, &args[0])?;
                let dst = lower_expr(ctx, &args[1])?;
                ctx.block().call_void("js_fs_copy_file_sync", &[(DOUBLE, &src), (DOUBLE, &dst)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            _ => {
                // Fall through — readFileSync/writeFileSync/existsSync/etc.
                // are handled as dedicated HIR Expr variants, not
                // NativeMethodCall. Warn on truly unhandled ones.
                eprintln!("perry-codegen: unhandled fs.{}() NativeMethodCall ({})", method, args.len());
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
                    "spawn" => args.get(0),
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
                        Expr::FuncRef(_)
                        | Expr::LocalGet(_)
                        | Expr::ExternFuncRef { .. } => {
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
            let mut llvm_args: Vec<(crate::types::LlvmType, String)> = Vec::with_capacity(1 + args.len());
            let mut runtime_param_types: Vec<crate::types::LlvmType> = Vec::with_capacity(1 + args.len());
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
                UiReturnKind::Widget => I64,
                UiReturnKind::F64 => DOUBLE,
                UiReturnKind::Void => crate::types::VOID,
            };
            ctx.pending_declares.push((sig.runtime.to_string(), return_type, runtime_param_types));
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
                UiReturnKind::F64 => {
                    Ok(blk.call(DOUBLE, sig.runtime, &ref_args))
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

    if module == "array" && (method == "push_single" || method == "push") {
        if args.is_empty() {
            bail!("array.push expects ≥1 arg, got 0");
        }
        // Lower every argument first so closures and string literals get
        // collected, then lower the receiver once. js_array_push_f64 may
        // realloc on each call, so we thread the returned pointer through
        // and write the final pointer back to the receiver.
        let mut lowered: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered.push(lower_expr(ctx, a)?);
        }
        let arr_box = lower_expr(ctx, recv)?;
        let blk = ctx.block();
        let mut arr_handle = unbox_to_i64(blk, &arr_box);
        for v in &lowered {
            let blk = ctx.block();
            arr_handle = blk.call(
                I64,
                "js_array_push_f64",
                &[(I64, &arr_handle), (DOUBLE, v)],
            );
        }
        let blk = ctx.block();
        let new_handle = arr_handle;
        let new_box = nanbox_pointer_inline(blk, &new_handle);
        // Write the (possibly-realloc'd) pointer back to the receiver.
        // Two cases:
        //   1. recv = LocalGet(id) → store back to the local's slot
        //   2. recv = PropertyGet { obj, prop } → set obj.prop = new_box
        // Anything else: skip the write-back (the array may dangle on
        // realloc, but we don't crash at codegen).
        match recv {
            Expr::LocalGet(id) => {
                if let Some(slot) = ctx.locals.get(id).cloned() {
                    ctx.block().store(DOUBLE, &new_box, &slot);
                } else if let Some(global_name) = ctx.module_globals.get(id).cloned() {
                    let g_ref = format!("@{}", global_name);
                    ctx.block().store(DOUBLE, &new_box, &g_ref);
                }
            }
            Expr::PropertyGet { object: obj_expr, property } => {
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
            _ => {
                // No write-back — the receiver is some computed value.
                // The array may dangle on realloc, but the immediate
                // call sees the right pointer.
            }
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

/// Extract a raw string pointer (i64) from a NaN-boxed JSValue via the
/// unified helper. Handles string literals, concat results, and any
/// other expression that produces a NaN-boxed double.
fn get_raw_string_ptr(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<String> {
    let v = lower_expr(ctx, e)?;
    let blk = ctx.block();
    Ok(blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &v)]))
}

/// Build a Headers handle from an inline object literal `{ "k": "v", ... }`.
/// Returns the f64 handle (raw numeric, not NaN-boxed).
fn build_headers_from_object(
    ctx: &mut FnCtx<'_>,
    props: &[(String, Expr)],
) -> Result<String> {
    let h = ctx.block().call(DOUBLE, "js_headers_new", &[]);
    for (k, vexpr) in props {
        let key_expr = Expr::String(k.clone());
        let key_ptr = get_raw_string_ptr(ctx, &key_expr)?;
        let val_ptr = get_raw_string_ptr(ctx, vexpr)?;
        ctx.block().call(
            DOUBLE,
            "js_headers_set",
            &[(DOUBLE, &h), (I64, &key_ptr), (I64, &val_ptr)],
        );
    }
    Ok(h)
}

/// Phase 3 compat: extract `{key: value, ...}` pairs from an options
/// argument in a form that works whether the options literal reached us
/// as a plain `Expr::Object(props)` (pre-Phase-3 / spread/dynamic shapes)
/// or as an `Expr::New { class_name: "__AnonShape_N", args }` (Phase 3's
/// closed-shape synthesis path). For the anon-class form, `ctx.classes`
/// carries the class with its synthesized constructor — we pair each
/// constructor param name with its positional arg to recover the literal's
/// (key, value) view.
///
/// Returns `None` when the expression is neither shape — callers should
/// fall through to whatever they did before when the 2nd arg wasn't an
/// inline object.
pub(crate) fn extract_options_fields(
    ctx: &FnCtx<'_>,
    e: &Expr,
) -> Option<Vec<(String, Expr)>> {
    match e {
        Expr::Object(props) => Some(props.clone()),
        Expr::New { class_name, args, .. } if class_name.starts_with("__AnonShape_") => {
            let class = ctx.classes.get(class_name)?;
            let ctor = class.constructor.as_ref()?;
            if ctor.params.len() != args.len() {
                return None;
            }
            let pairs: Vec<(String, Expr)> = ctor.params.iter()
                .zip(args.iter())
                .map(|(param, arg)| (param.name.clone(), arg.clone()))
                .collect();
            Some(pairs)
        }
        _ => None,
    }
}

/// Lower `new ClassName(args)` for the built-in Web classes that don't
/// live in `ctx.classes`. Returns `Ok(None)` if the class isn't one we
/// handle here (caller should fall through to the default path).
pub(crate) fn lower_builtin_new(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    args: &[Expr],
) -> Result<Option<String>> {
    match class_name {
        "Array" => {
            // `new Array()` → empty array, `new Array(n)` → length-n array
            // (zero-initialized slots), `new Array(a, b, c)` → 3-element array
            // [a, b, c]. We handle the no-arg and single-numeric-arg cases
            // here. Multi-arg / non-numeric single arg falls back to the
            // generic Expr::New path.
            let blk = ctx.block();
            let handle = if args.is_empty() {
                blk.call(I64, "js_array_create", &[])
            } else if args.len() == 1 {
                let cap = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let cap_i32 = blk.fptosi(DOUBLE, &cap, I32);
                blk.call(I64, "js_array_alloc_with_length", &[(I32, &cap_i32)])
            } else {
                return Ok(None);
            };
            let blk = ctx.block();
            return Ok(Some(nanbox_pointer_inline(blk, &handle)));
        }
        "Response" => {
            // new Response(body?, init?) — init = { status?, statusText?, headers? }
            let body_ptr = if !args.is_empty() {
                get_raw_string_ptr(ctx, &args[0])?
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
                    // Not an object literal — still evaluate for side effects.
                    let _ = lower_expr(ctx, &args[1])?;
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

        "Headers" => {
            // new Headers(init?) — init can be an object literal or another
            // Headers/array iterable. Only inline object literals are
            // handled so far; anything else falls back to empty.
            let h = ctx.block().call(DOUBLE, "js_headers_new", &[]);
            if !args.is_empty() {
                if let Some(props) = extract_options_fields(ctx, &args[0]) {
                    for (k, vexpr) in &props {
                        let key_expr = Expr::String(k.clone());
                        let key_ptr = get_raw_string_ptr(ctx, &key_expr)?;
                        let val_ptr = get_raw_string_ptr(ctx, vexpr)?;
                        ctx.block().call(
                            DOUBLE,
                            "js_headers_set",
                            &[(DOUBLE, &h), (I64, &key_ptr), (I64, &val_ptr)],
                        );
                    }
                } else {
                    let _ = lower_expr(ctx, &args[0])?;
                }
            }
            Ok(Some(h))
        }

        "Request" => {
            // new Request(url, init?) — init = { method?, body?, headers? }
            let url_ptr = if !args.is_empty() {
                get_raw_string_ptr(ctx, &args[0])?
            } else {
                "0".to_string()
            };

            let mut method_ptr = "0".to_string();
            let mut body_ptr = "0".to_string();
            let mut headers_handle = "0.0".to_string();

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
                ],
            );
            Ok(Some(handle))
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
            let p = blk.call(
                I64,
                "js_promise_new_with_executor",
                &[(I64, &exec_handle)],
            );
            Ok(Some(nanbox_pointer_inline(blk, &p)))
        }
        "WeakMap" => {
            // Lower init iterable args for side effects; the runtime's
            // js_weakmap_new takes no args and the HIR lowering of
            // `.set(k,v)` calls dispatch on the resulting handle.
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let handle = ctx.block().call(I64, "js_weakmap_new", &[]);
            // js_weakmap_new returns a raw `*mut ObjectHeader` — NaN-box
            // with POINTER_TAG so subsequent `js_weakmap_*` calls can
            // `js_nanbox_get_pointer` on the f64.
            let boxed = nanbox_pointer_inline(ctx.block(), &handle);
            Ok(Some(boxed))
        }
        "WeakSet" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let handle = ctx.block().call(I64, "js_weakset_new", &[]);
            let boxed = nanbox_pointer_inline(ctx.block(), &handle);
            Ok(Some(boxed))
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
            ctx.pending_declares.push((
                "js_ws_server_new".to_string(),
                I64,
                vec![DOUBLE],
            ));
            let blk = ctx.block();
            let handle = blk.call(I64, "js_ws_server_new", &[(DOUBLE, &opts)]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }

        _ => Ok(None),
    }
}

/// Returns `true` if the expression statically resolves to an
/// `AbortController`-typed value (either a local whose declared type
/// is `Named("AbortController")` or a `new AbortController()` call).
fn is_abort_controller_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::New { class_name, .. } => class_name == "AbortController",
        Expr::LocalGet(id) => matches!(
            ctx.local_types.get(id),
            Some(HirType::Named(n)) if n == "AbortController"
        ),
        _ => false,
    }
}

/// Lower AbortController / AbortSignal method calls:
/// - `controller.abort(reason?)`
/// - `controller.signal.addEventListener("abort", cb)`
/// - `AbortSignal.timeout(ms)` (static)
///
/// Returns `None` if the call shape doesn't match one of the handled
/// patterns — caller falls through to the generic dispatch.
fn lower_abort_controller_call(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
) -> Result<Option<String>> {
    // ── AbortSignal.timeout(ms) static ──
    if property == "timeout" {
        if let Expr::GlobalGet(_) = object {
            // Can't distinguish AbortSignal.timeout from other globals
            // without more context — skip.
        }
    }
    // Static `AbortSignal.timeout(ms)` — matched via a PropertyGet on a
    // GlobalGet-shaped object isn't quite right because GlobalGet has
    // no name; best we can do is detect by property name "timeout" and
    // the local-isn't-a-known-thing. Skip for now.

    // ── controller.abort(reason?) ──
    if property == "abort" && is_abort_controller_expr(ctx, object) {
        let recv_box = lower_expr(ctx, object)?;
        let blk = ctx.block();
        let ctrl_handle = unbox_to_i64(blk, &recv_box);
        if args.is_empty() {
            blk.call_void("js_abort_controller_abort", &[(I64, &ctrl_handle)]);
        } else {
            let reason = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            blk.call_void(
                "js_abort_controller_abort_reason",
                &[(I64, &ctrl_handle), (DOUBLE, &reason)],
            );
        }
        return Ok(Some(double_literal(f64::from_bits(
            crate::nanbox::TAG_UNDEFINED,
        ))));
    }

    // ── controller.signal.addEventListener("abort", cb) ──
    if property == "addEventListener" && args.len() >= 2 {
        if let Expr::PropertyGet {
            object: inner_obj,
            property: inner_prop,
        } = object
        {
            if inner_prop == "signal" && is_abort_controller_expr(ctx, inner_obj) {
                let ctrl_box = lower_expr(ctx, inner_obj)?;
                let blk = ctx.block();
                let ctrl_handle = unbox_to_i64(blk, &ctrl_box);
                // Get the signal pointer.
                let signal_handle = blk.call(
                    I64,
                    "js_abort_controller_signal",
                    &[(I64, &ctrl_handle)],
                );
                let evt = lower_expr(ctx, &args[0])?;
                let listener = lower_expr(ctx, &args[1])?;
                let blk = ctx.block();
                blk.call_void(
                    "js_abort_signal_add_listener",
                    &[(I64, &signal_handle), (DOUBLE, &evt), (DOUBLE, &listener)],
                );
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
        }
    }

    Ok(None)
}

/// Dispatch for the Web Fetch API family: Response/Headers/Request
/// methods and property getters. Called before the generic
/// `lower_native_method_call` path so static factories
/// (`Response.json(v)`) also land here. Returns `Ok(None)` if the
/// (module, method) combination isn't handled.
///
/// Handle ABI note: Response/Headers/Request handles are plain numeric
/// doubles (ids into the runtime's registry), not NaN-boxed pointers.
/// Most runtime functions take the handle as f64; status/statusText/
/// ok/text/json take i64 and we convert via `fptosi`.
fn lower_fetch_native_method(
    ctx: &mut FnCtx<'_>,
    module: &str,
    method: &str,
    object: Option<&Expr>,
    args: &[Expr],
) -> Result<Option<String>> {
    // ── Response static factories (no receiver) ──
    if module == "fetch" && object.is_none() {
        match method {
            "static_json" => {
                let v = if !args.is_empty() {
                    lower_expr(ctx, &args[0])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let handle = ctx
                    .block()
                    .call(DOUBLE, "js_response_static_json", &[(DOUBLE, &v)]);
                return Ok(Some(handle));
            }
            "static_redirect" => {
                let url_ptr = if !args.is_empty() {
                    get_raw_string_ptr(ctx, &args[0])?
                } else {
                    "0".to_string()
                };
                let status = if args.len() >= 2 {
                    lower_expr(ctx, &args[1])?
                } else {
                    "302.0".to_string()
                };
                let handle = ctx.block().call(
                    DOUBLE,
                    "js_response_static_redirect",
                    &[(I64, &url_ptr), (DOUBLE, &status)],
                );
                return Ok(Some(handle));
            }
            _ => {}
        }
    }

    // ── axios: static method calls (axios.get/post/put/delete/patch) ──
    // Must be before the receiver guard — these are receiver-less calls.
    if module == "axios" && object.is_none() {
        let url_box = if !args.is_empty() { lower_expr(ctx, &args[0])? } else {
            double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
        };
        let blk = ctx.block();
        let url_handle = unbox_to_i64(blk, &url_box);
        match method {
            "get" => {
                let promise = blk.call(I64, "js_axios_get", &[(I64, &url_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "delete" => {
                let promise = blk.call(I64, "js_axios_delete", &[(I64, &url_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "post" | "put" | "patch" => {
                let body_box = if args.len() > 1 { lower_expr(ctx, &args[1])? } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let body_handle = unbox_to_i64(ctx.block(), &body_box);
                let rt_fn = match method {
                    "post" => "js_axios_post",
                    "put" => "js_axios_put",
                    _ => "js_axios_patch",
                };
                let promise = ctx.block().call(I64, rt_fn, &[(I64, &url_handle), (I64, &body_handle)]);
                return Ok(Some(nanbox_pointer_inline(ctx.block(), &promise)));
            }
            _ => {}
        }
    }

    // Everything below needs a receiver.
    let Some(recv) = object else {
        return Ok(None);
    };

    // ── Headers method dispatch ──
    if module == "Headers" {
        let h_handle = lower_expr(ctx, recv)?;
        match method {
            "set" | "append" => {
                if args.len() < 2 {
                    return Ok(Some(double_literal(0.0)));
                }
                let key_ptr = get_raw_string_ptr(ctx, &args[0])?;
                let val_ptr = get_raw_string_ptr(ctx, &args[1])?;
                ctx.block().call(
                    DOUBLE,
                    "js_headers_set",
                    &[(DOUBLE, &h_handle), (I64, &key_ptr), (I64, &val_ptr)],
                );
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            "get" => {
                if args.is_empty() {
                    return Ok(Some(double_literal(0.0)));
                }
                let key_ptr = get_raw_string_ptr(ctx, &args[0])?;
                let str_ptr = ctx.block().call(
                    I64,
                    "js_headers_get",
                    &[(DOUBLE, &h_handle), (I64, &key_ptr)],
                );
                let blk = ctx.block();
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "has" => {
                if args.is_empty() {
                    return Ok(Some(double_literal(f64::from_bits(
                        crate::nanbox::TAG_FALSE,
                    ))));
                }
                let key_ptr = get_raw_string_ptr(ctx, &args[0])?;
                let out = ctx.block().call(
                    DOUBLE,
                    "js_headers_has",
                    &[(DOUBLE, &h_handle), (I64, &key_ptr)],
                );
                return Ok(Some(out));
            }
            "delete" => {
                if args.is_empty() {
                    return Ok(Some(double_literal(f64::from_bits(
                        crate::nanbox::TAG_UNDEFINED,
                    ))));
                }
                let key_ptr = get_raw_string_ptr(ctx, &args[0])?;
                ctx.block().call(
                    DOUBLE,
                    "js_headers_delete",
                    &[(DOUBLE, &h_handle), (I64, &key_ptr)],
                );
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            "forEach" => {
                if args.is_empty() {
                    return Ok(Some(double_literal(0.0)));
                }
                let cb = lower_expr(ctx, &args[0])?;
                ctx.block().call(
                    DOUBLE,
                    "js_headers_for_each",
                    &[(DOUBLE, &h_handle), (DOUBLE, &cb)],
                );
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            _ => return Ok(None),
        }
    }

    // ── Request property getters ──
    if module == "Request" {
        let h_handle = lower_expr(ctx, recv)?;
        match method {
            "url" => {
                let str_ptr = ctx
                    .block()
                    .call(I64, "js_request_get_url", &[(DOUBLE, &h_handle)]);
                let blk = ctx.block();
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "method" => {
                let str_ptr = ctx
                    .block()
                    .call(I64, "js_request_get_method", &[(DOUBLE, &h_handle)]);
                let blk = ctx.block();
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "body" => {
                let val = ctx
                    .block()
                    .call(DOUBLE, "js_request_get_body", &[(DOUBLE, &h_handle)]);
                return Ok(Some(val));
            }
            _ => return Ok(None),
        }
    }

    // ── Response methods / property getters ──
    if module == "fetch" {
        // Lower the receiver once. It may be a Response (f64 handle) or
        // a chained result from `.headers` / `.clone()` — in the former
        // case we dispatch the methods here; the chain cases are
        // recognised at the Call callsite in lower_call.
        let recv_handle = lower_expr(ctx, recv)?;
        match method {
            "text" => {
                let blk = ctx.block();
                let h_i64 = blk.fptosi(DOUBLE, &recv_handle, I64);
                let promise = blk.call(I64, "js_fetch_response_text", &[(I64, &h_i64)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "json" => {
                let blk = ctx.block();
                let h_i64 = blk.fptosi(DOUBLE, &recv_handle, I64);
                let promise = blk.call(I64, "js_fetch_response_json", &[(I64, &h_i64)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "status" => {
                let blk = ctx.block();
                let h_i64 = blk.fptosi(DOUBLE, &recv_handle, I64);
                let status = blk.call(DOUBLE, "js_fetch_response_status", &[(I64, &h_i64)]);
                return Ok(Some(status));
            }
            "statusText" => {
                let blk = ctx.block();
                let h_i64 = blk.fptosi(DOUBLE, &recv_handle, I64);
                let str_ptr =
                    blk.call(I64, "js_fetch_response_status_text", &[(I64, &h_i64)]);
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "ok" => {
                // js_fetch_response_ok returns 1.0 or 0.0 as f64. Map to
                // TAG_TRUE/TAG_FALSE so console.log prints "true"/"false".
                let blk = ctx.block();
                let h_i64 = blk.fptosi(DOUBLE, &recv_handle, I64);
                let raw = blk.call(DOUBLE, "js_fetch_response_ok", &[(I64, &h_i64)]);
                let cmp = blk.fcmp("une", &raw, "0.0");
                let tagged = blk.select(
                    crate::types::I1,
                    &cmp,
                    I64,
                    crate::nanbox::TAG_TRUE_I64,
                    crate::nanbox::TAG_FALSE_I64,
                );
                return Ok(Some(blk.bitcast_i64_to_double(&tagged)));
            }
            "headers" => {
                let out = ctx.block().call(
                    DOUBLE,
                    "js_response_get_headers",
                    &[(DOUBLE, &recv_handle)],
                );
                return Ok(Some(out));
            }
            "clone" => {
                let out = ctx
                    .block()
                    .call(DOUBLE, "js_response_clone", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(out));
            }
            "arrayBuffer" => {
                let blk = ctx.block();
                let promise =
                    blk.call(I64, "js_response_array_buffer", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "blob" => {
                let blk = ctx.block();
                let promise = blk.call(I64, "js_response_blob", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            _ => return Ok(None),
        }
    }

    // ── axios: response property access (response.status, .data, .statusText, .headers) ──
    if module == "axios" {
        if let Some(recv) = object {
            let recv_handle = lower_expr(ctx, recv)?;
            let blk = ctx.block();
            // The awaited axios response is a Handle (i64) stored in f64 bits
            // via f64::from_bits(handle as u64). Use bitcast, not fptosi —
            // fptosi interprets the f64 as a number (5e-324 for handle=1)
            // and truncates to 0.
            let h_i64 = blk.bitcast_double_to_i64(&recv_handle);
            match method {
                "status" => {
                    let status = blk.call(DOUBLE, "js_axios_response_status", &[(I64, &h_i64)]);
                    return Ok(Some(status));
                }
                "statusText" => {
                    let str_ptr = blk.call(I64, "js_axios_response_status_text", &[(I64, &h_i64)]);
                    return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
                }
                "data" => {
                    let str_ptr = blk.call(I64, "js_axios_response_data", &[(I64, &h_i64)]);
                    return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
                }
                _ => {}
            }
        }
    }

    Ok(None)
}

// =============================================================================
// perry/ui generic dispatch table
// =============================================================================

/// How a perry/ui FFI function expects each argument to be passed.
#[derive(Copy, Clone, Debug)]
enum UiArgKind {
    /// Widget handle: lower the JSValue, unbox the POINTER bits as i64.
    /// Used for the `handle` first arg of every setter, plus child / parent
    /// handle args. The runtime gets the raw 1-based widget handle.
    Widget,
    /// String pointer: lower the JSValue, then call
    /// `js_get_string_pointer_unified` to extract the underlying StringHeader
    /// pointer as i64. Handles both literal strings and runtime-built ones.
    Str,
    /// Raw f64 number. The JSValue is already a NaN-boxed double for numbers,
    /// so we pass it as-is. Used for sizes, colors, weights, alignment ids.
    F64,
    /// Closure handle: lower the JSValue (which is a `js_closure_alloc`
    /// pointer NaN-boxed as POINTER) and pass it as a raw f64. The runtime
    /// extracts the closure pointer via the same NaN-boxing convention.
    Closure,
    /// Raw i64 (rare; some setters take an enum tag as i64).
    I64Raw,
}

/// What the perry/ui FFI function returns and how to box it.
#[derive(Copy, Clone, Debug)]
enum UiReturnKind {
    /// Widget handle: NaN-box the i64 result with POINTER_TAG.
    Widget,
    /// Raw f64: pass through unchanged. Used by `scrollviewGetOffset` etc.
    F64,
    /// Void return: emit `call void` and return the `0.0` sentinel f64.
    Void,
}

#[derive(Copy, Clone, Debug)]
struct UiSig {
    /// TypeScript method name as it appears in the import (e.g. "Text",
    /// "textSetFontSize"). Matched against `method` from
    /// `lower_native_method_call` for `module == "perry/ui"`.
    method: &'static str,
    /// `perry_ui_*` runtime function symbol. Lazily declared via
    /// `pending_declares` so the linker picks it up from
    /// `libperry_ui_macos.a` (or the equivalent platform-specific lib).
    runtime: &'static str,
    /// Per-argument coercion rules. Length must equal `args.len()` at
    /// the call site, otherwise the dispatch falls through to the
    /// receiver-less early-out (which lowers everything as side effects
    /// and returns 0.0).
    args: &'static [UiArgKind],
    ret: UiReturnKind,
}

/// Static dispatch table for perry/ui receiver-less calls. Covers the
/// constructors + setters mango uses, plus the most common widgets from
/// the cross-cutting "any perry/ui app" surface. Keep alphabetized by
/// `method` for easy scanning.
///
/// Entries NOT in this table fall through to the receiver-less early-out
/// in `lower_native_method_call` (which lowers args for side effects and
/// returns the zero-sentinel). That's the behavior the entire perry/ui
/// surface had pre-v0.5.10 — adding a row here flips one method from
/// "silent no-op" to "real call into libperry_ui_macos.a".
const PERRY_UI_TABLE: &[UiSig] = &[
    // ---- Constructors (return widget handle) ----
    UiSig { method: "Divider", runtime: "perry_ui_divider_create",
            args: &[], ret: UiReturnKind::Widget },
    UiSig { method: "ScrollView", runtime: "perry_ui_scrollview_create",
            args: &[], ret: UiReturnKind::Widget },
    UiSig { method: "Spacer", runtime: "perry_ui_spacer_create",
            args: &[], ret: UiReturnKind::Widget },
    UiSig { method: "Text", runtime: "perry_ui_text_create",
            args: &[UiArgKind::Str], ret: UiReturnKind::Widget },
    UiSig { method: "TextArea", runtime: "perry_ui_textarea_create",
            args: &[UiArgKind::Str, UiArgKind::Closure], ret: UiReturnKind::Widget },
    UiSig { method: "TextField", runtime: "perry_ui_textfield_create",
            args: &[UiArgKind::Str, UiArgKind::Closure], ret: UiReturnKind::Widget },

    // ---- Menu / menu bar ----
    UiSig { method: "menuAddItem", runtime: "perry_ui_menu_add_item",
            args: &[UiArgKind::Widget, UiArgKind::Str, UiArgKind::Closure],
            ret: UiReturnKind::Void },
    UiSig { method: "menuAddSeparator", runtime: "perry_ui_menu_add_separator",
            args: &[UiArgKind::Widget], ret: UiReturnKind::Void },
    UiSig { method: "menuAddStandardAction", runtime: "perry_ui_menu_add_standard_action",
            args: &[UiArgKind::Widget, UiArgKind::Str, UiArgKind::Str, UiArgKind::Str],
            ret: UiReturnKind::Void },
    UiSig { method: "menuBarAddMenu", runtime: "perry_ui_menubar_add_menu",
            args: &[UiArgKind::Widget, UiArgKind::Str, UiArgKind::Widget],
            ret: UiReturnKind::Void },
    UiSig { method: "menuBarAttach", runtime: "perry_ui_menubar_attach",
            args: &[UiArgKind::Widget], ret: UiReturnKind::Void },
    UiSig { method: "menuBarCreate", runtime: "perry_ui_menubar_create",
            args: &[], ret: UiReturnKind::Widget },
    UiSig { method: "menuCreate", runtime: "perry_ui_menu_create",
            args: &[], ret: UiReturnKind::Widget },

    // ---- ScrollView ----
    UiSig { method: "scrollviewSetChild", runtime: "perry_ui_scrollview_set_child",
            args: &[UiArgKind::Widget, UiArgKind::Widget], ret: UiReturnKind::Void },
    UiSig { method: "scrollViewSetChild", runtime: "perry_ui_scrollview_set_child",
            args: &[UiArgKind::Widget, UiArgKind::Widget], ret: UiReturnKind::Void },
    UiSig { method: "scrollViewGetOffset", runtime: "perry_ui_scrollview_get_offset",
            args: &[UiArgKind::Widget], ret: UiReturnKind::F64 },
    UiSig { method: "scrollViewSetOffset", runtime: "perry_ui_scrollview_set_offset",
            args: &[UiArgKind::Widget, UiArgKind::F64, UiArgKind::F64], ret: UiReturnKind::Void },
    UiSig { method: "scrollViewScrollTo", runtime: "perry_ui_scrollview_scroll_to",
            args: &[UiArgKind::Widget, UiArgKind::F64, UiArgKind::F64], ret: UiReturnKind::Void },

    // ---- Stack layout ----
    UiSig { method: "stackSetAlignment", runtime: "perry_ui_stack_set_alignment",
            args: &[UiArgKind::Widget, UiArgKind::F64], ret: UiReturnKind::Void },
    UiSig { method: "stackSetDistribution", runtime: "perry_ui_stack_set_distribution",
            args: &[UiArgKind::Widget, UiArgKind::F64], ret: UiReturnKind::Void },

    // ---- Text setters ----
    UiSig { method: "textSetColor", runtime: "perry_ui_text_set_color",
            args: &[UiArgKind::Widget, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64],
            ret: UiReturnKind::Void },
    UiSig { method: "textSetFontFamily", runtime: "perry_ui_text_set_font_family",
            args: &[UiArgKind::Widget, UiArgKind::Str], ret: UiReturnKind::Void },
    UiSig { method: "textSetFontSize", runtime: "perry_ui_text_set_font_size",
            args: &[UiArgKind::Widget, UiArgKind::F64], ret: UiReturnKind::Void },
    UiSig { method: "textSetFontWeight", runtime: "perry_ui_text_set_font_weight",
            args: &[UiArgKind::Widget, UiArgKind::F64, UiArgKind::F64], ret: UiReturnKind::Void },
    UiSig { method: "textSetString", runtime: "perry_ui_text_set_string",
            args: &[UiArgKind::Widget, UiArgKind::Str], ret: UiReturnKind::Void },
    UiSig { method: "textSetWraps", runtime: "perry_ui_text_set_wraps",
            args: &[UiArgKind::Widget, UiArgKind::F64], ret: UiReturnKind::Void },

    // ---- Button setters ----
    UiSig { method: "buttonSetBordered", runtime: "perry_ui_button_set_bordered",
            args: &[UiArgKind::Widget, UiArgKind::F64], ret: UiReturnKind::Void },
    UiSig { method: "buttonSetTextColor", runtime: "perry_ui_button_set_text_color",
            args: &[UiArgKind::Widget, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64],
            ret: UiReturnKind::Void },
    UiSig { method: "buttonSetTitle", runtime: "perry_ui_button_set_title",
            args: &[UiArgKind::Widget, UiArgKind::Str], ret: UiReturnKind::Void },

    // ---- TextField / TextArea ----
    UiSig { method: "textfieldSetString", runtime: "perry_ui_textfield_set_string",
            args: &[UiArgKind::Widget, UiArgKind::Str], ret: UiReturnKind::Void },
    UiSig { method: "textareaSetString", runtime: "perry_ui_textarea_set_string",
            args: &[UiArgKind::Widget, UiArgKind::Str], ret: UiReturnKind::Void },

    // ---- Generic widget ops ----
    UiSig { method: "setCornerRadius", runtime: "perry_ui_widget_set_corner_radius",
            args: &[UiArgKind::Widget, UiArgKind::F64], ret: UiReturnKind::Void },
    UiSig { method: "widgetAddChild", runtime: "perry_ui_widget_add_child",
            args: &[UiArgKind::Widget, UiArgKind::Widget], ret: UiReturnKind::Void },
    UiSig { method: "widgetClearChildren", runtime: "perry_ui_widget_clear_children",
            args: &[UiArgKind::Widget], ret: UiReturnKind::Void },
    UiSig { method: "widgetMatchParentHeight", runtime: "perry_ui_widget_match_parent_height",
            args: &[UiArgKind::Widget], ret: UiReturnKind::Void },
    UiSig { method: "widgetMatchParentWidth", runtime: "perry_ui_widget_match_parent_width",
            args: &[UiArgKind::Widget], ret: UiReturnKind::Void },
    UiSig { method: "widgetSetBackgroundColor", runtime: "perry_ui_widget_set_background_color",
            args: &[UiArgKind::Widget, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64],
            ret: UiReturnKind::Void },
    UiSig { method: "widgetSetBackgroundGradient", runtime: "perry_ui_widget_set_background_gradient",
            args: &[
                UiArgKind::Widget, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64,
                UiArgKind::F64, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64,
            ], ret: UiReturnKind::Void },
    UiSig { method: "widgetSetHeight", runtime: "perry_ui_widget_set_height",
            args: &[UiArgKind::Widget, UiArgKind::F64], ret: UiReturnKind::Void },
    UiSig { method: "widgetSetHidden", runtime: "perry_ui_set_widget_hidden",
            args: &[UiArgKind::Widget, UiArgKind::I64Raw], ret: UiReturnKind::Void },
    UiSig { method: "widgetSetHugging", runtime: "perry_ui_widget_set_hugging",
            args: &[UiArgKind::Widget, UiArgKind::F64], ret: UiReturnKind::Void },
    UiSig { method: "widgetSetWidth", runtime: "perry_ui_widget_set_width",
            args: &[UiArgKind::Widget, UiArgKind::F64], ret: UiReturnKind::Void },

    // ---- Image ----
    UiSig { method: "ImageFile", runtime: "perry_ui_image_create_file",
            args: &[UiArgKind::Str], ret: UiReturnKind::Widget },
    UiSig { method: "ImageSymbol", runtime: "perry_ui_image_create_symbol",
            args: &[UiArgKind::Str], ret: UiReturnKind::Widget },
    UiSig { method: "imageSetSize", runtime: "perry_ui_image_set_size",
            args: &[UiArgKind::Widget, UiArgKind::F64, UiArgKind::F64], ret: UiReturnKind::Void },
    UiSig { method: "imageSetTint", runtime: "perry_ui_image_set_tint",
            args: &[UiArgKind::Widget, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64],
            ret: UiReturnKind::Void },

    // ---- Padding / Edge Insets ----
    UiSig { method: "setPadding", runtime: "perry_ui_widget_set_edge_insets",
            args: &[UiArgKind::Widget, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64],
            ret: UiReturnKind::Void },
    UiSig { method: "widgetSetEdgeInsets", runtime: "perry_ui_widget_set_edge_insets",
            args: &[UiArgKind::Widget, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64],
            ret: UiReturnKind::Void },

    // ---- LazyVStack (virtualized list) ----
    // `LazyVStack(count, (i) => Widget)` — on macOS backed by NSTableView
    // with lazy row rendering. The render closure is invoked only for rows
    // currently in the visible rect.
    UiSig { method: "LazyVStack", runtime: "perry_ui_lazyvstack_create",
            args: &[UiArgKind::F64, UiArgKind::Closure], ret: UiReturnKind::Widget },
    UiSig { method: "lazyvstackUpdate", runtime: "perry_ui_lazyvstack_update",
            args: &[UiArgKind::Widget, UiArgKind::I64Raw], ret: UiReturnKind::Void },
    UiSig { method: "lazyvstackSetRowHeight", runtime: "perry_ui_lazyvstack_set_row_height",
            args: &[UiArgKind::Widget, UiArgKind::F64], ret: UiReturnKind::Void },

    // ---- State ----
    UiSig { method: "State", runtime: "perry_ui_state_create",
            args: &[UiArgKind::F64], ret: UiReturnKind::Widget },
    UiSig { method: "stateCreate", runtime: "perry_ui_state_create",
            args: &[UiArgKind::F64], ret: UiReturnKind::Widget },
    UiSig { method: "stateGet", runtime: "perry_ui_state_get",
            args: &[UiArgKind::Widget], ret: UiReturnKind::F64 },
    UiSig { method: "stateSet", runtime: "perry_ui_state_set",
            args: &[UiArgKind::Widget, UiArgKind::F64], ret: UiReturnKind::Void },
    UiSig { method: "stateOnChange", runtime: "perry_ui_state_on_change",
            args: &[UiArgKind::Widget, UiArgKind::Closure], ret: UiReturnKind::Void },
    UiSig { method: "stateBindTextNumeric", runtime: "perry_ui_state_bind_text_numeric",
            args: &[UiArgKind::Widget, UiArgKind::Widget, UiArgKind::Str, UiArgKind::Str],
            ret: UiReturnKind::Void },
    UiSig { method: "stateBindSlider", runtime: "perry_ui_state_bind_slider",
            args: &[UiArgKind::Widget, UiArgKind::Widget], ret: UiReturnKind::Void },
    UiSig { method: "stateBindToggle", runtime: "perry_ui_state_bind_toggle",
            args: &[UiArgKind::Widget, UiArgKind::Widget], ret: UiReturnKind::Void },
    UiSig { method: "stateBindVisibility", runtime: "perry_ui_state_bind_visibility",
            args: &[UiArgKind::Widget, UiArgKind::Widget, UiArgKind::Widget],
            ret: UiReturnKind::Void },
    UiSig { method: "stateBindTextfield", runtime: "perry_ui_state_bind_textfield",
            args: &[UiArgKind::Widget, UiArgKind::Widget], ret: UiReturnKind::Void },

    // ---- TextField extras ----
    UiSig { method: "textfieldGetString", runtime: "perry_ui_textfield_get_string",
            args: &[UiArgKind::Widget], ret: UiReturnKind::F64 },
    UiSig { method: "textfieldFocus", runtime: "perry_ui_textfield_focus",
            args: &[UiArgKind::Widget], ret: UiReturnKind::Void },
    UiSig { method: "textfieldBlurAll", runtime: "perry_ui_textfield_blur_all",
            args: &[], ret: UiReturnKind::Void },
    UiSig { method: "textfieldSetNextKeyView", runtime: "perry_ui_textfield_set_next_key_view",
            args: &[UiArgKind::Widget, UiArgKind::Widget], ret: UiReturnKind::Void },
    UiSig { method: "textfieldSetOnSubmit", runtime: "perry_ui_textfield_set_on_submit",
            args: &[UiArgKind::Widget, UiArgKind::Closure], ret: UiReturnKind::Void },
    UiSig { method: "textfieldSetOnFocus", runtime: "perry_ui_textfield_set_on_focus",
            args: &[UiArgKind::Widget, UiArgKind::Closure], ret: UiReturnKind::Void },
    UiSig { method: "textfieldSetBackgroundColor", runtime: "perry_ui_textfield_set_background_color",
            args: &[UiArgKind::Widget, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64],
            ret: UiReturnKind::Void },
    UiSig { method: "textfieldSetBorderless", runtime: "perry_ui_textfield_set_borderless",
            args: &[UiArgKind::Widget, UiArgKind::F64], ret: UiReturnKind::Void },
    UiSig { method: "textfieldSetFontSize", runtime: "perry_ui_textfield_set_font_size",
            args: &[UiArgKind::Widget, UiArgKind::F64], ret: UiReturnKind::Void },
    UiSig { method: "textfieldSetTextColor", runtime: "perry_ui_textfield_set_text_color",
            args: &[UiArgKind::Widget, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64],
            ret: UiReturnKind::Void },
    UiSig { method: "textareaGetString", runtime: "perry_ui_textarea_get_string",
            args: &[UiArgKind::Widget], ret: UiReturnKind::F64 },

    // ---- Text extras ----
    UiSig { method: "textSetSelectable", runtime: "perry_ui_text_set_selectable",
            args: &[UiArgKind::Widget, UiArgKind::F64], ret: UiReturnKind::Void },

    // ---- Widget extras ----
    UiSig { method: "widgetAddChildAt", runtime: "perry_ui_widget_add_child_at",
            args: &[UiArgKind::Widget, UiArgKind::Widget, UiArgKind::I64Raw],
            ret: UiReturnKind::Void },
    UiSig { method: "widgetRemoveChild", runtime: "perry_ui_widget_remove_child",
            args: &[UiArgKind::Widget, UiArgKind::Widget], ret: UiReturnKind::Void },
    UiSig { method: "widgetReorderChild", runtime: "perry_ui_widget_reorder_child",
            args: &[UiArgKind::Widget, UiArgKind::I64Raw, UiArgKind::I64Raw],
            ret: UiReturnKind::Void },
    UiSig { method: "widgetSetOpacity", runtime: "perry_ui_widget_set_opacity",
            args: &[UiArgKind::Widget, UiArgKind::F64], ret: UiReturnKind::Void },
    UiSig { method: "widgetSetEnabled", runtime: "perry_ui_widget_set_enabled",
            args: &[UiArgKind::Widget, UiArgKind::I64Raw], ret: UiReturnKind::Void },
    UiSig { method: "widgetSetTooltip", runtime: "perry_ui_widget_set_tooltip",
            args: &[UiArgKind::Widget, UiArgKind::Str], ret: UiReturnKind::Void },
    UiSig { method: "widgetSetControlSize", runtime: "perry_ui_widget_set_control_size",
            args: &[UiArgKind::Widget, UiArgKind::I64Raw], ret: UiReturnKind::Void },
    UiSig { method: "widgetSetOnClick", runtime: "perry_ui_widget_set_on_click",
            args: &[UiArgKind::Widget, UiArgKind::Closure], ret: UiReturnKind::Void },
    UiSig { method: "widgetSetOnHover", runtime: "perry_ui_widget_set_on_hover",
            args: &[UiArgKind::Widget, UiArgKind::Closure], ret: UiReturnKind::Void },
    UiSig { method: "widgetSetOnDoubleClick", runtime: "perry_ui_widget_set_on_double_click",
            args: &[UiArgKind::Widget, UiArgKind::Closure], ret: UiReturnKind::Void },
    UiSig { method: "widgetAnimateOpacity", runtime: "perry_ui_widget_animate_opacity",
            args: &[UiArgKind::Widget, UiArgKind::F64, UiArgKind::F64], ret: UiReturnKind::Void },
    UiSig { method: "widgetAnimatePosition", runtime: "perry_ui_widget_animate_position",
            args: &[UiArgKind::Widget, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64],
            ret: UiReturnKind::Void },
    UiSig { method: "widgetAddOverlay", runtime: "perry_ui_widget_add_overlay",
            args: &[UiArgKind::Widget, UiArgKind::Widget], ret: UiReturnKind::Void },
    UiSig { method: "widgetSetBorderColor", runtime: "perry_ui_widget_set_border_color",
            args: &[UiArgKind::Widget, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64],
            ret: UiReturnKind::Void },
    UiSig { method: "widgetSetBorderWidth", runtime: "perry_ui_widget_set_border_width",
            args: &[UiArgKind::Widget, UiArgKind::F64], ret: UiReturnKind::Void },
    UiSig { method: "widgetSetContextMenu", runtime: "perry_ui_widget_set_context_menu",
            args: &[UiArgKind::Widget, UiArgKind::Widget], ret: UiReturnKind::Void },
    UiSig { method: "stackSetDetachesHidden", runtime: "perry_ui_stack_set_detaches_hidden",
            args: &[UiArgKind::Widget, UiArgKind::F64], ret: UiReturnKind::Void },

    // ---- Additional constructors ----
    UiSig { method: "Toggle", runtime: "perry_ui_toggle_create",
            args: &[UiArgKind::Str, UiArgKind::Closure], ret: UiReturnKind::Widget },
    UiSig { method: "Slider", runtime: "perry_ui_slider_create",
            args: &[UiArgKind::F64, UiArgKind::F64, UiArgKind::Closure], ret: UiReturnKind::Widget },
    UiSig { method: "SecureField", runtime: "perry_ui_securefield_create",
            args: &[UiArgKind::Str, UiArgKind::Closure], ret: UiReturnKind::Widget },
    UiSig { method: "ProgressView", runtime: "perry_ui_progressview_create",
            args: &[], ret: UiReturnKind::Widget },
    UiSig { method: "ZStack", runtime: "perry_ui_zstack_create",
            args: &[], ret: UiReturnKind::Widget },
    UiSig { method: "Section", runtime: "perry_ui_section_create",
            args: &[UiArgKind::Str], ret: UiReturnKind::Widget },

    // ---- ProgressView ----
    UiSig { method: "progressviewSetValue", runtime: "perry_ui_progressview_set_value",
            args: &[UiArgKind::Widget, UiArgKind::F64], ret: UiReturnKind::Void },

    // ---- Picker ----
    UiSig { method: "Picker", runtime: "perry_ui_picker_create",
            args: &[UiArgKind::Closure], ret: UiReturnKind::Widget },
    UiSig { method: "pickerAddItem", runtime: "perry_ui_picker_add_item",
            args: &[UiArgKind::Widget, UiArgKind::Str], ret: UiReturnKind::Void },
    UiSig { method: "pickerGetSelected", runtime: "perry_ui_picker_get_selected",
            args: &[UiArgKind::Widget], ret: UiReturnKind::F64 },
    UiSig { method: "pickerSetSelected", runtime: "perry_ui_picker_set_selected",
            args: &[UiArgKind::Widget, UiArgKind::I64Raw], ret: UiReturnKind::Void },

    // ---- NavigationStack ----
    UiSig { method: "NavStack", runtime: "perry_ui_navstack_create",
            args: &[], ret: UiReturnKind::Widget },
    UiSig { method: "navstackPush", runtime: "perry_ui_navstack_push",
            args: &[UiArgKind::Widget, UiArgKind::Widget, UiArgKind::Str], ret: UiReturnKind::Void },
    UiSig { method: "navstackPop", runtime: "perry_ui_navstack_pop",
            args: &[UiArgKind::Widget], ret: UiReturnKind::Void },

    // ---- TabBar ----
    UiSig { method: "TabBar", runtime: "perry_ui_tabbar_create",
            args: &[UiArgKind::Closure], ret: UiReturnKind::Widget },
    UiSig { method: "tabbarAddTab", runtime: "perry_ui_tabbar_add_tab",
            args: &[UiArgKind::Widget, UiArgKind::Str, UiArgKind::Widget], ret: UiReturnKind::Void },
    UiSig { method: "tabbarSetSelected", runtime: "perry_ui_tabbar_set_selected",
            args: &[UiArgKind::Widget, UiArgKind::I64Raw], ret: UiReturnKind::Void },

    // ---- Menu extras ----
    UiSig { method: "menuAddSubmenu", runtime: "perry_ui_menu_add_submenu",
            args: &[UiArgKind::Widget, UiArgKind::Str, UiArgKind::Widget],
            ret: UiReturnKind::Void },
    UiSig { method: "menuClear", runtime: "perry_ui_menu_clear",
            args: &[UiArgKind::Widget], ret: UiReturnKind::Void },
    UiSig { method: "menuAddItemWithShortcut", runtime: "perry_ui_menu_add_item_with_shortcut",
            args: &[UiArgKind::Widget, UiArgKind::Str, UiArgKind::Str, UiArgKind::Closure],
            ret: UiReturnKind::Void },

    // ---- ScrollView extras ----
    UiSig { method: "scrollViewSetOffset", runtime: "perry_ui_scrollview_set_offset",
            args: &[UiArgKind::Widget, UiArgKind::F64, UiArgKind::F64], ret: UiReturnKind::Void },
    UiSig { method: "scrollViewScrollTo", runtime: "perry_ui_scrollview_scroll_to",
            args: &[UiArgKind::Widget, UiArgKind::F64, UiArgKind::F64], ret: UiReturnKind::Void },

    // ---- Button extras ----
    UiSig { method: "buttonSetContentTintColor", runtime: "perry_ui_button_set_content_tint_color",
            args: &[UiArgKind::Widget, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64],
            ret: UiReturnKind::Void },
    UiSig { method: "buttonSetImage", runtime: "perry_ui_button_set_image",
            args: &[UiArgKind::Widget, UiArgKind::Str], ret: UiReturnKind::Void },
    UiSig { method: "buttonSetImagePosition", runtime: "perry_ui_button_set_image_position",
            args: &[UiArgKind::Widget, UiArgKind::I64Raw], ret: UiReturnKind::Void },

    // ---- Clipboard ----
    UiSig { method: "clipboardRead", runtime: "perry_ui_clipboard_read",
            args: &[], ret: UiReturnKind::F64 },
    UiSig { method: "clipboardWrite", runtime: "perry_ui_clipboard_write",
            args: &[UiArgKind::Str], ret: UiReturnKind::Void },

    // ---- Alert ----
    // `alert(title, message)` dispatches to a dedicated 2-arg FFI; the prior
    // entry pointed at the 4-arg `perry_ui_alert` symbol, which was ABI-broken
    // (buttons/callback read from uninitialized registers, usually segfaulting
    // inside js_array_get_length).
    UiSig { method: "alert", runtime: "perry_ui_alert_simple",
            args: &[UiArgKind::Str, UiArgKind::Str], ret: UiReturnKind::Void },
    // `alertWithButtons(title, message, buttons, cb)` — buttons is a JS array
    // of labels, callback receives the 0-based button index. Passed as F64
    // because the runtime extracts the array pointer via
    // `js_nanbox_get_pointer` just like closures.
    UiSig { method: "alertWithButtons", runtime: "perry_ui_alert",
            args: &[UiArgKind::Str, UiArgKind::Str, UiArgKind::F64, UiArgKind::Closure],
            ret: UiReturnKind::Void },

    // ---- Window (constructor — receiver-less) ----
    UiSig { method: "Window", runtime: "perry_ui_window_create",
            args: &[UiArgKind::Str, UiArgKind::F64, UiArgKind::F64], ret: UiReturnKind::Widget },

    // ---- VStack/HStack with built-in insets (no children array — children added via widgetAddChild) ----
    UiSig { method: "VStackWithInsets", runtime: "perry_ui_vstack_create_with_insets",
            args: &[UiArgKind::F64, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64],
            ret: UiReturnKind::Widget },
    UiSig { method: "HStackWithInsets", runtime: "perry_ui_hstack_create_with_insets",
            args: &[UiArgKind::F64, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64],
            ret: UiReturnKind::Widget },

    // ---- Embed external NSView ----
    UiSig { method: "embedNSView", runtime: "perry_ui_embed_nsview",
            args: &[UiArgKind::I64Raw], ret: UiReturnKind::Widget },

    // ---- File dialogs ----
    UiSig { method: "openFileDialog", runtime: "perry_ui_open_file_dialog",
            args: &[UiArgKind::Closure], ret: UiReturnKind::Void },
    UiSig { method: "openFolderDialog", runtime: "perry_ui_open_folder_dialog",
            args: &[UiArgKind::Closure], ret: UiReturnKind::Void },
    UiSig { method: "saveFileDialog", runtime: "perry_ui_save_file_dialog",
            args: &[UiArgKind::Closure, UiArgKind::Str, UiArgKind::Str],
            ret: UiReturnKind::Void },

    // ---- Widget overlay frame ----
    UiSig { method: "widgetSetOverlayFrame", runtime: "perry_ui_widget_set_overlay_frame",
            args: &[UiArgKind::Widget, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64, UiArgKind::F64],
            ret: UiReturnKind::Void },

    // ---- Toolbar ----
    UiSig { method: "toolbarCreate", runtime: "perry_ui_toolbar_create",
            args: &[], ret: UiReturnKind::Widget },
    UiSig { method: "toolbarAddItem", runtime: "perry_ui_toolbar_add_item",
            args: &[UiArgKind::Widget, UiArgKind::Str, UiArgKind::Str, UiArgKind::Closure],
            ret: UiReturnKind::Void },
    UiSig { method: "toolbarAttach", runtime: "perry_ui_toolbar_attach",
            args: &[UiArgKind::Widget, UiArgKind::Widget], ret: UiReturnKind::Void },

    // ---- SplitView ----
    UiSig { method: "SplitView", runtime: "perry_ui_splitview_create",
            args: &[], ret: UiReturnKind::Widget },
    UiSig { method: "splitViewAddChild", runtime: "perry_ui_splitview_add_child",
            args: &[UiArgKind::Widget, UiArgKind::Widget], ret: UiReturnKind::Void },

    // ---- Sheet ----
    UiSig { method: "sheetCreate", runtime: "perry_ui_sheet_create",
            args: &[UiArgKind::Widget, UiArgKind::F64, UiArgKind::F64], ret: UiReturnKind::Widget },
    UiSig { method: "sheetPresent", runtime: "perry_ui_sheet_present",
            args: &[UiArgKind::Widget], ret: UiReturnKind::Void },
    UiSig { method: "sheetDismiss", runtime: "perry_ui_sheet_dismiss",
            args: &[UiArgKind::Widget], ret: UiReturnKind::Void },

    // ---- FrameSplit (NSSplitView wrapper) ----
    UiSig { method: "frameSplitCreate", runtime: "perry_ui_frame_split_create",
            args: &[UiArgKind::F64], ret: UiReturnKind::Widget },
    UiSig { method: "frameSplitAddChild", runtime: "perry_ui_frame_split_add_child",
            args: &[UiArgKind::Widget, UiArgKind::Widget], ret: UiReturnKind::Void },

    // ---- File dialog polling ----
    UiSig { method: "pollOpenFile", runtime: "perry_ui_poll_open_file",
            args: &[], ret: UiReturnKind::F64 },

    // ---- Keyboard shortcuts ----
    UiSig { method: "addKeyboardShortcut", runtime: "perry_ui_add_keyboard_shortcut",
            args: &[UiArgKind::Str, UiArgKind::Closure], ret: UiReturnKind::Void },

    // ---- App lifecycle hooks ----
    UiSig { method: "onTerminate", runtime: "perry_ui_app_on_terminate",
            args: &[UiArgKind::Closure], ret: UiReturnKind::Void },
    UiSig { method: "onActivate", runtime: "perry_ui_app_on_activate",
            args: &[UiArgKind::Closure], ret: UiReturnKind::Void },

    // ---- App extras ----
    UiSig { method: "appSetTimer", runtime: "perry_ui_app_set_timer",
            args: &[UiArgKind::Widget, UiArgKind::F64, UiArgKind::Closure], ret: UiReturnKind::Void },
    UiSig { method: "appSetMinSize", runtime: "perry_ui_app_set_min_size",
            args: &[UiArgKind::Widget, UiArgKind::F64, UiArgKind::F64], ret: UiReturnKind::Void },
    UiSig { method: "appSetMaxSize", runtime: "perry_ui_app_set_max_size",
            args: &[UiArgKind::Widget, UiArgKind::F64, UiArgKind::F64], ret: UiReturnKind::Void },

    // ---- Extra ScrollView alias (lowercase-v spelling matching the runtime FFI
    // symbol; the runtime takes a single vertical offset, not the x/y pair
    // declared on `scrollViewSetOffset` in index.d.ts — they coexist for now). ----
    UiSig { method: "scrollviewSetOffset", runtime: "perry_ui_scrollview_set_offset",
            args: &[UiArgKind::Widget, UiArgKind::F64], ret: UiReturnKind::Void },
];

/// Instance method table for perry/ui receiver-based calls.
/// These methods are called on a widget/window handle: `handle.method(args)`.
/// The handle is automatically prepended as the first i64 arg.
const PERRY_UI_INSTANCE_TABLE: &[UiSig] = &[
    // ---- Window instance methods ----
    UiSig { method: "show", runtime: "perry_ui_window_show",
            args: &[], ret: UiReturnKind::Void },
    UiSig { method: "hide", runtime: "perry_ui_window_hide",
            args: &[], ret: UiReturnKind::Void },
    UiSig { method: "close", runtime: "perry_ui_window_close",
            args: &[], ret: UiReturnKind::Void },
    UiSig { method: "setBody", runtime: "perry_ui_window_set_body",
            args: &[UiArgKind::Widget], ret: UiReturnKind::Void },
    UiSig { method: "setSize", runtime: "perry_ui_window_set_size",
            args: &[UiArgKind::F64, UiArgKind::F64], ret: UiReturnKind::Void },
    UiSig { method: "onFocusLost", runtime: "perry_ui_window_on_focus_lost",
            args: &[UiArgKind::Closure], ret: UiReturnKind::Void },

    // ---- State instance methods ----
    UiSig { method: "value", runtime: "perry_ui_state_get",
            args: &[], ret: UiReturnKind::F64 },
    UiSig { method: "set", runtime: "perry_ui_state_set",
            args: &[UiArgKind::F64], ret: UiReturnKind::Void },
];

fn perry_ui_table_lookup(method: &str) -> Option<&'static UiSig> {
    PERRY_UI_TABLE.iter().find(|s| s.method == method)
}

fn perry_ui_instance_method_lookup(method: &str) -> Option<&'static UiSig> {
    PERRY_UI_INSTANCE_TABLE.iter().find(|s| s.method == method)
}

// =============================================================================
// perry/system dispatch table
// =============================================================================

/// Maps JS import names from `perry/system` to their `perry_system_*` / `perry_*`
/// runtime C symbols. Uses the same UiSig + lower_perry_ui_table_call machinery
/// since the calling convention is identical.
static PERRY_SYSTEM_TABLE: &[UiSig] = &[
    UiSig { method: "isDarkMode", runtime: "perry_system_is_dark_mode",
            args: &[], ret: UiReturnKind::F64 },
    UiSig { method: "getDeviceIdiom", runtime: "perry_get_device_idiom",
            args: &[], ret: UiReturnKind::F64 },
    UiSig { method: "openURL", runtime: "perry_system_open_url",
            args: &[UiArgKind::Str], ret: UiReturnKind::Void },
    UiSig { method: "keychainSave", runtime: "perry_system_keychain_save",
            args: &[UiArgKind::Str, UiArgKind::Str], ret: UiReturnKind::Void },
    UiSig { method: "keychainGet", runtime: "perry_system_keychain_get",
            args: &[UiArgKind::Str], ret: UiReturnKind::F64 },
    UiSig { method: "keychainDelete", runtime: "perry_system_keychain_delete",
            args: &[UiArgKind::Str], ret: UiReturnKind::Void },
    UiSig { method: "preferencesGet", runtime: "perry_system_preferences_get",
            args: &[UiArgKind::Str], ret: UiReturnKind::F64 },
    UiSig { method: "preferencesSet", runtime: "perry_system_preferences_set",
            args: &[UiArgKind::Str, UiArgKind::F64], ret: UiReturnKind::Void },
    UiSig { method: "notificationSend", runtime: "perry_system_notification_send",
            args: &[UiArgKind::Str, UiArgKind::Str], ret: UiReturnKind::Void },
    UiSig { method: "audioStart", runtime: "perry_system_audio_start",
            args: &[], ret: UiReturnKind::F64 },
    UiSig { method: "audioStop", runtime: "perry_system_audio_stop",
            args: &[], ret: UiReturnKind::Void },
    UiSig { method: "audioGetLevel", runtime: "perry_system_audio_get_level",
            args: &[], ret: UiReturnKind::F64 },
    UiSig { method: "audioGetPeak", runtime: "perry_system_audio_get_peak",
            args: &[], ret: UiReturnKind::F64 },
    UiSig { method: "audioGetWaveform", runtime: "perry_system_audio_get_waveform",
            args: &[UiArgKind::F64], ret: UiReturnKind::F64 },
    UiSig { method: "getDeviceModel", runtime: "perry_system_get_device_model",
            args: &[], ret: UiReturnKind::F64 },
];

fn perry_system_table_lookup(method: &str) -> Option<&'static UiSig> {
    PERRY_SYSTEM_TABLE.iter().find(|s| s.method == method)
}

/// Lower a perry/ui call described by `sig`. Walks each arg, applies
/// the per-kind coercion to produce an LLVM SSA value of the right type,
/// lazy-declares the runtime function, emits the call, and boxes the
/// return value per `sig.ret`.
///
/// Args length mismatch (caller passed wrong number of args) → falls
/// back to lowering all args for side effects + returning the
/// zero-sentinel. The catch-all is intentional: TS users may write
/// `Text()` (no arg) or `Text(s, extra)` and we don't want to bail
/// the entire compilation.
fn lower_perry_ui_table_call(
    ctx: &mut FnCtx<'_>,
    sig: &UiSig,
    args: &[Expr],
) -> Result<String> {
    if args.len() != sig.args.len() {
        // Mismatched arity — fall back to side-effect lowering only.
        for a in args {
            let _ = lower_expr(ctx, a)?;
        }
        return Ok(double_literal(0.0));
    }

    // Lower each arg according to its declared kind. Build two parallel
    // vectors so we can pass them through to `blk.call(...)` in one shot
    // without intermediate borrows.
    let mut llvm_args: Vec<(crate::types::LlvmType, String)> =
        Vec::with_capacity(args.len());
    let mut runtime_param_types: Vec<crate::types::LlvmType> =
        Vec::with_capacity(args.len());
    for (kind, arg) in sig.args.iter().zip(args.iter()) {
        match kind {
            UiArgKind::Widget => {
                // Widgets are NaN-boxed pointers. Lower as JSValue,
                // strip the POINTER_TAG bits to get the raw 1-based
                // handle as i64.
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
                // Closures are NaN-boxed pointers passed as f64. The
                // runtime side calls `js_closure_call0` (or callN) on
                // them, so it expects the f64 representation.
                let v = lower_expr(ctx, arg)?;
                llvm_args.push((DOUBLE, v));
                runtime_param_types.push(DOUBLE);
            }
            UiArgKind::I64Raw => {
                // Numeric arg the runtime wants as i64 (e.g. enum tag,
                // boolean flag). `fptosi` converts the f64 to a signed
                // integer.
                let v = lower_expr(ctx, arg)?;
                let blk = ctx.block();
                let i = blk.fptosi(DOUBLE, &v, I64);
                llvm_args.push((I64, i));
                runtime_param_types.push(I64);
            }
        }
    }

    // Lazy-declare the runtime function so the linker pulls in the
    // libperry_ui_*.a symbol. Same pending_declares mechanism the
    // cross-module call site uses for `perry_fn_*`.
    let return_type = match sig.ret {
        UiReturnKind::Widget => I64,
        UiReturnKind::F64 => DOUBLE,
        UiReturnKind::Void => crate::types::VOID,
    };
    ctx.pending_declares.push((
        sig.runtime.to_string(),
        return_type,
        runtime_param_types,
    ));

    // Emit the call. Slices need a borrow of `llvm_args` because the
    // tuple's second field is `String` and `blk.call` expects `&str`.
    let arg_slices: Vec<(crate::types::LlvmType, &str)> =
        llvm_args.iter().map(|(t, s)| (*t, s.as_str())).collect();
    match sig.ret {
        UiReturnKind::Widget => {
            let blk = ctx.block();
            let handle = blk.call(I64, sig.runtime, &arg_slices);
            Ok(nanbox_pointer_inline(blk, &handle))
        }
        UiReturnKind::F64 => {
            Ok(ctx.block().call(DOUBLE, sig.runtime, &arg_slices))
        }
        UiReturnKind::Void => {
            ctx.block().call_void(sig.runtime, &arg_slices);
            Ok(double_literal(0.0))
        }
    }
}

// ============================================================================
// Native stdlib module dispatch (fastify, mysql2, ws, pg, ioredis, mongodb,
// better-sqlite3, etc.). Ported from the old Cranelift codegen's dispatch
// table that was lost in the v0.5.0 LLVM cutover.
// ============================================================================

/// How each argument should be coerced before passing to the runtime fn.
#[derive(Copy, Clone, Debug)]
enum NativeArgKind {
    /// NaN-boxed f64 — pass as-is (objects, generic JSValues).
    F64,
    /// NaN-boxed string → extract raw i64 pointer via js_get_string_pointer_unified.
    /// Use for Rust signatures like `*const StringHeader`.
    StrPtr,
    /// NaN-boxed closure/pointer → unbox to i64 via the standard mask.
    PtrI64,
    /// Pass the NaN-boxed JSValue bits as-is (bitcast f64 → i64, no
    /// unboxing). Use for Rust signatures where the function receives
    /// `name: i64` and internally calls `string_from_nanboxed(name)` or
    /// similar — the callee expects the full NaN-boxed value, not an
    /// unboxed raw pointer. Common pattern in fastify context methods.
    JsvalI64,
}

/// What the runtime function returns.
#[derive(Copy, Clone, Debug)]
enum NativeRetKind {
    /// Returns i64 handle → NaN-box as POINTER.
    Ptr,
    /// Returns `*mut StringHeader` → NaN-box as STRING. Use for runtime
    /// functions whose Rust signature returns a raw string pointer; the
    /// caller (and `JSON.stringify`, string-comparison, etc.) needs the
    /// STRING_TAG to recognize it as a string rather than a heap object.
    Str,
    /// Returns f64 → pass through (NaN-boxed JSValue).
    F64,
    /// Returns i32 → ignored, return TAG_UNDEFINED.
    I32Void,
    /// Returns void → return TAG_UNDEFINED.
    Void,
}

#[derive(Copy, Clone, Debug)]
struct NativeModSig {
    module: &'static str,
    has_receiver: bool,
    method: &'static str,
    /// Optional class_name filter. When Some, only matches if the HIR's
    /// class_name equals this value (e.g. "Pool" vs "Connection" for mysql2).
    /// When None, matches regardless of class_name.
    class_filter: Option<&'static str>,
    runtime: &'static str,
    args: &'static [NativeArgKind],
    ret: NativeRetKind,
}

// Short aliases to keep the table compact without wildcard imports
// (wildcard would clash with crate::types::* names like I64, DOUBLE).
const NA_F64: NativeArgKind = NativeArgKind::F64;
const NA_STR: NativeArgKind = NativeArgKind::StrPtr;
const NA_PTR: NativeArgKind = NativeArgKind::PtrI64;
const NA_JSV: NativeArgKind = NativeArgKind::JsvalI64;
const NR_PTR: NativeRetKind = NativeRetKind::Ptr;
const NR_STR: NativeRetKind = NativeRetKind::Str;
const NR_F64: NativeRetKind = NativeRetKind::F64;
const NR_I32: NativeRetKind = NativeRetKind::I32Void;
const NR_VOID: NativeRetKind = NativeRetKind::Void;

/// Static dispatch table for native stdlib modules. Each entry maps
/// `(module, has_receiver, method)` → runtime function, with per-arg
/// coercion rules and return-value boxing.
///
/// The receiver (when `has_receiver = true`) is always NaN-unboxed to
/// an i64 pointer and passed as the first argument.
const NATIVE_MODULE_TABLE: &[NativeModSig] = &[
    // ========== Fastify HTTP Framework ==========
    NativeModSig { module: "fastify", has_receiver: false, method: "default",
        class_filter: None,
        runtime: "js_fastify_create_with_opts", args: &[NA_F64], ret: NR_PTR },
    NativeModSig { module: "fastify", has_receiver: true, method: "get",
        class_filter: None,
        runtime: "js_fastify_get", args: &[NA_STR, NA_PTR], ret: NR_I32 },
    NativeModSig { module: "fastify", has_receiver: true, method: "post",
        class_filter: None,
        runtime: "js_fastify_post", args: &[NA_STR, NA_PTR], ret: NR_I32 },
    NativeModSig { module: "fastify", has_receiver: true, method: "put",
        class_filter: None,
        runtime: "js_fastify_put", args: &[NA_STR, NA_PTR], ret: NR_I32 },
    NativeModSig { module: "fastify", has_receiver: true, method: "delete",
        class_filter: None,
        runtime: "js_fastify_delete", args: &[NA_STR, NA_PTR], ret: NR_I32 },
    NativeModSig { module: "fastify", has_receiver: true, method: "patch",
        class_filter: None,
        runtime: "js_fastify_patch", args: &[NA_STR, NA_PTR], ret: NR_I32 },
    NativeModSig { module: "fastify", has_receiver: true, method: "head",
        class_filter: None,
        runtime: "js_fastify_head", args: &[NA_STR, NA_PTR], ret: NR_I32 },
    NativeModSig { module: "fastify", has_receiver: true, method: "options",
        class_filter: None,
        runtime: "js_fastify_options", args: &[NA_STR, NA_PTR], ret: NR_I32 },
    NativeModSig { module: "fastify", has_receiver: true, method: "all",
        class_filter: None,
        runtime: "js_fastify_all", args: &[NA_STR, NA_PTR], ret: NR_I32 },
    NativeModSig { module: "fastify", has_receiver: true, method: "route",
        class_filter: None,
        runtime: "js_fastify_route", args: &[NA_STR, NA_STR, NA_PTR], ret: NR_I32 },
    NativeModSig { module: "fastify", has_receiver: true, method: "addHook",
        class_filter: None,
        runtime: "js_fastify_add_hook", args: &[NA_STR, NA_PTR], ret: NR_I32 },
    NativeModSig { module: "fastify", has_receiver: true, method: "setErrorHandler",
        class_filter: None,
        runtime: "js_fastify_set_error_handler", args: &[NA_PTR], ret: NR_I32 },
    NativeModSig { module: "fastify", has_receiver: true, method: "register",
        class_filter: None,
        runtime: "js_fastify_register", args: &[NA_PTR, NA_F64], ret: NR_I32 },
    NativeModSig { module: "fastify", has_receiver: true, method: "listen",
        class_filter: None,
        runtime: "js_fastify_listen", args: &[NA_F64, NA_PTR], ret: NR_VOID },
    // Fastify request methods
    NativeModSig { module: "fastify", has_receiver: true, method: "method",
        class_filter: None,
        runtime: "js_fastify_req_method", args: &[], ret: NR_STR },
    NativeModSig { module: "fastify", has_receiver: true, method: "url",
        class_filter: None,
        runtime: "js_fastify_req_url", args: &[], ret: NR_STR },
    NativeModSig { module: "fastify", has_receiver: true, method: "params",
        class_filter: None,
        // Returns the parsed path-params object (e.g. `{id: "42"}` for /users/:id),
        // not the raw JSON string — `request.params.id` must be the value, not
        // undefined. `js_fastify_req_params` (string) is still available via
        // the lower-level FFI but isn't reachable from TypeScript.
        runtime: "js_fastify_req_params_object", args: &[], ret: NR_F64 },
    NativeModSig { module: "fastify", has_receiver: true, method: "param",
        class_filter: None,
        runtime: "js_fastify_req_param", args: &[NA_JSV], ret: NR_STR },
    NativeModSig { module: "fastify", has_receiver: true, method: "query",
        class_filter: None,
        runtime: "js_fastify_req_query_object", args: &[], ret: NR_F64 },
    NativeModSig { module: "fastify", has_receiver: true, method: "rawBody",
        class_filter: None,
        runtime: "js_fastify_req_body", args: &[], ret: NR_STR },
    NativeModSig { module: "fastify", has_receiver: true, method: "headers",
        class_filter: None,
        runtime: "js_fastify_req_headers", args: &[], ret: NR_PTR },
    NativeModSig { module: "fastify", has_receiver: true, method: "header",
        class_filter: None,
        runtime: "js_fastify_req_header", args: &[NA_JSV], ret: NR_STR },
    NativeModSig { module: "fastify", has_receiver: true, method: "user",
        class_filter: None,
        runtime: "js_fastify_req_get_user_data", args: &[], ret: NR_F64 },
    // Fastify reply methods
    NativeModSig { module: "fastify", has_receiver: true, method: "status",
        class_filter: None,
        runtime: "js_fastify_reply_status", args: &[NA_F64], ret: NR_PTR },
    // `reply.code(N)` is an alias for `reply.status(N)` in npm Fastify. Without
    // this row, `reply.code(201)` silently no-op'd and the HTTP status stayed 200.
    NativeModSig { module: "fastify", has_receiver: true, method: "code",
        class_filter: None,
        runtime: "js_fastify_reply_status", args: &[NA_F64], ret: NR_PTR },
    NativeModSig { module: "fastify", has_receiver: true, method: "send",
        class_filter: None,
        runtime: "js_fastify_reply_send", args: &[NA_F64], ret: NR_I32 },
    // Fastify context methods (Hono-style)
    NativeModSig { module: "fastify", has_receiver: true, method: "text",
        class_filter: None,
        runtime: "js_fastify_ctx_text", args: &[NA_JSV, NA_F64], ret: NR_F64 },
    NativeModSig { module: "fastify", has_receiver: true, method: "html",
        class_filter: None,
        runtime: "js_fastify_ctx_html", args: &[NA_JSV, NA_F64], ret: NR_F64 },
    NativeModSig { module: "fastify", has_receiver: true, method: "redirect",
        class_filter: None,
        runtime: "js_fastify_ctx_redirect", args: &[NA_JSV, NA_F64], ret: NR_F64 },
    NativeModSig { module: "fastify", has_receiver: true, method: "json",
        class_filter: None,
        runtime: "js_fastify_ctx_json", args: &[NA_F64, NA_F64], ret: NR_F64 },
    NativeModSig { module: "fastify", has_receiver: true, method: "body",
        class_filter: None,
        runtime: "js_fastify_req_json", args: &[], ret: NR_F64 },

    // ========== MySQL2 ==========
    NativeModSig { module: "mysql2", has_receiver: false, method: "createConnection",
        class_filter: None,
        runtime: "js_mysql2_create_connection", args: &[NA_F64], ret: NR_PTR },
    NativeModSig { module: "mysql2", has_receiver: false, method: "createPool",
        class_filter: None,
        runtime: "js_mysql2_create_pool", args: &[NA_F64], ret: NR_PTR },
    NativeModSig { module: "mysql2/promise", has_receiver: false, method: "createConnection",
        class_filter: None,
        runtime: "js_mysql2_create_connection", args: &[NA_F64], ret: NR_PTR },
    NativeModSig { module: "mysql2/promise", has_receiver: false, method: "createPool",
        class_filter: None,
        runtime: "js_mysql2_create_pool", args: &[NA_F64], ret: NR_PTR },
    // mysql2 Pool-specific methods (class_filter: Some("Pool"))
    NativeModSig { module: "mysql2", has_receiver: true, method: "query",
        class_filter: Some("Pool"),
        runtime: "js_mysql2_pool_query", args: &[NA_STR, NA_PTR], ret: NR_PTR },
    NativeModSig { module: "mysql2", has_receiver: true, method: "execute",
        class_filter: Some("Pool"),
        runtime: "js_mysql2_pool_execute", args: &[NA_STR, NA_PTR], ret: NR_PTR },
    NativeModSig { module: "mysql2", has_receiver: true, method: "end",
        class_filter: Some("Pool"),
        runtime: "js_mysql2_pool_end", args: &[], ret: NR_PTR },
    NativeModSig { module: "mysql2/promise", has_receiver: true, method: "query",
        class_filter: Some("Pool"),
        runtime: "js_mysql2_pool_query", args: &[NA_STR, NA_PTR], ret: NR_PTR },
    NativeModSig { module: "mysql2/promise", has_receiver: true, method: "execute",
        class_filter: Some("Pool"),
        runtime: "js_mysql2_pool_execute", args: &[NA_STR, NA_PTR], ret: NR_PTR },
    NativeModSig { module: "mysql2/promise", has_receiver: true, method: "end",
        class_filter: Some("Pool"),
        runtime: "js_mysql2_pool_end", args: &[], ret: NR_PTR },
    // mysql2 PoolConnection-specific methods
    NativeModSig { module: "mysql2", has_receiver: true, method: "query",
        class_filter: Some("PoolConnection"),
        runtime: "js_mysql2_pool_connection_query", args: &[NA_STR, NA_PTR], ret: NR_PTR },
    NativeModSig { module: "mysql2", has_receiver: true, method: "execute",
        class_filter: Some("PoolConnection"),
        runtime: "js_mysql2_pool_connection_execute", args: &[NA_STR, NA_PTR], ret: NR_PTR },
    NativeModSig { module: "mysql2/promise", has_receiver: true, method: "query",
        class_filter: Some("PoolConnection"),
        runtime: "js_mysql2_pool_connection_query", args: &[NA_STR, NA_PTR], ret: NR_PTR },
    NativeModSig { module: "mysql2/promise", has_receiver: true, method: "execute",
        class_filter: Some("PoolConnection"),
        runtime: "js_mysql2_pool_connection_execute", args: &[NA_STR, NA_PTR], ret: NR_PTR },
    // mysql2 generic instance methods (Connection fallback, class_filter: None)
    NativeModSig { module: "mysql2", has_receiver: true, method: "query",
        class_filter: None,
        runtime: "js_mysql2_connection_query", args: &[NA_STR, NA_PTR], ret: NR_PTR },
    NativeModSig { module: "mysql2", has_receiver: true, method: "execute",
        class_filter: None,
        runtime: "js_mysql2_connection_execute", args: &[NA_STR, NA_PTR], ret: NR_PTR },
    NativeModSig { module: "mysql2", has_receiver: true, method: "end",
        class_filter: None,
        runtime: "js_mysql2_connection_end", args: &[], ret: NR_PTR },
    NativeModSig { module: "mysql2", has_receiver: true, method: "getConnection",
        class_filter: None,
        runtime: "js_mysql2_pool_get_connection", args: &[], ret: NR_PTR },
    NativeModSig { module: "mysql2", has_receiver: true, method: "release",
        class_filter: None,
        runtime: "js_mysql2_pool_connection_release", args: &[], ret: NR_VOID },
    NativeModSig { module: "mysql2", has_receiver: true, method: "beginTransaction",
        class_filter: None,
        runtime: "js_mysql2_connection_begin_transaction", args: &[], ret: NR_PTR },
    NativeModSig { module: "mysql2", has_receiver: true, method: "commit",
        class_filter: None,
        runtime: "js_mysql2_connection_commit", args: &[], ret: NR_PTR },
    NativeModSig { module: "mysql2", has_receiver: true, method: "rollback",
        class_filter: None,
        runtime: "js_mysql2_connection_rollback", args: &[], ret: NR_PTR },
    NativeModSig { module: "mysql2/promise", has_receiver: true, method: "query",
        class_filter: None,
        runtime: "js_mysql2_connection_query", args: &[NA_STR, NA_PTR], ret: NR_PTR },
    NativeModSig { module: "mysql2/promise", has_receiver: true, method: "execute",
        class_filter: None,
        runtime: "js_mysql2_connection_execute", args: &[NA_STR, NA_PTR], ret: NR_PTR },
    NativeModSig { module: "mysql2/promise", has_receiver: true, method: "end",
        class_filter: None,
        runtime: "js_mysql2_connection_end", args: &[], ret: NR_PTR },
    NativeModSig { module: "mysql2/promise", has_receiver: true, method: "getConnection",
        class_filter: None,
        runtime: "js_mysql2_pool_get_connection", args: &[], ret: NR_PTR },
    NativeModSig { module: "mysql2/promise", has_receiver: true, method: "release",
        class_filter: None,
        runtime: "js_mysql2_pool_connection_release", args: &[], ret: NR_VOID },
    NativeModSig { module: "mysql2/promise", has_receiver: true, method: "beginTransaction",
        class_filter: None,
        runtime: "js_mysql2_connection_begin_transaction", args: &[], ret: NR_PTR },
    NativeModSig { module: "mysql2/promise", has_receiver: true, method: "commit",
        class_filter: None,
        runtime: "js_mysql2_connection_commit", args: &[], ret: NR_PTR },
    NativeModSig { module: "mysql2/promise", has_receiver: true, method: "rollback",
        class_filter: None,
        runtime: "js_mysql2_connection_rollback", args: &[], ret: NR_PTR },

    // ========== PostgreSQL (pg) ==========
    NativeModSig { module: "pg", has_receiver: false, method: "connect",
        class_filter: None,
        runtime: "js_pg_connect", args: &[NA_F64], ret: NR_PTR },
    NativeModSig { module: "pg", has_receiver: false, method: "Pool",
        class_filter: None,
        runtime: "js_pg_create_pool", args: &[NA_F64], ret: NR_PTR },
    NativeModSig { module: "pg", has_receiver: true, method: "query",
        class_filter: None,
        runtime: "js_pg_client_query", args: &[NA_STR, NA_PTR], ret: NR_PTR },
    NativeModSig { module: "pg", has_receiver: true, method: "end",
        class_filter: None,
        runtime: "js_pg_client_end", args: &[], ret: NR_PTR },

    // ========== ioredis ==========
    NativeModSig { module: "ioredis", has_receiver: false, method: "createClient",
        class_filter: None,
        runtime: "js_redis_create_client", args: &[NA_F64], ret: NR_PTR },
    NativeModSig { module: "ioredis", has_receiver: true, method: "set",
        class_filter: None,
        runtime: "js_redis_set", args: &[NA_STR, NA_STR], ret: NR_PTR },
    NativeModSig { module: "ioredis", has_receiver: true, method: "get",
        class_filter: None,
        runtime: "js_redis_get", args: &[NA_STR], ret: NR_PTR },
    NativeModSig { module: "ioredis", has_receiver: true, method: "del",
        class_filter: None,
        runtime: "js_redis_del", args: &[NA_STR], ret: NR_PTR },
    NativeModSig { module: "ioredis", has_receiver: true, method: "exists",
        class_filter: None,
        runtime: "js_redis_exists", args: &[NA_STR], ret: NR_PTR },
    NativeModSig { module: "ioredis", has_receiver: true, method: "incr",
        class_filter: None,
        runtime: "js_redis_incr", args: &[NA_STR], ret: NR_PTR },
    NativeModSig { module: "ioredis", has_receiver: true, method: "decr",
        class_filter: None,
        runtime: "js_redis_decr", args: &[NA_STR], ret: NR_PTR },
    NativeModSig { module: "ioredis", has_receiver: true, method: "expire",
        class_filter: None,
        runtime: "js_redis_expire", args: &[NA_STR, NA_F64], ret: NR_PTR },
    NativeModSig { module: "ioredis", has_receiver: true, method: "quit",
        class_filter: None,
        runtime: "js_redis_quit", args: &[], ret: NR_PTR },

    // ========== MongoDB ==========
    NativeModSig { module: "mongodb", has_receiver: false, method: "connect",
        class_filter: None,
        runtime: "js_mongodb_connect", args: &[NA_F64], ret: NR_PTR },
    NativeModSig { module: "mongodb", has_receiver: true, method: "db",
        class_filter: None,
        runtime: "js_mongodb_db", args: &[NA_STR], ret: NR_PTR },
    NativeModSig { module: "mongodb", has_receiver: true, method: "collection",
        class_filter: None,
        runtime: "js_mongodb_collection", args: &[NA_STR], ret: NR_PTR },
    NativeModSig { module: "mongodb", has_receiver: true, method: "insertOne",
        class_filter: None,
        runtime: "js_mongodb_insert_one", args: &[NA_F64], ret: NR_PTR },
    NativeModSig { module: "mongodb", has_receiver: true, method: "insertMany",
        class_filter: None,
        runtime: "js_mongodb_insert_many", args: &[NA_F64], ret: NR_PTR },
    NativeModSig { module: "mongodb", has_receiver: true, method: "find",
        class_filter: None,
        runtime: "js_mongodb_find", args: &[NA_F64], ret: NR_PTR },
    NativeModSig { module: "mongodb", has_receiver: true, method: "findOne",
        class_filter: None,
        runtime: "js_mongodb_find_one", args: &[NA_F64], ret: NR_PTR },
    NativeModSig { module: "mongodb", has_receiver: true, method: "updateOne",
        class_filter: None,
        runtime: "js_mongodb_update_one", args: &[NA_F64, NA_F64], ret: NR_PTR },
    NativeModSig { module: "mongodb", has_receiver: true, method: "updateMany",
        class_filter: None,
        runtime: "js_mongodb_update_many", args: &[NA_F64, NA_F64], ret: NR_PTR },
    NativeModSig { module: "mongodb", has_receiver: true, method: "deleteOne",
        class_filter: None,
        runtime: "js_mongodb_delete_one", args: &[NA_F64], ret: NR_PTR },
    NativeModSig { module: "mongodb", has_receiver: true, method: "deleteMany",
        class_filter: None,
        runtime: "js_mongodb_delete_many", args: &[NA_F64], ret: NR_PTR },
    NativeModSig { module: "mongodb", has_receiver: true, method: "countDocuments",
        class_filter: None,
        runtime: "js_mongodb_count_documents", args: &[NA_F64], ret: NR_PTR },
    NativeModSig { module: "mongodb", has_receiver: true, method: "aggregate",
        class_filter: None,
        runtime: "js_mongodb_aggregate", args: &[NA_F64], ret: NR_PTR },
    NativeModSig { module: "mongodb", has_receiver: true, method: "createIndex",
        class_filter: None,
        runtime: "js_mongodb_create_index", args: &[NA_F64], ret: NR_PTR },
    NativeModSig { module: "mongodb", has_receiver: true, method: "close",
        class_filter: None,
        runtime: "js_mongodb_close", args: &[], ret: NR_PTR },
    NativeModSig { module: "mongodb", has_receiver: true, method: "toArray",
        class_filter: None,
        runtime: "js_mongodb_to_array", args: &[], ret: NR_PTR },

    // ========== better-sqlite3 ==========
    NativeModSig { module: "better-sqlite3", has_receiver: false, method: "default",
        class_filter: None,
        runtime: "js_sqlite_open", args: &[NA_STR], ret: NR_PTR },
    NativeModSig { module: "better-sqlite3", has_receiver: true, method: "prepare",
        class_filter: None,
        runtime: "js_sqlite_prepare", args: &[NA_STR], ret: NR_PTR },
    NativeModSig { module: "better-sqlite3", has_receiver: true, method: "run",
        class_filter: None,
        runtime: "js_sqlite_stmt_run", args: &[NA_F64], ret: NR_PTR },
    NativeModSig { module: "better-sqlite3", has_receiver: true, method: "get",
        class_filter: None,
        runtime: "js_sqlite_stmt_get", args: &[NA_F64], ret: NR_F64 },
    NativeModSig { module: "better-sqlite3", has_receiver: true, method: "all",
        class_filter: None,
        runtime: "js_sqlite_stmt_all", args: &[NA_F64], ret: NR_PTR },
    NativeModSig { module: "better-sqlite3", has_receiver: true, method: "exec",
        class_filter: None,
        runtime: "js_sqlite_exec", args: &[NA_STR], ret: NR_VOID },
    NativeModSig { module: "better-sqlite3", has_receiver: true, method: "close",
        class_filter: None,
        runtime: "js_sqlite_close", args: &[], ret: NR_VOID },

    // ========== WebSocket (ws) ==========
    NativeModSig { module: "ws", has_receiver: false, method: "Server",
        class_filter: None,
        runtime: "js_ws_server_new", args: &[NA_F64], ret: NR_PTR },
    NativeModSig { module: "ws", has_receiver: false, method: "WebSocket",
        class_filter: None,
        runtime: "js_ws_connect", args: &[NA_STR], ret: NR_PTR },
    NativeModSig { module: "ws", has_receiver: true, method: "on",
        class_filter: None,
        runtime: "js_ws_on", args: &[NA_STR, NA_PTR], ret: NR_I32 },
    NativeModSig { module: "ws", has_receiver: true, method: "send",
        class_filter: None,
        runtime: "js_ws_send", args: &[NA_STR], ret: NR_VOID },
    NativeModSig { module: "ws", has_receiver: true, method: "close",
        class_filter: None,
        runtime: "js_ws_close", args: &[], ret: NR_VOID },
    // Server-side helpers — the user receives a client handle as a plain
    // f64 number from `wss.on('connection', (handle) => …)`, then passes
    // it back to these free functions to write/close that specific peer.
    // Without these entries the receiver-less call falls through to the
    // silent stub a few hundred lines down, evaluates the args for side
    // effects, and returns TAG_UNDEFINED — so frames silently never ship
    // (issue #136).
    NativeModSig { module: "ws", has_receiver: false, method: "sendToClient",
        class_filter: None,
        runtime: "js_ws_send_to_client", args: &[NA_F64, NA_STR], ret: NR_VOID },
    NativeModSig { module: "ws", has_receiver: false, method: "closeClient",
        class_filter: None,
        runtime: "js_ws_close_client", args: &[NA_F64], ret: NR_VOID },

    // ========== Raw TCP sockets (net) + TLS ==========
    // Factory: `net.createConnection(host, port)` returns a Socket handle.
    // HIR lowering at crates/perry-hir/src/lower.rs registers the return
    // value as class "Socket" so subsequent methods dispatch via the
    // class_filter entries below.
    NativeModSig { module: "net", has_receiver: false, method: "createConnection",
        class_filter: None,
        runtime: "js_net_socket_connect", args: &[NA_STR, NA_F64], ret: NR_PTR },
    NativeModSig { module: "net", has_receiver: true, method: "write",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_write", args: &[NA_PTR], ret: NR_VOID },
    NativeModSig { module: "net", has_receiver: true, method: "end",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_end", args: &[], ret: NR_VOID },
    NativeModSig { module: "net", has_receiver: true, method: "destroy",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_destroy", args: &[], ret: NR_VOID },
    NativeModSig { module: "net", has_receiver: true, method: "on",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_on", args: &[NA_STR, NA_PTR], ret: NR_VOID },
    // upgradeToTLS returns a Promise (handle pointer) — await it to wait
    // for the TLS handshake before sending anything over the upgraded stream.
    // upgradeToTLS(servername, verify): verify is 0/1 (number, not bool).
    // verify=1 uses the system trust store + hostname check (sslmode=verify-full);
    // verify=0 accepts any cert (sslmode=require, for local self-signed DBs).
    NativeModSig { module: "net", has_receiver: true, method: "upgradeToTLS",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_upgrade_tls", args: &[NA_STR, NA_F64], ret: NR_PTR },

    // Factory: `tls.connect(host, port, servername, verify)` opens plain TCP
    // then runs a full TLS handshake before firing 'connect'. Returns a Socket
    // handle that behaves identically to one produced by net.createConnection
    // (same write/end/destroy/on surface).
    NativeModSig { module: "tls", has_receiver: false, method: "connect",
        class_filter: None,
        runtime: "js_tls_connect", args: &[NA_STR, NA_F64, NA_STR, NA_F64], ret: NR_PTR },

    // ========== Events ==========
    NativeModSig { module: "events", has_receiver: false, method: "EventEmitter",
        class_filter: None,
        runtime: "js_event_emitter_new", args: &[], ret: NR_PTR },
    NativeModSig { module: "events", has_receiver: true, method: "on",
        class_filter: None,
        runtime: "js_event_emitter_on", args: &[NA_STR, NA_PTR], ret: NR_PTR },
    NativeModSig { module: "events", has_receiver: true, method: "emit",
        class_filter: None,
        runtime: "js_event_emitter_emit", args: &[NA_STR, NA_F64], ret: NR_F64 },
    NativeModSig { module: "events", has_receiver: true, method: "removeListener",
        class_filter: None,
        runtime: "js_event_emitter_remove_listener", args: &[NA_STR, NA_PTR], ret: NR_PTR },
    NativeModSig { module: "events", has_receiver: true, method: "removeAllListeners",
        class_filter: None,
        runtime: "js_event_emitter_remove_all_listeners", args: &[NA_STR], ret: NR_PTR },

    // ========== LRU Cache ==========
    NativeModSig { module: "lru-cache", has_receiver: false, method: "default",
        class_filter: None,
        runtime: "js_lru_cache_new", args: &[NA_F64], ret: NR_PTR },
    NativeModSig { module: "lru-cache", has_receiver: true, method: "get",
        class_filter: None,
        runtime: "js_lru_cache_get", args: &[NA_F64], ret: NR_F64 },
    NativeModSig { module: "lru-cache", has_receiver: true, method: "set",
        class_filter: None,
        runtime: "js_lru_cache_set", args: &[NA_F64, NA_F64], ret: NR_PTR },
    NativeModSig { module: "lru-cache", has_receiver: true, method: "has",
        class_filter: None,
        runtime: "js_lru_cache_has", args: &[NA_F64], ret: NR_F64 },
    NativeModSig { module: "lru-cache", has_receiver: true, method: "delete",
        class_filter: None,
        runtime: "js_lru_cache_delete", args: &[NA_F64], ret: NR_F64 },
    NativeModSig { module: "lru-cache", has_receiver: true, method: "clear",
        class_filter: None,
        runtime: "js_lru_cache_clear", args: &[], ret: NR_VOID },
    NativeModSig { module: "lru-cache", has_receiver: true, method: "size",
        class_filter: None,
        runtime: "js_lru_cache_size", args: &[], ret: NR_F64 },

    // ========== uuid ==========
    NativeModSig { module: "uuid", has_receiver: false, method: "v4",
        class_filter: None,
        runtime: "js_uuid_v4", args: &[], ret: NR_PTR },
    NativeModSig { module: "uuid", has_receiver: false, method: "v1",
        class_filter: None,
        runtime: "js_uuid_v1", args: &[], ret: NR_PTR },
    NativeModSig { module: "uuid", has_receiver: false, method: "v7",
        class_filter: None,
        runtime: "js_uuid_v7", args: &[], ret: NR_PTR },
    NativeModSig { module: "uuid", has_receiver: false, method: "validate",
        class_filter: None,
        runtime: "js_uuid_validate", args: &[NA_F64], ret: NR_F64 },

    // ========== jsonwebtoken ==========
    NativeModSig { module: "jsonwebtoken", has_receiver: false, method: "sign",
        class_filter: None,
        runtime: "js_jwt_sign", args: &[NA_F64, NA_F64, NA_F64], ret: NR_PTR },
    NativeModSig { module: "jsonwebtoken", has_receiver: false, method: "verify",
        class_filter: None,
        runtime: "js_jwt_verify", args: &[NA_F64, NA_F64], ret: NR_F64 },
    NativeModSig { module: "jsonwebtoken", has_receiver: false, method: "decode",
        class_filter: None,
        runtime: "js_jwt_decode", args: &[NA_F64], ret: NR_F64 },

    // ========== nodemailer ==========
    NativeModSig { module: "nodemailer", has_receiver: false, method: "createTransport",
        class_filter: None,
        runtime: "js_nodemailer_create_transport", args: &[NA_PTR], ret: NR_F64 },
    NativeModSig { module: "nodemailer", has_receiver: true, method: "sendMail",
        class_filter: None,
        runtime: "js_nodemailer_send_mail", args: &[NA_PTR], ret: NR_PTR },
    NativeModSig { module: "nodemailer", has_receiver: true, method: "verify",
        class_filter: None,
        runtime: "js_nodemailer_verify", args: &[], ret: NR_PTR },

    // ========== dotenv ==========
    NativeModSig { module: "dotenv", has_receiver: false, method: "config",
        class_filter: None,
        runtime: "js_dotenv_config", args: &[], ret: NR_F64 },

    // ========== nanoid ==========
    NativeModSig { module: "nanoid", has_receiver: false, method: "nanoid",
        class_filter: None,
        runtime: "js_nanoid", args: &[], ret: NR_PTR },

    // ========== slugify ==========
    NativeModSig { module: "slugify", has_receiver: false, method: "slugify",
        class_filter: None,
        runtime: "js_slugify", args: &[NA_STR], ret: NR_PTR },

    // ========== validator ==========
    NativeModSig { module: "validator", has_receiver: false, method: "isEmail",
        class_filter: None,
        runtime: "js_validator_is_email", args: &[NA_STR], ret: NR_F64 },
    NativeModSig { module: "validator", has_receiver: false, method: "isURL",
        class_filter: None,
        runtime: "js_validator_is_url", args: &[NA_STR], ret: NR_F64 },
    NativeModSig { module: "validator", has_receiver: false, method: "isUUID",
        class_filter: None,
        runtime: "js_validator_is_uuid", args: &[NA_STR], ret: NR_F64 },
    NativeModSig { module: "validator", has_receiver: false, method: "isJSON",
        class_filter: None,
        runtime: "js_validator_is_json", args: &[NA_STR], ret: NR_F64 },
    NativeModSig { module: "validator", has_receiver: false, method: "isEmpty",
        class_filter: None,
        runtime: "js_validator_is_empty", args: &[NA_STR], ret: NR_F64 },

    // ========== exponential-backoff ==========
    NativeModSig { module: "exponential-backoff", has_receiver: false, method: "backOff",
        class_filter: None,
        runtime: "backOff", args: &[NA_PTR, NA_F64], ret: NR_PTR },

    // ========== argon2 ==========
    NativeModSig { module: "argon2", has_receiver: false, method: "hash",
        class_filter: None,
        runtime: "js_argon2_hash", args: &[NA_F64], ret: NR_PTR },
    NativeModSig { module: "argon2", has_receiver: false, method: "verify",
        class_filter: None,
        runtime: "js_argon2_verify", args: &[NA_F64, NA_F64], ret: NR_PTR },

    // ========== bcrypt ==========
    NativeModSig { module: "bcrypt", has_receiver: false, method: "hash",
        class_filter: None,
        runtime: "js_bcrypt_hash", args: &[NA_F64, NA_F64], ret: NR_PTR },
    NativeModSig { module: "bcrypt", has_receiver: false, method: "compare",
        class_filter: None,
        runtime: "js_bcrypt_compare", args: &[NA_F64, NA_F64], ret: NR_PTR },

    // ========== perry/thread (parallelMap, parallelFilter, spawn) ==========
    // Runtime expects both args as NaN-boxed f64 values and returns the same
    // — no unboxing/reboxing needed on either side. Closure is a POINTER_TAG'd
    // ClosureHeader; the runtime reads `func_ptr` and calls it per element.
    NativeModSig { module: "perry/thread", has_receiver: false, method: "parallelMap",
        class_filter: None,
        runtime: "js_thread_parallel_map", args: &[NA_F64, NA_F64], ret: NR_F64 },
    NativeModSig { module: "perry/thread", has_receiver: false, method: "parallelFilter",
        class_filter: None,
        runtime: "js_thread_parallel_filter", args: &[NA_F64, NA_F64], ret: NR_F64 },
    NativeModSig { module: "perry/thread", has_receiver: false, method: "spawn",
        class_filter: None,
        runtime: "js_thread_spawn", args: &[NA_F64], ret: NR_F64 },
];

/// Walk a statement to collect LocalIds declared inside a closure body —
/// `Stmt::Let` and `Stmt::For` init `let`s. Used by the perry/thread
/// thread-safety check to distinguish inner locals (safe to write) from
/// captures (unsafe). Recurses into nested control-flow but deliberately
/// NOT into nested closures: those have their own inner-id set.
fn collect_closure_introduced_ids(
    stmt: &perry_hir::Stmt,
    out: &mut std::collections::HashSet<perry_types::LocalId>,
) {
    use perry_hir::Stmt;
    match stmt {
        Stmt::Let { id, .. } => { out.insert(*id); }
        Stmt::If { then_branch, else_branch, .. } => {
            for s in then_branch { collect_closure_introduced_ids(s, out); }
            if let Some(eb) = else_branch {
                for s in eb { collect_closure_introduced_ids(s, out); }
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            for s in body { collect_closure_introduced_ids(s, out); }
        }
        Stmt::For { init, body, .. } => {
            if let Some(init_stmt) = init.as_ref() {
                collect_closure_introduced_ids(init_stmt, out);
            }
            for s in body { collect_closure_introduced_ids(s, out); }
        }
        Stmt::Try { body, catch, finally } => {
            for s in body { collect_closure_introduced_ids(s, out); }
            if let Some(cc) = catch {
                if let Some((id, _)) = &cc.param { out.insert(*id); }
                for s in &cc.body { collect_closure_introduced_ids(s, out); }
            }
            if let Some(fb) = finally {
                for s in fb { collect_closure_introduced_ids(s, out); }
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases {
                for s in &case.body { collect_closure_introduced_ids(s, out); }
            }
        }
        Stmt::Labeled { body, .. } => collect_closure_introduced_ids(body, out),
        _ => {} // Expr, Return, Throw, Break, Continue, LabeledBreak/Continue — don't declare locals
    }
}

/// Walk a statement looking for LocalSet / Update whose target LocalId is
/// NOT in `inner_ids` — i.e. the closure is writing to a captured or
/// module-level variable. Does NOT recurse into nested Closure expressions
/// (those are a separate scope with their own check when they're passed to
/// a threading primitive).
fn find_outer_writes_stmt(
    stmt: &perry_hir::Stmt,
    inner_ids: &std::collections::HashSet<perry_types::LocalId>,
    out: &mut Vec<perry_types::LocalId>,
) {
    use perry_hir::Stmt;
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(expr) = init { find_outer_writes_expr(expr, inner_ids, out); }
        }
        Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => {
            find_outer_writes_expr(e, inner_ids, out);
        }
        Stmt::Return(None) | Stmt::Break | Stmt::Continue
        | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
        Stmt::If { condition, then_branch, else_branch } => {
            find_outer_writes_expr(condition, inner_ids, out);
            for s in then_branch { find_outer_writes_stmt(s, inner_ids, out); }
            if let Some(eb) = else_branch {
                for s in eb { find_outer_writes_stmt(s, inner_ids, out); }
            }
        }
        Stmt::While { condition, body } => {
            find_outer_writes_expr(condition, inner_ids, out);
            for s in body { find_outer_writes_stmt(s, inner_ids, out); }
        }
        Stmt::DoWhile { condition, body } => {
            for s in body { find_outer_writes_stmt(s, inner_ids, out); }
            find_outer_writes_expr(condition, inner_ids, out);
        }
        Stmt::For { init, condition, update, body } => {
            if let Some(init_stmt) = init.as_ref() {
                find_outer_writes_stmt(init_stmt, inner_ids, out);
            }
            if let Some(c) = condition { find_outer_writes_expr(c, inner_ids, out); }
            if let Some(u) = update { find_outer_writes_expr(u, inner_ids, out); }
            for s in body { find_outer_writes_stmt(s, inner_ids, out); }
        }
        Stmt::Try { body, catch, finally } => {
            for s in body { find_outer_writes_stmt(s, inner_ids, out); }
            if let Some(cc) = catch {
                for s in &cc.body { find_outer_writes_stmt(s, inner_ids, out); }
            }
            if let Some(fb) = finally {
                for s in fb { find_outer_writes_stmt(s, inner_ids, out); }
            }
        }
        Stmt::Switch { discriminant, cases } => {
            find_outer_writes_expr(discriminant, inner_ids, out);
            for case in cases {
                if let Some(val) = &case.test {
                    find_outer_writes_expr(val, inner_ids, out);
                }
                for s in &case.body { find_outer_writes_stmt(s, inner_ids, out); }
            }
        }
        Stmt::Labeled { body, .. } => find_outer_writes_stmt(body, inner_ids, out),
    }
}

fn find_outer_writes_expr(
    expr: &perry_hir::Expr,
    inner_ids: &std::collections::HashSet<perry_types::LocalId>,
    out: &mut Vec<perry_types::LocalId>,
) {
    use perry_hir::Expr;
    match expr {
        Expr::LocalSet(id, val) => {
            if !inner_ids.contains(id) { out.push(*id); }
            find_outer_writes_expr(val, inner_ids, out);
        }
        Expr::Update { id, .. } => {
            if !inner_ids.contains(id) { out.push(*id); }
        }
        Expr::Closure { .. } => {
            // Stop at nested closure boundary — it has its own scope and
            // will be checked separately if it's the one being passed to
            // a threading primitive.
        }
        Expr::Binary { left, right, .. } => {
            find_outer_writes_expr(left, inner_ids, out);
            find_outer_writes_expr(right, inner_ids, out);
        }
        Expr::Call { callee, args, .. } => {
            find_outer_writes_expr(callee, inner_ids, out);
            for a in args { find_outer_writes_expr(a, inner_ids, out); }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object { find_outer_writes_expr(o, inner_ids, out); }
            for a in args { find_outer_writes_expr(a, inner_ids, out); }
        }
        Expr::PropertyGet { object, .. } => {
            find_outer_writes_expr(object, inner_ids, out);
        }
        Expr::IndexGet { object, index } => {
            find_outer_writes_expr(object, inner_ids, out);
            find_outer_writes_expr(index, inner_ids, out);
        }
        Expr::Array(elems) => for e in elems { find_outer_writes_expr(e, inner_ids, out); }
        Expr::Conditional { condition, then_expr, else_expr } => {
            find_outer_writes_expr(condition, inner_ids, out);
            find_outer_writes_expr(then_expr, inner_ids, out);
            find_outer_writes_expr(else_expr, inner_ids, out);
        }
        _ => {} // Literals, LocalGet, GlobalGet, etc. — no writes
    }
}

/// Look up a native module method in the static dispatch table.
/// Entries with `class_filter: Some("Pool")` only match when
/// `class_name == Some("Pool")`; entries with `class_filter: None`
/// match any class_name. More-specific entries (with class_filter)
/// are checked first.
fn native_module_lookup(module: &str, has_receiver: bool, method: &str, class_name: Option<&str>) -> Option<&'static NativeModSig> {
    // First pass: look for an exact class_filter match.
    let exact = NATIVE_MODULE_TABLE.iter().find(|sig| {
        sig.module == module && sig.has_receiver == has_receiver && sig.method == method
            && sig.class_filter.is_some() && sig.class_filter == class_name
    });
    if exact.is_some() {
        return exact;
    }
    // Second pass: generic (class_filter == None) entries.
    NATIVE_MODULE_TABLE.iter().find(|sig| {
        sig.module == module && sig.has_receiver == has_receiver && sig.method == method
            && sig.class_filter.is_none()
    })
}

/// Lower a native module call through the dispatch table.
/// For receiver-less calls, `recv_i64` should be None.
/// For instance method calls, `recv_i64` should be Some(handle_i64_ssa).
fn lower_native_module_dispatch(
    ctx: &mut FnCtx<'_>,
    sig: &NativeModSig,
    recv_i64: Option<&str>,
    args: &[Expr],
) -> Result<String> {
    // Build the LLVM arg list: receiver handle (if any) + coerced args.
    let mut llvm_args: Vec<(crate::types::LlvmType, String)> = Vec::new();
    let mut arg_types: Vec<crate::types::LlvmType> = Vec::new();

    // Receiver handle
    if let Some(handle) = recv_i64 {
        llvm_args.push((I64, handle.to_string()));
        arg_types.push(I64);
    }

    // Coerce each arg per the sig's coercion rules.
    // If more args are passed than the sig declares, pass extras as F64.
    for (i, arg) in args.iter().enumerate() {
        let kind = sig.args.get(i).copied().unwrap_or(NativeArgKind::F64);
        let lowered = lower_expr(ctx, arg)?;
        match kind {
            NativeArgKind::F64 => {
                llvm_args.push((DOUBLE, lowered));
                arg_types.push(DOUBLE);
            }
            NativeArgKind::StrPtr => {
                let blk = ctx.block();
                let ptr = blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &lowered)]);
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
        }
    }
    // If fewer args than sig expects, pad with undefined / 0.
    for i in args.len()..sig.args.len() {
        match sig.args[i] {
            NativeArgKind::F64 => {
                llvm_args.push((DOUBLE, double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))));
                arg_types.push(DOUBLE);
            }
            NativeArgKind::StrPtr | NativeArgKind::PtrI64 | NativeArgKind::JsvalI64 => {
                llvm_args.push((I64, "0".to_string()));
                arg_types.push(I64);
            }
        }
    }

    // Determine return type for the declare
    let ret_type = match sig.ret {
        NativeRetKind::Ptr | NativeRetKind::Str => I64,
        NativeRetKind::F64 => DOUBLE,
        NativeRetKind::I32Void => I32,
        NativeRetKind::Void => crate::types::VOID,
    };

    ctx.pending_declares.push((sig.runtime.to_string(), ret_type, arg_types));

    let arg_slices: Vec<(crate::types::LlvmType, &str)> =
        llvm_args.iter().map(|(t, s)| (*t, s.as_str())).collect();

    match sig.ret {
        NativeRetKind::Ptr => {
            let blk = ctx.block();
            let raw = blk.call(I64, sig.runtime, &arg_slices);
            Ok(nanbox_pointer_inline(blk, &raw))
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
        NativeRetKind::F64 => {
            Ok(ctx.block().call(DOUBLE, sig.runtime, &arg_slices))
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
}
