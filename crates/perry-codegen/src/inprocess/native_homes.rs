//! Native statepoint homes (#12023).
//!
//! Retain managed alloca contents until final emission, then merge their
//! storage into one frame range. LLVM 22 records gc-live allocas directly;
//! ordinary IR optimization removes entries without gc.relocate users, so
//! publication must follow that optimization. Volatile accesses preserve
//! collector writes and entry initialization in the intervening pipeline.
use super::*;
use llvm_sys::{core::*, prelude::*, LLVMOpcode, LLVMTypeKind};

/// Client-owned statepoint ID: upper word identifies a native root range;
/// lower word is its length in eight-byte GC words, derived from the LLVM
/// alloca's allocated type. The stack-map location remains the authority for
/// its address. No runtime registration or side table is involved.
use crate::gc_map::HOME_RANGE_ID;

pub(super) fn retain(module: &inkwell::module::Module<'_>) {
    for function in module.get_functions() {
        let (_, sites) = rs4gc_preflight_factors(function);
        if sites < 2 {
            continue;
        }
        // Representation choice only: a root that is read after multiple
        // intervening calls benefits from a stable home. Short lifetimes keep
        // ordinary SSA statepoints. Both are lowered by this one backend.
        // Source-order spans need not be precise CFG liveness: a false positive
        // merely retains a home; a false negative remains fully rooted SSA.
        let mut slots = std::collections::HashMap::new();
        for bb in function.get_basic_blocks() {
            let mut next = bb.get_first_instruction();
            while let Some(inst) = next {
                next = inst.get_next_instruction();
                if inst.get_opcode() == inkwell::values::InstructionOpcode::Alloca
                    && matches!(inst.get_allocated_type(),
                        Ok(inkwell::types::BasicTypeEnum::PointerType(ptr))
                        if ptr.get_address_space() == inkwell::AddressSpace::from(1u16))
                {
                    slots.insert(inst.as_value_ref(), 0usize);
                }
            }
        }
        let mut retained = std::collections::HashSet::new();
        let mut ordinal = 0usize;
        for bb in function.get_basic_blocks() {
            let mut next = bb.get_first_instruction();
            while let Some(inst) = next {
                next = inst.get_next_instruction();
                unsafe {
                    match inst.get_opcode() {
                        inkwell::values::InstructionOpcode::Store => {
                            if let Some(last) =
                                slots.get_mut(&LLVMGetOperand(inst.as_value_ref(), 1))
                            {
                                *last = ordinal;
                            }
                        }
                        inkwell::values::InstructionOpcode::Load => {
                            let slot = LLVMGetOperand(inst.as_value_ref(), 0);
                            if slots
                                .get(&slot)
                                .is_some_and(|last| ordinal.saturating_sub(*last) >= 2)
                            {
                                retained.insert(slot);
                            }
                        }
                        _ => {}
                    }
                }
                ordinal += usize::from(rs4gc_call_may_collect(inst));
            }
        }
        for slot in retained {
            unsafe {
                let mut use_ = LLVMGetFirstUse(slot);
                while !use_.is_null() {
                    let user = LLVMGetUser(use_);
                    match LLVMGetInstructionOpcode(user) {
                        LLVMOpcode::LLVMLoad | LLVMOpcode::LLVMStore => LLVMSetVolatile(user, 1),
                        _ => panic!("native managed home has an unsupported address use"),
                    }
                    use_ = LLVMGetNextUse(use_);
                }
            }
        }
    }
}

/// Publish a single native range on every statepoint. Combining storage late
/// prevents SROA splitting it back into one gc-live operand per local. The
/// range is initialized once at entry, including slots introduced by inlining
/// whose own initialization executes later in the function.
pub(super) fn publish(module: &inkwell::module::Module<'_>) -> Result<()> {
    unsafe {
        let context = LLVMGetModuleContext(module.as_mut_ptr());
        let builder = LLVMCreateBuilderInContext(context);
        let result = publish_with_builder(module, context, builder);
        LLVMDisposeBuilder(builder);
        result
    }
}

