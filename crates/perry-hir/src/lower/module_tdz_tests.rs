use std::collections::{HashMap, HashSet};

use super::{apply, ModuleTdzFacts};
use crate::ir::*;
use crate::tdz_check;
use crate::types::{FuncId, LocalId, Type};

const X: LocalId = 7;
const Y: LocalId = 8;

fn let_stmt(id: LocalId, name: &str, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(init),
    }
}

fn function(id: FuncId, name: &str, body: Vec<Stmt>) -> Function {
    Function {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn closure(func_id: FuncId, body: Vec<Stmt>) -> Expr {
    Expr::Closure {
        func_id,
        params: Vec::new(),
        return_type: Type::Any,
        body,
        captures: Vec::new(),
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: true,
    }
}

fn call(callee: Expr) -> Expr {
    Expr::Call {
        callee: Box::new(callee),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

fn empty_class(name: &str) -> Class {
    Class {
        id: 1,
        name: name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: Vec::new(),
        constructor: None,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        computed_members: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    }
}

fn ret(expr: Expr) -> Stmt {
    Stmt::Return(Some(expr))
}

fn run(module: &mut Module, exports_may_run_early: bool) {
    let lexical: HashSet<LocalId> = [X, Y].into_iter().collect();
    let classes = HashMap::new();
    apply(
        module,
        &ModuleTdzFacts {
            lexical_ids: &lexical,
            class_positions: &classes,
            exports_may_run_early,
        },
    );
}

fn guarded(id: LocalId, name: &str, access: Expr) -> Expr {
    Expr::Sequence(vec![tdz_check::check(id, name), access])
}

fn dbg<T: std::fmt::Debug>(value: &T) -> String {
    format!("{value:?}")
}

#[test]
fn a_top_level_read_before_the_declarator_throws_and_one_after_does_not() {
    let mut module = Module::new("m.ts");
    module.init = vec![
        Stmt::Expr(Expr::LocalGet(X)),
        let_stmt(X, "x", Expr::Integer(1)),
        Stmt::Expr(Expr::LocalGet(X)),
    ];
    run(&mut module, false);
    assert_eq!(
        dbg(&module.init[0]),
        dbg(&Stmt::Expr(tdz_check::throw("x")))
    );
    assert_eq!(dbg(&module.init[2]), dbg(&Stmt::Expr(Expr::LocalGet(X))));
}

#[test]
fn a_read_in_the_binding_s_own_initializer_throws() {
    let mut module = Module::new("m.ts");
    module.init = vec![let_stmt(
        X,
        "x",
        Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(X)),
            property: "a".into(),
            byte_offset: 0,
        },
    )];
    run(&mut module, false);
    let Stmt::Let {
        init: Some(init), ..
    } = &module.init[0]
    else {
        panic!()
    };
    assert!(dbg(init).contains(tdz_check::TDZ_THROW), "{init:?}");
}

#[test]
fn a_top_level_write_before_the_declarator_evaluates_the_value_then_throws() {
    let mut module = Module::new("m.ts");
    module.init = vec![
        Stmt::Expr(Expr::LocalSet(X, Box::new(call(Expr::FuncRef(1))))),
        let_stmt(X, "x", Expr::Integer(1)),
    ];
    run(&mut module, false);
    assert_eq!(
        dbg(&module.init[0]),
        dbg(&Stmt::Expr(Expr::Sequence(vec![
            call(Expr::FuncRef(1)),
            tdz_check::throw("x")
        ])))
    );
}

#[test]
fn a_closure_created_before_the_declarator_is_checked_and_one_after_is_not() {
    let mut module = Module::new("m.ts");
    module.init = vec![
        Stmt::Expr(closure(10, vec![ret(Expr::LocalGet(X))])),
        let_stmt(X, "x", Expr::Integer(1)),
        Stmt::Expr(closure(11, vec![ret(Expr::LocalGet(X))])),
    ];
    run(&mut module, false);
    let early = dbg(&module.init[0]);
    assert!(early.contains(tdz_check::TDZ_CHECK), "{early}");
    assert!(!early.contains(tdz_check::TDZ_THROW), "{early}");
    assert!(!dbg(&module.init[2]).contains(tdz_check::TDZ_CHECK));
}

#[test]
fn a_hoisted_function_is_checked_only_when_it_can_run_before_the_declarator() {
    let mut module = Module::new("m.ts");
    module.functions = vec![
        function(1, "early", vec![ret(Expr::LocalGet(X))]),
        function(2, "late", vec![ret(Expr::LocalGet(X))]),
        function(3, "via", vec![ret(call(Expr::FuncRef(4)))]),
        function(4, "indirect", vec![ret(Expr::LocalGet(X))]),
    ];
    module.init = vec![
        Stmt::Expr(call(Expr::FuncRef(1))),
        Stmt::Expr(closure(10, vec![ret(call(Expr::FuncRef(3)))])),
        let_stmt(X, "x", Expr::Integer(1)),
        Stmt::Expr(call(Expr::FuncRef(2))),
    ];
    run(&mut module, false);
    let checked = |i: usize| dbg(&module.functions[i].body).contains(tdz_check::TDZ_CHECK);
    assert!(checked(0), "called before the declarator");
    assert!(!checked(1), "first reachable after the declarator");
    assert!(checked(3), "reachable through a closure created early");
}

/// Nothing refers to `f` and nothing exports it, so it can never run: no
/// check. The same body referred to before the declarator is checked, so
/// the absence of the check is the reach rule, not a missed read.
#[test]
fn an_unreferenced_function_is_never_checked() {
    for (referenced, expect) in [(false, false), (true, true)] {
        let mut module = Module::new("m.ts");
        module.functions = vec![function(1, "f", vec![ret(Expr::LocalGet(X))])];
        module.init = vec![let_stmt(X, "x", Expr::Integer(1))];
        if referenced {
            module.init.insert(0, Stmt::Expr(call(Expr::FuncRef(1))));
        }
        run(&mut module, true);
        assert_eq!(
            dbg(&module.functions[0].body).contains(tdz_check::TDZ_CHECK),
            expect
        );
    }
}

#[test]
fn an_exported_function_is_checked_only_when_an_importer_can_run_first() {
    for (early, expect) in [(true, true), (false, false)] {
        let mut module = Module::new("m.ts");
        let mut f = function(1, "f", vec![ret(Expr::LocalGet(X))]);
        f.is_exported = true;
        module.functions = vec![f];
        module.init = vec![
            let_stmt(X, "x", Expr::Integer(1)),
            Stmt::Expr(call(Expr::FuncRef(1))),
        ];
        run(&mut module, early);
        assert_eq!(
            dbg(&module.functions[0].body).contains(tdz_check::TDZ_CHECK),
            expect
        );
    }
}

#[test]
fn a_checked_member_read_keeps_its_shape_under_the_check() {
    let mut module = Module::new("m.ts");
    let read = Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(X)),
        property: "a".into(),
        byte_offset: 0,
    };
    module.functions = vec![function(1, "f", vec![ret(call(read.clone()))])];
    // Called before the declarator, so its read is checked.
    module.init = vec![
        Stmt::Expr(call(Expr::FuncRef(1))),
        let_stmt(X, "x", Expr::Integer(1)),
    ];
    run(&mut module, false);
    assert_eq!(
        dbg(&module.functions[0].body),
        dbg(&vec![ret(guarded(X, "x", call(read)))])
    );
}

