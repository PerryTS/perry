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
        // Loads in collecting loop components also retain their home. One
        // linear CFG walk covers loop carries without per-root dataflow.
        // Source-order
        // spans need not be precise CFG liveness: a false positive
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
        let collecting_loops = collecting_loop_blocks(function);
        let mut ordinal = 0usize;
        for bb in function.get_basic_blocks() {
            let mut next = bb.get_first_instruction();
            while let Some(inst) = next {
                next = inst.get_next_instruction();
                unsafe {
                    match inst.get_opcode() {
                        inkwell::values::InstructionOpcode::Store => {
                            let slot = LLVMGetOperand(inst.as_value_ref(), 1);
                            if let Some(last) = slots.get_mut(&slot) {
                                *last = ordinal;
                            }
                        }
                        inkwell::values::InstructionOpcode::Load => {
                            let slot = LLVMGetOperand(inst.as_value_ref(), 0);
                            if slots.contains_key(&slot) {
                                let block = LLVMGetInstructionParent(inst.as_value_ref());
                                if collecting_loops.contains(&block) {
                                    retained.insert(slot);
                                }
                            }
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
                let context = LLVMGetModuleContext(module.as_mut_ptr());
                let kind =
                    LLVMGetMDKindIDInContext(context, b"perry.native.home".as_ptr().cast(), 17);
                let node = LLVMMDNodeInContext2(context, std::ptr::null_mut(), 0);
                LLVMSetMetadata(slot, kind, LLVMMetadataAsValue(context, node));
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

// Kosaraju's algorithm visits each block/edge once. A load in a cyclic
// component containing two collecting calls can cross them on a later
// iteration even when source order places the load before either call.
fn collecting_loop_blocks(
    function: inkwell::values::FunctionValue<'_>,
) -> std::collections::HashSet<LLVMBasicBlockRef> {
    let blocks = function.get_basic_blocks();
    let indices: std::collections::HashMap<_, _> = blocks
        .iter()
        .enumerate()
        .map(|(i, block)| (block.as_mut_ptr(), i))
        .collect();
    let mut edges = vec![Vec::new(); blocks.len()];
    let mut reverse = vec![Vec::new(); blocks.len()];
    let mut calls = vec![0usize; blocks.len()];
    for (i, block) in blocks.iter().enumerate() {
        let mut instruction = block.get_first_instruction();
        while let Some(inst) = instruction {
            calls[i] += usize::from(rs4gc_call_may_collect(inst));
            instruction = inst.get_next_instruction();
        }
        if let Some(term) = block.get_terminator() {
            unsafe {
                for n in 0..LLVMGetNumSuccessors(term.as_value_ref()) {
                    let successor = LLVMGetSuccessor(term.as_value_ref(), n);
                    if let Some(&j) = indices.get(&successor) {
                        edges[i].push(j);
                        reverse[j].push(i);
                    }
                }
            }
        }
    }
    let mut seen = vec![false; blocks.len()];
    let mut order = Vec::with_capacity(blocks.len());
    for start in 0..blocks.len() {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut pending = vec![(start, 0)];
        while let Some((block, next)) = pending.last_mut() {
            if *next == edges[*block].len() {
                order.push(*block);
                pending.pop();
                continue;
            }
            let successor = edges[*block][*next];
            *next += 1;
            if !seen[successor] {
                seen[successor] = true;
                pending.push((successor, 0));
            }
        }
    }
    seen.fill(false);
    let mut retained = std::collections::HashSet::new();
    for start in order.into_iter().rev() {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut pending = vec![start];
        let mut component = Vec::new();
        let mut sites = 0;
        while let Some(block) = pending.pop() {
            component.push(block);
            sites += calls[block];
            for &predecessor in &reverse[block] {
                if !seen[predecessor] {
                    seen[predecessor] = true;
                    pending.push(predecessor);
                }
            }
        }
        let cyclic = component.len() > 1 || edges[start].contains(&start);
        if cyclic && sites >= 2 {
            retained.extend(component.into_iter().map(|i| blocks[i].as_mut_ptr()));
        }
    }
    retained
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
        let home_kind = LLVMGetMDKindIDInContext(context, b"perry.native.home".as_ptr().cast(), 17);
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
                            if LLVMGetMetadata(raw, home_kind).is_null() {
                                continue;
                            }
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
                // Inlining may add lifetime markers for an individual home.
                // This storage now lives with the frame range; an interior GEP
                // is not a valid LLVM 22 lifetime operand, and ending the full
                // range at one child's end would invalidate its other homes.
                let mut lifetime_ends = Vec::new();
                let mut use_ = LLVMGetFirstUse(slot);
                while !use_.is_null() {
                    let user = LLVMGetUser(use_);
                    if LLVMGetInstructionOpcode(user) == LLVMOpcode::LLVMCall {
                        let callee = LLVMGetCalledValue(user);
                        let mut length = 0;
                        let name = LLVMGetValueName2(callee, &mut length);
                        if !name.is_null() {
                            let name = std::slice::from_raw_parts(name.cast::<u8>(), length);
                            if name.starts_with(b"llvm.lifetime.") {
                                lifetime_ends.push(user);
                            }
                        }
                    }
                    use_ = LLVMGetNextUse(use_);
                }
                for marker in lifetime_ends {
                    LLVMInstructionEraseFromParent(marker);
                }
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
