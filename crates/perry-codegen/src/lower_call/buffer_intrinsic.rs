//! Issue #92 Buffer numeric-read intrinsics.
//!
//! Extracted from `lower_call.rs` (#1099, part of #1097) — pure move,
//! no behavior change. `try_emit_buffer_read_intrinsic` and its
//! classification helper inline `buf.readInt32BE(offset)`-style reads
//! as LLVM load + bswap + `LoweredValue` instead of a runtime dispatch.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{access_facts_for_spec, attach_buffer_view_facts, BufferAccessSpec, FnCtx};
use crate::native_value::{BufferEndian, LoweredValue};
use crate::types::{DOUBLE, F32, I1, I32, I64, PTR};
use perry_abi::BuiltinProto;

/// Issue #92: inline Buffer numeric reads (`buf.readInt32BE(offset)` etc.)
/// as LLVM load + bswap + convert instead of a runtime dispatch through
/// `js_native_call_method`. Called from the PropertyGet branch below when
/// the receiver is a Buffer / Uint8Array and the method name matches one
/// of the Node-style numeric read accessors. Returns `Ok(None)` when
/// intrinsification isn't possible (the generic path then catches it) —
/// currently that's any receiver that isn't a tracked `buffer_data_slot`.
///
/// The folded body is the builtin `Buffer.prototype.<read>` only while that
/// prototype is unwritten: the realm declares every numeric reader an OWN
/// member of Buffer.prototype, so the exact fact is Buffer.prototype's own
/// shadow bit, the same word the runtime's Buffer name dispatch reads
/// (`prototype_unwritten(BuiltinProto::Buffer)`, perry-runtime
/// `object/buffer_dispatch.rs`). Every fold tests it; a set bit (any write,
/// define, delete or relink of Buffer.prototype, however the key was built,
/// by eval or `new Function` included) takes the ordinary dispatch call
/// ([`emit_dispatch_call`]), which reads the same bit. An own property on the
/// receiver is the operand proof's business: a local view is trusted only
/// while its binding is sealed (`collectors/sealed_buffers.rs`), and a
/// parameter read tests the receiver's property bag.
struct BufferNumericReadSpec {
    width_bytes: u32,
    endian: BufferEndian,
    signed: bool,   // signed vs unsigned JS-number materialization
    is_float: bool, // true for readFloat*/readDouble*
}

fn classify_buffer_numeric_read(method: &str) -> Option<BufferNumericReadSpec> {
    use BufferNumericReadSpec as S;
    Some(match method {
        "readUInt8" | "readUint8" => S {
            width_bytes: 1,
            endian: BufferEndian::Native,
            signed: false,
            is_float: false,
        },
        "readInt8" => S {
            width_bytes: 1,
            endian: BufferEndian::Native,
            signed: true,
            is_float: false,
        },
        "readUInt16BE" | "readUint16BE" => S {
            width_bytes: 2,
            endian: BufferEndian::Big,
            signed: false,
            is_float: false,
        },
        "readUInt16LE" | "readUint16LE" => S {
            width_bytes: 2,
            endian: BufferEndian::Little,
            signed: false,
            is_float: false,
        },
        "readInt16BE" => S {
            width_bytes: 2,
            endian: BufferEndian::Big,
            signed: true,
            is_float: false,
        },
        "readInt16LE" => S {
            width_bytes: 2,
            endian: BufferEndian::Little,
            signed: true,
            is_float: false,
        },
        "readUInt32BE" | "readUint32BE" => S {
            width_bytes: 4,
            endian: BufferEndian::Big,
            signed: false,
            is_float: false,
        },
        "readUInt32LE" | "readUint32LE" => S {
            width_bytes: 4,
            endian: BufferEndian::Little,
            signed: false,
            is_float: false,
        },
        "readInt32BE" => S {
            width_bytes: 4,
            endian: BufferEndian::Big,
            signed: true,
            is_float: false,
        },
        "readInt32LE" => S {
            width_bytes: 4,
            endian: BufferEndian::Little,
            signed: true,
            is_float: false,
        },
        "readFloatBE" => S {
            width_bytes: 4,
            endian: BufferEndian::Big,
            signed: false,
            is_float: true,
        },
        "readFloatLE" => S {
            width_bytes: 4,
            endian: BufferEndian::Little,
            signed: false,
            is_float: true,
        },
        "readDoubleBE" => S {
            width_bytes: 8,
            endian: BufferEndian::Big,
            signed: false,
            is_float: true,
        },
        "readDoubleLE" => S {
            width_bytes: 8,
            endian: BufferEndian::Little,
            signed: false,
            is_float: true,
        },
        _ => return None,
    })
}

