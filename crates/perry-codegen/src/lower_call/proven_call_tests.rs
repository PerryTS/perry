//! FVA's codegen consumer (`proven_call.rs`): a call through a value whose
//! callee set the facts prove makes no runtime dispatch. One body is a
//! direct call; two to four bodies are one load of the callee's info word
//! and a direct call of the body it names (one compare per body but the
//! last, no other arm); more are one load of the info word, one of its code
//! word and an indirect call. Without the facts each site
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
    // A known body is called through its typed entry (the entry contract
    // inline, the generic body on a miss), as the closure-typed local's
    // direct arm calls it.
    let typed = format!("__{inc}$typed_f64(");
    assert_eq!(
        twice.matches(&typed).count(),
        2,
        "both calls are direct calls of the body's typed entry:\n{twice}"
    );
}

const FIVE: &str = r#"
    const b0 = (x: number) => x + 1;
    const b1 = (x: number) => x - 1;
    const b2 = (x: number) => x * 2;
    const b3 = (x: number, y: number) => x * 3 + (y === undefined ? 0 : y);
    const b4 = (x: number) => x * 4;
    function once(g: (x: number) => number, x: number) { return g(x); }
    console.log(once(b0, 1), once(b1, 1), once(b2, 1), once(b3, 1), once(b4, 1));
"#;

fn once_with(n: usize) -> (String, Vec<u32>) {
    let m = lower(FIVE);
    let g = param_id(&m, "once", 0);
    let ids: Vec<u32> = (0..n).map(|i| closure_id(&m, &format!("b{i}"))).collect();
    let bodies = ids
        .iter()
        .map(|&func_id| FactsBody {
            func_id,
            params: if Some(&func_id) == ids.get(3) { 2 } else { 1 },
            closure: true,
        })
        .collect();
    let with = ir(&m, facts_for(g, bodies));
    (body(&with, "once").to_string(), ids)
}

fn indirect_calls(f: &str) -> usize {
    f.lines()
        .filter(|l| l.contains("call double %") || l.contains("call fastcc double %"))
        .count()
}

#[test]
fn a_proven_set_of_up_to_four_bodies_calls_the_body_its_info_names() {
    for n in 2..=4 {
        let (once, ids) = once_with(n);
        assert!(!once.contains("js_closure_call"), "{once}");
        assert_eq!(
            indirect_calls(&once),
            0,
            "no indirect call (n={n}):\n{once}"
        );
        // One info load, no code-word load: the arms are direct calls.
        let compares: Vec<&str> = once
            .lines()
            .filter(|l| l.contains("icmp eq ptr") && l.contains("$info"))
            .collect();
        assert_eq!(
            compares.len(),
            n - 1,
            "one info compare per body but the last (n={n}):\n{once}"
        );
        for (i, id) in ids.iter().enumerate() {
            // The body's typed entry, or its public entry where the call
            // has no typed entry (b3: one argument for two parameters).
            let sym = format!("__{id}(");
            let typed = format!("__{id}$typed_f64(");
            let calls: Vec<&str> = once
                .lines()
                .filter(|l| l.contains("call ") && (l.contains(&sym) || l.contains(&typed)))
                .collect();
            assert_eq!(
                calls.len(),
                1,
                "one direct call of body {i} (n={n}):\n{once}"
            );
            // Each arm passes its own body's parameters: b3 declares two,
            // the call passes one, so its arm pads with undefined.
            let doubles = calls[0].matches("double ").count();
            let want = if i == 3 { 3 } else { 2 };
            assert_eq!(doubles, want, "body {i}'s arguments (n={n}): {}", calls[0]);
            if i + 1 < n {
                assert!(
                    compares.iter().any(|c| c.contains(&format!("__{id}$info"))),
                    "body {i} is compared (n={n}):\n{once}"
                );
            } else {
                assert!(
                    !compares.iter().any(|c| c.contains(&format!("__{id}$info"))),
                    "the last body is the arm no compare matched (n={n}):\n{once}"
                );
            }
        }
        // The set is complete: no arm reaches a dispatcher or a trap.
        for gone in ["switch ", "unreachable", "js_closure_call", "js_throw"] {
            assert!(!once.contains(gone), "{gone} (n={n}):\n{once}");
        }
    }
}