unsafe fn publish_with_builder(
    module: &inkwell::module::Module<'_>,
    context: LLVMContextRef,
    builder: LLVMBuilderRef,
) -> Result<()> {
    unsafe {
        for function in module.get_functions() {
            let mut slots = Vec::new();
            let mut points = Vec::new();
            for bb in function.get_basic_blocks() {
                let mut next = bb.get_first_instruction();
                while let Some(inst) = next {
                    next = inst.get_next_instruction();
                    let raw = inst.as_value_ref();
                    match inst.get_opcode() {
                        inkwell::values::InstructionOpcode::Alloca => {
                            let ty = LLVMGetAllocatedType(raw);
                            let (element, words) = match LLVMGetTypeKind(ty) {
                                LLVMTypeKind::LLVMPointerTypeKind => (ty, 1),
                                LLVMTypeKind::LLVMArrayTypeKind => {
                                    (LLVMGetElementType(ty), LLVMGetArrayLength2(ty))
                                }
                                _ => continue,
                            };
                            if LLVMGetTypeKind(element) == LLVMTypeKind::LLVMPointerTypeKind
                                && LLVMGetPointerAddressSpace(element) == 1
                            {
                                slots.push((raw, words));
                            }
                        }
                        inkwell::values::InstructionOpcode::Call
                        | inkwell::values::InstructionOpcode::Invoke => {
                            let callee = LLVMGetCalledValue(raw);
                            let mut len = 0;
                            let name = LLVMGetValueName2(callee, &mut len);
                            if !name.is_null()
                                && std::slice::from_raw_parts(name.cast::<u8>(), len)
                                    .starts_with(b"llvm.experimental.gc.statepoint.")
                            {
                                points.push(raw);
                            }
                        }
                        _ => {}
                    }
                }
            }
            if slots.is_empty() || points.is_empty() {
                continue;
            }
            let words: u64 = slots.iter().map(|(_, n)| n).sum();
            let count =
                u32::try_from(words).map_err(|_| anyhow!("native root range exceeds u32 words"))?;
            let pointer = LLVMPointerTypeInContext(context, 1);
            let array = LLVMArrayType2(pointer, words);
            let first = LLVMGetFirstInstruction(LLVMGetFirstBasicBlock(function.as_value_ref()));
            LLVMPositionBuilderBefore(builder, first);
            let home = LLVMBuildAlloca(builder, array, c"gc.homes".as_ptr());
            LLVMSetAlignment(home, 8);
            let mut offset = 0;
            for &(slot, size) in &slots {
                let mut indices = [
                    LLVMConstInt(LLVMInt32TypeInContext(context), 0, 0),
                    LLVMConstInt(LLVMInt64TypeInContext(context), offset, 0),
                ];
                let address = LLVMBuildInBoundsGEP2(
                    builder,
                    array,
                    home,
                    indices.as_mut_ptr(),
                    2,
                    c"gc.home".as_ptr(),
                );
                LLVMReplaceAllUsesWith(slot, address);
                offset += size;
            }
            // The homes are scanned from the first statepoint, including
            // before an inlined scope begins. Zero is an inert root word.
            let zero = LLVMConstInt(LLVMInt8TypeInContext(context), 0, 0);
            let bytes = LLVMConstInt(LLVMInt64TypeInContext(context), words * 8, 0);
            let init = LLVMBuildMemSet(builder, home, zero, bytes, 8);
            LLVMSetOperand(init, 3, LLVMConstInt(LLVMInt1TypeInContext(context), 1, 0));
            for &(slot, _) in &slots {
                LLVMInstructionEraseFromParent(slot);
            }
            for point in points {
                replace_statepoint(builder, context, point, home, count)?;
            }
        }
        Ok(())
    }
}

