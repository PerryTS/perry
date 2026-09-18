//! #10357: the builder fold evaluates a value BEFORE the builder binding is
//! initialized, and an implicit conversion (`"" + w`, `-w`, `` `${w}` ``) can
//! call a user `valueOf`/`toString`. When that user code can read the binding
//! the fold turns a successful read into a TDZ `ReferenceError`, so such a
//! value must not fold. When nothing can read the binding before the literal
//! initializes it, the conversion is unobservable and must keep folding —
//! these tests pin both directions.

use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Expr, Stmt};
use perry_parser::parse_typescript_with_cache;

fn lower_src(src: &str) -> perry_hir::Module {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "builder_fold_conversion.ts", &mut cache)
        .expect("parse should succeed");
    lower_module(&parsed.module, "test", "builder_fold_conversion.ts")
        .expect("lower should succeed")
}

/// Number of constructor args on the `__AnonShape_…` allocation bound to
/// `name`, i.e. how many properties the literal carries after folding.
fn anon_shape_arity(stmts: &[Stmt], name: &str) -> Option<usize> {
    stmts.iter().find_map(|stmt| match stmt {
        Stmt::Let {
            name: binding,
            init: Some(Expr::New {
                class_name, args, ..
            }),
            ..
        } if binding == name && class_name.starts_with("__AnonShape_") => Some(args.len()),
        _ => None,
    })
}

