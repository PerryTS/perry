use super::*;

fn wide_roots(word_type: &str) -> String {
    let zero = if word_type == "double" { "0.0" } else { "0" };
    let mut ir = format!(
        "declare void @js_shadow_slot_bind(i32, ptr)\ndeclare {word_type} @make(i64) \"gc-leaf-function\"\ndeclare void @collect()\ndeclare void @use({word_type}) \"gc-leaf-function\"\ndefine void @wide(i64 %arg) gc \"statepoint-example\" {{\nentry:\n",
    );
    for i in 0..128 {
        ir.push_str(&format!(
            "  %slot{i} = alloca {word_type}\n  store {word_type} {zero}, ptr %slot{i}\n  call void @js_shadow_slot_bind(i32 {i}, ptr %slot{i})\n  %p{i} = call {word_type} @make(i64 %arg)\n  store {word_type} %p{i}, ptr %slot{i}\n"
        ));
    }
    ir.push_str("  call void @collect()\n");
    for i in 0..128 {
        ir.push_str(&format!(
            "  %v{i} = load {word_type}, ptr %slot{i}\n  call void @use({word_type} %v{i})\n"
        ));
    }
    ir.push_str("  ret void\n}\n");
    lower_precise_roots_to_native_stack(&ir, "wide", 128)
}

#[test]
fn one_tied_register_crosses_the_root_representation_boundary() {
    for word_type in ["i64", "double"] {
        let lowered = wide_roots(word_type);
        let target = crate::codegen::default_target_triple();
        let rewritten =
            crate::inprocess::statepoint_rewritten_ir(&lowered, &target, "wide_typed_reload")
                .unwrap();
        assert_eq!(
            rewritten
                .lines()
                .filter(|line| line.contains(" = call")
                    && line.contains("@llvm.experimental.gc.relocate"))
                .count(),
            128,
            "all encoded FFI words must still be live through the collecting call"
        );
        let compiled = crate::inprocess::with_test_rs4gc_budget(950, || {
            crate::inprocess::compile_ll_to_object_inprocess(
                &lowered,
                &target,
                &["-S".into(), "-Os".into()],
                "wide_reload_budget",
                true,
            )
        });
        assert!(
        compiled.is_ok(),
        "{word_type}: removing only redundant IR must make this function fit the enforced budget: {compiled:?}"
    );
    }
}

// The typed input must also remain a root if a caller simplifies integer
// consumers before rewriting. The ordinary i64 result still breaks the
// identity on a later re-root (covered by the #9499 lifetime-hole witness).
#[test]
fn simplifying_integer_consumers_cannot_cancel_the_managed_root_cast() {
    let target = crate::codegen::default_target_triple();
    for word_type in ["i64", "double"] {
        let rewritten = crate::inprocess::statepoint_rewritten_ir_with_passes(
            &wide_roots(word_type),
            &target,
            "typed_reload_liveness",
            "always-inline,function(mem2reg,sccp,instcombine),rewrite-statepoints-for-gc",
        )
        .unwrap();
        assert_eq!(
            rewritten
                .lines()
                .filter(|line| line.contains(" = call")
                    && line.contains("@llvm.experimental.gc.relocate"))
                .count(),
            128
        );
    }
}
