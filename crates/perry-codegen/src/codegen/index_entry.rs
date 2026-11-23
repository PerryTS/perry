//! The raw-i32 entry of an indexed method clone (`$idx_u31_N`).
//!
//! Every route into an indexed clone has already proved each index argument
//! a non-negative signed i32: the public entry with the exact-i32 guard
//! (`emit_guarded_nonnegative_index`), a proven-Number call site with the same
//! guard's plain-double half (`lower_call::index_split`), and the remaining
//! direct routes with an integer array-index proof. The clone body therefore
//! takes those parameters as raw `i32` (the representation-selection
//! specialized ABI, RFC §5.4, with an `I32` slot), under
//! [`raw_body_name`]. The routes that hold the proved `i32` (the guard and the
//! split) pass it straight through, so the body no longer re-derives it from
//! the double it was handed.
//!
//! The clone's own name keeps the boxed `double` ABI as a one-call adapter
//! ([`emit_double_adapter`]): every other route still names that symbol, and
//! the adapter performs exactly the conversion the body used to perform at
//! entry (`emit_typed_i32_raw_assuming_guarded`).
//!
//! `PERRY_INDEX_RAW_I32=0` keeps the boxed-ABI clone body.

use std::collections::HashSet;

use perry_hir::Function;

use crate::module::LlModule;
use crate::types::{LlvmType, DOUBLE, I32};

pub(crate) fn raw_i32_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_INDEX_RAW_I32").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// The raw body of the indexed clone named `clone_name`.
pub(crate) fn raw_body_name(clone_name: &str) -> String {
    format!("{clone_name}$i32")
}

/// The LLVM name of a raw index parameter inside the raw body.
pub(crate) fn raw_param_name(id: u32) -> String {
    format!("%argi{id}")
}

/// Define `clone_name` as the boxed-ABI adapter of its raw body and return the
/// raw body's name, which the caller then defines.
pub(crate) fn emit_double_adapter(
    llmod: &mut LlModule,
    clone_name: &str,
    method: &Function,
    raw_params: &HashSet<u32>,
) -> String {
    let body_name = raw_body_name(clone_name);
    let mut params: Vec<(LlvmType, String)> = Vec::with_capacity(method.params.len() + 1);
    params.push((DOUBLE, "%this_arg".to_string()));
    for p in &method.params {
        params.push((DOUBLE, format!("%arg{}", p.id)));
    }
    let wf = llmod.define_function(clone_name, DOUBLE, params);
    wf.linkage = "internal".to_string();
    wf.pre_statepoint_inline = true;
    let _ = wf.create_block("entry");
    let blk = wf.block_mut(0).unwrap();
    let mut call_args: Vec<(LlvmType, String)> = Vec::with_capacity(method.params.len() + 1);
    call_args.push((DOUBLE, "%this_arg".to_string()));
    for p in &method.params {
        let arg = format!("%arg{}", p.id);
        if raw_params.contains(&p.id) {
            let raw = super::typed_abi::emit_typed_i32_raw_assuming_guarded(blk, &arg);
            call_args.push((I32, raw));
        } else {
            call_args.push((DOUBLE, arg));
        }
    }
    let call_args: Vec<(LlvmType, &str)> = call_args
        .iter()
        .map(|(ty, value)| (*ty, value.as_str()))
        .collect();
    let value = blk.call(DOUBLE, &body_name, &call_args);
    blk.ret(DOUBLE, &value);
    body_name
}

/// Arguments for a direct call of the raw body: `args[0]` is the receiver and
/// user argument `k` is `args[k + 1]`; the argument at each `positions[i]` is
/// replaced by its proved `raws[i]`.
pub(crate) fn raw_body_args<'a>(
    args: &[(LlvmType, &'a str)],
    positions: &[usize],
    raws: &'a [String],
) -> Vec<(LlvmType, &'a str)> {
    let mut out = args.to_vec();
    for (pos, raw) in positions.iter().zip(raws) {
        out[pos + 1] = (I32, raw.as_str());
    }
    out
}
