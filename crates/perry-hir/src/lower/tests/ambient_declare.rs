//! #10363: `declare const/let/var` is an ambient declaration. TypeScript erases
//! it, so it binds nothing and every reference resolves to the global object
//! exactly as if the line were absent. Each test pairs the ambient name with an
//! ordinary binding of the same shape, which must keep lowering as a local.

use crate::ir::{Expr, Module, Stmt};

fn lower(source: &str) -> Module {
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    super::lower_module(&module, "t", "t.ts").expect("source lowers")
}

fn function_debug(hir: &Module, name: &str) -> String {
    let function = hir
        .functions
        .iter()
        .find(|function| function.name == name)
        .unwrap_or_else(|| panic!("function {name} is lowered"));
    format!("{function:?}")
}

fn binds(debug: &str, name: &str) -> bool {
    debug.contains(&format!("name: \"{name}\""))
}

fn global_read(name: &str) -> String {
    format!(
        "ExternFuncRef {{ name: \"js_global_get_or_throw_unresolved\", param_types: [Any], \
         return_type: Any }}, args: [String(\"{name}\")]"
    )
}

#[test]
fn ambient_const_let_var_bind_nothing_at_module_scope() {
    let hir = lower(
        r#"
        declare const dConst: number;
        declare let dLet: string;
        declare var dVar: any;
        const realConst = 1;
        console.log(dConst, dLet, dVar, realConst);
        "#,
    );
    let debug = format!("{:?}", hir.init);
    assert!(
        binds(&debug, "realConst"),
        "control binding is lowered: {debug}"
    );
    for name in ["dConst", "dLet", "dVar"] {
        assert!(
            !binds(&debug, name),
            "`declare` must not materialize a `{name}` binding: {debug}"
        );
        assert!(
            debug.contains(&global_read(name)),
            "`{name}` must be read off the global object: {debug}"
        );
    }
}

/// The issue's decl4 shape: a function lowered BEFORE the declaration reads
/// the name. The forward pre-registration pass used to hand it a local.
#[test]
fn function_declared_before_the_ambient_line_reads_the_global() {
    let hir = lower(
        r#"
        function ambient(): string { return typeof dInner; }
        function real(): string { return typeof realLater; }
        declare const dInner: string;
        const realLater = "x";
        "#,
    );
    let ambient = function_debug(&hir, "ambient");
    assert!(
        ambient.contains("js_global_get_optional") && ambient.contains("String(\"dInner\")"),
        "`typeof` of an ambient name is a non-throwing global lookup: {ambient}"
    );
    let real = function_debug(&hir, "real");
    assert!(
        !real.contains("String(\"realLater\")"),
        "control: a real module binding still resolves lexically: {real}"
    );
}

/// `declare var` in a Script entry used to be published as a non-configurable
/// global property before user code ran, so a later
/// `Object.defineProperty(globalThis, "dVar", …)` threw.
#[test]
fn ambient_var_is_not_a_script_global_var() {
    let source = r#"
        declare var dVar: any;
        var realVar = 1;
        (globalThis as any).probe = realVar;
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    // The driver installs the source; its `globalThis` mention is what arms
    // the Script global-var reflection.
    crate::ir::set_current_module_source(source.to_string());
    let lowered = super::lower_module_with_class_id_types_seed_and_entry(
        &module, "t", "t.ts", 1, None, None, None, true,
    );
    crate::ir::clear_current_module_source();
    let (hir, _) = lowered.expect("source lowers");
    let names = &hir.annexb_global_undefined_names;
    assert!(
        names.iter().any(|name| name == "realVar"),
        "control: a real Script `var` is a global var: {names:?}"
    );
    assert!(
        !names.iter().any(|name| name == "dVar"),
        "an ambient `declare var` must not create a global property: {names:?}"
    );
}

/// `export declare const` is erased exactly like `export declare function`:
/// no binding and no export.
#[test]
fn export_declare_const_exports_nothing() {
    let hir = lower(
        r#"
        export declare const dExported: number;
        export const alias = dExported;
        "#,
    );
    let exports = format!("{:?}", hir.exports);
    assert!(exports.contains("\"alias\""), "control export: {exports}");
    assert!(
        !exports.contains("\"dExported\""),
        "an ambient declaration has no runtime export: {exports}"
    );
    let init = format!("{:?}", hir.init);
    assert!(
        init.contains(&global_read("dExported")),
        "a reference to an erased export reads the global: {init}"
    );
}

#[test]
fn ambient_declaration_in_a_namespace_binds_nothing() {
    let hir = lower(
        r#"
        namespace N {
            declare const nsAmbient: number;
            const nsReal = 2;
            export const viaAmbient = nsAmbient;
            export const viaReal = nsReal;
        }
        "#,
    );
    let init = format!("{:?}", hir.init);
    assert!(binds(&init, "nsReal"), "control namespace binding: {init}");
    assert!(
        !binds(&init, "nsAmbient"),
        "namespace ambient binding: {init}"
    );
    assert!(init.contains(&global_read("nsAmbient")), "{init}");
}