/// Every statement list in the module, depth-first: the builder under test
/// may sit in module init, a function body, or a block nested in either.
fn all_stmt_lists(module: &perry_hir::Module) -> Vec<&[Stmt]> {
    fn nested<'a>(stmts: &'a [Stmt], out: &mut Vec<&'a [Stmt]>) {
        out.push(stmts);
        for stmt in stmts {
            match stmt {
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    nested(then_branch, out);
                    if let Some(else_branch) = else_branch {
                        nested(else_branch, out);
                    }
                }
                Stmt::Switch { cases, .. } => {
                    for case in cases {
                        nested(&case.body, out);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
                    nested(body, out)
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    nested(body, out);
                    if let Some(catch) = catch {
                        nested(&catch.body, out);
                    }
                    if let Some(finally) = finally {
                        nested(finally, out);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    nested(&module.init, &mut out);
    for function in &module.functions {
        nested(&function.body, &mut out);
    }
    out
}

fn arity_anywhere(module: &perry_hir::Module, name: &str) -> Option<usize> {
    all_stmt_lists(module)
        .into_iter()
        .find_map(|stmts| anon_shape_arity(stmts, name))
}

#[test]
fn conversions_keep_folding_when_nothing_can_observe_the_builder() {
    // The #6812 motivating shape: no closure anywhere names `o`, so no user
    // code a conversion reaches can read it early.
    let module = lower_src(
        r#"
        export function build(r: number, i: number): any {
            const o: any = {};
            o.a = i; o.b = r + i; o.c = `k${i}`; o.d = -r;
            return o;
        }
        "#,
    );
    assert_eq!(
        arity_anywhere(&module, "o"),
        Some(4),
        "every conversion should still fold: {:?}",
        module.functions
    );
}

#[test]
fn a_conversion_does_not_fold_when_a_closure_can_read_the_builder() {
    // The #10357 repro: `weird.valueOf()` reads `o`.
    let module = lower_src(
        r#"
        export function run(): string {
            const weird = { valueOf(): any { return o; } };
            const o: any = {};
            o.a = "" + weird;
            return typeof o.a;
        }
        "#,
    );
    assert_eq!(
        arity_anywhere(&module, "o"),
        Some(0),
        "the conversion must stay after the allocation: {:?}",
        module.functions
    );
}

#[test]
fn conversion_free_values_still_fold_on_an_observable_builder() {
    let module = lower_src(
        r#"
        export function run(w: any): any {
            const peek = () => o;
            const o: any = {};
            o.a = 1 + 2; o.b = typeof w; o.c = !w; o.d = w === 1; o.e = `lit`; o.f = w;
            return peek;
        }
        "#,
    );
    assert_eq!(
        arity_anywhere(&module, "o"),
        Some(6),
        "values that run no conversion on an unknown operand should fold: {:?}",
        module.functions
    );
}

#[test]
fn each_converting_form_stops_the_fold_on_an_observable_builder() {
    for value in [
        "-w",
        "+w",
        "~w",
        "`${w}`",
        "w + 1",
        "w < 1",
        "w == 1",
        "(w && 1) + 1",
    ] {
        let module = lower_src(&format!(
            r#"
            export function run(w: any): any {{
                const peek = () => o;
                const o: any = {{}};
                o.a = {value};
                return peek;
            }}
            "#
        ));
        assert_eq!(
            arity_anywhere(&module, "o"),
            Some(0),
            "`o.a = {value}` must not fold when a closure reads `o`: {:?}",
            module.functions
        );
    }
}

#[test]
fn a_gap_statement_with_a_conversion_does_not_sink_an_observable_allocation() {
    // #10355's gap test shares the predicate: `"" + w` below would run
    // `w.valueOf()` before `o` exists.
    let module = lower_src(
        r#"
        export function run(w: any): any {
            const peek = () => o;
            const o: any = {};
            const s = "" + w;
            o.a = s;
            return peek;
        }
        "#,
    );
    assert_eq!(
        arity_anywhere(&module, "o"),
        Some(0),
        "the allocation must not sink below the conversion: {:?}",
        module.functions
    );
}

#[test]
fn a_closure_in_another_switch_case_observes_the_builder() {
    // A `const` in a case clause is scoped to the whole switch, so a closure
    // made in an earlier case can fall through into the builder and read it:
    // the scan must cover the enclosing function, not just the statement
    // list being folded.
    let module = lower_src(
        r#"
        export function run(k: number, w: any): any {
            let f: any;
            switch (k) {
                case 2:
                    f = () => o;
                case 1:
                    const o: any = {};
                    o.a = "" + w;
                    break;
            }
            return f;
        }
        "#,
    );
    assert_eq!(
        arity_anywhere(&module, "o"),
        Some(0),
        "a sibling-case closure must count: {:?}",
        module.functions
    );
}

#[test]
fn a_block_var_builder_is_observed_by_a_function_level_closure() {
    let module = lower_src(
        r#"
        export function run(w: any): any {
            if (w) {
                var o: any = {};
                o.a = "" + w;
            }
            return () => o;
        }
        "#,
    );
    assert_eq!(
        arity_anywhere(&module, "o"),
        Some(0),
        "a `var` is function-scoped, so the closure outside the block counts: {:?}",
        module.functions
    );
}

#[test]
fn an_exported_top_level_builder_is_observable() {
    let module = lower_src(
        r#"
        declare const w: any;
        const o: any = {};
        o.a = "" + w;
        export { o };
        "#,
    );
    assert_eq!(
        anon_shape_arity(&module.init, "o"),
        Some(0),
        "an importer can read the live binding: {:?}",
        module.init
    );
}

#[test]
fn a_top_level_var_builder_is_observable() {
    let module = lower_src(
        r#"
        declare const w: any;
        var o: any = {};
        o.a = "" + w;
        "#,
    );
    assert_eq!(
        anon_shape_arity(&module.init, "o"),
        Some(0),
        "a top-level `var` is a global-object property: {:?}",
        module.init
    );
}

#[test]
fn an_unobserved_top_level_const_builder_still_folds() {
    let module = lower_src(
        r#"
        declare const w: any;
        const o: any = {};
        o.a = "" + w;
        console.log(o.a);
        "#,
    );
    assert_eq!(
        anon_shape_arity(&module.init, "o"),
        Some(1),
        "nothing can read `o` before the literal: {:?}",
        module.init
    );
}

#[test]
fn eval_makes_every_builder_in_its_scope_observable() {
    let module = lower_src(
        r#"
        export function run(w: any): any {
            const o: any = {};
            o.a = "" + w;
            return eval("o");
        }
        "#,
    );
    assert_eq!(
        arity_anywhere(&module, "o"),
        Some(0),
        "name-based reasoning is void once `eval` is in scope: {:?}",
        module.functions
    );
}

#[test]
fn a_closure_created_after_a_const_builder_does_not_observe_it() {
    // It cannot exist yet while the folded values run.
    let module = lower_src(
        r#"
        export function build(r: number, i: number): any {
            const o: any = {};
            o.a = i; o.b = r + i; o.c = -r;
            const peek = () => o;
            return peek();
        }
        "#,
    );
    assert_eq!(
        arity_anywhere(&module, "o"),
        Some(3),
        "a later closure is no observer: {:?}",
        module.functions
    );
}

#[test]
fn a_hoisted_function_declared_after_the_builder_observes_it() {
    let module = lower_src(
        r#"
        export function run(w: any): any {
            const o: any = {};
            o.a = "" + w;
            return peek;
            function peek(): any { return o; }
        }
        "#,
    );
    assert_eq!(
        arity_anywhere(&module, "o"),
        Some(0),
        "a function declaration exists from scope entry: {:?}",
        module.functions
    );
}

#[test]
fn a_var_builder_is_observed_by_a_closure_from_an_earlier_loop_pass() {
    // The `var` binding is shared by every pass, so the closure pushed after
    // the builder in pass 0 can run during pass 1's fold.
    let module = lower_src(
        r#"
        export function run(w: any): any {
            const fns: any[] = [];
            for (let k = 0; k < 2; k++) {
                var o: any = {};
                o.a = "" + w;
                fns.push(() => o);
            }
            return fns;
        }
        "#,
    );
    assert_eq!(
        arity_anywhere(&module, "o"),
        Some(0),
        "position says nothing for a `var`: {:?}",
        module.functions
    );
}

#[test]
fn a_nested_function_with_its_own_binding_is_not_an_observer() {
    let module = lower_src(
        r#"
        export function outer(x: number): any {
            function inner(y: number): any {
                const o: any = {};
                o.a = y + 1;
                return o;
            }
            const peek = (o: any) => o;
            const o: any = {};
            o.a = x + 1;
            o.n = inner(x);
            return peek(o);
        }
        "#,
    );
    let folded: Vec<usize> = all_stmt_lists(&module)
        .into_iter()
        .filter_map(|stmts| anon_shape_arity(stmts, "o"))
        .collect();
    assert!(
        folded.contains(&1) && !folded.contains(&0),
        "both builders should fold, since `inner` and `peek` bind their own `o`: {folded:?} {:?}",
        module.functions
    );
}

#[test]
fn a_body_declaration_does_not_shadow_a_parameter_default() {
    // The default is evaluated in its own scope and reads the OUTER `o`.
    let module = lower_src(
        r#"
        export function run(w: any): any {
            const peek = (a = o) => { var o; return a; };
            const o: any = {};
            o.a = "" + w;
            return peek;
        }
        "#,
    );
    assert_eq!(
        arity_anywhere(&module, "o"),
        Some(0),
        "the default parameter observes the builder: {:?}",
        module.functions
    );
}

#[test]
fn a_block_level_redeclaration_does_not_shadow() {
    let module = lower_src(
        r#"
        export function run(w: any): any {
            const peek = () => { { const o = 1; } return o; };
            const o: any = {};
            o.a = "" + w;
            return peek;
        }
        "#,
    );
    assert_eq!(
        arity_anywhere(&module, "o"),
        Some(0),
        "only function-level bindings shadow: {:?}",
        module.functions
    );
}

#[test]
fn a_global_read_does_not_fold_on_an_observable_builder() {
    // `gObs` is no declared binding, so it may be an accessor on the global
    // object — a getter that reads `o` before it is initialized.
    let module = lower_src(
        r#"
        export function run(): any {
            const peek = () => o;
            const o: any = {};
            o.a = gObs;
            return peek;
        }
        "#,
    );
    assert_eq!(
        arity_anywhere(&module, "o"),
        Some(0),
        "an undeclared identifier may run a global getter: {:?}",
        module.functions
    );
}

#[test]
fn reads_of_declared_bindings_still_fold_on_an_observable_builder() {
    let module = lower_src(
        r#"
        const top = 1;
        export function run(w: any): any {
            var fnVar = 2;
            const peek = () => o;
            for (let i = 0; i < 1; i++) {
                try {
                    throw 0;
                } catch (err) {
                    const o: any = {};
                    o.a = w; o.b = top; o.c = fnVar; o.d = i; o.e = err; o.f = undefined;
                    return peek;
                }
            }
        }
        "#,
    );
    assert_eq!(
        arity_anywhere(&module, "o"),
        Some(6),
        "parameters, outer and loop-head bindings, catch parameters and `undefined` run no code: {:?}",
        module.functions
    );
}

#[test]
fn an_ambient_declaration_is_not_a_binding() {
    // `declare` binds nothing at run time: the read still goes to the global
    // object (#10363 tracks perry materializing it today).
    let module = lower_src(
        r#"
        declare const gAmb: any;
        export function run(): any {
            const peek = () => o;
            const o: any = {};
            o.a = gAmb;
            return peek;
        }
        "#,
    );
    assert_eq!(
        arity_anywhere(&module, "o"),
        Some(0),
        "`declare const` must not count as a binding: {:?}",
        module.functions
    );
}

#[test]
fn a_declaration_in_a_sibling_block_does_not_resolve_the_read() {
    let module = lower_src(
        r#"
        export function run(): any {
            const peek = () => o;
            {
                const gSib = 1;
            }
            const o: any = {};
            o.a = gSib;
            return peek;
        }
        "#,
    );
    assert_eq!(
        arity_anywhere(&module, "o"),
        Some(0),
        "`gSib` here is the global, not the block's binding: {:?}",
        module.functions
    );
}
