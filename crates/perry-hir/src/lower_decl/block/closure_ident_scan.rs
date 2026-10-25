//! `cic_*` — the "collect identifiers referenced from inside a closure"
//! traversal used by `pre_register_forward_captured_lets`.
//!
//! The walk is swc's own generic visitor, so every position that can hold an
//! expression is visited by default: a syntax kind added to the AST, or one
//! this module never named, is scanned instead of skipped. The visitor
//! overrides only what carries context:
//!
//! - `in_cl`: whether the walk is inside a closure (a function, an arrow, or
//!   the body of a class member that runs later: methods, constructors, field
//!   initializers, static blocks). A name is collected only there. A class's
//!   decorators, `extends` clause and member names run where the class is
//!   declared, so they keep the enclosing `in_cl`.
//! - `targets`: whether the walk is inside an assignment pattern, where a
//!   plain identifier names an enclosing binding. In a binding pattern (a
//!   declarator, a parameter, a catch parameter) it declares one and is not a
//!   reference. The defaults and computed names inside either kind of
//!   pattern are expressions, scanned like any other.
//!
//! The scan is name-based and over-approximates: it has no shadowing rules,
//! and a name collected that is not a `let` / `const` of the scope being
//! pre-registered is simply ignored. The identifiers it skips are the ones
//! that cannot read a binding: the declared names of functions and classes,
//! and everything inside a type position.
//!
//! A direct `eval(..)` can name any binding of its scope, in a string the scan
//! cannot read (and a constant string lowers into real reads later), and it
//! can create a closure that outlives the call: in a closure or in the code
//! that makes closures, it reads whatever a closure could. It is recorded as
//! [`DIRECT_EVAL`] wherever the walk meets one (the member bodies the hide
//! keeps off a pending binding are not walked), and [`reads_class`] counts it
//! as a read of every class.

use std::collections::HashSet;
use swc_ecma_ast as ast;
use swc_ecma_visit::{noop_visit_type, Visit, VisitWith};

/// What a direct `eval(..)` adds to the collected names, in a closure or not
/// (the eval itself can create the closure that reads a class): no identifier
/// spells it, and [`reads_class`] takes it for a read of any class.
const DIRECT_EVAL: &str = "\0direct eval";

/// Does the closure code collected in `reads` (by [`class_binding_reads_stmt`])
/// read the class `name`: by the name, or through a direct `eval(..)` that can
/// name it?
pub(super) fn reads_class(reads: &HashSet<String>, name: &str) -> bool {
    reads.contains(name) || reads.contains(DIRECT_EVAL)
}

/// `eval(..)` or `(eval)(..)`: a call that sees the caller's bindings
/// (`eval?.(..)` and `(0, eval)(..)` are indirect and do not).
fn is_direct_eval(c: &ast::CallExpr) -> bool {
    let ast::Callee::Expr(callee) = &c.callee else {
        return false;
    };
    let mut callee: &ast::Expr = callee;
    while let ast::Expr::Paren(p) = callee {
        callee = &p.expr;
    }
    matches!(callee, ast::Expr::Ident(i) if i.sym.as_ref() == "eval")
}

struct ClosureRefs<'a> {
    in_cl: bool,
    targets: bool,
    /// Skip the bodies `with_pending_forward_class_bindings_hidden` keeps off a
    /// pending class binding (see `class_binding_reads_stmt`).
    hidden_members: bool,
    out: &'a mut HashSet<String>,
}

impl ClosureRefs<'_> {
    fn reference(&mut self, id: &ast::Ident) {
        if self.in_cl {
            self.out.insert(id.sym.to_string());
        }
    }

    /// Run `f` as code that executes later, when the closure is called.
    fn closure(&mut self, f: impl FnOnce(&mut Self)) {
        let outer = std::mem::replace(&mut self.in_cl, true);
        f(self);
        self.in_cl = outer;
    }

    /// Is this a member body of a class that sits directly in the function
    /// whose pending class bindings are being collected? Such a body lowers
    /// with those bindings hidden, so nothing in it reads one.
    fn member_body_is_hidden(&self) -> bool {
        self.hidden_members && !self.in_cl
    }

    fn patterns_are_targets(&mut self, targets: bool, f: impl FnOnce(&mut Self)) {
        let outer = std::mem::replace(&mut self.targets, targets);
        f(self);
        self.targets = outer;
    }
}

