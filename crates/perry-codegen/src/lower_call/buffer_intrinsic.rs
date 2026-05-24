//! Issue #92 Buffer numeric-read intrinsics.
//!
//! Extracted from `lower_call.rs` (#1099, part of #1097) — pure move,
//! no behavior change. `try_emit_buffer_read_intrinsic` and its
//! classification helper inline `buf.readInt32BE(offset)`-style reads
//! as LLVM load + bswap + convert instead of a runtime dispatch.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{BufferAccessSpec, FnCtx};
use crate::types::{DOUBLE, I32};

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
    let access_spec = BufferAccessSpec::buffer_numeric_read(spec.width_bytes);
    let Some(proof) = crate::expr::lower_buffer_access_proof(ctx, object, &args[0], access_spec)?
    else {
        return Ok(None);
    };
    let emission = crate::expr::emit_buffer_access_pointer(ctx, &proof, access_spec);
    let blk = ctx.block();
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
        raw, load_ty, emission.elem_ptr, emission.alias_metadata
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
    let lowered = crate::expr::buffer_view_lowered_value(
        &emission.data_ptr,
        &emission.len_i32,
        proof.bounds.clone(),
        proof.alias.clone(),
    );
    ctx.record_lowered_value_with_access_mode(
        "BufferNumericRead",
        Some(proof.buffer_local_id),
        "BufferNumericRead.BufferView",
        &lowered,
        Some(proof.bounds),
        Some(proof.alias),
        Some(proof.access_mode),
        None,
        proof.may_emit_inbounds,
        proof.may_emit_noalias,
        vec![format!("width_bytes={}", spec.width_bytes)],
    );
    Ok(Some(result))
}
