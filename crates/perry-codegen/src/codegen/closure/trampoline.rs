//! Raw-return branches of typed closure entry trampolines.
use super::super::typed_abi::{TypedFunctionTrampolineKind, TypedParamRep};
use super::super::typed_entry::emit_typed_arg_to_raw_after_entry_tier;
use crate::types::{LlvmType, DOUBLE, I1, I32, I64};

pub(super) fn emit_typed_closure_trampoline_fast_value(
    blk: &mut crate::block::LlBlock,
    kind: TypedFunctionTrampolineKind,
    typed_name: &str,
    arg_names: &[String],
    arg_reps: &[TypedParamRep],
) -> String {
    match kind {
        TypedFunctionTrampolineKind::F64 => {
            let raw_args: Vec<String> = arg_names
                .iter()
                .zip(arg_reps.iter())
                .map(|(arg, rep)| emit_typed_arg_to_raw_after_entry_tier(blk, *rep, arg))
                .collect();
            let mut typed_args: Vec<(LlvmType, &str)> = Vec::with_capacity(raw_args.len() + 1);
            typed_args.push((I64, "%this_closure"));
            typed_args.extend(
                raw_args
                    .iter()
                    .zip(arg_reps.iter())
                    .map(|(arg, rep)| (rep.llvm_ty(), arg.as_str())),
            );
            blk.call(DOUBLE, typed_name, &typed_args)
        }
        TypedFunctionTrampolineKind::I32 => {
            let raw_args: Vec<String> = arg_names
                .iter()
                .zip(arg_reps.iter())
                .map(|(arg, rep)| emit_typed_arg_to_raw_after_entry_tier(blk, *rep, arg))
                .collect();
            let mut typed_args: Vec<(LlvmType, &str)> = Vec::with_capacity(raw_args.len() + 1);
            typed_args.push((I64, "%this_closure"));
            typed_args.extend(
                raw_args
                    .iter()
                    .zip(arg_reps.iter())
                    .map(|(arg, rep)| (rep.llvm_ty(), arg.as_str())),
            );
            let raw_i32 = blk.call(I32, typed_name, &typed_args);
            crate::expr::i32_to_nanbox(blk, &raw_i32)
        }
        TypedFunctionTrampolineKind::I1 => {
            let raw_args: Vec<String> = arg_names
                .iter()
                .zip(arg_reps.iter())
                .map(|(arg, rep)| emit_typed_arg_to_raw_after_entry_tier(blk, *rep, arg))
                .collect();
            let mut typed_args: Vec<(LlvmType, &str)> = Vec::with_capacity(raw_args.len() + 1);
            typed_args.push((I64, "%this_closure"));
            typed_args.extend(
                raw_args
                    .iter()
                    .zip(arg_reps.iter())
                    .map(|(arg, rep)| (rep.llvm_ty(), arg.as_str())),
            );
            let typed_i1 = blk.call(I1, typed_name, &typed_args);
            let typed_i32 = blk.zext(I1, &typed_i1, I32);
            crate::expr::i32_bool_to_nanbox(blk, &typed_i32)
        }
        TypedFunctionTrampolineKind::StringRef => {
            let raw_args: Vec<String> = arg_names
                .iter()
                .zip(arg_reps.iter())
                .map(|(arg, rep)| emit_typed_arg_to_raw_after_entry_tier(blk, *rep, arg))
                .collect();
            let mut typed_args: Vec<(LlvmType, &str)> = Vec::with_capacity(raw_args.len() + 1);
            typed_args.push((I64, "%this_closure"));
            typed_args.extend(
                raw_args
                    .iter()
                    .zip(arg_reps.iter())
                    .map(|(arg, rep)| (rep.llvm_ty(), arg.as_str())),
            );
            let raw_string = blk.call(I64, typed_name, &typed_args);
            blk.call(DOUBLE, "js_nanbox_string", &[(I64, &raw_string)])
        }
    }
}
