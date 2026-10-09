//! NaN-box operands read from the runtime's operand table instead of written
//! as 64-bit immediates (x86-64).
//!
//! Generated code compares, masks and offsets JS values against a handful of
//! 64-bit NaN-box constants (`POINTER_TAG`, `TAG_MASK`, `TAG_UNDEFINED`, ...).
//! x86-64 has no 64-bit immediate operand outside `movabs`, so every use
//! became a 10-byte `movabs` into a register and then the operation, and the
//! register allocator rematerializes `movabs` freely ("as cheap as a move"),
//! so one function wrote the same immediate again at most of its uses.
//!
//! The runtime holds these values once, in the read-only table
//! `PERRY_NANBOX_OPERANDS` (`perry_abi::NANBOX_OPERANDS`). After the
//! optimization pipeline, in every function the module marks
//! [`FUNCTION_ATTR`], each such operand of an integer operation, a compare, a
//! select, a store or a call argument becomes an `!invariant.load` of its
//! table entry, placed right before its user. Instruction selection folds
//! the load into the operation (`cmp PERRY_NANBOX_OPERANDS+8(%rip), %rax`,
//! one 7-byte instruction instead of a 10-byte `movabs` and the operation);
//! where it cannot fold, the load is a 7-byte `mov`. The register allocator
//! may still rematerialize the load, as it did the `movabs`, and MachineLICM
//! may hoist it, because an invariant load from a dereferenceable constant
//! is as free to move as the immediate was.
//!
//! The pass runs after the optimizer so that every IR-level fold (known bits,
//! instcombine's tag-test rewrites) still sees the constants. The table is
//! `external hidden constant`: the optimizer never sees its contents, and the
//! reference binds inside the image without a GOT. Codegen marks functions
//! only where the runtime archive is linked into the same image (an ELF
//! executable), so the hidden reference always resolves.

use inkwell::module::Module;
use llvm_sys::core::*;
use llvm_sys::prelude::*;
use llvm_sys::LLVMOpcode;

/// The function attribute codegen stamps on every function whose module may
/// read the table (`function.rs`, `LlModule::read_nanbox_operands_from_runtime`).
pub(crate) const FUNCTION_ATTR: &str = "perry-nanbox-operands";
/// The runtime's table (`perry-runtime/src/value/tags.rs`).
pub(crate) const TABLE_SYMBOL: &str = "PERRY_NANBOX_OPERANDS";

const TABLE: &[u64] = &crate::runtime_abi::NANBOX_OPERANDS;

fn table_index(value: u64) -> Option<usize> {
    TABLE.iter().position(|&v| v == value)
}

/// Rewrite the marked functions of `module` (see the module docs). Returns
/// the number of operands rewritten.
pub(crate) fn apply(module: &Module<'_>, effective_target: &str) -> usize {
    if !effective_target.starts_with("x86_64") {
        return 0;
    }
    // SAFETY: plain LLVM C API walks and edits of a module this thread owns;
    // every handle comes from the module and is used while it is alive.
    unsafe { rewrite_module(module.as_mut_ptr()) }
}

unsafe fn has_marker(f: LLVMValueRef) -> bool {
    let attr = LLVMGetStringAttributeAtIndex(
        f,
        llvm_sys::LLVMAttributeFunctionIndex,
        FUNCTION_ATTR.as_ptr().cast(),
        FUNCTION_ATTR.len() as u32,
    );
    !attr.is_null()
}