impl Visit for ClosureRefs<'_> {
    noop_visit_type!();

    fn visit_ident(&mut self, id: &ast::Ident) {
        self.reference(id);
    }

    /// A pattern leaf: a reference where it is assigned to, a declaration
    /// where it is bound.
    fn visit_binding_ident(&mut self, id: &ast::BindingIdent) {
        if self.targets {
            self.reference(&id.id);
        }
    }

    /// Whatever an expression contains, a pattern leaf inside it is a
    /// declaration again (an arrow parameter in an assignment pattern's
    /// default), unless an assignment re-enters target mode.
    fn visit_expr(&mut self, e: &ast::Expr) {
        self.patterns_are_targets(false, |s| e.visit_children_with(s));
    }

    fn visit_call_expr(&mut self, c: &ast::CallExpr) {
        if is_direct_eval(c) {
            self.out.insert(DIRECT_EVAL.to_string());
        }
        c.visit_children_with(self);
    }

    fn visit_assign_expr(&mut self, a: &ast::AssignExpr) {
        self.patterns_are_targets(true, |s| a.left.visit_with(s));
        a.right.visit_with(self);
    }

    /// `for (x of ..)` assigns to existing bindings; `for (const x of ..)`
    /// declares them.
    fn visit_for_head(&mut self, h: &ast::ForHead) {
        let targets = matches!(h, ast::ForHead::Pat(_));
        self.patterns_are_targets(targets, |s| h.visit_children_with(s));
    }

    /// A function's name is its own declaration. Its parameters' defaults and
    /// its body run at call time; its decorators run where it is declared.
    fn visit_fn_decl(&mut self, f: &ast::FnDecl) {
        f.function.visit_with(self);
    }

    fn visit_fn_expr(&mut self, f: &ast::FnExpr) {
        f.function.visit_with(self);
    }

    fn visit_function(&mut self, f: &ast::Function) {
        f.decorators.visit_with(self);
        self.closure(|s| {
            f.params.visit_with(s);
            f.body.visit_with(s);
        });
    }

    fn visit_arrow_expr(&mut self, a: &ast::ArrowExpr) {
        self.closure(|s| {
            a.params.visit_with(s);
            a.body.visit_with(s);
        });
    }

    fn visit_class_decl(&mut self, c: &ast::ClassDecl) {
        c.class.visit_with(self);
    }

    fn visit_class_expr(&mut self, c: &ast::ClassExpr) {
        c.class.visit_with(self);
    }

    // A class member's name is evaluated where the class is declared (the
    // default walk of the member, in the enclosing `in_cl`); what the member
    // runs later is a closure. #6523: a CONSTRUCTOR body references a binding
    // declared after the class (`class C { constructor(){ a() } } const a =
    // ...`) exactly like a method body does.
    fn visit_class_prop(&mut self, p: &ast::ClassProp) {
        p.decorators.visit_with(self);
        p.key.visit_with(self);
        // A static field's initializer runs when the class is defined: not hidden.
        if !p.is_static && self.member_body_is_hidden() {
            return;
        }
        self.closure(|s| p.value.visit_with(s));
    }

    fn visit_private_prop(&mut self, p: &ast::PrivateProp) {
        p.decorators.visit_with(self);
        p.key.visit_with(self);
        if !p.is_static && self.member_body_is_hidden() {
            return;
        }
        self.closure(|s| p.value.visit_with(s));
    }

    fn visit_class_method(&mut self, m: &ast::ClassMethod) {
        if self.member_body_is_hidden() {
            m.function.decorators.visit_with(self);
            m.key.visit_with(self);
            return;
        }
        m.key.visit_with(self);
        m.function.visit_with(self);
    }

    fn visit_private_method(&mut self, m: &ast::PrivateMethod) {
        if self.member_body_is_hidden() {
            m.function.decorators.visit_with(self);
            return;
        }
        m.key.visit_with(self);
        m.function.visit_with(self);
    }

    fn visit_auto_accessor(&mut self, p: &ast::AutoAccessor) {
        p.decorators.visit_with(self);
        p.key.visit_with(self);
        self.closure(|s| p.value.visit_with(s));
    }

    fn visit_constructor(&mut self, c: &ast::Constructor) {
        c.key.visit_with(self);
        if self.member_body_is_hidden() {
            return;
        }
        self.closure(|s| {
            c.params.visit_with(s);
            c.body.visit_with(s);
        });
    }

    fn visit_static_block(&mut self, b: &ast::StaticBlock) {
        if self.member_body_is_hidden() {
            return;
        }
        self.closure(|s| b.body.visit_with(s));
    }

    /// `{ a = 1 }` is only valid as a pattern; as an expression only its
    /// value is read.
    fn visit_assign_prop(&mut self, p: &ast::AssignProp) {
        p.value.visit_with(self);
    }
}

