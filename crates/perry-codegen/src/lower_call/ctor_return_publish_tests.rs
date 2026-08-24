//! #8648: what a standalone `<Class>_constructor` symbol RETURNS.
//!
//! An IR-census test, and the "assert the subject was live" kind (CLAUDE.md):
//! nothing behavioural can see this. Both spellings produce the same program
//! output, because every caller of the symbol maps an `undefined` return onto
//! its own receiver — which for an ordinary class IS the value the other
//! spelling returns. The difference is only ever visible in a profile.
//!
//! #8630 made every constructor symbol end in
//! `js_ctor_return_override(this, <slot>, …)` so that a derived `super()` whose
//! base hands back a replacement object could publish it. Correct, but it also
//! changed the ORDINARY constructor from `ret undefined` to `ret this`, and the
//! callers' own `is_undef` fast arm — the one `lower_call/new.rs` documents as
//! "the fast arm ran no constructor, which is `undefined` — the same thing an
//! ordinary ctor body returns" — went from never-taken to always-taken. The
//! call it guards is not cheap: `constructor_return_overrides_this` probes the
//! typed-array registry, the buffer registry, callability, the Proxy registry,
//! `arguments`, `clean_arr_ptr` (which walks GC forwarding chains) and finally
//! the GC header, per construction, to answer "yes, an object" and hand back
//! the value the caller already held. Measured on a two-class `new B(x, y)`
//! loop: 1.65x, and on `benchmarks/issue-8289/cycles.ts` 1.68x.
//!
//! So the positive test asserts the ordinary constructor is back to
//! `ret undefined` with no override call at all, and the negative asserts the
//! publish survives for the one shape that needs it — a constructor that can
//! hand back a replacement `this`.

use super::typed_shape_bake_tests::emit;
use perry_hir::types::Type;
use perry_hir::{Class, ClassField, Expr, Function, Module, Param, Stmt};

/// The NaN-boxed `undefined` literal codegen prints for `TAG_UNDEFINED`.
const RET_UNDEFINED: &str = "ret double 0x7FFC000000000001";
const OVERRIDE_CALL: &str = "@js_ctor_return_override(";

fn field(name: &str) -> ClassField {
    ClassField {
        name: name.to_string(),
        key_expr: None,
        ty: Type::Number,
        init: None,
        is_private: false,
        is_readonly: false,
        decorators: Vec::new(),
    }
}

fn param(id: u32, name: &str) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty: Type::Number,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