unsafe fn rewrite_module(m: LLVMModuleRef) -> usize {
    let ctx = LLVMGetModuleContext(m);
    let i64t = LLVMInt64TypeInContext(ctx);
    let table_ty = LLVMArrayType2(i64t, TABLE.len() as u64);
    let invariant_kind = LLVMGetMDKindIDInContext(ctx, c"invariant.load".as_ptr(), 14);
    let empty_node = LLVMMDNodeInContext2(ctx, std::ptr::null_mut(), 0);
    let empty_node = LLVMMetadataAsValue(ctx, empty_node);
    let builder = LLVMCreateBuilderInContext(ctx);
    let mut table: LLVMValueRef = std::ptr::null_mut();
    let mut entries: Vec<LLVMValueRef> = vec![std::ptr::null_mut(); TABLE.len()];
    let mut rewritten = 0usize;
    let mut f = LLVMGetFirstFunction(m);
    while !f.is_null() {
        if LLVMCountBasicBlocks(f) > 0 && has_marker(f) {
            let mut bb = LLVMGetFirstBasicBlock(f);
            while !bb.is_null() {
                let mut inst = LLVMGetFirstInstruction(bb);
                while !inst.is_null() {
                    for idx in rewritable_operands(inst) {
                        let op = LLVMGetOperand(inst, idx);
                        let Some(k) = table_operand(op, i64t) else {
                            continue;
                        };
                        if table.is_null() {
                            table = table_global(m, table_ty);
                        }
                        if entries[k].is_null() {
                            let mut gep_idx =
                                [LLVMConstInt(i64t, 0, 0), LLVMConstInt(i64t, k as u64, 0)];
                            entries[k] =
                                LLVMConstInBoundsGEP2(table_ty, table, gep_idx.as_mut_ptr(), 2);
                        }
                        LLVMPositionBuilderBefore(builder, inst);
                        let load = LLVMBuildLoad2(builder, i64t, entries[k], c"".as_ptr());
                        LLVMSetAlignment(load, 8);
                        LLVMSetMetadata(load, invariant_kind, empty_node);
                        LLVMSetOperand(inst, idx, load);
                        rewritten += 1;
                    }
                    inst = LLVMGetNextInstruction(inst);
                }
                bb = LLVMGetNextBasicBlock(bb);
            }
        }
        f = LLVMGetNextFunction(f);
    }
    LLVMDisposeBuilder(builder);
    rewritten
}

/// The table entry an operand names: an `i64` constant integer whose value is
/// in the table.
unsafe fn table_operand(op: LLVMValueRef, i64t: LLVMTypeRef) -> Option<usize> {
    if op.is_null() || LLVMIsAConstantInt(op).is_null() || LLVMTypeOf(op) != i64t {
        return None;
    }
    table_index(LLVMConstIntGetZExtValue(op))
}

/// Operand indices of `inst` that may hold a table value: both operands of an
/// integer operation or compare, the two values of a select, the value a
/// store writes, and the arguments of a call to a non-intrinsic function (of
/// a `gc.statepoint`, the wrapped call's arguments only). Never an operand
/// bundle (`gc-live`, `deopt`), never an intrinsic's argument (an `immarg`
/// must stay a constant), never a phi's incoming value or a switch case.
unsafe fn rewritable_operands(inst: LLVMValueRef) -> std::ops::Range<u32> {
    match LLVMGetInstructionOpcode(inst) {
        LLVMOpcode::LLVMAdd
        | LLVMOpcode::LLVMSub
        | LLVMOpcode::LLVMAnd
        | LLVMOpcode::LLVMOr
        | LLVMOpcode::LLVMXor
        | LLVMOpcode::LLVMICmp => 0..2,
        LLVMOpcode::LLVMSelect => 1..3,
        LLVMOpcode::LLVMStore => 0..1,
        LLVMOpcode::LLVMCall | LLVMOpcode::LLVMInvoke => call_arguments(inst),
        _ => 0..0,
    }
}

unsafe fn call_arguments(inst: LLVMValueRef) -> std::ops::Range<u32> {
    let callee = LLVMGetCalledValue(inst);
    if callee.is_null() || !LLVMIsAInlineAsm(callee).is_null() {
        return 0..0;
    }
    let args = LLVMGetNumArgOperands(inst);
    if LLVMIsAFunction(callee).is_null() {
        return 0..args;
    }
    if LLVMGetIntrinsicID(callee) == 0 {
        return 0..args;
    }
    // `gc.statepoint(i64 id, i32 patch, ptr target, i32 n, i32 flags,
    // <n call args>, i32 0, i32 0)`: only the wrapped call's arguments.
    let mut len = 0usize;
    let name = LLVMGetValueName2(callee, &mut len);
    let name = std::slice::from_raw_parts(name.cast::<u8>(), len);
    if !name.starts_with(b"llvm.experimental.gc.statepoint") || args < 5 {
        return 0..0;
    }
    let n = LLVMGetOperand(inst, 3);
    if LLVMIsAConstantInt(n).is_null() {
        return 0..0;
    }
    let n = LLVMConstIntGetZExtValue(n) as u32;
    5..(5 + n).min(args)
}