pub(super) fn cic_stmt(s: &ast::Stmt, in_cl: bool, out: &mut HashSet<String>) {
    s.visit_with(&mut ClosureRefs {
        in_cl,
        targets: false,
        hidden_members: false,
        out,
    });
}

/// The names the closures of statement `s` (a statement of the function body
/// or block being scanned, not of a closure) can resolve to a class binding
/// pre-registered for it: [`cic_stmt`] without the member bodies of the
/// classes declared or expressed directly in `s` (methods, accessors,
/// constructors, instance field initializers, static blocks, with the closures
/// nested in them). Those lower with the pending class bindings hidden
/// (`with_pending_forward_class_bindings_hidden`) and name a later class
/// statically. What runs when a class is defined stays in: a computed key, the
/// `extends` clause, decorators, a static field initializer, and every class
/// inside a closure.
pub(super) fn class_binding_reads_stmt(s: &ast::Stmt, out: &mut HashSet<String>) {
    s.visit_with(&mut ClosureRefs {
        in_cl: false,
        targets: false,
        hidden_members: true,
        out,
    });
}

pub(super) fn cic_expr(e: &ast::Expr, in_cl: bool, out: &mut HashSet<String>) {
    e.visit_with(&mut ClosureRefs {
        in_cl,
        targets: false,
        hidden_members: false,
        out,
    });
}

/// The names a function's parameter list (defaults, patterns' computed names)
/// and body reference: everything that runs when the function is called.
pub(crate) fn cic_function(f: &ast::Function, out: &mut HashSet<String>) {
    f.visit_with(&mut ClosureRefs {
        in_cl: false,
        targets: false,
        hidden_members: false,
        out,
    });
}

#[cfg(test)]
mod tests {
    use super::{cic_stmt, class_binding_reads_stmt, reads_class};
    use std::collections::HashSet;

    /// The names `cic_stmt` collects from `source`'s top-level statements.
    fn closure_refs(source: &str) -> HashSet<String> {
        let parsed = perry_parser::parse_typescript(source, "scan.ts").unwrap();
        let mut refs = HashSet::new();
        for item in &parsed.body {
            if let swc_ecma_ast::ModuleItem::Stmt(stmt) = item {
                cic_stmt(stmt, false, &mut refs);
            }
        }
        refs
    }