/// True for a Buffer numeric READ accessor name (`readUInt8`, `readInt32BE`,
/// …) — the method family `try_emit_buffer_read_intrinsic` inline-folds
/// (one source of truth: `classify_buffer_numeric_read`).
pub(crate) fn is_buffer_numeric_read_method(name: &str) -> bool {
    classify_buffer_numeric_read(name).is_some()
}

fn emit_numeric_load(
    blk: &mut crate::block::LlBlock,
    spec: &BufferNumericReadSpec,
    element_ptr: &str,
    alias_metadata: &str,
) -> LoweredValue {
    // Load raw bytes at the correct width.
    let (load_ty, swap_intrinsic) = match spec.width_bytes {
        1 => ("i8", None),
        2 => ("i16", Some("llvm.bswap.i16")),
        4 => ("i32", Some("llvm.bswap.i32")),
        8 => ("i64", Some("llvm.bswap.i64")),
        _ => unreachable!(),
    };
    let raw = blk.fresh_reg();
    let load_align = 1;
    blk.emit_raw(format!(
        "{} = load {}, ptr {}, align {}{}",
        raw, load_ty, element_ptr, load_align, alias_metadata
    ));
    // Byte-swap for BE on multi-byte widths (swap.i8 doesn't exist; width=1
    // never has `swap=true` in the spec table anyway).
    let should_swap = spec.width_bytes > 1
        && spec.endian != BufferEndian::Native
        && spec.endian != target_endian();
    let swapped = match (should_swap, swap_intrinsic) {
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
    let result = if spec.is_float {
        // Float/double: bitcast int bits → native float bits. readFloat*
        // stays region-local as f32; JS boundaries fpext it explicitly.
        let float_ty = if spec.width_bytes == 4 { F32 } else { "double" };
        let as_float = blk.fresh_reg();
        blk.emit_raw(format!(
            "{} = bitcast {} {} to {}",
            as_float, load_ty, swapped, float_ty
        ));
        // #10779: `buf.readDoubleLE(i)` / `readFloatLE(i)` read arbitrary
        // user bytes, exactly like a `Float64Array` lane, so the same
        // canonicalisation applies before the value can reach a tag-dispatching
        // consumer. Integer `read*` accessors cannot be NaN and pay nothing.
        if spec.width_bytes == 4 {
            let canon = crate::expr::nanbox_inline::canonicalize_lane_f32(blk, &as_float);
            LoweredValue::f32(canon)
        } else {
            let canon = crate::expr::nanbox_inline::canonicalize_lane_f64(blk, &as_float);
            LoweredValue::f64(canon)
        }
    } else {
        // Integer: keep the raw i32 in the native lattice. Signed reads
        // materialize with `sitofp`; unsigned reads materialize with
        // `uitofp`, including `readUInt32*` values whose high bit is set.
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
                unreachable!("eight-byte integer readers are not classified here");
            }
            _ => unreachable!(),
        };
        if spec.signed {
            LoweredValue::i32(i32_val)
        } else {
            LoweredValue::u32(i32_val)
        }
    };
    result
}

/// The ordinary dispatch call of a Buffer reader, `recv.<method>(index)`:
/// `js_native_call_method`, whose Buffer arm tests the Buffer.prototype bit
/// itself and performs the real `[[Get]]` once it is set.
fn emit_dispatch_call(ctx: &mut FnCtx<'_>, boxed: &str, method: &str, index: &str) -> String {
    let argument = ctx.block().sitofp(I32, index, DOUBLE);
    let arguments = ctx.func.alloca_entry(DOUBLE);
    ctx.block().store(DOUBLE, &argument, &arguments);
    let key = ctx.strings.intern(method);
    let name = format!("@{}", ctx.strings.entry(key).bytes_global);
    let length = method.len();
    ctx.block().call(
        DOUBLE,
        "js_native_call_method",
        &[
            (DOUBLE, boxed),
            (PTR, &name),
            (I64, &length.to_string()),
            (PTR, &arguments),
            (I64, "1"),
        ],
    )
}