/// The module's declaration of the table, added on first use:
/// `@PERRY_NANBOX_OPERANDS = external hidden constant [N x i64], align 8`.
unsafe fn table_global(m: LLVMModuleRef, table_ty: LLVMTypeRef) -> LLVMValueRef {
    let name = std::ffi::CString::new(TABLE_SYMBOL).expect("symbol has no NUL");
    let existing = LLVMGetNamedGlobal(m, name.as_ptr());
    if !existing.is_null() {
        return existing;
    }
    let g = LLVMAddGlobal(m, table_ty, name.as_ptr());
    LLVMSetLinkage(g, llvm_sys::LLVMLinkage::LLVMExternalLinkage);
    LLVMSetVisibility(g, llvm_sys::LLVMVisibility::LLVMHiddenVisibility);
    LLVMSetGlobalConstant(g, 1);
    LLVMSetAlignment(g, 8);
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_holds_each_value_once() {
        for (i, v) in TABLE.iter().enumerate() {
            assert_eq!(table_index(*v), Some(i), "{v:#018x} listed twice");
        }
    }

    fn fixture(marked: bool) -> String {
        let attr = if marked {
            " \"perry-nanbox-operands\""
        } else {
            ""
        };
        // xor with TAG_UNDEFINED, or with POINTER_TAG, compare with
        // -POINTER_TAG: nothing the optimizer can fold away.
        format!(
            "define i64 @f(i64 %x, i64 %y){attr} {{\n\
             entry:\n\
             \x20 %a = xor i64 %x, 9222246136947933185\n\
             \x20 %b = or i64 %y, 9222527611924643840\n\
             \x20 %c = icmp ult i64 %a, -9222527611924643840\n\
             \x20 %s = add i64 %a, %b\n\
             \x20 %r = select i1 %c, i64 %s, i64 %b\n\
             \x20 ret i64 %r\n\
             }}\n"
        )
    }

    fn assembly(ir: &str) -> String {
        let context = inkwell::context::Context::create();
        let module = super::super::parse_ir_text(&context, ir, "nanbox_operands").expect("parses");
        let pieces = super::super::optimize_and_emit_module(
            &module,
            "x86_64-unknown-linux-gnu",
            &["-O2".into(), "-S".into()],
            false,
        )
        .expect("emits");
        String::from_utf8(super::super::single_piece(pieces)).expect("utf-8 assembly")
    }

    #[test]
    fn a_marked_function_reads_its_operands_from_the_table() {
        let asm = assembly(&fixture(true));
        assert!(
            !asm.contains("movabs"),
            "no 64-bit immediate may remain:\n{asm}"
        );
        for (value, what) in [
            (0x7FFC_0000_0000_0001_u64, "TAG_UNDEFINED"),
            (0x7FFD_0000_0000_0000, "POINTER_TAG"),
            (0x7FFD_0000_0000_0000_u64.wrapping_neg(), "-POINTER_TAG"),
        ] {
            let k = table_index(value).expect("a table value");
            let operand = match k * 8 {
                0 => format!("{TABLE_SYMBOL}(%rip)"),
                off => format!("{TABLE_SYMBOL}+{off}(%rip)"),
            };
            assert!(
                asm.contains(&operand),
                "{what} must be read as `{operand}`:\n{asm}"
            );
        }
        assert!(
            asm.contains(&format!(".hidden\t{TABLE_SYMBOL}")),
            "the table binds inside the image (hidden):\n{asm}"
        );
    }

    #[test]
    fn an_unmarked_function_keeps_its_immediates() {
        let asm = assembly(&fixture(false));
        assert!(asm.contains("movabs"), "{asm}");
        assert!(!asm.contains(TABLE_SYMBOL), "{asm}");
    }

    #[test]
    fn every_entry_needs_a_64_bit_immediate() {
        // A value a sign-extended 32-bit immediate can hold is already one
        // short instruction; reading it from memory would only add a load.
        for v in TABLE {
            let s = *v as i64;
            assert!(
                s < i32::MIN as i64 || s > i32::MAX as i64,
                "{v:#018x} fits an imm32"
            );
        }
    }
}
