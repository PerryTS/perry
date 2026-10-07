use perry_hir::types::Type;
use perry_hir::{Expr, Stmt};

#[test]
fn native_step_output_is_an_unknown_value_rooted_before_the_body() {
    crate::temp_root_coverage::under_both_lowerings(|mode| {
        let marker = Expr::NativeMethodCall {
            module: "__perry_runtime".into(),
            class_name: None,
            object: None,
            method: "iteratorStepOutput".into(),
            args: vec![],
        };
        let step = Expr::NativeMethodCall {
            module: "__perry_runtime".into(),
            class_name: None,
            object: None,
            method: "iteratorStep".into(),
            args: vec![
                Expr::LocalGet(1),
                Expr::LocalGet(2),
                Expr::LocalSet(3, Box::new(marker)),
            ],
        };
        let let_any = |id| Stmt::Let {
            id,
            name: format!("step_{id}"),
            ty: Type::Any,
            mutable: true,
            init: Some(Expr::Undefined),
        };
        let ir = crate::temp_root_coverage::main_ir_for(
            "native_step",
            vec![
                let_any(1),
                let_any(2),
                let_any(3),
                Stmt::Expr(step),
                crate::temp_root_coverage::console_log(vec![Expr::LocalGet(3)]),
            ],
        );
        assert!(
            ir.contains("call i32 @js_iterator_step("),
            "{mode}: native consumer must execute"
        );
        assert!(
            !ir.contains("call double @js_for_of_next("),
            "{mode}: no result-object call"
        );
        let call = ir
            .lines()
            .find(|line| line.contains("call i32 @js_iterator_step("))
            .unwrap();
        let slot = call
            .rsplit("ptr ")
            .next()
            .unwrap()
            .split(')')
            .next()
            .unwrap();
        let load = ir
            .lines()
            .find(|line| line.contains(&format!("load double, ptr {slot}")))
            .unwrap();
        let value = load.trim().split(" = ").next().unwrap();
        let direct = format!("store double {value}, ptr");
        let store = if let Some(line) = ir.lines().find(|line| line.contains(&direct)) {
            line
        } else {
            let cast = ir
                .lines()
                .find(|line| line.contains(&format!("bitcast double {value} to i64")))
                .expect("native root word");
            let bits = cast.trim().split(" = ").next().unwrap();
            let cast = ir
                .lines()
                .find(|line| line.contains(&format!("inttoptr i64 {bits} to ptr addrspace(1)")))
                .expect("native root carrier");
            let root = cast.trim().split(" = ").next().unwrap();
            ir.lines()
                .find(|line| line.contains(&format!("store ptr addrspace(1) {root}, ptr")))
                .expect("native root publication")
        };
        assert!(
            ir.find(store).unwrap() < ir.find("call i64 @js_array_alloc").unwrap(),
            "{mode}: publish the unknown native value before the allocating body"
        );
        assert!(
            !ir.contains("iteratorStepOutput"),
            "output marker is not a runtime call"
        );
    });
}
