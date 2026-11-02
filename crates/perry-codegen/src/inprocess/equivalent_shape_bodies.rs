//! Share exact retained shape bodies after useful inlining has finished.
//!
//! Plain `$pshape` symbols are producer capabilities, never JS callable
//! identities (collectors/proven_this.rs). Keep the ordinary callable and all
//! its metadata intact. An alias preserves the capability's external ABI
//! without a forwarding branch, root frame, or runtime lookup.

use inkwell::attributes::{Attribute, AttributeLoc};
use inkwell::module::{Linkage, Module};
use inkwell::types::AsTypeRef;
use inkwell::values::{AnyValue, AsValueRef, FunctionValue};
use llvm_sys::core::*;
use llvm_sys::LLVMOpcode;
use std::ffi::CString;

fn attributes_match(a: FunctionValue<'_>, b: FunctionValue<'_>) -> bool {
    let always = Attribute::get_named_enum_kind_id("alwaysinline");
    let hint = Attribute::get_named_enum_kind_id("inlinehint");
    let relevant = |attribute: &Attribute| {
        !attribute.is_enum() || ![always, hint].contains(&attribute.get_enum_kind_id())
    };
    a.attributes(AttributeLoc::Function)
        .into_iter()
        .filter(relevant)
        .eq(b
            .attributes(AttributeLoc::Function)
            .into_iter()
            .filter(relevant))
}

// Compare LLVM's complete final body, including all metadata operands and
// site IDs. Only the definition's own name, address-significance hint and
// attribute-group number may differ;
// the attributes themselves are compared above. No SSA or ID normalization.
fn exact_body(a: FunctionValue<'_>, b: FunctionValue<'_>) -> bool {
    if a.get_type() != b.get_type() || !attributes_match(a, b) {
        return false;
    }
    let a_ir = a.print_to_string().to_string();
    let b_ir = b.print_to_string().to_string();
    let Some(a_def) = a_ir.lines().position(|line| line.starts_with("define ")) else {
        return false;
    };
    let Some(b_def) = b_ir.lines().position(|line| line.starts_with("define ")) else {
        return false;
    };
    let a_name = a.get_name().to_string_lossy();
    let b_name = b.get_name().to_string_lossy();
    let header = |line: &str, name: &str| {
        line.replacen(&format!("@\"{name}\"("), "@__body(", 1)
            .replacen(&format!("@{name}("), "@__body(", 1)
    };
    let a_header = header(a_ir.lines().nth(a_def).unwrap(), &a_name);
    let b_header = header(b_ir.lines().nth(b_def).unwrap(), &b_name);
    let attr_group = |token: &&str| {
        !matches!(*token, "local_unnamed_addr" | "unnamed_addr")
            && !token
                .strip_prefix('#')
                .is_some_and(|rest| rest.bytes().all(|byte| byte.is_ascii_digit()))
    };
    a_header
        .split_whitespace()
        .filter(attr_group)
        .eq(b_header.split_whitespace().filter(attr_group))
        && a_ir
            .lines()
            .skip(a_def + 1)
            .eq(b_ir.lines().skip(b_def + 1))
}

fn only_direct_calls(function: FunctionValue<'_>) -> bool {
    unsafe {
        let value = function.as_value_ref();
        let mut usage = LLVMGetFirstUse(value);
        while !usage.is_null() {
            let user = LLVMGetUser(usage);
            if LLVMIsAInstruction(user).is_null()
                || !matches!(
                    LLVMGetInstructionOpcode(user),
                    LLVMOpcode::LLVMCall | LLVMOpcode::LLVMInvoke
                )
                || LLVMGetCalledValue(user) != value
            {
                return false;
            }
            // The same call may also pass the function as an argument. Its
            // user is a direct call, but that second use observes identity.
            if (0..LLVMGetNumOperands(user) as u32)
                .filter(|index| LLVMGetOperand(user, *index) == value)
                .count()
                != 1
            {
                return false;
            }
            usage = LLVMGetNextUse(usage);
        }
    }
    true
}

