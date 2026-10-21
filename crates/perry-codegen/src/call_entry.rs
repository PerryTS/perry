//! Callee-owned argument-count adaptation. The generic entry consumes the
//! caller's argument vector and calls the body's one fixed-signature operation.

use crate::fn_info::FnInfoFacts;
use crate::module::LlModule;
use crate::types::{LlvmType, DOUBLE, I1, I32, I64, PTR};

fn bind(
    function: &mut crate::function::LlFunction,
    index: usize,
    ty: LlvmType,
    value: &str,
) -> String {
    let slot = function.alloca_entry(ty);
    let block = function.block_mut(0).unwrap();
    block.store(ty, value, &slot);
    block.call_void(
        "js_shadow_slot_bind",
        &[(I32, &index.to_string()), (PTR, &slot)],
    );
    slot
}

pub(crate) fn emit(
    module: &mut LlModule,
    body: &str,
    params: usize,
    facts: &FnInfoFacts,
) -> String {
    let name = format!("{body}$call_entry");
    let count_ty = if crate::target_layout::target_is_ilp32(&module.target_triple) {
        I32
    } else {
        I64
    };
    let rest = facts.flags
        & (crate::runtime_abi::FN_REST_USER
            | crate::runtime_abi::FN_REST_SYNTHETIC_ARGUMENTS
            | crate::runtime_abi::FN_REST_USER_AND_ARGUMENTS)
        != 0;
    let fixed = if rest {
        usize::from(facts.rest_fixed)
    } else {
        params
    };
    module.declare_function(
        "js_closure_entry_bundle",
        crate::types::VOID,
        &[PTR, PTR, I64, I32, I32, PTR, PTR],
    );
    let function = module.define_function(
        &name,
        DOUBLE,
        vec![
            (PTR, "%entry_callee".into()),
            (I64, "%entry_this".into()),
            (PTR, "%entry_args".into()),
            (count_ty, "%entry_count".into()),
        ],
    );
    function.linkage = "internal".into();
    if !rest {
        // No value survives an allocation or user call in this adapter. The
        // operation roots its own inputs; an adapter frame would be redundant.
        function.create_block("entry");
        let undefined = function.alloca_entry(DOUBLE);
        let block = function.block_mut(0).unwrap();
        block.store(
            DOUBLE,
            &crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
            &undefined,
        );
        let callee = block.ptrtoint("%entry_callee", I64);
        let mut arguments = Vec::with_capacity(params);
        for i in 0..params {
            let provided = block.icmp_ugt(count_ty, "%entry_count", &i.to_string());
            let argument = block.gep(DOUBLE, "%entry_args", &[(count_ty, &i.to_string())]);
            let address = block.select(I1, &provided, PTR, &argument, &undefined);
            arguments.push(block.load(DOUBLE, &address));
        }
        let value = crate::expr::body_call::emit_js_body_operation_call(
            block,
            body,
            &callee,
            "%entry_this",
            &arguments,
        );
        block.ret(DOUBLE, &value);
        return name;
    }

    function.enable_shadow_frame((params + 2) as u32);
    function.create_block("entry");
    let callee = function
        .block_mut(0)
        .unwrap()
        .ptrtoint("%entry_callee", I64);
    let callee_slot = bind(function, 0, I64, &callee);
    let receiver = function
        .block_mut(0)
        .unwrap()
        .bitcast_i64_to_double("%entry_this");
    let this_slot = bind(function, 1, DOUBLE, &receiver);
    let undefined = function.alloca_entry(DOUBLE);
    function.block_mut(0).unwrap().store(
        DOUBLE,
        &crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
        &undefined,
    );
    let mut slots = Vec::with_capacity(params);
    for i in 0..fixed {
        let block = function.block_mut(0).unwrap();
        let provided = block.icmp_ugt(count_ty, "%entry_count", &i.to_string());
        let argument = block.gep(DOUBLE, "%entry_args", &[(count_ty, &i.to_string())]);
        // Select the address before loading: absent arguments are never read.
        let address = block.select(I1, &provided, PTR, &argument, &undefined);
        let value = block.load(DOUBLE, &address);
        slots.push(bind(function, i + 2, DOUBLE, &value));
    }
    if rest {
        let first = function.alloca_entry(DOUBLE);
        let second = function.alloca_entry(DOUBLE);
        let block = function.block_mut(0).unwrap();
        // Runtime declarations keep the codegen's native i64 count ABI.
        // The WASI adapter narrows it to Rust's target usize; this entry's
        // own pointer/count ABI already uses the target width.
        let bundle_count = if count_ty == I32 {
            block.zext(I32, "%entry_count", I64)
        } else {
            "%entry_count".to_string()
        };
        block.call_void(
            "js_closure_entry_bundle",
            &[
                (PTR, "%entry_callee"),
                (PTR, "%entry_args"),
                (I64, &bundle_count),
                (I32, &fixed.to_string()),
                (I32, &facts.flags.to_string()),
                (PTR, &first),
                (PTR, &second),
            ],
        );
        let first_value = function.block_mut(0).unwrap().load(DOUBLE, &first);
        slots.push(bind(function, fixed + 2, DOUBLE, &first_value));
        if facts.flags & crate::runtime_abi::FN_REST_USER_AND_ARGUMENTS != 0 {
            let second_value = function.block_mut(0).unwrap().load(DOUBLE, &second);
            slots.push(bind(function, fixed + 3, DOUBLE, &second_value));
        }
    }
    assert_eq!(slots.len(), params, "entry and body must agree: {body}");
    let block = function.block_mut(0).unwrap();
    let callee = block.load(I64, &callee_slot);
    let receiver = block.load(DOUBLE, &this_slot);
    let receiver = block.bitcast_double_to_i64(&receiver);
    let arguments: Vec<String> = slots.iter().map(|slot| block.load(DOUBLE, slot)).collect();
    let result = crate::expr::body_call::emit_js_body_operation_call(
        block, body, &callee, &receiver, &arguments,
    );
    block.ret(DOUBLE, &result);
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_loads_missing_values_through_undefined_before_the_operation() {
        let mut module = LlModule::new("x86_64-unknown-linux-gnu");
        let facts = FnInfoFacts::default();
        let name = emit(&mut module, "operation", 3, &facts);
        assert_eq!(name, "operation$call_entry");
        let ir = module.to_ir();
        assert_eq!(ir.matches("icmp ugt i64 %entry_count").count(), 3, "{ir}");
        assert_eq!(ir.matches("select i1").count(), 3, "{ir}");
        assert!(ir.contains("call double @operation(i64"), "{ir}");
        // Runtime vector callers borrow their plain input slice until this
        // one operation call: all values must be loaded before any call.
        assert_eq!(ir.matches("call ").count(), 1, "{ir}");
        assert!(!ir.contains("@dispatch_with_arity"), "{ir}");
    }

    #[test]
    fn rest_entry_materializes_once_and_keeps_the_arguments_count() {
        let mut module = LlModule::new("x86_64-unknown-linux-gnu");
        let mut facts = FnInfoFacts::default();
        facts.set_rest(1, crate::fn_info::RestKind::UserAndArguments);
        emit(&mut module, "operation", 3, &facts);
        let ir = module.to_ir();
        assert_eq!(
            ir.matches("call void @js_closure_entry_bundle(").count(),
            1,
            "{ir}"
        );
        assert!(ir.contains("i64 %entry_count, i32 1, i32 4"), "{ir}");
        assert!(!ir.contains("@js_array_push"), "{ir}");
    }
    #[test]
    fn class_method_function_objects_get_the_same_generic_entry() {
        let mut module = LlModule::new("x86_64-unknown-linux-gnu");
        let method = module.define_function(
            "method__eclo",
            DOUBLE,
            vec![
                (I64, "%callee".into()),
                (I64, "%this".into()),
                (DOUBLE, "%a0".into()),
                (DOUBLE, "%a1".into()),
            ],
        );
        method.create_block("entry").ret(DOUBLE, "%a0");
        module.note_fn_info("method__eclo", |facts| facts.set_declared(2));
        module.emit_fn_infos(false);
        let ir = module.to_ir();
        assert!(ir.contains("ptr @method__eclo$call_entry"), "{ir}");
        assert!(
            ir.contains("define internal double @method__eclo$call_entry(ptr %entry_callee"),
            "{ir}"
        );
    }
    #[test]
    fn a_wasm_entry_normalizes_its_count_for_the_runtime_adapter() {
        let mut module = LlModule::new("wasm32-unknown-wasi");
        let mut facts = FnInfoFacts::default();
        facts.set_rest(0, crate::fn_info::RestKind::User);
        emit(&mut module, "operation", 1, &facts);
        let ir = module.to_ir();
        assert!(ir.contains("i32 %entry_count)"), "{ir}");
        assert!(ir.contains("zext i32 %entry_count to i64"), "{ir}");
        assert!(
            ir.contains("@js_closure_entry_bundle(ptr, ptr, i64, i32, i32, ptr, ptr)"),
            "{ir}"
        );
    }
}