unsafe fn replace_statepoint(
    builder: LLVMBuilderRef,
    context: LLVMContextRef,
    old: LLVMValueRef,
    home: LLVMValueRef,
    words: u32,
) -> Result<()> {
    unsafe {
        let mut live = Vec::new();
        let mut bundles = Vec::new();
        for index in 0..LLVMGetNumOperandBundles(old) {
            let bundle = LLVMGetOperandBundleAtIndex(old, index);
            let mut len = 0;
            let tag = LLVMGetOperandBundleTag(bundle, &mut len);
            if std::slice::from_raw_parts(tag.cast::<u8>(), len) == b"gc-live" {
                for arg in 0..LLVMGetNumOperandBundleArgs(bundle) {
                    live.push(LLVMGetOperandBundleArgAtIndex(bundle, arg));
                }
                LLVMDisposeOperandBundle(bundle);
            } else {
                bundles.push(bundle);
            }
        }
        // Append: existing gc.relocate indices must continue to refer to the
        // original gc-live values.
        live.push(home);
        bundles.push(LLVMCreateOperandBundle(
            c"gc-live".as_ptr(),
            7,
            live.as_mut_ptr(),
            live.len() as u32,
        ));
        let nargs = LLVMGetNumArgOperands(old);
        let mut args = (0..nargs)
            .map(|i| LLVMGetOperand(old, i))
            .collect::<Vec<_>>();
        args[0] = LLVMConstInt(
            LLVMInt64TypeInContext(context),
            HOME_RANGE_ID | u64::from(words),
            0,
        );
        LLVMPositionBuilderBefore(builder, old);
        let new = if LLVMGetInstructionOpcode(old) == LLVMOpcode::LLVMInvoke {
            LLVMBuildInvokeWithOperandBundles(
                builder,
                LLVMGetCalledFunctionType(old),
                LLVMGetCalledValue(old),
                args.as_mut_ptr(),
                nargs,
                LLVMGetNormalDest(old),
                LLVMGetUnwindDest(old),
                bundles.as_mut_ptr(),
                bundles.len() as u32,
                c"".as_ptr(),
            )
        } else {
            LLVMBuildCallWithOperandBundles(
                builder,
                LLVMGetCalledFunctionType(old),
                LLVMGetCalledValue(old),
                args.as_mut_ptr(),
                nargs,
                bundles.as_mut_ptr(),
                bundles.len() as u32,
                c"".as_ptr(),
            )
        };
        LLVMSetInstructionCallConv(new, LLVMGetInstructionCallConv(old));
        for index in std::iter::once(u32::MAX).chain(0..=nargs) {
            let n = LLVMGetCallSiteAttributeCount(old, index);
            let mut attrs = vec![std::ptr::null_mut(); n as usize];
            LLVMGetCallSiteAttributes(old, index, attrs.as_mut_ptr());
            for attr in attrs {
                LLVMAddCallSiteAttribute(new, index, attr);
            }
        }
        let mut n = 0;
        let entries = LLVMInstructionGetAllMetadataOtherThanDebugLoc(old, &mut n);
        for index in 0..n {
            let kind = LLVMValueMetadataEntriesGetKind(entries, index as u32);
            let metadata = LLVMValueMetadataEntriesGetMetadata(entries, index as u32);
            LLVMSetMetadata(new, kind, LLVMMetadataAsValue(context, metadata));
        }
        LLVMDisposeValueMetadataEntries(entries);
        // Debug locations are not included in the enumeration above.
        let debug_kind = LLVMGetMDKindIDInContext(context, c"dbg".as_ptr(), 3);
        let debug = LLVMGetMetadata(old, debug_kind);
        if !debug.is_null() {
            LLVMSetMetadata(new, debug_kind, debug);
        }
        LLVMReplaceAllUsesWith(old, new);
        LLVMInstructionEraseFromParent(old);
        for bundle in bundles {
            LLVMDisposeOperandBundle(bundle);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "native_homes_tests.rs"]
mod tests;
