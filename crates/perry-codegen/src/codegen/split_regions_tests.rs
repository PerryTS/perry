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

#[test]
fn class_functions_are_offered_to_the_splitter() {
    let work = "let x = 1; x = x * 3 + 1; x = x * 5 + 2; x = x * 7 + 3; x = x * 11 + 4;";
    let members = [
        format!("constructor() {{ {work} console.log(x); }}"),
        format!("method() {{ {work} return x; }}"),
        format!("static method() {{ {work} return x; }}"),
        format!("get value() {{ {work} return x; }}"),
        format!("set value(v: number) {{ {work} console.log(x, v); }}"),
        format!("['computed']() {{ {work} return x; }}"),
        format!("get ['computed']() {{ {work} return x; }}"),
        format!("set ['computed'](v: number) {{ {work} console.log(x, v); }}"),
    ];
    for member in members {
        let mut m = lower_src(&format!("class C {{ {member} }}"));
        let made = split_large_bodies_with(&mut m, tiny(false));
        assert!(made > 0, "class function skipped: {member}");
        assert!(regions(&m).iter().all(|f| f.name.starts_with("C::")));
    }
}

#[test]
fn field_initializer_closures_are_seen_by_the_cheap_exit_and_split() {
    let work = "let x = 1; x = x * 3 + 1; x = x * 5 + 2; x = x * 7 + 3; x = x * 11 + 4; return x;";
    for prefix in ["", "static "] {
        let mut m = lower_src(&format!(
            "class C {{ {prefix}value = () => {{ {work} }}; }}"
        ));
        assert!(
            m.functions.is_empty(),
            "fixture must have no ordinary functions"
        );
        assert!(m
            .init
            .iter()
            .all(|s| !matches!(s, Stmt::Expr(Expr::Closure { .. }))));
        let made = split_large_bodies_with(&mut m, tiny(false));
        assert!(made > 0, "{prefix}field closure skipped");
    }
}

#[test]
fn class_frame_operations_stay_in_the_parent() {
    let mut m = lower_src(
        r#"
        class C {
            value = 1;
            method() {
                this.value += 1;
                let x = 1;
                x = x * 3 + 1; x = x * 5 + 2;
                x = x * 7 + 3; x = x * 11 + 4;
                this.value += x;
                return this.value;
            }
        }
    "#,
    );
    assert!(split_large_bodies_with(&mut m, tiny(false)) > 0);
    for f in regions(&m) {
        assert!(
            !format!("{:?}", f.body).contains("This"),
            "frame use moved: {:?}",
            f.body
        );
    }
    let method = &m.classes.iter().find(|c| c.name == "C").unwrap().methods[0];
    assert!(format!("{:?}", method.body).contains("This"));
}

#[test]
fn mutable_class_capture_parameters_stay_in_the_parent() {
    let mut m = lower_src(
        r#"class C {
        constructor(value: number) {
            value = value + 1;
            value = value * 2;
            value = value + 3;
            value = value * 5;
            let x = 1; x = x * 3 + 1; x = x * 5 + 2;
            x = x * 7 + 3; x = x * 11 + 4;
            console.log(value, x);
        }
    }"#,
    );
    let ctor = m.classes[0].constructor.as_mut().unwrap();
    let id = ctor.params[0].id;
    // Lowering uses this reserved name for values shared with member/field
    // closures. Those closures live outside ctor.body.
    ctor.params[0].name = perry_hir::cap_fields::cap_field_name(1, id);
    assert!(split_large_bodies_with(&mut m, tiny(false)) > 0);
    for f in regions(&m) {
        assert!(
            f.params
                .iter()
                .all(|p| !p.name.starts_with(perry_hir::cap_fields::CAP_FIELD_PREFIX)),
            "class capture passed by value: {}",
            f.name
        );
    }
}

#[test]
fn namespace_values_have_their_expanded_size_but_qualified_reads_do_not() {
    let limits = Limits {
        min_body: 4000,
        target: 1500,
        chunk_min: 200,
        interface_nodes: 40,
        ..tiny(false)
    };
    for (value, expected) in [
        ("factory({ typescript: ns })", true),
        ("ns.member(factory)", false),
    ] {
        let mut m = lower_src(&format!(
            r#"
            import * as ns from './many';
            class C {{ method(factory: any) {{
                const result = {value}; return result;
            }} }}
        "#
        ));
        m.imports[0].resolved_path = Some("/virtual/many.ts".into());
        let made = super::split_regions::split_large_bodies_with_cost(&mut m, limits, &|source| {
            assert_eq!(source, "/virtual/many.ts");
            2000
        });
        assert_eq!(made > 0, expected, "namespace use: {value}");
    }
}

#[test]
fn namespace_expansion_in_a_closure_does_not_inflate_its_parent() {
    let mut m = lower_src(
        r#"
        import * as ns from './many';
        class C { method(factory: any) {
            return () => { const result = factory(ns); return result; };
        } }
    "#,
    );
    let limits = Limits {
        min_body: 4000,
        target: 1500,
        chunk_min: 200,
        interface_nodes: 40,
        ..tiny(false)
    };
    assert!(super::split_regions::split_large_bodies_with_cost(&mut m, limits, &|_| 2000,) > 0);
    assert!(regions(&m)
        .iter()
        .all(|f| !f.name.starts_with("C::method#region_")));
}
