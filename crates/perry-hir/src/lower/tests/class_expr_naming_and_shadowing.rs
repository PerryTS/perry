//! Named class-expression naming and nested-class shadowing tests (split out
//! of `lower/tests.rs` to keep it under the 2,000-line cap, #10750).

/// #6679: a NAMED class EXPRESSION's `.name` is its own explicit name
/// (`Named` in `const B = class Named {}`), not the outer binding name. Per
/// spec a named class expression is not an anonymous function definition, so
/// the assignment's NamedEvaluation (`SetFunctionName` from `const B =`) must
/// not clobber the declared name. The module-top-level `const X = class {…}`
/// fast path registers the class under the binding name so `new B()` /
/// `instanceof B` resolve statically, and records a `class_display_names`
/// override to the explicit name for codegen to emit as `.name`. An ANONYMOUS
/// `const A = class {}` takes the inferred binding name and needs no override.
#[test]
fn test_named_class_expression_var_decl_reports_explicit_name() {
    let source = r#"
        const B = class Named {};
        const A = class {};
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");

    let named = hir
        .classes
        .iter()
        .find(|c| c.name == "B")
        .expect("class registered under binding name `B`");
    assert_eq!(
        hir.class_display_names.get(&named.id).map(String::as_str),
        Some("Named"),
        "named class expression must report its explicit name as `.name`"
    );

    let anon = hir
        .classes
        .iter()
        .find(|c| c.name == "A")
        .expect("anonymous class registered under inferred name `A`");
    assert_eq!(
        hir.class_display_names.get(&anon.id),
        None,
        "anonymous class expression uses the inferred binding name, no override"
    );
}

/// #8040: a `class A` declared inside a nested factory, referenced by `new A()`
/// from one of its OWN method bodies, while a same-named binding (`var A`)
/// exists in an enclosing scope.
///
/// `expr_new.rs` snapshotted `ctx.lookup_local("A")` unconditionally and, when
/// it hit, rerouted the construct to `NewDynamic { callee: LocalGet(<outer
/// slot>) }`. A method compiles to its own function, so that slot index names
/// an unrelated (undefined) local there and the construct threw `TypeError:
/// undefined is not a constructor` at runtime. The bare-ident read arm already
/// resolved the same name to the class via `forward_class_shadows_local`; this
/// makes `new` agree.
///
/// Next 16's webpack chunk for the bundled `@opentelemetry/api` is exactly this
/// shape — `var …,i,…` in the module IIFE and `class i { static getInstance(){
/// return this._instance || (this._instance = new i), this._instance } }` in an
/// inner factory — so `context.active()` was unreachable at request time.
#[test]
fn nested_class_shadowing_outer_var_constructs_the_class_not_the_local() {
    let source = r#"
        var A: any;
        const g = () => {
            class A {
                static mk(): any {
                    return new A();
                }
                m(): string {
                    return "ok";
                }
            }
            return A;
        };
        const out: any = g().mk().m();
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");

    let mk = hir
        .classes
        .iter()
        .find(|c| c.name == "A")
        .expect("class A is lowered")
        .static_methods
        .iter()
        .find(|m| m.name == "mk")
        .expect("static method mk is lowered");
    let body = format!("{:#?}", mk.body);

    assert!(
        !body.contains("NewDynamic"),
        "`new A()` inside A's own method must not construct through an \
         enclosing-scope local slot: {body}"
    );
    assert!(
        body.contains("class_name: \"A\""),
        "`new A()` inside A's own method must construct class A: {body}"
    );
}

/// Self-construction is lowered before a named class expression's capture
/// union is known. The post-body pass appends the lexical self cell, and must
/// also mark it as a capture argument so constructor binding does not discard
/// it as an ordinary user argument.
#[test]
fn named_class_expr_self_new_records_appended_capture_provenance() {
    let source = r#"
        const make = () => class c {
            static create(): any { return new c(); }
            constructor() { if (c === null) throw new Error("unreachable"); }
        };
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let create = hir
        .classes
        .iter()
        .find(|class| class.name.starts_with("c__class_expr_"))
        .expect("named class expression is lowered")
        .static_methods
        .iter()
        .find(|method| method.name == "create")
        .expect("static create method is lowered");
    let body = format!("{:#?}", create.body);

    assert!(
        body.contains("cap_args_appended: 1"),
        "the appended lexical-self cell must be identified as a capture arg: {body}"
    );
}

