//! #10463: the lowerings that used to place an argument buffer or an
//! out-parameter `alloca` in whatever block was current, each compiled inside
//! a counted loop.
//!
//! `LlFunction::for_each_final_item` now refuses any `alloca` outside the entry
//! block (`function/entry_allocas.rs`), so a regression at one of these sites
//! fails the compile below with that refusal. The IR is also read back here
//! with a scanner of its own, so the corpus does not rest on the refusal it is
//! meant to back up. Each case asserts its subject is live first: the runtime
//! entry the construct lowers to has to be called from a block after the entry
//! block, or the fixture never reached the site and a clean verdict would be
//! vacuous.

use crate::compile_module;
use perry_hir::types::Type;
use perry_hir::{CompareOp, Expr, Function, Module, Param, Stmt, UpdateOp};

const N: u32 = 1;
const DATE: u32 = 2;
const ARR: u32 = 3;
const ARRAY_LIKE: u32 = 4;
const I: u32 = 10;

fn param(id: u32, name: &str, ty: Type) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn get(id: u32) -> Box<Expr> {
    Box::new(Expr::LocalGet(id))
}

fn num(v: f64) -> Box<Expr> {
    Box::new(Expr::Number(v))
}

/// `function probe(n, date, arr, arrayLike) { for (let i = 0; i < n; i++) { body } }`
fn looped(body: Vec<Stmt>) -> Module {
    let mut module = Module::new("entry_block_alloca.ts");
    module.functions = vec![Function {
        id: 90,
        name: "probe".to_string(),
        type_params: Vec::new(),
        params: vec![
            param(N, "n", Type::Number),
            param(DATE, "date", Type::Any),
            param(ARR, "arr", Type::Array(Box::new(Type::Number))),
            param(ARRAY_LIKE, "arrayLike", Type::Any),
        ],
        return_type: Type::Void,
        body: vec![Stmt::For {
            init: Some(Box::new(Stmt::Let {
                id: I,
                name: "i".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Number(0.0)),
            })),
            condition: Some(Expr::Compare {
                op: CompareOp::Lt,
                left: get(I),
                right: get(N),
            }),
            update: Some(Expr::Update {
                id: I,
                op: UpdateOp::Increment,
                prefix: false,
            }),
            body,
        }],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }];
    module
}

fn method_call(receiver: u32, method: &str, args: Vec<Expr>) -> Stmt {
    Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: get(receiver),
            property: method.to_string(),
            byte_offset: 0,
        }),
        args,
        type_args: Vec::new(),
        byte_offset: 0,
    })
}

/// Every `alloca` outside its function's entry block, as `fn: line`. Labels are
/// flush-left `name:` lines; the first one after a `define` opens the entry
/// block and the next one closes it.
fn non_entry_allocas(ir: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut function = None;
    let mut labels = 0usize;
    for line in ir.lines() {
        if line.starts_with("define ") {
            function = Some(line.to_string());
            labels = 0;
        } else if line.starts_with('}') {
            function = None;
        } else if let Some(define) = &function {
            if line.ends_with(':') && !line.starts_with(|c: char| c.is_whitespace() || c == ';') {
                labels += 1;
            } else if labels > 1 && line.contains(" = alloca ") {
                found.push(format!("{define}: {}", line.trim()));
            }
        }
    }
    found
}

/// The number of calls to `@callee` in blocks after an entry block.
fn calls_outside_entry(ir: &str, callee: &str) -> usize {
    let needle = format!("@{callee}(");
    let mut in_function = false;
    let mut labels = 0usize;
    let mut count = 0;
    for line in ir.lines() {
        if line.starts_with("define ") {
            in_function = true;
            labels = 0;
        } else if line.starts_with('}') {
            in_function = false;
        } else if in_function {
            if line.ends_with(':') && !line.starts_with(|c: char| c.is_whitespace() || c == ';') {
                labels += 1;
            } else if labels > 1 && line.contains(&needle) {
                count += 1;
            }
        }
    }
    count
}

fn assert_entry_block_allocas_only(case: &str, callees: &[&str], body: Vec<Stmt>) {
    let bytes = compile_module(&looped(body), crate::temp_root_coverage::entry_opts())
        .unwrap_or_else(|e| panic!("{case}: codegen failed: {e}"));
    let ir = String::from_utf8(bytes).expect("LLVM IR should be UTF-8");
    for callee in callees {
        assert!(
            calls_outside_entry(&ir, callee) > 0,
            "{case}: the fixture must lower to `@{callee}` inside the loop, or this \
             case checks nothing:\n{ir}"
        );
    }
    let stray = non_entry_allocas(&ir);
    assert!(
        stray.is_empty(),
        "{case}: every alloca must be in its function's entry block (#10463); \
         found outside it:\n{}\n\n{ir}",
        stray.join("\n")
    );
}