/// `constructor(a) { this.v = a; <tail> }`
fn ctor(tail: Vec<Stmt>) -> Function {
    let mut body = vec![Stmt::Expr(Expr::PutValueSet {
        target: Box::new(Expr::This),
        key: Box::new(Expr::String("v".to_string())),
        value: Box::new(Expr::LocalGet(1)),
        receiver: Box::new(Expr::This),
        strict: false,
    })];
    body.extend(tail);
    Function {
        id: 900,
        name: "constructor".to_string(),
        type_params: Vec::new(),
        params: vec![param(1, "a")],
        return_type: Type::Void,
        body,
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn class(id: u32, name: &str, extends_name: Option<&str>, ctor_tail: Vec<Stmt>) -> Class {
    Class {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: extends_name.map(str::to_string),
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![field("v")],
        constructor: Some(ctor(ctor_tail)),
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        computed_members: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    }
}

fn module(classes: Vec<Class>, construct: &str) -> Module {
    let mut m = Module::new("ctor_return_publish.ts");
    m.classes = classes;
    m.init = vec![Stmt::Let {
        id: 20,
        name: "x".to_string(),
        ty: Type::Named(construct.to_string()),
        mutable: false,
        init: Some(Expr::New {
            class_name: construct.to_string(),
            args: vec![Expr::Integer(1)],
            type_args: Vec::new(),
            byte_offset: 0,
            cap_args_appended: 0,
        }),
    }];
    m
}

/// The body of the `define` for `<name>_constructor`, `}`-terminated.
fn constructor_body(ir: &str, name: &str) -> String {
    let needle = format!("_{name}_constructor(");
    let define = ir
        .split("\ndefine ")
        .find(|chunk| {
            chunk
                .split('\n')
                .next()
                .is_some_and(|h| h.contains(&needle))
        })
        .unwrap_or_else(|| panic!("no `{name}_constructor` in the emitted IR:\n{ir}"));
    let end = define.find("\n}").expect("unterminated define");
    define[..end].to_string()
}

/// Every `ret` instruction in `body`, trimmed. The `ret` is NOT the last line
/// of the text: codegen emits blocks in creation order, so the constructor's
/// completion block precedes the field-store diamonds it was created before.
fn returns(body: &str) -> Vec<String> {
    body.lines()
        .map(str::trim)
        .filter(|line| line.starts_with("ret "))
        .map(str::to_string)
        .collect()
}

#[test]
fn an_ordinary_constructor_symbol_returns_undefined() {
    let ir = emit(&module(
        vec![class(404, "Plain", None, Vec::new())],
        "Plain",
    ));
    let body = constructor_body(&ir, "Plain");
    assert!(
        !body.contains(OVERRIDE_CALL),
        "a constructor that cannot hand back a replacement `this` still emits \
         the return-override publish, so its callers' own `is_undef` fast arm \
         is dead and every construction pays \
         `constructor_return_overrides_this`:\n{body}"
    );
    assert_eq!(
        returns(&body),
        vec![RET_UNDEFINED.to_string()],
        "the ordinary constructor symbol no longer returns `undefined`; every \
         caller maps that onto its own receiver, and returning `this` instead \
         is what #8648's second regression was:\n{body}"
    );
}

/// The same class with `extends`, whose parent also cannot replace `this`.
/// A derived constructor is not by itself a reason to publish.
#[test]
fn a_derived_constructor_with_an_ordinary_base_returns_undefined() {
    let ir = emit(&module(
        vec![
            class(404, "Base", None, Vec::new()),
            class(
                405,
                "Sub",
                Some("Base"),
                vec![Stmt::Expr(Expr::SuperCall(vec![Expr::LocalGet(1)]))],
            ),
        ],
        "Sub",
    ));
    let body = constructor_body(&ir, "Sub");
    // Only the RETURN is asserted here. `super()` inlines the parent body and
    // emits its own return-override diamond over the parent's completion slot;
    // with an ordinary parent that slot is a compile-time `undefined`, so LLVM
    // folds the diamond away and the call never reaches the binary. The publish
    // this ticket is about is the one over the SYMBOL's own result.
    assert_eq!(
        returns(&body),
        vec![RET_UNDEFINED.to_string()],
        "expected the only `ret` to be `undefined` for an ordinary derived \
         constructor:\n{body}"
    );
}

/// The shape #8630 added the publish for: a constructor whose `return` can
/// hand back a different object. The publish MUST survive — this is the
/// negative control that stops the elision from being unconditional.
#[test]
fn a_value_returning_constructor_still_publishes_its_this() {
    let ir = emit(&module(
        vec![class(
            404,
            "Swap",
            None,
            vec![Stmt::Return(Some(Expr::LocalGet(1)))],
        )],
        "Swap",
    ));
    let body = constructor_body(&ir, "Swap");
    assert!(
        body.contains(OVERRIDE_CALL),
        "a constructor with a value-bearing `return` dropped its \
         return-override publish, so `new Swap(...)` would keep the \
         provisional allocation instead of the returned object:\n{body}"
    );
}

/// And through an ancestor: the leaf's own body is ordinary, but its base can
/// replace `this`, so the leaf must still publish what `super()` bound.
#[test]
fn a_value_returning_base_makes_its_subclass_publish() {
    let ir = emit(&module(
        vec![
            class(
                404,
                "SwapBase",
                None,
                vec![Stmt::Return(Some(Expr::LocalGet(1)))],
            ),
            class(
                405,
                "Leaf",
                Some("SwapBase"),
                vec![Stmt::Expr(Expr::SuperCall(vec![Expr::LocalGet(1)]))],
            ),
        ],
        "Leaf",
    ));
    let body = constructor_body(&ir, "Leaf");
    assert!(
        body.contains(OVERRIDE_CALL),
        "the leaf stopped publishing although its base returns a value — the \
         replacement `this` bound by `super()` would never reach the \
         caller:\n{body}"
    );
}