/// A private update wraps its receiver twice: once for the read and once for
/// the write. Both guards must preserve the named class expression's lexical
/// self binding, including when that binding is captured by a nested arrow.
#[test]
fn named_class_expr_static_private_update_in_arrow_keeps_lexical_brand_owner() {
    let source = r#"
        const make = () => class c {
            static #v = 0;
            static f() { return (() => { c.#v++; return c.#v; })(); }
        };
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let method = hir
        .classes
        .iter()
        .find(|class| class.name.starts_with("c__class_expr_"))
        .expect("named class expression is lowered")
        .static_methods
        .iter()
        .find(|method| method.name == "f")
        .expect("static f method is lowered");
    let body = format!("{:#?}", method.body);

    assert_eq!(
        body.matches("receiver_is_brand_owner: true").count(),
        3,
        "the update's read/write guards and the following read must identify the lexical class owner: {body}"
    );
    assert!(
        !body.contains("receiver_is_brand_owner: false"),
        "both guards around the private update must retain the lexical class owner: {body}"
    );
}

/// A named class expression whose outer binding has a different name uses a
/// synthetic registry key. `typeof` must still resolve the source-level inner
/// name through the class body's lexical binding rather than an optional
/// global lookup.
#[test]
fn typeof_named_class_expr_inner_binding_uses_the_current_class() {
    let source = r#"
        var B = class l {
            static selfType(): string { return typeof l; }
        };
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let method = hir
        .classes
        .iter()
        .flat_map(|class| &class.static_methods)
        .find(|method| method.name == "selfType")
        .expect("static selfType method is lowered");
    let body = format!("{:#?}", method.body);

    assert!(
        body.contains("ClassRef") && !body.contains("js_global_get_optional"),
        "the class's inner name must resolve to its synthetic ClassRef: {body}"
    );
}

/// A sibling class declaration is already a known lexical binding while an
/// earlier class method is lowered, even though its registry entry is emitted
/// later. The unresolved-constructor guard must preserve that forward binding.
#[test]
fn nested_method_constructs_forward_declared_sibling_class() {
    let source = r#"
        function make() {
            class Base {
                makeChild(): any {
                    return new Child();
                }
            }
            class Child extends Base {}
            return Base;
        }
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let make_child = hir
        .classes
        .iter()
        .find(|class| class.name == "Base")
        .expect("Base class is lowered")
        .methods
        .iter()
        .find(|method| method.name == "makeChild")
        .expect("makeChild method is lowered");

    assert!(
        matches!(
            make_child.body.as_slice(),
            [crate::Stmt::Return(Some(crate::Expr::New { class_name, .. }))]
                if class_name == "Child"
        ),
        "forward sibling construction must remain a static class construct: {:#?}",
        make_child.body
    );
}

/// Forward-declaration bookkeeping uses source identifiers, while a sibling
/// class may use a collision-safe registration name. Constructor resolution
/// must compare the source identifier before rejecting the forward binding.
#[test]
fn nested_method_constructs_collision_renamed_forward_sibling_class() {
    let source = r#"
        function first() {
            class Child {}
            return Child;
        }
        function make() {
            class Base {
                makeChild(): any {
                    return new Child();
                }
            }
            class Child extends Base {}
            return Base;
        }
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let make_child = hir
        .classes
        .iter()
        .find(|class| class.name == "Base")
        .expect("Base class is lowered")
        .methods
        .iter()
        .find(|method| method.name == "makeChild")
        .expect("makeChild method is lowered");

    assert!(
        matches!(
            make_child.body.as_slice(),
            [crate::Stmt::Return(Some(crate::Expr::New { class_name, .. }))]
                if class_name.starts_with("Child$")
        ),
        "collision-renamed forward sibling construction must remain a static class construct: {:#?}",
        make_child.body
    );
}

/// A collision-safe registration key is compiler-internal; the evaluated
/// class declaration must still bind and read through its source-level name.
#[test]
fn fresh_class_declaration_collision_keeps_lexical_binding() {
    let source = r#"
        function first() {
            class C { #x = 1; }
            return C;
        }
        function second() {
            class C { #x = 2; static missing; }
            const value = C.missing;
            return C;
        }
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let second = hir
        .functions
        .iter()
        .find(|function| function.name == "second")
        .expect("second function lowers");
    let (binding_id, template) = second
        .body
        .iter()
        .find_map(|stmt| match stmt {
            crate::Stmt::Let {
                id,
                name,
                init: Some(crate::Expr::ClassExprFresh { template, .. }),
                ..
            } if name == "C" => Some((*id, template.as_str())),
            _ => None,
        })
        .expect("fresh class is bound under source name");
    assert_ne!(template, "C", "second template should be collision-renamed");
    assert!(second.body.iter().any(|stmt| {
        matches!(stmt, crate::Stmt::Return(Some(crate::Expr::LocalGet(id))) if *id == binding_id)
    }));
    assert!(second.body.iter().any(|stmt| {
        matches!(
            stmt,
            crate::Stmt::Let {
                name,
                init: Some(crate::Expr::PropertyGet { object, property, .. }),
                ..
            } if name == "value"
                && property == "missing"
                && matches!(object.as_ref(), crate::Expr::LocalGet(id) if *id == binding_id)
        )
    }));
}

/// A fresh class's end-of-body capture refresh must preserve the whole
/// one-element shared-mutable cell, matching the initial `ClassExprFresh`
/// snapshot. Refreshing with `cell[0]` stores the scalar value, while lifted
/// members still read the constructor capture as `capture[0]`.
#[test]
fn fresh_class_refresh_keeps_shared_capture_cell_handle() {
    let source = r#"
        const exported = (() => {
            let dep;
            dep = { default: "ok" };
            const holder = {};
            holder.default = class {
                read() { return dep.default; }
            };
            return holder.default;
        })();
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let compact: String = format!("{:#?}", hir.init)
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect();

    let mut remainder = compact.as_str();
    let mut refreshes = 0usize;
    while let Some(offset) = remainder.find("RefreshClassExprCaptures{") {
        remainder = &remainder[offset + "RefreshClassExprCaptures{".len()..];
        let captures = remainder
            .find("captures:[")
            .map(|index| &remainder[index + "captures:[".len()..])
            .expect("refresh includes a captures vector");
        assert!(
            captures.starts_with("LocalGet("),
            "fresh-class refresh must carry the shared cell handle, not an indexed value: {captures}"
        );
        refreshes += 1;
    }
    assert!(
        refreshes > 0,
        "fixture must emit at least one fresh-class refresh"
    );
}

/// A mutable lexical classic-for binding captured by a class uses the shared
/// one-element cell representation.  The backedge must copy its current value
/// into a fresh cell before the update, matching CreatePerIterationEnvironment:
/// instances from completed iterations retain their own capture cell, while a
/// `var` head continues to share one binding for the whole loop.
#[test]
fn classic_for_class_capture_freshens_lexical_cell_before_update() {
    let source = r#"
        const lexical = [];
        for (let i = 0; i < 3; i++) {
            class Lexical { value() { return i; } }
            lexical.push(new Lexical());
        }

        const shared = [];
        for (var i = 0; i < 3; i++) {
            class Shared { value() { return i; } }
            shared.push(new Shared());
        }
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let loops: Vec<_> = hir
        .init
        .iter()
        .filter_map(|stmt| match stmt {
            crate::Stmt::For { init, update, .. } => Some((init, update)),
            _ => None,
        })
        .collect();
    assert_eq!(loops.len(), 2, "fixture lowers to lexical and var loops");

    let lexical_id = match loops[0].0.as_deref() {
        Some(crate::Stmt::Let {
            id,
            init: Some(crate::Expr::Array(items)),
            ..
        }) if items.len() == 1 => *id,
        other => panic!("lexical head must lower to one shared capture cell: {other:#?}"),
    };
    assert!(matches!(
        loops[0].1,
        Some(crate::Expr::Sequence(items))
            if matches!(
                items.as_slice(),
                [
                    crate::Expr::LocalSet(set_id, fresh),
                    crate::Expr::IndexUpdate { object, .. },
                ] if *set_id == lexical_id
                    && matches!(
                        fresh.as_ref(),
                        crate::Expr::Array(values)
                            if matches!(
                                values.as_slice(),
                                [crate::Expr::IndexGet { object, .. }]
                                    if matches!(object.as_ref(), crate::Expr::LocalGet(id) if *id == lexical_id)
                            )
                    )
                    && matches!(object.as_ref(), crate::Expr::LocalGet(id) if *id == lexical_id)
            )
    ));

    assert!(
        loops[1].0.is_none(),
        "var head is hoisted outside For::init"
    );
    assert!(matches!(loops[1].1, Some(crate::Expr::IndexUpdate { .. })));
}

/// Companion (the case the depth rule must NOT break): a module-scope `class e`
/// and a factory-local `let e` holding a different constructor. JS says the
/// nearer local wins, so `new e()` inside the factory must still construct the
/// LOCAL's value — mysql2's bundled chunk shape, where taking the class instead
/// silently ran the wrong constructor.
#[test]
fn factory_local_still_shadows_module_scope_class_in_new() {
    let source = r#"
        class e {
            tag(): string { return "class-e"; }
        }
        function make(): any {
            const e: any = function () { return undefined; };
            return new e();
        }
        const keep: any = e;
        const out: any = make();
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");

    let make = hir
        .functions
        .iter()
        .find(|f| f.name == "make")
        .expect("function make is lowered");
    let body = format!("{:#?}", make.body);

    assert!(
        body.contains("NewDynamic"),
        "a factory-local binding must keep shadowing a module-scope class of \
         the same name for `new`: {body}"
    );
}

/// #8040, the shape the minified `@opentelemetry/api` bundle actually has: a
/// file with MANY same-named single-letter classes over one outer `var i`.
///
/// The collision rename accidentally immunised every duplicate — `i$0`, `i$1`,
/// … match no local, so `lookup_local` missed and the reroute never fired for
/// them. Only the FIRST `class i`, the one that keeps the bare name, was
/// broken. That asymmetry is why the bundle's `trace` API worked while its
/// `context` and `propagation` APIs did not, and why a symptom that looks like
/// "prototype methods are missing" moves when unrelated code is added to the
/// file. All three must construct their own class.
#[test]
fn first_of_several_same_named_nested_classes_constructs_itself() {
    let source = r#"
        function t(n: string, f: () => any): void {
            try { console.log(n + ": " + String(f())); } catch (e) { console.log(String(e)); }
        }
        var i: any;
        const f1 = () => {
            class i {
                static mk(): any { return new i(); }
                m(): string { return "one"; }
            }
            return i;
        };
        const f2 = () => {
            class i {
                static mk(): any { return new i(); }
                m(): string { return "two"; }
            }
            return i;
        };
        const f3 = () => {
            class i {
                static mk(): any { return new i(); }
                m(): string { return "three"; }
            }
            return i;
        };
        t("f1", () => f1().mk().m());
        t("f2", () => f2().mk().m());
        t("f3", () => f3().mk().m());
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");

    // The first `class i` keeps the bare name; the duplicate is renamed.
    let first = hir
        .classes
        .iter()
        .find(|c| c.name == "i")
        .expect("the first class keeps the bare name `i`");
    let mk = first
        .static_methods
        .iter()
        .find(|m| m.name == "mk")
        .expect("static method mk is lowered");
    let body = format!("{:#?}", mk.body);

    assert!(
        !body.contains("NewDynamic"),
        "`new i()` inside i's own method must not construct through the \
         enclosing binding's slot: {body}"
    );
    assert!(
        body.contains("class_name: \"i\""),
        "`new i()` inside i's own method must construct class i: {body}"
    );
}

/// Over-trigger guard: a binding declared in the METHOD's own scope still wins.
/// `m() { const C = Other; return new C(); }` constructs `Other`, not the
/// enclosing class — `lookup_local_in_current_scope` is what keeps that true.
#[test]
fn method_local_shadowing_the_class_name_still_wins_in_new() {
    let source = r#"
        class Other {
            tag(): string { return "other"; }
        }
        const g = () => {
            class C {
                static mk(): any {
                    const C: any = Other;
                    return new C();
                }
            }
            return C;
        };
        const out: any = g().mk();
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");

    let mk = hir
        .classes
        .iter()
        .find(|c| c.name == "C")
        .expect("class C is lowered")
        .static_methods
        .iter()
        .find(|m| m.name == "mk")
        .expect("static method mk is lowered");
    let body = format!("{:#?}", mk.body);

    assert!(
        body.contains("NewDynamic"),
        "a method-scope local named after the class must still win for `new`: {body}"
    );
}
