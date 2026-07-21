//! Pad omitted trailing native-library call arguments with defined
//! sentinels (#5812 item 4).
//!
//! A `perry.nativeLibrary.functions` call only marshals the arguments the
//! caller actually passed. When the call omits trailing params the manifest
//! declares — e.g. `textureCreateView(tex)` where the `descriptor` param is
//! optional — the emitted LLVM call and its `pending_declares` declaration
//! end up with fewer ABI slots than the native function reads. The C/Rust
//! function then reads an uninitialized register/stack slot for the missing
//! param; for a `JsValue`/string descriptor that garbage gets dereferenced
//! (the `read_string` crash the #5812 reporter hit on win64).
//!
//! [`pad_omitted_native_params`] fills each omitted slot with a defined
//! null/zero sentinel of the correct ABI type. The sentinel is deliberately
//! NOT routed through the `js_native_abi_check_*` validators the present-arg
//! path uses: those throw on `undefined`, which would turn a previously
//! (luckily) working "native fn ignores the trailing param" call into a
//! runtime throw. A raw null/zero is what the wrappers already expect for an
//! absent optional — `read_string`/`read_bytes` null-check the handle and
//! return `None`, and numeric defaults read as 0. Structural multi-slot
//! descriptors (pod / pod+count / buffer+len) genuinely need an object/buffer
//! and have no sound empty form, so an omitted one routes through the normal
//! lowering helper (defined error) rather than fabricating a struct.

use anyhow::Result;
use perry_api_manifest::NativeAbiType;
use perry_hir::Expr;

use super::extern_func::{
    lower_buffer_and_len_param, lower_manifest_pod_param, lower_manifest_pod_view_param,
};
use crate::expr::FnCtx;
use crate::nanbox::double_literal;
use crate::types::{LlvmType, DOUBLE, F32, I32, I64, PTR};

/// Append a sentinel for every manifest param past `passed_args`. No-op when
/// the caller passed at least as many args as the manifest declares.
/// `abi_slot_index` is the running ABI slot count after the present args.
pub(super) fn pad_omitted_native_params(
    ctx: &mut FnCtx<'_>,
    manifest_params: &[NativeAbiType],
    passed_args: usize,
    abi_slot_index: usize,
    lowered: &mut Vec<String>,
    arg_types: &mut Vec<LlvmType>,
) -> Result<()> {
    if manifest_params.len() <= passed_args {
        return Ok(());
    }
    // The `lower_*` helpers borrow `ctx` mutably, so clone the omitted
    // descriptors up front rather than holding a borrow on the slice.
    let omitted: Vec<(usize, NativeAbiType)> = manifest_params
        .iter()
        .enumerate()
        .skip(passed_args)
        .map(|(idx, descriptor)| (idx, descriptor.clone()))
        .collect();
    let mut abi_slot_index = abi_slot_index;
    for (idx, descriptor) in &omitted {
        match descriptor {
            NativeAbiType::Pod(pod) => lower_manifest_pod_param(
                ctx,
                descriptor,
                pod,
                *idx,
                abi_slot_index,
                &Expr::Undefined,
                lowered,
                arg_types,
            )?,
            NativeAbiType::PodAndCount(pod) => lower_manifest_pod_view_param(
                ctx,
                descriptor,
                pod,
                *idx,
                abi_slot_index,
                &Expr::Undefined,
                lowered,
                arg_types,
            )?,
            NativeAbiType::BufferAndLen => lower_buffer_and_len_param(
                ctx,
                descriptor,
                *idx,
                abi_slot_index,
                &double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
                lowered,
                arg_types,
            ),
            // `String` and `Json` both occupy a single `*const StringHeader`
            // slot; a null header decodes to `None`/empty in the wrapper
            // (`read_string`/serde null-check), the right "absent" value.
            NativeAbiType::String | NativeAbiType::Json => {
                let null_ptr = ctx.block().inttoptr(I64, "0");
                lowered.push(null_ptr);
                arg_types.push(PTR);
            }
            NativeAbiType::F32 => {
                lowered.push("0.0".to_string());
                arg_types.push(F32);
            }
            NativeAbiType::Bool
            | NativeAbiType::I32
            | NativeAbiType::U32
            | NativeAbiType::BufferLen => {
                lowered.push("0".to_string());
                arg_types.push(I32);
            }
            NativeAbiType::I64
            | NativeAbiType::I64String
            | NativeAbiType::U64
            | NativeAbiType::USize
            | NativeAbiType::Ptr
            | NativeAbiType::HandleId
            | NativeAbiType::Handle(_)
            | NativeAbiType::Promise(_) => {
                lowered.push("0".to_string());
                arg_types.push(I64);
            }
            // JsValue / F64 / Void all occupy a NaN-boxed `double` slot;
            // `undefined` is the natural empty.
            NativeAbiType::JsValue | NativeAbiType::F64 | NativeAbiType::Void => {
                lowered.push(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                arg_types.push(DOUBLE);
            }
        }
        abi_slot_index += descriptor.abi_slot_count();
    }
    Ok(())
}
