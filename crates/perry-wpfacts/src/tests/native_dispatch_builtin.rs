//! Exact named builtin routes, including refused guards and protocol inputs.
use super::*;

fn solve(src: &str) -> ProgramFacts {
    solve_with_pairs(src, true)
}

fn solve_with_pairs(src: &str, namespace_pairs: bool) -> ProgramFacts {
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let mut set = perry_hir::PatchedBuiltins::new();
    perry_hir::scan_patched_builtins(&ast, &mut set);
    set.namespace_pairs = namespace_pairs;
    perry_hir::set_patched_builtins(std::sync::Arc::new(set));
    let hir = perry_hir::lower_module(&ast, "t", "t.ts");
    perry_hir::clear_patched_builtins();
    let hir = hir.expect("lowers");
    let cfg = FactsConfig {
        namespace_pairs,
        ..Default::default()
    };
    compute(&[("t.ts".into(), &hir)], &cfg)
}

fn assert_named(src: &str, key: &str, redispatch: bool) {
    assert_named_facts(solve(src), src, key, redispatch);
}

fn assert_named_facts(pf: ProgramFacts, src: &str, key: &str, redispatch: bool) {
    let calls: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.is_call && s.key == key)
        .collect();
    assert!(!calls.is_empty(), "missing {key}\n{}", pf.dump_text());
    for s in calls {
        assert_eq!(
            s.dispatch_obligation().unwrap().redispatch,
            redispatch,
            "{src}\n{}",
            pf.dump_text()
        );
    }
}

#[test]
fn native_dispatch_console_requires_selected_intrinsic_and_safe_operands() {
    for src in [
        "console.log('hello');",
        "const n = process.argv.length; console.log('hello ' + n);",
        "console.log();",
    ] {
        assert_named_facts(solve_with_pairs(src, false), src, "log", false);
        // Pair publication refuses the mixed Console/Other namespace callee.
        assert_named(src, "log", true);
    }
}

#[test]
fn native_dispatch_console_protocols_and_replacement_remain_open() {
    for src in [
        "console.log = function(x: any) {}; console.log('hello');",
        "console.log({ toString() { return 'custom'; } });",
        "console.log('%s', { toString() { return 'custom'; } });",
        "console.log((globalThis as any).unknown);",
        "import { performance } from 'node:perf_hooks'; performance.now(); console.log('hello');",
        "import * as dc from 'node:diagnostics_channel'; dc.subscribe('console.log', (m: any) => { m.arguments[0] = {}; }); console.log('hello');",
    ] {
        assert_named(src, "log", true);
        assert_named_facts(solve_with_pairs(src, false), src, "log", true);
    }
}

#[test]
fn native_dispatch_namespace_intrinsic_and_primitive_operands_are_closed() {
    for (src, key) in [
        ("console.log(Math.abs(-3));", "abs"),
        ("console.log(Math.round(1.5));", "round"),
        ("console.log(Math.min(1, 2));", "min"),
        ("console.log(Math.max(1, 2));", "max"),
        ("console.log(Number.isInteger(1));", "isInteger"),
        ("console.log(Number.isSafeInteger(1));", "isSafeInteger"),
    ] {
        assert_named(src, key, false);
    }
}

#[test]
fn native_dispatch_namespace_mutation_and_coercion_remain_open() {
    for (src, key) in [
        ("Math.abs = x => 7; console.log(Math.abs(-3));", "abs"),
        (
            "Number.isInteger = x => false; console.log(Number.isInteger(1));",
            "isInteger",
        ),
        (
            "console.log(Math.abs({valueOf() { return -3; }} as any));",
            "abs",
        ),
        ("console.log(Math.abs((globalThis as any).unknown));", "abs"),
        ("const f = Math.abs; console.log(f.call(Math, -3));", "call"),
        (
            "const f = Math.abs; console.log(f.apply(Math, [-3]));",
            "apply",
        ),
    ] {
        assert_named(src, key, true);
    }
}

#[test]
fn native_dispatch_unextended_classes_keep_their_fallbacks() {
    for (src, key) in [
        ("String.prototype.charAt = function(i) { return 'x'; }; const s = 'abc'; console.log(s.charAt(0));", "charAt"),
        ("Array.prototype.fill = function(x) { return this; }; const a: any = [1, 2]; console.log(a.fill(3));", "fill"),
        ("Map.prototype.clear = function() {}; const m: any = new Map(); m.clear();", "clear"),
        ("const s: any = process.argv.length > 5 ? 'abc' : { charAt(i) { return 'x'; } }; console.log(s.charAt(0));", "charAt"),
    ] { assert_named(src, key, true); }
}

#[test]
fn native_dispatch_computed_calls_never_gain_builtin_certificate() {
    let pf = solve("const k = process.argv[2]; (Math as any)[k](1); (console as any)[k]('hello');");
    assert!(
        pf.sites.iter().all(|s| !s.builtin_dispatch_certified()),
        "{}",
        pf.dump_text()
    );
    assert!(
        pf.elements.iter().any(|e| e.op == crate::ElementOp::Call),
        "computed call was exercised"
    );
}

#[test]
fn native_dispatch_builtin_refuses_argument_twins_in_either_order() {
    fn offset(e: &mut perry_hir::Expr) {
        match e {
            perry_hir::Expr::Call { byte_offset, .. }
            | perry_hir::Expr::PropertyGet { byte_offset, .. } => *byte_offset = 41,
            _ => {}
        }
        perry_hir::walker::walk_expr_children_mut(e, &mut offset);
    }
    for source in [
        "console.log('hello'); console.log({});",
        "console.log({}); console.log('hello');",
    ] {
        let ast = perry_parser::parse_typescript(source, "t.ts").unwrap();
        let mut hir = perry_hir::lower_module(&ast, "t", "t.ts").unwrap();
        for stmt in &mut hir.init {
            if let perry_hir::Stmt::Expr(e) = stmt {
                offset(e);
            }
        }
        let pf = compute(&[("t.ts".into(), &hir)], &FactsConfig::default());
        let sites: Vec<_> = pf
            .sites
            .iter()
            .filter(|s| s.is_call && s.key == "log")
            .collect();
        assert!(!sites.is_empty(), "{}", pf.dump_text());
        assert!(
            sites
                .iter()
                .all(|s| s.dispatch_obligation().unwrap().redispatch),
            "{}",
            pf.dump_text()
        );
    }
}

#[test]
fn native_dispatch_builtin_refuses_check_sabotage_and_incomplete_worlds() {
    let ast = perry_parser::parse_typescript("console.log('hello');", "t.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").unwrap();
    for cfg in [
        FactsConfig {
            verify_facts: true,
            ..Default::default()
        },
        FactsConfig {
            sabotage: Some("unchecked".into()),
            ..Default::default()
        },
        FactsConfig {
            unit: true,
            ..Default::default()
        },
        FactsConfig {
            max_work: Some(1),
            ..Default::default()
        },
    ] {
        let pf = compute(&[("t.ts".into(), &hir)], &cfg);
        assert!(
            pf.sites.iter().all(|s| !s.builtin_dispatch_certified()),
            "{}",
            pf.dump_text()
        );
    }
}

#[test]
fn native_dispatch_map_needs_brand_and_exact_lookup() {
    let source = "class H { table = new Map(); run() { this.table.clear(); } } new H().run();";
    assert_named(source, "clear", false);
    assert_named(
        &format!("Map.prototype.clear = function() {{}}; {source}"),
        "clear",
        true,
    );
    assert_named("class H { table = new Map(); run() { this.table.clear = function() {}; this.table.clear(); } } new H().run();", "clear", true);
}