#[test]
fn a_checked_write_evaluates_the_value_before_the_check() {
    let mut module = Module::new("m.ts");
    module.functions = vec![function(
        1,
        "f",
        vec![Stmt::Expr(Expr::LocalSet(Y, Box::new(Expr::Integer(2))))],
    )];
    // Called before the declarator, so its write is checked.
    module.init = vec![
        Stmt::Expr(call(Expr::FuncRef(1))),
        let_stmt(Y, "y", Expr::Integer(1)),
    ];
    run(&mut module, false);
    assert_eq!(
        dbg(&module.functions[0].body),
        dbg(&vec![Stmt::Expr(Expr::LocalSet(
            Y,
            Box::new(tdz_check::check_then(Y, "y", Expr::Integer(2)))
        ))])
    );
}

#[test]
fn a_class_defined_after_the_declarator_is_not_checked() {
    for (position, expect) in [(0usize, true), (1usize, false)] {
        let mut module = Module::new("m.ts");
        let mut class = empty_class("K");
        class.methods = vec![function(1, "m", vec![ret(Expr::LocalGet(X))])];
        module.classes = vec![class];
        module.init = vec![let_stmt(X, "x", Expr::Integer(1))];
        let lexical: HashSet<LocalId> = [X].into_iter().collect();
        let classes: HashMap<String, usize> = [("K".to_string(), position)].into_iter().collect();
        apply(
            &mut module,
            &ModuleTdzFacts {
                lexical_ids: &lexical,
                class_positions: &classes,
                exports_may_run_early: false,
            },
        );
        assert_eq!(
            dbg(&module.classes[0].methods[0].body).contains(tdz_check::TDZ_CHECK),
            expect,
            "class at {position}"
        );
    }
}