#[test]
fn date_setters_keep_their_argument_buffer_in_the_entry_block() {
    assert_entry_block_allocas_only(
        "Date.prototype.set*",
        &["js_date_apply_setter"],
        vec![
            Stmt::Expr(Expr::DateSetTime {
                date: get(DATE),
                args: vec![Expr::LocalGet(I)],
            }),
            Stmt::Expr(Expr::DateSetUtcHours {
                date: get(DATE),
                args: vec![
                    Expr::LocalGet(I),
                    Expr::Number(1.0),
                    Expr::Number(2.0),
                    Expr::Number(3.0),
                ],
            }),
        ],
    );
}

#[test]
fn date_utc_keeps_its_argument_buffer_in_the_entry_block() {
    assert_entry_block_allocas_only(
        "Date.UTC",
        &["js_date_utc"],
        vec![Stmt::Expr(Expr::DateUtc(vec![
            Expr::Number(2000.0),
            Expr::LocalGet(I),
        ]))],
    );
}

#[test]
fn to_spliced_keeps_its_item_buffer_in_the_entry_block() {
    assert_entry_block_allocas_only(
        "Array.prototype.toSpliced",
        &["js_array_to_spliced"],
        vec![Stmt::Expr(Expr::ArrayToSpliced {
            array: get(ARR),
            start: num(1.0),
            delete_count: num(1.0),
            items: vec![Expr::LocalGet(I)],
        })],
    );
}

/// `Expr::ArraySplice`: the `i64` out-parameter AND the item buffer, with and
/// without items.
#[test]
fn local_splice_keeps_its_out_slot_and_item_buffer_in_the_entry_block() {
    assert_entry_block_allocas_only(
        "Expr::ArraySplice",
        &["js_array_splice"],
        vec![
            Stmt::Expr(Expr::ArraySplice {
                array_id: ARR,
                start: num(1.0),
                delete_count: Some(num(1.0)),
                items: vec![Expr::LocalGet(I)],
            }),
            Stmt::Expr(Expr::ArraySplice {
                array_id: ARR,
                start: num(1.0),
                delete_count: Some(num(0.0)),
                items: Vec::new(),
            }),
        ],
    );
}

/// The generic array-method lowering (`lower_array_method`): `concat`,
/// `unshift`, and `splice` with its out-parameter.
#[test]
fn array_methods_keep_their_buffers_in_the_entry_block() {
    assert_entry_block_allocas_only(
        "arr.concat / arr.unshift / arr.splice",
        &[
            "js_array_concat_variadic",
            "js_array_unshift_variadic",
            "js_array_splice",
        ],
        vec![
            method_call(ARR, "concat", vec![Expr::LocalGet(I)]),
            method_call(ARR, "unshift", vec![Expr::LocalGet(I)]),
            method_call(
                ARR,
                "splice",
                vec![Expr::Number(0.0), Expr::Number(1.0), Expr::LocalGet(I)],
            ),
        ],
    );
}

/// `Array.prototype.{push,unshift,splice,concat}.call(arrayLike, …)`.
#[test]
fn array_like_methods_keep_their_argument_buffer_in_the_entry_block() {
    let call = |method: &str, args: Vec<Expr>| {
        Stmt::Expr(Expr::ArrayLikeMethod {
            method: method.to_string(),
            receiver: get(ARRAY_LIKE),
            args,
        })
    };
    assert_entry_block_allocas_only(
        "Array.prototype.*.call",
        &[
            "js_arraylike_push",
            "js_arraylike_unshift",
            "js_arraylike_splice",
            "js_arraylike_concat",
        ],
        vec![
            call("push", vec![Expr::LocalGet(I)]),
            call("unshift", vec![Expr::LocalGet(I)]),
            call("splice", vec![Expr::Number(0.0), Expr::Number(2.0)]),
            call("concat", vec![Expr::LocalGet(I)]),
        ],
    );
}

/// The scanner itself: it must report the pre-fix shape.
#[test]
fn the_scanner_reports_an_alloca_in_a_loop_body() {
    let ir = "define double @f() {\nentry.0:\n  %a = alloca double\n  br label %for.body.1\n\
              \nfor.body.1:\n  %b = alloca [1 x double]\n  call double @g(ptr %b)\n}\n";
    assert_eq!(
        non_entry_allocas(ir),
        vec!["define double @f() {: %b = alloca [1 x double]".to_string()]
    );
    assert_eq!(calls_outside_entry(ir, "g"), 1);
}
