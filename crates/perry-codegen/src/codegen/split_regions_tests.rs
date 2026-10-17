//! Structural splitting: region shapes on small sources under tiny limits.

use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, Stmt};

use super::split_regions::{
    is_region_function, region_param_proofs, split_large_bodies_with, Limits,
};

fn lower_src(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(&src, "test.ts", &mut cache)
                .expect("parse should succeed");
            perry_hir::lower_module(&parsed.module, "test", "test.ts")
                .expect("lowering should succeed")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread")
}

fn tiny(closures: bool) -> Limits {
    Limits {
        min_body: 10,
        target: 30,
        guard_min: 4,
        chunk_min: 4,
        closures,
        sink: true,
        typed: true,
        max_regions: usize::MAX,
        interface_nodes: 0,
    }
}

fn regions(m: &Module) -> Vec<&Function> {
    m.functions
        .iter()
        .filter(|f| is_region_function(f))
        .collect()
}

fn count_closure_regions(m: &Module) -> usize {
    m.closure_display_names
        .values()
        .filter(|n| n.contains("#region_"))
        .count()
}

fn body_has_call_to(body: &[Stmt], id: u32) -> bool {
    let mut found = false;
    for s in body {
        perry_hir::walker::stmt_any_expr(s, &mut |e| {
            fn walk(e: &Expr, id: u32, found: &mut bool) {
                if let Expr::Call { callee, .. } = e {
                    if matches!(callee.as_ref(), Expr::FuncRef(f) if *f == id) {
                        *found = true;
                    }
                }
                perry_hir::walker::walk_expr_children(e, &mut |c| walk(c, id, found));
            }
            walk(e, id, &mut found);
            false
        });
        let mut nested = Vec::new();
        match s {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                nested.push(then_branch.as_slice());
                if let Some(e) = else_branch {
                    nested.push(e.as_slice());
                }
            }
            Stmt::Try { body, .. } => nested.push(body.as_slice()),
            _ => {}
        }
        for l in nested {
            found |= body_has_call_to(l, id);
        }
    }
    found
}

#[test]
fn a_typeof_guarded_arm_becomes_a_region_with_a_string_parameter() {
    let mut m = lower_src(
        r#"
        function big(x: any, y: any) {
          let h = 17;
          if (typeof x === 'string') {
            const up = x.toUpperCase();
            const n = up.length + x.length;
            h = (h * 31 + n) % 1000003;
          }
          const g = y[0];
          h = (h * 31 + (g | 0)) % 1000003;
          return h;
        }
        console.log(big('ab', [1]));
        "#,
    );
    let made = split_large_bodies_with(&mut m, tiny(true));
    assert!(made >= 1, "made {made}");
    let guard = regions(&m)
        .into_iter()
        .find(|f| {
            region_param_proofs(f)
                .values()
                .any(|t| matches!(t, Type::String))
        })
        .expect("a region with a String parameter");
    let proofs = region_param_proofs(guard);
    assert!(
        proofs.values().any(|t| matches!(t, Type::Number)),
        "h is a Number: {proofs:?}"
    );
    // Its one call site is in the parent or in a region the parent calls.
    let callers = m
        .functions
        .iter()
        .filter(|f| body_has_call_to(&f.body, guard.id))
        .count();
    assert_eq!(callers, 1);
}

#[test]
fn a_return_stays_in_the_parent() {
    let mut m = lower_src(
        r#"
        function big(a: any) {
          let s = 0;
          for (let i = 0; i < 3; i++) { s += i * a; }
          if (s > 100) { return -1; }
          s = s * 2 + a;
          return s;
        }
        console.log(big(2));
        "#,
    );
    split_large_bodies_with(&mut m, tiny(true));
    for f in regions(&m) {
        let mut ret = false;
        for s in &f.body[..f.body.len().saturating_sub(1)] {
            if let Stmt::If { then_branch, .. } = s {
                ret |= then_branch.iter().any(|t| matches!(t, Stmt::Return(_)));
            }
        }
        assert!(!ret, "a region must not hold the parent's return");
    }
}

#[test]
fn shared_mutable_state_never_crosses_by_value() {
    let src = r#"
        function big(a: any) {
          let count = 0;
          const inc = () => { count += 1; return count; };
          let s = '';
          for (let i = 0; i < 3; i++) { s += String(inc()) + ','; }
          count = count + a;
          s = s + count;
          return s;
        }
        console.log(big(2));
        "#;
    // `count` is written by a closure and by the body: no region may take it
    // by value, with or without closure regions.
    for closures in [false, true] {
        let mut m = lower_src(src);
        split_large_bodies_with(&mut m, tiny(closures));
        for f in regions(&m) {
            let names: Vec<&str> = f.params.iter().map(|p| p.name.as_str()).collect();
            assert!(
                !names.contains(&"count"),
                "count passed by value: {names:?}"
            );
        }
        if !closures {
            assert_eq!(count_closure_regions(&m), 0);
        }
    }
}

#[test]
fn a_hoisted_function_moves_to_its_first_use_and_its_region_owns_its_cells() {
    let mut m = lower_src(
        r#"
        function big(a: any, b: any) {
          let h = 17;
          const m0 = a + 2;
          function mul0(p: any) { return p * m0; }
          let cl0 = 0;
          for (let u = 0; u < 5; u++) { cl0 += mul0(u + 1); }
          h = (h * 31 + cl0) % 1000003;
          const g = b[0];
          h = (h * 31 + (g | 0)) % 1000003;
          return h;
        }
        console.log(big(1, [5]));
        "#,
    );
    split_large_bodies_with(&mut m, tiny(true));
    // Whichever region declares `m0` (captured by `mul0` before its
    // declaration) preallocates its cell itself.
    for f in regions(&m) {
        let declares_m0 = f
            .body
            .iter()
            .any(|s| matches!(s, Stmt::Let { name, .. } if name == "m0"));
        if declares_m0 {
            assert!(
                f.body
                    .iter()
                    .any(|s| matches!(s, Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_))),
                "region declaring m0 must preallocate it"
            );
        }
    }
}

#[test]
fn small_bodies_are_left_alone() {
    let mut m = lower_src(
        r#"
        function small(a: any) { return a + 1; }
        console.log(small(1));
        "#,
    );
    let made = split_large_bodies_with(
        &mut m,
        Limits {
            min_body: 4000,
            target: 1500,
            guard_min: 24,
            chunk_min: 200,
            closures: true,
            sink: true,
            typed: true,
            max_regions: usize::MAX,
            interface_nodes: 40,
        },
    );
    assert_eq!(made, 0);
}
