//! Assign copy identities after LLVM has inlined, cloned and removed code.
//! Metadata operands remain image constants, including when LLVM joined
//! failure paths with a phi or select. Nothing here runs outside check mode.
use anyhow::{bail, Context, Result};
use inkwell::module::Module;
use llvm_sys::core::*;
use llvm_sys::prelude::*;
use llvm_sys::{LLVMLinkage, LLVMOpcode};
use serde_json::{json, Value};

pub(super) struct Inventory {
    origins: Vec<(String, u64)>,
}

impl Inventory {
    pub(super) fn capture(module: &Module<'_>) -> Result<Self> {
        let mut origins = Vec::new();
        unsafe {
            let mut global = LLVMGetFirstGlobal(module.as_mut_ptr());
            while !global.is_null() {
                if name(global).contains("__verify_") {
                    if let Some(meta) = metadata(global)? {
                        origins.push(origin(&meta)?);
                    }
                }
                global = LLVMGetNextGlobal(global);
            }
        }
        Ok(Self { origins })
    }

    pub(super) fn finish(self, module: &Module<'_>) -> Result<()> {
        unsafe {
            rewrite(module.as_mut_ptr())?;
        }
        crate::verify_facts::optimized(&self.origins);
        Ok(())
    }
}

unsafe fn name(value: LLVMValueRef) -> String {
    let mut n = 0;
    let p = LLVMGetValueName2(value, &mut n);
    if n == 0 {
        return String::new();
    }
    String::from_utf8_lossy(std::slice::from_raw_parts(p.cast::<u8>(), n)).into_owned()
}

unsafe fn metadata(global: LLVMValueRef) -> Result<Option<Value>> {
    if LLVMIsAGlobalVariable(global).is_null() {
        return Ok(None);
    }
    let init = LLVMGetInitializer(global);
    if init.is_null() || LLVMIsAConstantDataArray(init).is_null() {
        return Ok(None);
    }
    let mut n = 0;
    let p = LLVMGetAsString(init, &mut n);
    let bytes = std::slice::from_raw_parts(p.cast::<u8>(), n);
    if !bytes.starts_with(b"{") {
        return Ok(None);
    }
    let mut bytes = bytes.to_vec();
    bytes.push(b'}');
    let Ok(meta) = serde_json::from_slice::<Value>(&bytes) else {
        return Ok(None);
    };
    if meta["marker"] != "PERRY_FALSE_PROOF" {
        return Ok(None);
    }
    Ok(Some(meta))
}

fn origin(meta: &Value) -> Result<(String, u64)> {
    Ok((
        meta["module"]
            .as_str()
            .context("diagnostic module identity")?
            .to_owned(),
        meta["copy"]["emission"]
            .as_u64()
            .context("diagnostic emission identity")?,
    ))
}

unsafe fn rewrite(module: LLVMModuleRef) -> Result<()> {
    let context = LLVMGetModuleContext(module);
    let builder = LLVMCreateBuilderInContext(context);
    // Dispose the LLVM builder even when an unsupported operand rejects the compile.
    let result = rewrite_with(module, builder);
    LLVMDisposeBuilder(builder);
    result
}

unsafe fn rewrite_with(module: LLVMModuleRef, builder: LLVMBuilderRef) -> Result<()> {
    let mut ordinal = 0;
    let mut function = LLVMGetFirstFunction(module);
    while !function.is_null() {
        let body = name(function);
        let mut block = LLVMGetFirstBasicBlock(function);
        while !block.is_null() {
            let mut call = LLVMGetFirstInstruction(block);
            while !call.is_null() {
                let next = LLVMGetNextInstruction(call);
                if !LLVMIsACallInst(call).is_null() || !LLVMIsAInvokeInst(call).is_null() {
                    let mut callee = LLVMGetCalledValue(call);
                    let mut first = 0;
                    let mut count = LLVMGetNumArgOperands(call);
                    if name(callee).starts_with("llvm.experimental.gc.statepoint") {
                        callee = LLVMGetOperand(call, 2);
                        count = LLVMConstIntGetZExtValue(LLVMGetOperand(call, 3)) as u32;
                        first = 5;
                    }
                    if matches!(
                        name(callee).as_str(),
                        "perry_verify_facts_field"
                            | "perry_verify_facts_call"
                            | "perry_verify_facts_call_miss"
                            | "perry_verify_facts_miss"
                            | "perry_verify_facts_record_identity"
                            | "perry_verify_facts_entry"
                    ) {
                        let pointer_index = first + count - 2;
                        let pointer = LLVMGetOperand(call, pointer_index);
                        let copy = json!({"body":body,"instruction":ordinal});
                        let mut rewrite = Copy {
                            module,
                            builder,
                            copy: &copy,
                            ordinal,
                            leaf: 0,
                        };
                        let (pointer, length) =
                            rewrite.operand(pointer, call, 0).with_context(|| {
                                format!("diagnostic copy in {body}, call {ordinal}")
                            })?;
                        LLVMSetOperand(call, pointer_index, pointer);
                        LLVMSetOperand(call, pointer_index + 1, length);
                        ordinal += 1;
                    }
                }
                call = next;
            }
            block = LLVMGetNextBasicBlock(block);
        }
        function = LLVMGetNextFunction(function);
    }
    Ok(())
}

