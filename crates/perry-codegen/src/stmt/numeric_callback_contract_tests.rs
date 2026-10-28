use crate::codegen::{ElisionFacts, FactsBody, ProgramFactsView};
use std::collections::BTreeMap;
use std::sync::Arc;

fn compile(body: &str, facts: bool) -> String {
    let source = format!(
        r#"
        function make(n: number) {{ return (x: number): number => x + n; }}
        function run(f: (x: number) => number, count: number, start: number) {{
            let value = start;
            for (let i = 0; i < count; ++i) {{ {body} }}
            console.log(f("after" as any));
            return value;
        }}
        console.log(run(make(4), 10, 2));
    "#
    );
    let ast = perry_parser::parse_typescript(&source, "callback_contract.ts").unwrap();
    let m = perry_hir::lower_module(&ast, "callback_contract", "callback_contract.ts").unwrap();
    let id = perry_hir::emitted_closures(&m)[0].0;
    let param = m.functions.iter().find(|f| f.name == "run").unwrap().params[0].id;
    let elision = if facts {
        ElisionFacts {
            local_bodies: BTreeMap::from([(
                param,
                vec![FactsBody {
                    func_id: id,
                    params: 1,
                    closure: true,
                }],
            )]),
            ..Default::default()
        }
    } else {
        ElisionFacts::default()
    };
    String::from_utf8(
        crate::compile_module(
            &m,
            crate::CompileOptions {
                emit_ir_only: true,
                program_facts: ProgramFactsView {
                    elision: Arc::new(elision),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn run_body(ir: &str) -> &str {
    let at = ir
        .find("__run(")
        .and_then(|p| ir[..p].rfind("define "))
        .unwrap();
    let end = ir[at..].find("\n}\n").unwrap() + at;
    &ir[at..end]
}

#[test]
fn callback_number_loop_has_live_capture_admission_private_body_and_full_slow_loop() {
    let with = compile("value = f(value);", true);
    let run = run_body(&with);
    assert!(
        !run.contains("@js_closure_resolve_plain_direct_call("),
        "known bodies need no runtime entry resolution: {run}"
    );
    assert!(run.contains("for.number_locals.fast.preheader"), "{run}");
    assert!(run.contains("for.number_locals.slow.preheader"), "{run}");
    assert!(
        run.contains(".$normalized_captures(i64 0,"),
        "private call must carry scalar seeds, not a heap owner: {run}"
    );
    assert!(
        run.contains("gc-leaf-function"),
        "the admitted arithmetic body must be a leaf: {run}"
    );
    // Both the original slow loop and the post-loop string call still use
    // the original public entry: the private proof ends at scope exit.
    assert!(
        run.lines()
            .filter(|line| line.contains("call double @perry_closure_")
                && !line.contains(".$normalized_captures("))
            .count()
            >= 2,
        "{run}"
    );
    let without = compile("value = f(value);", false);
    assert!(!run_body(&without).contains("for.number_locals.fast.preheader"));
}

#[test]
fn callback_binding_writes_reject_capture_hoisting() {
    let ir = compile("value = f(value); f = make(9);", true);
    assert!(!run_body(&ir).contains("for.number_locals.fast.preheader"));
}

#[test]
fn callback_argument_arity_and_all_carried_writes_are_part_of_the_induction() {
    for body in [
        "value = f(value); f();",
        "value = f(value); value = 'bad' as any;",
    ] {
        let ir = compile(body, true);
        assert!(
            !run_body(&ir).contains("for.number_locals.fast.preheader"),
            "{body}"
        );
    }
}