/// Buffer parameters use the same owner/view guard and retained starts as
/// indexed typed parameters. Named-method shadows and a written
/// Buffer.prototype go through normal dispatch.
fn emit_guarded_param_read(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    method: &str,
    index: &Expr,
    spec: &BufferNumericReadSpec,
) -> Result<LoweredValue> {
    let boxed = crate::expr::lower_expr(ctx, object)?;
    let index = crate::expr::lower_expr_as_i32(ctx, index)?;
    let slow = ctx.new_block("bytes.numeric.slow");
    let load = ctx.new_block("bytes.numeric.load");
    let done = ctx.new_block("bytes.numeric.done");
    let slow_l = ctx.block_label(slow);
    let load_l = ctx.block_label(load);
    let done_l = ctx.block_label(done);
    let access = crate::expr::byte_cell::resolve_read(
        ctx,
        object,
        &boxed,
        &[crate::runtime_abi::GC_TYPE_BUFFER],
        &slow_l,
    );
    let id = match object {
        Expr::LocalGet(id) => *id,
        _ => unreachable!(),
    };
    let cache = ctx
        .receiver_descriptors
        .byte_view_param(id)
        .unwrap()
        .clone();
    // No bag, no own props: an owner's link is 0 and an unbagged view's link
    // is its owner. A bagged view (link = bag) takes normal dispatch.
    // The proof already holds the owner untagged, so this is one compare.
    let owner_raw = ctx.block().load(I64, &cache.owner_raw_slot);
    let link_addr = ctx.block().add(
        I64,
        &access.raw,
        &crate::runtime_abi::BYTES_LINK.to_string(),
    );
    let link_ptr = ctx.block().inttoptr(I64, &link_addr);
    let link = ctx.block().load(I64, &link_ptr);
    let no_bag = ctx.block().icmp_eq(I64, &link, "0");
    let view = ctx.block().icmp_eq(I64, &link, &owner_raw);
    let no_shadow = ctx.block().or(I1, &view, &no_bag);
    let width = spec.width_bytes.to_string();
    let enough = ctx.block().icmp_uge(I32, &access.len, &width);
    let last = ctx.block().sub(I32, &access.len, &width);
    let in_bounds = ctx.block().icmp_ule(I32, &index, &last);
    let in_bounds = ctx.block().and(I1, &enough, &in_bounds);
    let admitted = ctx.block().and(I1, &no_shadow, &in_bounds);
    let written = crate::builtin_proto::emit_prototype_written(ctx, BuiltinProto::Buffer);
    let unwritten = ctx.block().xor(I1, &written, "true");
    let admitted = ctx.block().and(I1, &admitted, &unwritten);
    ctx.block().cond_br(&admitted, &load_l, &slow_l);
    ctx.current_block = load;
    let offset = ctx.block().zext(I32, &index, I64);
    let addr = ctx.block().add(I64, &access.data, &offset);
    let ptr = ctx.block().inttoptr(I64, &addr);
    let fast = emit_numeric_load(ctx.block(), spec, &ptr, "");
    let fast = crate::expr::materialize_js_value(
        ctx,
        fast,
        crate::native_value::MaterializationReason::FunctionAbi,
    );
    let fast_l = ctx.block().label.clone();
    ctx.block().br(&done_l);
    ctx.current_block = slow;
    let result = emit_dispatch_call(ctx, &boxed, method, &index);
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&done_l);
    ctx.current_block = done;
    let result = ctx
        .block()
        .phi(DOUBLE, &[(&fast, &fast_l), (&result, &slow_end)]);
    Ok(LoweredValue::js_value(result))
}