/// tsc rejects `declare` inside a function body, but swc parses it and type
/// stripping erases it, so it must bind nothing there either, including in
/// the body's `var` hoisting and closure forward-capture passes.
#[test]
fn ambient_declarations_in_function_bodies_bind_nothing() {
    let hir = lower(
        r#"
        function body(): number {
            const early = () => bodyConst + realLater;
            declare const bodyConst: number;
            declare var bodyVar: number;
            const realLater = 3;
            return early() + bodyVar;
        }
        const expr = function (): number {
            function inner(): number { return exprConst + exprReal; }
            declare const exprConst: number;
            declare var exprVar: number;
            const exprReal = 4;
            return inner() + exprVar;
        };
        "#,
    );
    let debug = format!("{hir:?}");
    for real in ["realLater", "exprReal"] {
        assert!(
            binds(&debug, real),
            "control function binding `{real}`: {debug}"
        );
    }
    for name in ["bodyConst", "bodyVar", "exprConst", "exprVar"] {
        assert!(
            !binds(&debug, name),
            "function-body ambient `{name}`: {debug}"
        );
        assert!(
            debug.contains(&global_read(name)),
            "`{name}` reads the global: {debug}"
        );
    }
}

/// With the declaration erased, a closure in the body reads the enclosing
/// binding. The body's forward-capture pass must not box that outer binding
/// as if the body had declared its own `var`.
#[test]
fn closure_behind_an_ambient_var_reads_the_enclosing_binding() {
    let hir = lower(
        r#"
        const outerShared = 1;
        function ambient(): number {
            const read = () => outerShared;
            declare var outerShared: number;
            return read();
        }
        function control(): number {
            const read = () => realShared;
            var realShared = 2;
            return read();
        }
        "#,
    );
    let control = function_debug(&hir, "control");
    assert!(
        control.contains("PreallocateBoxes"),
        "control: a forward-captured real `var` is boxed at entry: {control}"
    );
    let ambient = function_debug(&hir, "ambient");
    assert!(
        !ambient.contains("PreallocateBoxes"),
        "the enclosing binding must not be re-boxed by the function: {ambient}"
    );
}

/// Annex B.3.3: a block-level function declaration also creates a function
/// `var`, unless a lexical declaration of that name forbids it. An ambient
/// `declare let` declares nothing, so it must not forbid it.
#[test]
fn ambient_let_does_not_block_the_annex_b_function_var() {
    let hir = lower(
        r#"
        function ambient(): string {
            declare let ambientFn: any;
            { function ambientFn() {} }
            return typeof ambientFn;
        }
        function control(): string {
            { function plainFn() {} }
            return typeof plainFn;
        }
        "#,
    );
    let control = function_debug(&hir, "control");
    assert!(
        !control.contains("String(\"plainFn\")"),
        "control: the Annex B `var` resolves `typeof plainFn` locally: {control}"
    );
    let ambient = function_debug(&hir, "ambient");
    assert!(
        !ambient.contains("String(\"ambientFn\")"),
        "`typeof ambientFn` must read the Annex B `var`, not the global: {ambient}"
    );
}

/// A block-scoped `let` is in its TDZ before its declarator, so `typeof`
/// throws. An ambient `declare let` has no TDZ: it is a global.
#[test]
fn typeof_before_an_ambient_let_does_not_throw() {
    let hir = lower(
        r#"
        function probe(): string {
            {
                const ambient = typeof dBlock;
                const real = typeof realBlock;
                declare let dBlock: number;
                let realBlock = 1;
                return ambient + real + realBlock;
            }
        }
        "#,
    );
    let probe = function_debug(&hir, "probe");
    assert!(
        probe.contains(&global_read("realBlock")),
        "control: `typeof` of a lexical binding in its TDZ throws: {probe}"
    );
    assert!(
        !probe.contains(&global_read("dBlock")),
        "`typeof` of an ambient name must not throw: {probe}"
    );
    assert!(
        probe.contains("js_global_get_optional") && probe.contains("String(\"dBlock\")"),
        "{probe}"
    );
}

/// `declare const __platform__: number` is Perry's compile-time platform
/// constant. The backends fold it from a `Stmt::Let` with NO initializer; since
/// #6871 materialized `undefined` for every uninitialized lexical binding, the
/// constant read `undefined` on every target.
#[test]
fn compile_time_constants_keep_their_uninitialized_binding() {
    let hir = lower(
        r#"
        declare const __platform__: number;
        declare const __plugins__: number;
        const realUninitialized = undefined;
        console.log(__platform__, __plugins__, realUninitialized);
        "#,
    );
    for name in ["__platform__", "__plugins__"] {
        let init = hir.init.iter().find_map(|stmt| match stmt {
            Stmt::Let { name: n, init, .. } if n == name => Some(init),
            _ => None,
        });
        assert!(
            matches!(init, Some(None)),
            "`{name}` must lower to `Let {{ init: None }}`, got {init:?}"
        );
    }
    let control = hir.init.iter().find_map(|stmt| match stmt {
        Stmt::Let { name, init, .. } if name == "realUninitialized" => Some(init),
        _ => None,
    });
    assert!(
        matches!(control, Some(Some(Expr::Undefined))),
        "control: other bindings keep their initializer: {control:?}"
    );
}
