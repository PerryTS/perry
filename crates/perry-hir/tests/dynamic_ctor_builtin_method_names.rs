//! #11128: a method call on an instance built by `new` from a class VALUE
//! (an `any` binding, a factory result, a destructured `require(...)`
//! namespace) must not be folded to an Array/collection fast path just
//! because the method's name collides with an Array builtin. The receiver is
//! not proven to be an array, so the call has to stay a generic method call
//! that reaches the user's method at runtime.

use perry_hir::lower_module;
use perry_parser::parse_typescript;

/// Every dense Array HIR fold a colliding user method name could be lowered
/// to. `NativeMethodCall { module: "array" }` is the no-argument `push` form.
const ARRAY_FOLDS: &[&str] = &[
    "ArrayPush",
    "ArrayPushSpread",
    "ArrayPop",
    "ArrayShift",
    "ArrayUnshift",
    "ArraySplice",
    "ArraySort",
    "ArrayReverseValue",
    "ArrayIndexOf",
    "ArrayIncludes",
    "ArraySlice",
    "ArrayJoin",
    "ArrayMap",
    "ArrayForEach",
    "ArrayAt",
    "ArrayLikeMethod",
    "module: \"array\"",
];

/// (method, argument list) pairs whose names collide with Array builtins.
const CALLS: &[(&str, &str)] = &[
    ("push", "1"),
    ("push", ""),
    ("pop", ""),
    ("shift", ""),
    ("unshift", "1"),
    ("splice", "1"),
    ("sort", ""),
    ("reverse", ""),
    ("concat", "1"),
    ("indexOf", "1"),
    ("includes", "1"),
    ("slice", "1"),
    ("join", "\"-\""),
    ("map", "(x: number) => x"),
    ("forEach", "(x: number) => x"),
    ("at", "0"),
];

fn lower(source: &str) -> String {
    let source = source.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let parsed = parse_typescript(&source, "dynamic_ctor.ts").expect("parse");
            format!(
                "{:?}",
                lower_module(&parsed, "test", "dynamic_ctor.ts").expect("lower")
            )
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

fn assert_no_array_fold(receiver_setup: &str, receiver: &str, what: &str) {
    for (method, args) in CALLS {
        let source = format!(
            "class R {{ n = 0; {method}(...a: any[]) {{ this.n += 1; return this; }} }}\n\
             {receiver_setup}\n\
             {receiver}.{method}({args});\n"
        );
        let hir = lower(&source);
        for fold in ARRAY_FOLDS {
            assert!(
                !hir.contains(fold),
                "{what}: `{receiver}.{method}({args})` lowered to {fold}\n{source}\n{hir}"
            );
        }
    }
}

#[test]
fn new_from_any_class_value_is_not_an_array() {
    assert_no_array_fold(
        "const C: any = R;\nconst r = new C();",
        "r",
        "any-typed class value",
    );
}

#[test]
fn new_from_any_class_value_with_type_args_is_not_an_array() {
    assert_no_array_fold(
        "const C: any = R;\nconst r = new C<number>();",
        "r",
        "any-typed class value with type args",
    );
}

#[test]
fn new_from_factory_class_value_is_not_an_array() {
    assert_no_array_fold(
        "function make(): any { return R; }\nconst K = make();\nconst r = new K();",
        "r",
        "factory-returned class value",
    );
}

#[test]
fn new_from_destructured_namespace_is_not_an_array() {
    // redis@6.1.0 linked-list.js: `const { SinglyLinkedList } = require(...)`.
    assert_no_array_fold(
        "const lib: any = { R };\nconst { R: SLL } = lib;\nconst r = new SLL();",
        "r",
        "destructured class value",
    );
}

#[test]
fn instance_type_param_is_not_an_array() {
    for (method, args) in CALLS {
        let source = format!(
            "class R {{ n = 0; {method}(...a: any[]) {{ this.n += 1; return this; }} }}\n\
             function use(x: InstanceType<typeof R>) {{ return x.{method}({args}); }}\n"
        );
        let hir = lower(&source);
        for fold in ARRAY_FOLDS {
            assert!(
                !hir.contains(fold),
                "InstanceType param: `x.{method}({args})` lowered to {fold}\n{hir}"
            );
        }
    }
}

/// Control: a statically proven array keeps its dense fast path, so the fix
/// does not tax real `push` sites.
#[test]
fn proven_array_keeps_the_push_fast_path() {
    let hir = lower("const a: number[] = [];\na.push(1);\na.pop();\n");
    assert!(hir.contains("ArrayPush"), "{hir}");
    assert!(hir.contains("ArrayPop"), "{hir}");
}

/// Control: `new` of a declared class still types the instance as that class
/// (the class-method path), and `new` of a builtin still yields the builtin.
#[test]
fn declared_class_and_builtin_constructors_keep_their_types() {
    let hir =
        lower("class R { n = 0; push(v: number) { return v; } }\nconst r = new R();\nr.push(1);\n");
    assert!(hir.contains("ty: Named(\"R\")"), "{hir}");
    assert!(!hir.contains("ArrayPush"), "{hir}");
    let hir = lower("const a = new Array();\na.push(1);\n");
    assert!(hir.contains("ArrayPush"), "{hir}");
}