#[test]
fn a_proven_set_of_five_bodies_is_one_load_and_an_indirect_call() {
    let (once, ids) = once_with(5);
    assert!(!once.contains("js_closure_call"), "{once}");
    assert!(!once.contains("switch "), "{once}");
    assert_eq!(indirect_calls(&once), 1, "one indirect call:\n{once}");
    assert!(!once.contains("icmp eq ptr"), "no info compare:\n{once}");
    for id in ids {
        assert!(
            !once.contains(&format!("__{id}(")),
            "no direct call:\n{once}"
        );
    }
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

/// The body of the function whose `define` line names `sym` followed by `(`.
fn defined<'a>(ir: &'a str, sym: &str) -> &'a str {
    let pat = format!("{sym}(");
    let at = ir
        .match_indices("define ")
        .map(|(i, _)| i)
        .find(|&i| ir[i..].lines().next().is_some_and(|l| l.contains(&pat)))
        .unwrap_or_else(|| panic!("no definition of {sym} in\n{ir}"));
    let end = ir[at..].find("\n}\n").map_or(ir.len(), |e| at + e);
    &ir[at..end]
}

#[test]
fn a_proven_call_of_a_captured_closure_with_fewer_arguments_is_direct() {
    // `add` is bound once and captured: the closure arm calls `add(n, 2)`
    // straight through its body with no test, but `add(n)` passes fewer
    // arguments than `add` declares, which that arm dispatches. The facts
    // prove the body, so that call is a direct call of it too.
    let m = lower(
        r#"
        function make() {
            const add = (a: number, b: number) => a + (b === undefined ? 10 : b);
            return (n: number) => add(n) + add(n, 2);
        }
        console.log(make()(1));
    "#,
    );
    let make = m
        .functions
        .iter()
        .find(|f| f.name == "make")
        .expect("function make");
    let (local, add) = make
        .body
        .iter()
        .find_map(|s| match s {
            Stmt::Let {
                id,
                name,
                init: Some(Expr::Closure { func_id, .. }),
                ..
            } if name == "add" => Some((*id, *func_id)),
            _ => None,
        })
        .expect("closure add");
    let inner = make
        .body
        .iter()
        .find_map(|s| match s {
            Stmt::Return(Some(Expr::Closure { func_id, .. })) => Some(*func_id),
            _ => None,
        })
        .expect("returned closure");
    let inner_sym = format!("__{inner}");
    let without = ir(&m, ElisionFacts::default());
    assert!(
        defined(&without, &inner_sym).contains("@js_closure_call1("),
        "without facts the one-argument call dispatches:\n{}",
        defined(&without, &inner_sym)
    );
    let with = ir(
        &m,
        facts_for(
            local,
            vec![FactsBody {
                func_id: add,
                params: 2,
                closure: true,
            }],
        ),
    );
    let body = defined(&with, &inner_sym);
    assert!(!body.contains("@js_closure_call1("), "{body}");
    let direct = body
        .lines()
        .filter(|l| l.contains("call ") && l.contains(&format!("__{add}(")))
        .count();
    assert!(
        direct >= 2,
        "both calls are direct calls of add's body (the short one padded):\n{body}"
    );
}

/// `PERRY_VERIFY_FACTS` unset: a proven call carries no witness (no check
/// call, no declaration use, no site string); the default build is the
/// build without the verification mode. Set (`PERRY_VERIFY_FACTS=1 cargo
/// test`), the same site asks `perry_wpfacts_check_call` and names its
/// source position.
#[test]
fn the_facts_witness_is_emitted_only_by_the_verification_build() {
    let m = lower(VALUE);
    let g = param_id(&m, "twice", 0);
    let inc = closure_id(&m, "inc");
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
    if crate::verify_facts::witnesses() {
        assert_eq!(
            twice.matches("@perry_wpfacts_check_call(").count(),
            2,
            "each proven call is checked:\n{twice}"
        );
        assert!(
            with.contains("pc:?") || with.contains("pc:@"),
            "site names:\n{with}"
        );
    } else {
        assert!(!twice.contains("perry_wpfacts_check"), "{twice}");
        assert!(
            !with.contains("proven call bodies="),
            "no site string:\n{with}"
        );
    }
}