pub(super) fn apply(module: &Module<'_>) {
    // Walk existing definitions directly. The next pointer is captured before
    // deleting a redundant body; no side table or memoized body hashes.
    let mut next = module.get_first_function();
    while let Some(clone) = next {
        next = clone.get_next_function();
        let name = clone.get_name().to_string_lossy().into_owned();
        let Some(ordinary_name) = name.strip_suffix("$pshape") else {
            continue;
        };
        if !ordinary_name.starts_with("perry_method_")
            || clone.count_basic_blocks() == 0
            || clone.get_linkage() != Linkage::External
            || !only_direct_calls(clone)
        {
            continue;
        }
        let Some(ordinary) = module.get_function(ordinary_name) else {
            continue;
        };
        if ordinary.count_basic_blocks() == 0
            || ordinary.get_linkage() != Linkage::External
            || !exact_body(ordinary, clone)
        {
            continue;
        }
        unsafe {
            // Both definitions belong to this live LLVM module and have the
            // same ABI. Transfer every direct use to an alias before deleting
            // the clone; the ordinary body and its GC instructions stay put.
            let symbol = CString::new(name).expect("LLVM symbol has no NUL");
            LLVMSetValueName2(clone.as_value_ref(), c"".as_ptr(), 0);
            let alias = LLVMAddAlias2(
                module.as_mut_ptr(),
                ordinary.get_type().as_type_ref(),
                LLVMGetPointerAddressSpace(LLVMTypeOf(ordinary.as_value_ref())),
                ordinary.as_value_ref(),
                symbol.as_ptr(),
            );
            LLVMSetVisibility(alias, LLVMGetVisibility(clone.as_value_ref()));
            LLVMReplaceAllUsesWith(clone.as_value_ref(), alias);
            clone.delete();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkwell::context::Context;

    #[test]
    fn exact_capability_shares_body_but_callable_address_is_untouched() {
        let context = Context::create();
        let module = super::super::parse_ir_text(
            &context,
            r#"
@callable_identity = global ptr @perry_method_C_score
define double @perry_method_C_score(double %x) { ret double %x }
define double @perry_method_C_score$pshape(double %x) local_unnamed_addr alwaysinline { ret double %x }
define double @caller(double %x) {
  %v = call double @perry_method_C_score$pshape(double %x)
  ret double %v
}
"#,
            "exact_shape",
        )
        .unwrap();
        apply(&module);
        module.verify().unwrap();
        let ir = module.print_to_string().to_string();
        assert!(ir.contains(
            "@\"perry_method_C_score$pshape\" = alias double (double), ptr @perry_method_C_score"
        ));
        assert!(ir.contains("@callable_identity = global ptr @perry_method_C_score"));
        assert!(module.get_function("perry_method_C_score").is_some());
        assert!(module.get_function("perry_method_C_score$pshape").is_none());
    }

    #[test]
    fn different_site_ids_or_observable_clone_addresses_are_not_folded() {
        for extra in ["", "@identity = global ptr @perry_method_C_score$pshape"] {
            let context = Context::create();
            let id = if extra.is_empty() { 2 } else { 1 };
            let ir = format!(
                r#"
{extra}
declare double @observe(i32)
define double @perry_method_C_score(double %x) {{
  %v = call double @observe(i32 1)
  ret double %v
}}
define double @perry_method_C_score$pshape(double %x) {{
  %v = call double @observe(i32 {id})
  ret double %v
}}
"#
            );
            let module = super::super::parse_ir_text(&context, &ir, "distinct_shape").unwrap();
            apply(&module);
            module.verify().unwrap();
            assert!(module.get_function("perry_method_C_score$pshape").is_some());
        }
    }

    #[test]
    fn gc_relocation_and_exception_bodies_remain_intact() {
        let context = Context::create();
        let body = r#"
entry:
  %token = invoke token (i64, i32, ptr, i32, i32, ...) @llvm.experimental.gc.statepoint.p0(i64 0, i32 0, ptr elementtype(void ()) @collect, i32 0, i32 0, i32 0, i32 0) [ "gc-live"(ptr addrspace(1) %root) ] to label %ok unwind label %fail
ok:
  %moved = call ptr addrspace(1) @llvm.experimental.gc.relocate.p1(token %token, i32 0, i32 0)
  ret ptr addrspace(1) %moved
fail:
  %pad = landingpad token cleanup
  %unwound = call ptr addrspace(1) @llvm.experimental.gc.relocate.p1(token %pad, i32 0, i32 0)
  ret ptr addrspace(1) %unwound
"#;
        let ir = format!(
            r#"
declare void @collect()
declare i32 @personality(...)
declare token @llvm.experimental.gc.statepoint.p0(i64 immarg, i32 immarg, ptr, i32 immarg, i32 immarg, ...)
declare ptr addrspace(1) @llvm.experimental.gc.relocate.p1(token, i32 immarg, i32 immarg)
define ptr addrspace(1) @perry_method_C_keep(ptr addrspace(1) %root) gc "statepoint-example" personality ptr @personality {{ {body} }}
define ptr addrspace(1) @perry_method_C_keep$pshape(ptr addrspace(1) %root) gc "statepoint-example" personality ptr @personality {{ {body} }}
"#
        );
        let module = super::super::parse_ir_text(&context, &ir, "rooted_shape").unwrap();
        module.verify().unwrap();
        let before = module
            .get_function("perry_method_C_keep")
            .unwrap()
            .print_to_string()
            .to_string();
        apply(&module);
        module.verify().unwrap();
        assert!(module.get_function("perry_method_C_keep$pshape").is_none());
        assert_eq!(
            before,
            module
                .get_function("perry_method_C_keep")
                .unwrap()
                .print_to_string()
                .to_string()
        );
    }

    #[test]
    fn direct_call_passing_its_own_callee_is_an_address_use() {
        let context = Context::create();
        let module = super::super::parse_ir_text(
            &context,
            r#"
define void @perry_method_C_visit(ptr %fn) { ret void }
define void @perry_method_C_visit$pshape(ptr %fn) { ret void }
define void @caller() {
  call void @perry_method_C_visit$pshape(ptr @perry_method_C_visit$pshape)
  ret void
}
"#,
            "address_argument",
        )
        .unwrap();
        apply(&module);
        module.verify().unwrap();
        assert!(module.get_function("perry_method_C_visit$pshape").is_some());
    }

    #[test]
    fn parameter_abi_gc_personality_and_other_attributes_must_match() {
        for (params, prefix, suffix) in [
            ("i64 %x", "", ""),
            ("double %x", "fastcc ", ""),
            ("double %x", "", "gc \"statepoint-example\""),
            ("double %x", "", "personality ptr @personality"),
            ("double %x", "", "\"disable-tail-calls\"=\"true\""),
        ] {
            let context = Context::create();
            let ir = format!(
                r#"
declare i32 @personality(...)
define double @perry_method_C_value(double %x) {{ ret double 0.0 }}
define {prefix}double @perry_method_C_value$pshape({params}) {suffix} {{ ret double 0.0 }}
"#
            );
            let module = super::super::parse_ir_text(&context, &ir, "different_contract").unwrap();
            apply(&module);
            module.verify().unwrap();
            assert!(module.get_function("perry_method_C_value$pshape").is_some());
        }
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn emitted_alias_links_from_an_independent_native_unit_without_a_thunk() {
        use std::fs;
        use std::process::Command;
        let target = inkwell::targets::TargetMachine::get_default_triple();
        let pieces = super::super::compile_ll_to_object_inprocess(
            r#"
define double @perry_method_C_value(double %x) {
  %result = fadd double %x, %x
  ret double %result
}
define double @perry_method_C_value$pshape(double %x) alwaysinline {
  %result = fadd double %x, %x
  ret double %result
}
"#,
            target.as_str().to_str().unwrap(),
            &["-O2".into(), "-c".into()],
            "shape_alias_producer",
            false,
        )
        .unwrap();
        assert_eq!(pieces.len(), 1);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("perry-shape-alias-{}-{stamp}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("producer.o"), &pieces[0]).unwrap();
        fs::write(
            dir.join("consumer.c"),
            r#"
#ifdef __APPLE__
#define SYMBOL_PREFIX "_"
#else
#define SYMBOL_PREFIX ""
#endif
extern double ordinary(double) __asm__(SYMBOL_PREFIX "perry_method_C_value");
extern double shape(double) __asm__(SYMBOL_PREFIX "perry_method_C_value$pshape");
int main(void) {
    double (*volatile a)(double) = ordinary;
    double (*volatile b)(double) = shape;
    return a != b || a(9.0) != 18.0 || b(7.0) != 14.0;
}
"#,
        )
        .unwrap();
        let binary = dir.join("consumer");
        let linked = Command::new("cc")
            .arg(dir.join("consumer.c"))
            .arg(dir.join("producer.o"))
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            linked.status.success(),
            "{}",
            String::from_utf8_lossy(&linked.stderr)
        );
        assert!(Command::new(&binary).status().unwrap().success());
        fs::remove_dir_all(dir).unwrap();
    }
}
