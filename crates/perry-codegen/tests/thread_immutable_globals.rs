//! IR contract of the immutable module-global transfer (perry/thread).
//!
//! An eligible binding (one initializer, never reassigned, an initializer that
//! can hold a String/BigInt) gets a publication cell, a thread-local cache, a
//! producer accessor, a publish call after its initializer, and every read in
//! generated code goes through the cache. The caches of a module are the
//! entries of ONE thread-local block per module, reached through one address
//! per function computed in its entry block. Nothing changes for thread-free
//! programs, structurally non-leaf initializers, scalars, or reassigned
//! bindings.

use perry_codegen::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, Export, Expr, Function, Module, Stmt};

const PREFIX: &str = "producer_ts";

fn reader(id: u32, name: &str, local: u32) -> Function {
    Function {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(Expr::LocalGet(local)))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn let_stmt(id: u32, name: &str, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(init),
    }
}

/// `producer.ts` with one module binding per case, each read by an exported
/// function; `shared` is also exported as a value.
fn producer(reassign: bool) -> Module {
    let mut module = Module::new("producer.ts");
    let computed = Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(Expr::String("computed-leaf-".into())),
        right: Box::new(Expr::Number(1.0)),
    };
    module.init.push(let_stmt(1, "shared", computed));
    module.init.push(let_stmt(
        2,
        "big",
        Expr::BigInt("12345678901234567890".into()),
    ));
    module.init.push(let_stmt(3, "count", Expr::Number(7.0)));
    module.init.push(let_stmt(
        4,
        "config",
        Expr::Object(vec![("a".into(), Expr::Number(1.0))]),
    ));
    module
        .init
        .push(let_stmt(5, "mutable", Expr::String("first".into())));
    module.functions.push(reader(10, "readShared", 1));
    module.functions.push(reader(11, "readBig", 2));
    module.functions.push(reader(12, "readCount", 3));
    module.functions.push(reader(13, "readConfig", 4));
    module.functions.push(reader(14, "readMutable", 5));
    // Reads two bindings, both inside a loop.
    let mut looped = reader(16, "readLoop", 1);
    looped.body = vec![Stmt::While {
        condition: Expr::LocalGet(1),
        body: vec![
            Stmt::Expr(Expr::LocalGet(2)),
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
    }];
    module.functions.push(looped);
    if reassign {
        let mut writer = reader(15, "writeMutable", 5);
        writer.body = vec![Stmt::Expr(Expr::LocalSet(
            5,
            Box::new(Expr::String("second".into())),
        ))];
        module.functions.push(writer);
    }
    module.exported_objects.push("shared".into());
    module.exports.push(Export::Named {
        local: "shared".into(),
        exported: "shared".into(),
    });
    module
}

fn ir(module: &Module, thread_graph: bool) -> String {
    let opts = CompileOptions {
        emit_ir_only: true,
        is_entry_module: false,
        thread_literal_module_prefixes: if thread_graph {
            vec!["main_ts".into(), PREFIX.into()]
        } else {
            Vec::new()
        },
        ..Default::default()
    };
    String::from_utf8(compile_module(module, opts).expect("producer compiles")).unwrap()
}

fn function_body<'a>(ir: &'a str, symbol: &str) -> &'a str {
    let anchor = format!("define double @{symbol}(");
    ir.split(anchor.as_str())
        .nth(1)
        .unwrap_or_else(|| panic!("{symbol} must be defined"))
        .split("\n}")
        .next()
        .unwrap()
}

fn has_transfer(ir: &str, id: u32) -> bool {
    ir.contains(&format!("define double @perry_gread_{PREFIX}__{id}("))
}

const TLS_ADDRESS: &str = "call ptr @llvm.threadlocal.address.p0(ptr @perry_gtc_producer_ts)";

/// The part of a function body before its second label: the entry block.
fn entry_block(body: &str) -> &str {
    let mut seen = 0;
    let mut end = body.len();
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        if !line.starts_with(' ') && line.trim_end().ends_with(':') {
            seen += 1;
            if seen == 2 {
                end = offset;
                break;
            }
        }
        offset += line.len();
    }
    &body[..end]
}

/// One TLS address per function, in its entry block; no other reference to
/// the block in the body.
fn assert_one_entry_tls_address(body: &str) {
    assert_eq!(body.matches(TLS_ADDRESS).count(), 1, "{body}");
    assert!(entry_block(body).contains(TLS_ADDRESS), "{body}");
    assert_eq!(body.matches("@perry_gtc_").count(), 1, "{body}");
}

