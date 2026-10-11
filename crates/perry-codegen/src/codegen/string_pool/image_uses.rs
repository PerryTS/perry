//! Recover the raw address of a proven immutable image literal before LLVM.
//! Masking a boxed relocation hides that address from LLVM's constant folder.
//! Keep ordinary boxed consumers; replace only their raw-pointer derivation.
use crate::inst::{LlInst, LoadFlavor};
use crate::module::LlModule;
use crate::strings::StringPool;
use crate::types::{DOUBLE, I64, I8, PTR};
use std::collections::HashMap;

pub(super) fn fold(module: &mut LlModule, strings: &StringPool) {
    let handles: HashMap<_, _> = strings
        .iter()
        .map(|entry| {
            (
                format!("@{}", entry.handle_global),
                format!("@{}", super::image::name(entry)),
            )
        })
        .collect();
    for function in module.functions_mut() {
        let mut boxed = HashMap::new();
        let mut bits = HashMap::new();
        // Discover SSA origins separately: a dominator can be serialized after
        // a consumer block. Only immutable literal globals supply this proof.
        for block in function.blocks() {
            for inst in block.insts() {
                if let LlInst::Load {
                    dst,
                    ty: DOUBLE,
                    ptr,
                    flavor: LoadFlavor::Plain,
                } = inst
                {
                    if let Some(pointer) = handles.get(ptr) {
                        boxed.insert(dst.clone(), pointer.clone());
                    }
                }
            }
        }
        for block in function.blocks() {
            for inst in block.insts() {
                if let LlInst::Cast {
                    dst,
                    op: "bitcast",
                    from: DOUBLE,
                    v,
                    to: I64,
                } = inst
                {
                    if let Some(pointer) = boxed.get(v) {
                        bits.insert(dst.clone(), pointer.clone());
                    }
                }
            }
        }
        for block in function.blocks_mut() {
            let instructions = std::mem::take(block.insts_mut());
            for mut inst in instructions {
                let replacement = match &mut inst {
                    LlInst::Bin {
                        dst,
                        op: "and",
                        ty: I64,
                        a,
                        b,
                        ..
                    } if b == crate::nanbox::POINTER_MASK_I64 => {
                        bits.get(a).map(|pointer| (dst.clone(), pointer.clone()))
                    }
                    LlInst::Call {
                        dst: Some(dst),
                        ret: I64,
                        callee,
                        args,
                        ..
                    } if callee == "js_get_string_pointer_unified"
                        && args.len() == 1
                        && args[0].0 == DOUBLE =>
                    {
                        boxed
                            .get(&args[0].1)
                            .map(|pointer| (dst.clone(), pointer.clone()))
                    }
                    _ => None,
                };
                if let Some((dst, pointer)) = replacement {
                    // Ordinary SSA GEPs are supported by both native unit
                    // construction and the text reader. A pointer-valued GEP
                    // constant operand is not accepted by the native reader.
                    let address = format!("{dst}.image");
                    block.insts_mut().push(LlInst::Gep {
                        dst: address.clone(),
                        inbounds: false,
                        ty: I8,
                        ptr: pointer,
                        idxs: vec![(I64, crate::target_layout::GC_HEADER_SIZE_BYTES.to_string())],
                    });
                    inst = LlInst::Cast {
                        dst,
                        op: "ptrtoint",
                        from: PTR,
                        v: address,
                        to: I64,
                    };
                }
                block.insts_mut().push(inst);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_literal_raw_consumers_lose_tagging_and_materialization() {
        let mut module = LlModule::new("x86_64-unknown-linux-gnu");
        let mut strings = StringPool::with_prefix("proof".into());
        let index = strings.intern("key");
        let handle = format!("@{}", strings.entry(index).handle_global);
        let pointer = super::super::image::emit(&mut module, strings.entry(index));
        let init = format!(
            "bitcast (i64 add (i64 ptrtoint (ptr {pointer} to i64), i64 {}) to double)",
            crate::nanbox::STRING_TAG_I64
        );
        module.add_raw_global(format!(
            "@{} = internal externally_initialized constant double {}",
            strings.entry(index).handle_global,
            init
        ));
        module.declare_function("js_get_string_pointer_unified", I64, &[DOUBLE]);
        let block = module
            .define_function("probe", I64, vec![(DOUBLE, "%dynamic".into())])
            .create_block("entry");
        let literal = block.load(DOUBLE, &handle);
        let bits = block.bitcast_double_to_i64(&literal);
        let raw = block.and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
        block.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &literal)]);
        block.call(
            I64,
            "js_get_string_pointer_unified",
            &[(DOUBLE, "%dynamic")],
        );
        block.ret(I64, &raw);
        fold(&mut module, &strings);
        let ir = module.to_ir();
        assert_eq!(
            ir.matches(" = getelementptr i8, ptr @perry_image_literal_")
                .count(),
            2
        );
        assert_eq!(ir.matches(" = ptrtoint ptr %").count(), 2);
        assert!(!ir.contains(" = and i64"));
        assert_eq!(
            ir.matches("call i64 @js_get_string_pointer_unified")
                .count(),
            1
        );
        assert!(ir.contains("double %dynamic"));
        #[cfg(feature = "llvm-inprocess")]
        {
            let ctx = inkwell::context::Context::create();
            let native = crate::inprocess::parse_ir_text(&ctx, &module.skeleton_ir(), "image_fold")
                .expect("actual emitted image skeleton parses");
            let start = ir.find("define ").unwrap();
            let end = start + ir[start..].find("\n}").unwrap() + 2;
            crate::dialect::add_function_from_text(&ctx, &native, &ir[start..end])
                .expect("actual folded image operands construct natively");
            native.verify().expect("folded image function verifies");
        }
    }
}