pub(super) fn try_emit_buffer_read_intrinsic(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    method: &str,
    args: &[Expr],
) -> Result<Option<LoweredValue>> {
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
    if let Expr::LocalGet(id) = object {
        if ctx.receiver_descriptors.byte_view_param(*id).is_some()
            && crate::expr::numeric_index_has_integer_array_index_proof(ctx, &args[0])
        {
            return emit_guarded_param_read(ctx, object, method, &args[0], &spec).map(Some);
        }
    }
    let access_spec = BufferAccessSpec::buffer_numeric_read(
        spec.width_bytes,
        spec.endian,
        spec.signed,
        spec.is_float,
    );
    let Some(proof) = crate::expr::lower_buffer_access_proof(ctx, object, &args[0], access_spec)?
    else {
        return Ok(None);
    };
    // The proof evaluated the index; the receiver is a local read. Both
    // arms below use these values, so nothing is evaluated twice.
    let boxed = crate::expr::lower_expr(ctx, object)?;
    let written = crate::builtin_proto::emit_prototype_written(ctx, BuiltinProto::Buffer);
    let direct = ctx.new_block("bytes.numeric.load");
    let generic = ctx.new_block("bytes.numeric.slow");
    let done = ctx.new_block("bytes.numeric.done");
    let direct_l = ctx.block_label(direct);
    let generic_l = ctx.block_label(generic);
    let done_l = ctx.block_label(done);
    ctx.block().cond_br(&written, &generic_l, &direct_l);
    ctx.current_block = generic;
    let generic_value = emit_dispatch_call(ctx, &boxed, method, &proof.index.value);
    let generic_end = ctx.block().label.clone();
    ctx.block().br(&done_l);
    ctx.current_block = direct;
    let emission = crate::expr::emit_buffer_access_pointer(
        ctx,
        &proof,
        access_spec,
        crate::expr::OutOfBoundsArm::Abort,
    );
    let result = emit_numeric_load(
        ctx.block(),
        &spec,
        &emission.elem_ptr,
        &emission.alias_metadata,
    );
    let buffer_view = crate::expr::buffer_view_lowered_value(
        &emission.data_ptr,
        &emission.len_i32,
        proof.view.elem.clone(),
        proof.view.element_width_bytes,
        proof.view.index_unit,
        proof.view.view_byte_offset,
        proof.view.length_offset_from_data,
        proof.bounds.clone(),
        proof.alias.clone(),
    );
    let facts = access_facts_for_spec(access_spec, &proof.view, Some(&emission.len_i32));
    ctx.record_lowered_value_with_access_mode_and_conversion(
        "BufferNumericRead",
        Some(proof.buffer_local_id),
        "BufferNumericRead.BufferView",
        &buffer_view,
        Some(proof.bounds.clone()),
        Some(proof.alias.clone()),
        Some(proof.access_mode),
        None,
        None,
        Some(facts),
        proof.may_emit_inbounds,
        proof.may_emit_noalias,
        vec![format!("width_bytes={}", spec.width_bytes)],
    );
    attach_buffer_view_facts(ctx, &proof.view);
    let result_consumer = match result.rep.name() {
        "i32" => "BufferNumericRead.native_i32",
        "u32" => "BufferNumericRead.native_u32",
        "f32" => "BufferNumericRead.native_f32",
        "f64" => "BufferNumericRead.native_f64",
        _ => "BufferNumericRead.native_value",
    };
    let facts = access_facts_for_spec(access_spec, &proof.view, Some(&emission.len_i32));
    ctx.record_lowered_value_with_access_mode_and_conversion(
        "BufferNumericRead",
        Some(proof.buffer_local_id),
        result_consumer,
        &result,
        Some(proof.bounds),
        Some(proof.alias),
        Some(proof.access_mode),
        None,
        None,
        Some(facts),
        false,
        false,
        vec![
            format!("method={}", method),
            format!("width_bytes={}", spec.width_bytes),
            format!("endian={:?}", spec.endian),
        ],
    );
    attach_buffer_view_facts(ctx, &proof.view);
    let direct_value = crate::expr::materialize_js_value(
        ctx,
        result,
        crate::native_value::MaterializationReason::FunctionAbi,
    );
    let direct_end = ctx.block().label.clone();
    ctx.block().br(&done_l);
    ctx.current_block = done;
    let merged = ctx.block().phi(
        DOUBLE,
        &[(&direct_value, &direct_end), (&generic_value, &generic_end)],
    );
    Ok(Some(LoweredValue::js_value(merged)))
}

fn target_endian() -> BufferEndian {
    if cfg!(target_endian = "big") {
        BufferEndian::Big
    } else {
        BufferEndian::Little
    }
}
