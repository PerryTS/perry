//! FVA's codegen consumer (`proven_call.rs`): a call through a value whose
//! callee set the facts prove makes no runtime dispatch. One body is a
//! direct call; several plain bodies are one load of the callee's info word,
//! one of its code word and an indirect call. Without the facts each site
//! keeps the dispatcher, so every test here can fail. (The class method
//! tower's proven arm and calls through a field need the program's layouts
//! and prototype census: `crates/perry/tests/proven_calls.rs`.)

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::codegen::{ElisionFacts, FactsBody, ProgramFactsView};
use crate::{compile_module, CompileOptions};
use perry_hir::{Expr, Module, Stmt};

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(&src, "pc.ts", &mut cache)
                .expect("parses");
            perry_hir::lower_module(&parsed.module, "pc", "pc.ts").expect("lowers")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread")
}

fn ir(module: &Module, elision: ElisionFacts) -> String {
    let opts = CompileOptions {
        emit_ir_only: true,
        program_facts: ProgramFactsView {
            elision: Arc::new(elision),
            ..Default::default()
        },
        ..Default::default()
    };
    String::from_utf8(compile_module(module, opts).expect("fixture compiles")).expect("UTF-8 IR")
}

/// The body of the function whose symbol ends with `__<name>(`.
fn body<'a>(ir: &'a str, name: &str) -> &'a str {
    let at = ir
        .find(&format!("__{name}("))
        .and_then(|i| ir[..i].rfind("define "))
        .unwrap_or_else(|| panic!("no function {name} in\n{ir}"));
    let end = ir[at..].find("\n}\n").map_or(ir.len(), |e| at + e);
    &ir[at..end]
}

fn param_id(m: &Module, func: &str, i: usize) -> u32 {
    m.functions
        .iter()
        .find(|f| f.name == func)
        .unwrap_or_else(|| panic!("function {func}"))
        .params[i]
        .id
}

fn func_id(m: &Module, func: &str) -> u32 {
    m.functions
        .iter()
        .find(|f| f.name == func)
        .expect("function")
        .id
}

/// The closure bound by `const <name> = (...) => ...` at module level.
fn closure_id(m: &Module, name: &str) -> u32 {
    m.init
        .iter()
        .find_map(|s| match s {
            Stmt::Let {
                name: n,
                init: Some(Expr::Closure { func_id, .. }),
                ..
            } if n == name => Some(*func_id),
            _ => None,
        })
        .unwrap_or_else(|| panic!("closure {name}"))
}

fn facts_for(local: u32, bodies: Vec<FactsBody>) -> ElisionFacts {
    ElisionFacts {
        local_bodies: BTreeMap::from([(local, bodies)]),
        ..Default::default()
    }
}

const VALUE: &str = r#"
    const inc = (x: number) => x + 1;
    const dec = (x: number) => x - 1;
    function twice(g: (x: number) => number, x: number) { return g(g(x)); }
    console.log(twice(inc, 1), twice(dec, 1));
"#;

#[test]
fn a_proven_one_body_value_call_is_a_direct_call() {
    let m = lower(VALUE);
    let g = param_id(&m, "twice", 0);
    let inc = closure_id(&m, "inc");
    let without = ir(&m, ElisionFacts::default());
    assert!(
        body(&without, "twice").contains("@js_closure_call1("),
        "without facts the call goes through the dispatcher:\n{}",
        body(&without, "twice")
    );
    let with = ir(
        &m,
        facts_for(
            g,
            vec![FactsBody {
                func_id: inc,
                params: 1,
                closure: true,
            }],
        ),
    );
    let twice = body(&with, "twice");
    assert!(!twice.contains("js_closure_call"), "{twice}");
    let direct = format!("__{inc}(");
    assert_eq!(
        twice.matches(&direct).count(),
        2,
        "both calls are direct calls of the body:\n{twice}"
    );
}

#[test]
fn a_proven_set_of_plain_bodies_is_one_load_and_an_indirect_call() {
    let m = lower(VALUE);
    let g = param_id(&m, "twice", 0);
    let (inc, dec) = (closure_id(&m, "inc"), closure_id(&m, "dec"));
    let bodies = [inc, dec]
        .iter()
        .map(|&func_id| FactsBody {
            func_id,
            params: 1,
            closure: true,
        })
        .collect();
    let with = ir(&m, facts_for(g, bodies));
    let twice = body(&with, "twice");
    assert!(!twice.contains("js_closure_call"), "{twice}");
    // No switch, no test: the info word, its code word, the call.
    assert!(!twice.contains("switch "), "{twice}");
    let indirect = twice
        .lines()
        .filter(|l| l.contains("call double %") || l.contains("call fastcc double %"))
        .count();
    assert_eq!(indirect, 2, "one indirect call per site:\n{twice}");
    assert!(
        twice.matches("load ptr, ptr").count() >= 4,
        "each site loads the info word and the code word:\n{twice}"
    );
}

#[test]
fn a_proven_declaration_is_called_through_its_value_wrapper() {
    let m = lower(
        r#"
        function add(a: number, b: number) { return a + b; }
        function apply1(g: (a: number, b: number) => number) { return g(7); }
        console.log(apply1(add));
    "#,
    );
    let g = param_id(&m, "apply1", 0);
    let add = func_id(&m, "add");
    let with = ir(
        &m,
        facts_for(
            g,
            vec![FactsBody {
                func_id: add,
                params: 2,
                closure: false,
            }],
        ),
    );
    let apply1 = body(&with, "apply1");
    assert!(!apply1.contains("js_closure_call"), "{apply1}");
    let call = apply1
        .lines()
        .find(|l| l.contains("@__perry_wrap_") && l.contains("__add("))
        .unwrap_or_else(|| panic!("a direct call of add's wrapper:\n{apply1}"));
    // The missing parameter is padded with undefined.
    assert_eq!(call.matches("double ").count(), 3, "{call}");
}