    /// Every position of a closure body that can hold an expression names the
    /// enclosing binding `marker`, so the scan must collect it. One entry per
    /// syntax kind that carries an `Expr`, `Pat` or computed name.
    #[test]
    fn every_expression_position_in_a_closure_is_scanned() {
        let positions = [
            // Object literal members: the computed names run when the object
            // literal is evaluated, inside the closure.
            "({ [marker]: 1 })",
            "({ [marker]() {} })",
            "({ get [marker]() { return 1; } })",
            "({ set [marker](v) {} })",
            "({ async [marker]() {} })",
            "({ *[marker]() {} })",
            "({ m() { return super[marker]; } })",
            "({ ...marker })",
            "({ marker })",
            // Class members.
            "(class { [marker]() {} })",
            "(class { static [marker]() {} })",
            "(class { get [marker]() { return 1; } })",
            "(class { set [marker](v) {} })",
            "(class { [marker] = 1; })",
            "(class { static [marker] = 1; })",
            "(class { p = marker; })",
            "(class { static p = marker; })",
            "(class { static { marker; } })",
            "(class { #p = marker; })",
            "(class { m(p = marker) {} })",
            "(class { constructor(p = marker) {} })",
            "(class { constructor() { marker; } })",
            "(class B extends marker {})",
            "(class extends Object { constructor() { super(marker); } })",
            "(class { accessor p = marker; })",
            "(class { accessor [marker] = 1; })",
            "(class { @marker m() {} })",
            "(class { @marker p = 1; })",
            "(@marker class {})",
            // Declarations.
            "using u = marker;",
            "using a = null, u = marker;",
            "await using u = marker;",
            "const { v = marker } = {};",
            "const { [marker]: v } = {};",
            "const [v = marker] = [];",
            "let u = marker;",
            "class K { [marker]() {} }",
            "class K extends marker {}",
            "function g(p = marker) {}",
            "function g({ p = marker }) {}",
            // Statements.
            "if (marker) {}",
            "while (marker) {}",
            "do {} while (marker);",
            "for (marker; ;) { break; }",
            "for (let i = marker; ;) { break; }",
            "for (const x of marker) {}",
            "for (const x in marker) {}",
            "for (using u of marker) {}",
            "for (await using u of marker) {}",
            "for (const { v = marker } of []) {}",
            "for (x of [marker]) {}",
            "for (x[marker] of []) {}",
            "switch (marker) {}",
            "switch (0) { case marker: }",
            "try { marker; } catch {}",
            "try {} catch (e) { marker; }",
            "try {} catch ({ v = marker }) {}",
            "try {} finally { marker; }",
            "throw marker;",
            "label: { marker; }",
            "return marker;",
            "with (marker) {}",
            // Expressions.
            "marker;",
            "marker = 1;",
            "marker += 1;",
            "marker++;",
            "[marker] = [];",
            "({ marker } = {});",
            "({ a: marker } = {});",
            "({ ...marker } = {});",
            "[...marker] = [];",
            "(marker) = 1;",
            "(marker as any) = 1;",
            "x[marker] = 1;",
            "marker.x = 1;",
            "[marker];",
            "[...marker];",
            "!marker;",
            "marker + 1;",
            "marker ? 1 : 2;",
            "0 ? marker : 2;",
            "0 ? 1 : marker;",
            "marker();",
            "f(marker);",
            "f(...marker);",
            "new marker();",
            "new f(marker);",
            "(1, marker);",
            "`${marker}`;",
            "marker`x`;",
            "f`${marker}`;",
            "(marker, String.raw)`x`;",
            "marker?.x;",
            "f?.(marker);",
            "f?.[marker];",
            "marker?.[0];",
            "x?.y(marker);",
            "(await marker);",
            "(function* () { yield marker; });",
            "(function* () { yield* marker; });",
            "x[marker];",
            "(marker);",
            "marker as any;",
            "<any>marker;",
            "marker!;",
            "marker satisfies any;",
            "typeof marker;",
            "void marker;",
            "delete x[marker];",
            "marker in x;",
            "x instanceof marker;",
            "(() => marker);",
            "((p = marker) => 1);",
            "(function (p = marker) {});",
            "(async () => { await marker; });",
            "(<div>{marker}</div>);",
            "(<A b={marker} />);",
            "(<A {...marker} />);",
            "(<marker.B />);",
        ];
        let mut lost = Vec::new();
        for position in positions {
            let source = format!("const read = async () => {{ {position} }};");
            // JSX needs a tsx file name.
            let filename = if position.contains('<') && !position.contains("<any>") {
                "scan.tsx"
            } else {
                "scan.ts"
            };
            let Ok(parsed) = perry_parser::parse_typescript(&source, filename) else {
                lost.push(format!("does not parse: {position}"));
                continue;
            };
            let mut refs = HashSet::new();
            for item in &parsed.body {
                if let swc_ecma_ast::ModuleItem::Stmt(stmt) = item {
                    cic_stmt(stmt, false, &mut refs);
                }
            }
            if !refs.contains("marker") {
                lost.push(format!("not scanned: {position}"));
            }
        }
        assert!(
            lost.is_empty(),
            "every expression position of a closure must be scanned: {lost:#?}"
        );
    }