#[test]
fn a_var_binding_is_never_touched() {
    let mut module = Module::new("m.ts");
    const V: LocalId = 9;
    module.init = vec![
        Stmt::Expr(Expr::LocalGet(V)),
        let_stmt(V, "v", Expr::Integer(1)),
    ];
    run(&mut module, false);
    assert_eq!(dbg(&module.init[0]), dbg(&Stmt::Expr(Expr::LocalGet(V))));
}

fn import(source: &str, imported: &str, local: &str) -> Import {
    Import {
        source: source.to_string(),
        specifiers: vec![ImportSpecifier::Named {
            imported: imported.to_string(),
            local: local.to_string(),
        }],
        is_native: false,
        module_kind: ModuleKind::NativeCompiled,
        resolved_path: None,
        type_only: false,
        runtime_erased: false,
        is_dynamic: false,
        is_dynamic_target: false,
        is_deferred_require: false,
        is_adopted_require: false,
    }
}

fn export(local: &str) -> Export {
    Export::Named {
        local: local.to_string(),
        exported: local.to_string(),
    }
}

#[test]
fn an_exported_binding_is_named_after_its_declarator_when_an_importer_can_run_first() {
    let mut module = Module::new("a.ts");
    module.init = vec![let_stmt(X, "x", Expr::Integer(7))];
    module.exports = vec![export("x")];
    run(&mut module, true);
    assert_eq!(
        dbg(&module.init),
        dbg(&vec![
            let_stmt(X, "x", Expr::Integer(7)),
            Stmt::Expr(tdz_check::check(X, "x")),
        ])
    );
}

#[test]
fn a_commonjs_module_s_exports_are_never_seeded() {
    // The CJS-to-ESM wrap: `const _cjs = (function(){…})()` and
    // `export const y = _cjs.y`. Codegen drops `y`'s initializer and reads
    // `module.exports.y` live, so a check after it would see a sentinel that
    // nothing ever clears (#11987).
    let mut module = Module::new("utils.js");
    module.imports = vec![import(
        "node:module",
        "createRequire",
        "__perry_cjs_create_require",
    )];
    let wrap = vec![
        let_stmt(X, "_cjs", call(closure(1, Vec::new()))),
        let_stmt(
            Y,
            "y",
            Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(X)),
                property: "y".to_string(),
                byte_offset: 0,
            },
        ),
    ];
    module.init = wrap.clone();
    module.exports = vec![export("_cjs"), export("y")];
    run(&mut module, true);
    assert_eq!(dbg(&module.init), dbg(&wrap));
}