struct Copy<'a> {
    module: LLVMModuleRef,
    builder: LLVMBuilderRef,
    copy: &'a Value,
    ordinal: u64,
    leaf: u64,
}

impl Copy<'_> {
    unsafe fn operand(
        &mut self,
        pointer: LLVMValueRef,
        before: LLVMValueRef,
        depth: usize,
    ) -> Result<(LLVMValueRef, LLVMValueRef)> {
        if depth > 128 {
            bail!("cyclic or excessively nested diagnostic metadata operand");
        }
        let context = LLVMGetModuleContext(self.module);
        let i64t = LLVMInt64TypeInContext(context);
        if !LLVMIsAConstantExpr(pointer).is_null()
            && LLVMGetConstOpcode(pointer) == LLVMOpcode::LLVMPtrToInt
        {
            let global = LLVMGetOperand(pointer, 0);
            let mut meta =
                metadata(global)?.context("metadata pointer does not name static UTF-8")?;
            let (module, emission) = origin(&meta)?;
            meta["copy"]["lowered"] = self.copy.clone();
            let mut bytes = serde_json::to_vec(&meta)?;
            bytes.pop();
            let init = LLVMConstStringInContext2(context, bytes.as_ptr().cast(), bytes.len(), 1);
            let symbol = std::ffi::CString::new(format!(
                "__perry_verify_copy_{}_{}",
                self.ordinal, self.leaf
            ))?;
            self.leaf += 1;
            let copy = LLVMAddGlobal(self.module, LLVMTypeOf(init), symbol.as_ptr());
            LLVMSetInitializer(copy, init);
            LLVMSetGlobalConstant(copy, 1);
            LLVMSetLinkage(copy, LLVMLinkage::LLVMPrivateLinkage);
            LLVMSetAlignment(copy, 1);
            crate::verify_facts::emitted(&module, emission, meta["copy"].clone());
            return Ok((
                LLVMConstPtrToInt(copy, i64t),
                LLVMConstInt(i64t, bytes.len() as u64, 0),
            ));
        }
        if !LLVMIsAPHINode(pointer).is_null() {
            let block = LLVMGetInstructionParent(pointer);
            let insert = LLVMGetFirstInstruction(block);
            LLVMPositionBuilderBefore(self.builder, insert);
            let p = LLVMBuildPhi(self.builder, i64t, c"verify.copy".as_ptr());
            let n = LLVMBuildPhi(self.builder, i64t, c"verify.copy.len".as_ptr());
            for i in 0..LLVMCountIncoming(pointer) {
                let incoming = LLVMGetIncomingValue(pointer, i);
                let mut predecessor = LLVMGetIncomingBlock(pointer, i);
                let end = LLVMGetBasicBlockTerminator(predecessor);
                let (mut ip, mut il) = self.operand(incoming, end, depth + 1)?;
                LLVMAddIncoming(p, &mut ip, &mut predecessor, 1);
                LLVMAddIncoming(n, &mut il, &mut predecessor, 1);
            }
            return Ok((p, n));
        }
        if !LLVMIsASelectInst(pointer).is_null() {
            let cond = LLVMGetOperand(pointer, 0);
            let (ap, an) = self.operand(LLVMGetOperand(pointer, 1), before, depth + 1)?;
            let (bp, bn) = self.operand(LLVMGetOperand(pointer, 2), before, depth + 1)?;
            LLVMPositionBuilderBefore(self.builder, before);
            return Ok((
                LLVMBuildSelect(self.builder, cond, ap, bp, c"verify.copy".as_ptr()),
                LLVMBuildSelect(self.builder, cond, an, bn, c"verify.copy.len".as_ptr()),
            ));
        }
        bail!("unsupported diagnostic metadata operand; refusing an unchecked copy")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkwell::{
        context::Context,
        memory_buffer::MemoryBuffer,
        passes::PassBuilderOptions,
        targets::{CodeModel, RelocMode, Target, TargetTriple},
        OptimizationLevel,
    };
    use std::collections::BTreeSet;

    fn literal(emission: u64) -> (String, usize) {
        let mut bytes = serde_json::to_vec(&json!({"marker":"PERRY_FALSE_PROOF",
            "module":"fixture.ts","copy":{"body":"source","site":1,"emission":emission}}))
        .unwrap();
        bytes.pop();
        let escaped: String = bytes.iter().map(|byte| format!("\\{byte:02X}")).collect();
        (
            format!(
                "@site__verify_{emission} = private constant [{} x i8] c\"{escaped}\"\n",
                bytes.len()
            ),
            bytes.len(),
        )
    }

    #[test]
    fn llvm_inlined_copies_keep_distinct_static_reports() {
        super::super::global_init(&[]);
        let context = Context::create();
        let (global, length) = literal(0);
        let ir = format!(
            r#"{global}
            declare void @perry_verify_facts_entry(i64, i64)
            define internal void @body() alwaysinline {{
              call void @perry_verify_facts_entry(i64 ptrtoint (ptr @site__verify_0 to i64), i64 {length})
              ret void
            }}
            define void @first() {{ call void @body() ret void }}
            define void @second() {{ call void @body() ret void }}
        "#
        );
        let module = context
            .create_module_from_ir(MemoryBuffer::create_from_memory_range_copy(
                ir.as_bytes(),
                "copies",
            ))
            .unwrap();
        let inventory = Inventory::capture(&module).unwrap();
        let triple = TargetTriple::create("x86_64-unknown-linux-gnu");
        let machine = Target::from_triple(&triple)
            .unwrap()
            .create_target_machine(
                &triple,
                "generic",
                "",
                OptimizationLevel::Default,
                RelocMode::PIC,
                CodeModel::Default,
            )
            .unwrap();
        module
            .run_passes(
                "always-inline,globaldce",
                &machine,
                PassBuilderOptions::create(),
            )
            .unwrap();
        assert!(
            module.get_function("body").is_none(),
            "the fixture must really inline"
        );
        inventory.finish(&module).unwrap();
        module.verify().unwrap();
        let mut copies = BTreeSet::new();
        unsafe {
            let mut global = LLVMGetFirstGlobal(module.as_mut_ptr());
            while !global.is_null() {
                if name(global).starts_with("__perry_verify_copy_") {
                    let meta = metadata(global).unwrap().unwrap();
                    assert_eq!(meta["copy"]["body"], "source");
                    copies.insert(meta["copy"]["lowered"]["body"].as_str().unwrap().to_owned());
                }
                global = LLVMGetNextGlobal(global);
            }
        }
        assert_eq!(copies, BTreeSet::from(["first".into(), "second".into()]));
    }

    #[test]
    fn merged_failure_metadata_keeps_pointer_and_length_paired() {
        super::super::global_init(&[]);
        let context = Context::create();
        let (a, an) = literal(0);
        let (b, bn) = literal(1000);
        let ir = format!(
            r#"{a}{b}
            declare void @perry_verify_facts_entry(i64, i64)
            define void @choose(i1 %cond) {{
            start: br i1 %cond, label %a, label %b
            a: br label %join
            b: br label %join
            join:
              %ptr = phi i64 [ptrtoint (ptr @site__verify_0 to i64), %a], [ptrtoint (ptr @site__verify_1000 to i64), %b]
              %len = phi i64 [{an}, %a], [{bn}, %b]
              call void @perry_verify_facts_entry(i64 %ptr, i64 %len)
              ret void
            }}
        "#
        );
        let module = context
            .create_module_from_ir(MemoryBuffer::create_from_memory_range_copy(
                ir.as_bytes(),
                "merged",
            ))
            .unwrap();
        Inventory::capture(&module)
            .unwrap()
            .finish(&module)
            .unwrap();
        module.verify().unwrap();
        unsafe {
            let function = LLVMGetNamedFunction(module.as_mut_ptr(), c"choose".as_ptr());
            let join = LLVMGetLastBasicBlock(function);
            let mut call = LLVMGetFirstInstruction(join);
            while LLVMIsACallInst(call).is_null() {
                call = LLVMGetNextInstruction(call);
            }
            let pointers = LLVMGetOperand(call, 0);
            let lengths = LLVMGetOperand(call, 1);
            assert_eq!(LLVMCountIncoming(pointers), 2);
            assert_eq!(LLVMCountIncoming(lengths), 2);
            for i in 0..2 {
                let ptr = LLVMGetIncomingValue(pointers, i);
                let global = LLVMGetOperand(ptr, 0);
                let init = LLVMGetInitializer(global);
                let mut n = 0;
                LLVMGetAsString(init, &mut n);
                assert_eq!(
                    LLVMConstIntGetZExtValue(LLVMGetIncomingValue(lengths, i)),
                    n as u64
                );
                assert!(n > an, "final copy identity must grow the static payload");
            }
        }
    }
}