    /// A name that a closure declares is not a reference to an enclosing
    /// binding of that name, a name in a type position is not a read, and a
    /// class evaluates its heritage and member names where it is declared, not
    /// inside a closure.
    #[test]
    fn declarations_and_definition_time_positions_are_not_closure_references() {
        let source = "\
            const outer = () => { \
              const a1 = 1; let [a2] = []; const { a3 } = {}; function a4() {} class a5 {} \
              for (const a6 of []) {} try {} catch (a7) {} \
            }; \
            const params = (a8, { a9 }, [a10], ...a11) => 1; \
            function declared(a12) {} \
            class Definition extends a13 { [a14]() {} static [a15] = 1; } \
            const typed = (p: a16): a17 => { \
              let v: a18 = 1; type T = a19; interface I { x: a20 } f<a21>(); \
              class Impl implements a22 {} return v as a23; \
            }; \
            ";
        let refs = closure_refs(source);
        let mut leaked: Vec<String> = (1..=23)
            .map(|n| format!("a{n}"))
            .filter(|name| refs.contains(name))
            .collect();
        leaked.sort();
        assert!(
            leaked.is_empty(),
            "declared or definition-time names are not closure references: {leaked:?}"
        );
        let refs = closure_refs("class Definition { m() { return a1; } p = a2; static { a3; } }");
        for name in ["a1", "a2", "a3"] {
            assert!(
                refs.contains(name),
                "a class member body is a closure: {name}"
            );
        }
    }

    /// What `class_binding_reads_stmt` collects from `source` (one statement).
    fn class_binding_reads(source: &str) -> HashSet<String> {
        let parsed = perry_parser::parse_typescript(source, "scan.ts").unwrap();
        let mut refs = HashSet::new();
        for item in &parsed.body {
            if let swc_ecma_ast::ModuleItem::Stmt(stmt) = item {
                class_binding_reads_stmt(stmt, &mut refs);
            }
        }
        refs
    }

    /// A class's deferred member bodies lower with the pending class bindings
    /// hidden, so they do not read one; everything that runs when the class is
    /// defined, and every class inside a closure, does.
    #[test]
    fn class_binding_reads_skip_only_the_hidden_member_bodies() {
        let hidden = [
            "class H { m() { return C; } }",
            "class H { static m() { return C; } }",
            "class H { m() { return () => C; } }",
            "class H { m(p = C) {} }",
            "class H { get v() { return C; } }",
            "class H { set v(x) { C; } }",
            "class H { async *m() { C; } }",
            "class H { constructor() { C; } }",
            "class H { constructor(p = C) {} }",
            "class H { p = C; }",
            "class H { p = () => C; }",
            "class H { #p = C; }",
            "class H { #m() { C; } }",
            "class H { static { C; } }",
            "class H { static { read = () => C; } }",
            "x = class { m() { return C; } };",
        ];
        for source in hidden {
            assert!(
                !class_binding_reads(source).contains("C"),
                "a hidden member body does not read the class binding: {source}"
            );
            // The unrestricted scan still sees these closures.
            assert!(
                closure_refs(source).contains("C"),
                "the closure scan counts every member body: {source}"
            );
        }
        let definition_time = [
            "class H { [(read = () => C, 'k')]() {} }",
            "class H { static [(read = () => C, 'k')] = 1; }",
            "class H { [(read = () => C, 'k')] = 1; }",
            "class H extends (read = () => C, Object) {}",
            "class H { static p = () => C; }",
            "class H { static #p = () => C; }",
            "class H { @(() => C) m() {} }",
            "(@(() => C) class {})",
            "x = class { [(read = () => C, 'k')]() {} };",
            "x = class extends (read = () => C, Object) {};",
            "read = () => C;",
            "read = function () { return C; };",
            "f(() => C);",
            "const make = () => class { m() { return C; } };",
            "function make() { return class { m() { return C; } }; }",
            "class H { m() {} static p = () => C; }",
        ];
        for source in definition_time {
            assert!(
                class_binding_reads(source).contains("C"),
                "a closure that runs outside a hidden member body reads the class binding: {source}"
            );
        }
    }

