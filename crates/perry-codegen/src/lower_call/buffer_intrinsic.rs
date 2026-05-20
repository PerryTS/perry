//! Issue #92 Buffer numeric-read intrinsics.
//!
//! Extracted from `lower_call.rs` (#1099, part of #1097) — pure move,
//! no behavior change. `try_emit_buffer_read_intrinsic` and its
//! classification helper inline `buf.readInt32BE(offset)`-style reads
//! as LLVM load + bswap + convert instead of a runtime dispatch.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, FnCtx};
use crate::types::{DOUBLE, I32, I8, PTR};

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
        "readUInt8" | "readUint8" => S {
            width_bytes: 1,
            swap: false,
            signed: false,
            is_float: false,
        },
        "readInt8" => S {
            width_bytes: 1,
            swap: false,
            signed: true,
            is_float: false,
        },
        "readUInt16BE" | "readUint16BE" => S {
            width_bytes: 2,
            swap: true,
            signed: false,
            is_float: false,
        },
        "readUInt16LE" | "readUint16LE" => S {
            width_bytes: 2,
            swap: false,
            signed: false,
            is_float: false,
        },
        "readInt16BE" => S {
            width_bytes: 2,
            swap: true,
            signed: true,
            is_float: false,
        },
        "readInt16LE" => S {
            width_bytes: 2,
            swap: false,
            signed: true,
            is_float: false,
        },
        "readUInt32BE" | "readUint32BE" => S {
            width_bytes: 4,
            swap: true,
            signed: false,
            is_float: false,
        },
        "readUInt32LE" | "readUint32LE" => S {
            width_bytes: 4,
            swap: false,
            signed: false,
            is_float: false,
        },
        "readInt32BE" => S {
            width_bytes: 4,
            swap: true,
            signed: true,
            is_float: false,
        },
        "readInt32LE" => S {
            width_bytes: 4,
            swap: false,
            signed: true,
            is_float: false,
        },
        "readFloatBE" => S {
            width_bytes: 4,
            swap: true,
            signed: true,
            is_float: true,
        },
        "readFloatLE" => S {
            width_bytes: 4,
            swap: false,
            signed: true,
            is_float: true,
        },
        "readDoubleBE" => S {
            width_bytes: 8,
            swap: true,
            signed: true,
            is_float: true,
        },
        "readDoubleLE" => S {
            width_bytes: 8,
            swap: false,
            signed: true,
            is_float: true,
        },
        _ => return None,
    })
}

pub(super) fn try_emit_buffer_read_intrinsic(
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
        1 => ("i8", None),
        2 => ("i16", Some("llvm.bswap.i16")),
        4 => ("i32", Some("llvm.bswap.i32")),
        8 => ("i64", Some("llvm.bswap.i64")),
        _ => unreachable!(),
    };
    let raw = blk.fresh_reg();
    blk.emit_raw(format!(
        "{} = load {}, ptr {}{}",
        raw, load_ty, elem_ptr, meta
    ));
    // Byte-swap for BE on multi-byte widths (swap.i8 doesn't exist; width=1
    // never has `swap=true` in the spec table anyway).
    let swapped = match (spec.swap, swap_intrinsic) {
        (true, Some(intr)) => {
            let r = blk.fresh_reg();
            blk.emit_raw(format!(
                "{} = call {} @{}({} {})",
                r, load_ty, intr, load_ty, raw
            ));
            r
        }
        _ => raw,
    };
    // Convert to f64.
    let result = if spec.is_float {
        // Float/double: bitcast int bits → float bits, then fpext f32→f64 if needed.
        let float_ty = if spec.width_bytes == 4 {
            "float"
        } else {
            "double"
        };
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