#[test]
fn eligible_leaves_publish_once_and_read_through_the_agent_cache() {
    let ir = ir(&producer(true), true);
    assert!(ir.contains(&format!(
        "@perry_gtc_{PREFIX} = internal thread_local global [2 x {{ double, i64 }}] \
         [{{ double, i64 }} {{ double 0x7FFC000000000001, i64 0 }}, \
         {{ double, i64 }} {{ double 0x7FFC000000000001, i64 0 }}]"
    )));
    assert!(ir.contains("declare ptr @llvm.threadlocal.address.p0(ptr)"));
    for id in [1, 2] {
        assert!(ir.contains(&format!(
            "@perry_gpub_{PREFIX}__{id} = internal global i64 0"
        )));
        assert!(ir.contains(&format!("define double @perry_gread_{PREFIX}__{id}(")));
        assert_one_entry_tls_address(function_body(&ir, &format!("perry_gread_{PREFIX}__{id}")));
    }
    assert_eq!(
        ir.matches("call void @js_thread_global_publish(").count(),
        2
    );

    for (symbol, id, slot) in [("readShared", 1, 0), ("readBig", 2, 1)] {
        let body = function_body(&ir, &format!("perry_fn_{PREFIX}__{symbol}"));
        assert_one_entry_tls_address(body);
        let entry_reg = body
            .lines()
            .find(|line| {
                line.contains(&format!("getelementptr inbounds {{ double, i64 }}, ptr %r"))
                    && line.ends_with(&format!(", i64 {slot}"))
            })
            .and_then(|line| line.trim().split(' ').next())
            .unwrap_or_else(|| panic!("entry {slot} of the block: {body}"));
        assert!(body.contains(&format!(
            "getelementptr inbounds {{ double, i64 }}, ptr {entry_reg}, i32 0, i32 1"
        )));
        assert!(body.contains(&format!("load double, ptr {entry_reg}")));
        assert!(body.contains("call double @js_thread_global_materialize("));
        // The canonical slot is read only behind the published-nonleaf state.
        assert_eq!(
            body.matches(&format!("load double, ptr @perry_global_{PREFIX}__{id}"))
                .count(),
            1
        );
    }

    // A function reading two of the module's bindings inside a loop computes
    // the block address once, in its entry block.
    let looped = function_body(&ir, &format!("perry_fn_{PREFIX}__readLoop"));
    assert_one_entry_tls_address(looped);
    assert!(
        looped
            .matches("call double @js_thread_global_materialize(")
            .count()
            >= 2
    );

    // The exported value getter goes through the producer accessor.
    let getter = function_body(&ir, &format!("perry_fn_{PREFIX}__shared"));
    assert!(getter.contains(&format!("call double @perry_gread_{PREFIX}__1(")));
    assert!(!getter.contains(&format!("@perry_global_{PREFIX}__1")));
}

#[test]
fn scalars_objects_and_reassigned_bindings_keep_the_canonical_route() {
    let ir = ir(&producer(true), true);
    for (symbol, id) in [("readCount", 3), ("readConfig", 4), ("readMutable", 5)] {
        assert!(
            !has_transfer(&ir, id),
            "binding {id} must not get a transfer"
        );
        let body = function_body(&ir, &format!("perry_fn_{PREFIX}__{symbol}"));
        assert!(!body.contains("js_thread_global_materialize"));
    }
    // Without the write, the same string binding is eligible.
    assert!(has_transfer(&self::ir(&producer(false), true), 5));
}

#[test]
fn thread_free_programs_emit_no_transfer_machinery() {
    let ir = ir(&producer(false), false);
    for needle in [
        "perry_gtc_",
        "perry_gpub_",
        "perry_gread_",
        "js_thread_global_publish(",
        "js_thread_global_materialize(",
    ] {
        let uses = ir
            .lines()
            .filter(|line| line.contains(needle) && !line.starts_with("declare "))
            .count();
        assert_eq!(uses, 0, "thread-free IR mentions {needle}");
    }
    let body = function_body(&ir, &format!("perry_fn_{PREFIX}__readShared"));
    assert!(body.contains(&format!("load double, ptr @perry_global_{PREFIX}__1")));
}