    /// A direct `eval(..)` in a closure can name any binding through a string
    /// the scan cannot read, so it reads every class; one outside a closure
    /// (it runs where it stands), an indirect one and a hidden member body
    /// do not.
    #[test]
    fn a_direct_eval_in_a_closure_reads_every_class() {
        for source in [
            "read = () => eval('C');",
            "read = (s) => eval(s);",
            "read = () => eval();",
            "read = () => (eval)('C');",
            "read = () => () => eval('C');",
            "read = function () { return eval(s); };",
            "class H { [(read = () => eval('C'), 'k')]() {} }",
            "class H { static p = () => eval('C'); }",
            "x = () => { class H { m() { return eval('C'); } } };",
        ] {
            let reads = class_binding_reads(source);
            assert!(
                reads_class(&reads, "C") && reads_class(&reads, "Other"),
                "a closure with a direct eval reads any class: {source}"
            );
        }
        for source in [
            "class H { m() { return eval('C'); } }",
            "class H { constructor() { eval('C'); } }",
            "read = () => (0, eval)('C');",
            "read = () => eval?.('C');",
            "read = () => other.eval('C');",
            "read = () => { const eval2 = 1; return eval2; };",
            "read = () => 'C';",
        ] {
            let reads = class_binding_reads(source);
            assert!(
                !reads_class(&reads, "C"),
                "no direct eval of a closure outside a hidden member body: {source}"
            );
        }
    }

    #[test]
    fn a_direct_eval_outside_a_closure_reads_every_class() {
        // The eval can create a closure that outlives the call, or a class
        // whose key, heritage or static initializer runs where it is made.
        for source in [
            "eval('C');",
            "read = eval('() => C');",
            "read = eval(s);",
            "const read = (eval)('() => C');",
            "class H { static p = eval('() => C'); }",
            "class H { [eval('k')]() {} }",
            "class H extends eval('B') {}",
        ] {
            let reads = class_binding_reads(source);
            assert!(
                reads_class(&reads, "C") && reads_class(&reads, "Other"),
                "a direct eval reads any class: {source}"
            );
        }
        for source in [
            "(0, eval)('() => C');",
            "eval?.('() => C');",
            "other.eval('() => C');",
            "read = 'eval()';",
            "class H { static m() { return eval('() => C'); } }",
            "class H { p = eval('() => C'); }",
        ] {
            let reads = class_binding_reads(source);
            assert!(
                !reads_class(&reads, "C"),
                "no direct eval outside a hidden member body: {source}"
            );
        }
    }

    #[test]
    fn erased_wrappers_preserve_forward_lexical_capture() {
        for expression in [
            "later(7) as number",
            "<number>later(7)",
            "later!",
            "later satisfies Function",
            "[later] as const",
            "later<number>",
        ] {
            let source = format!(
                "function identity(x:any) {{ return x; }} function f() {{ \
                 const read = () => ({expression}); \
                 const later = identity((x:number) => x + 1); return read; }}"
            );
            let parsed = perry_parser::parse_typescript(&source, "capture.ts").unwrap();
            let hir = crate::lower_module(&parsed, "capture", "capture.ts").unwrap();
            assert!(
                !format!("{hir:#?}").contains("js_global_get_or_throw_unresolved"),
                "erased wrapper must keep the lexical binding: {expression}"
            );
        }
    }

    /// The defaults and computed keys of a binding pattern (declarator, `for`
    /// head, catch parameter) and of an assignment pattern run inside the
    /// closure, so a binding declared after the closure that they name must be
    /// pre-registered like any other forward reference.
    #[test]
    fn pattern_expressions_preserve_forward_lexical_capture() {
        let bodies = [
            "const { v = later } = {}; return v;",
            "let { v = later } = {}; return v;",
            "const [v = later] = []; return v;",
            "const { a: { v = later } } = { a: {} }; return v;",
            "const { [later]: v } = {}; return v;",
            "for (let i = 0, { v = later } = {}; i < 1; i++) { return v; }",
            "for (const { v = later } of [{}]) { return v; }",
            "for (const { v = later } in { k: 1 }) { return v; }",
            "try { throw {}; } catch ({ v = later }) { return v; }",
            "let v; ({ v = later } = {}); return v;",
            "let v; [v = later] = []; return v;",
            "let v; for ({ v = later } of [{}]) { return v; }",
        ];
        let mut lost = Vec::new();
        for body in bodies {
            let source = format!(
                "function f() {{ const read = () => {{ {body} }}; \
                 const later = 1; return read; }}"
            );
            let parsed = perry_parser::parse_typescript(&source, "capture.ts").unwrap();
            let hir = crate::lower_module(&parsed, "capture", "capture.ts").unwrap();
            if format!("{hir:#?}").contains("js_global_get_or_throw_unresolved") {
                lost.push(body);
            }
        }
        assert!(
            lost.is_empty(),
            "a pattern expression must keep the lexical binding: {lost:#?}"
        );
    }
}
