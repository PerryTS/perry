//! Proof installation must dominate call lowering; special owners use runtime reads.
use crate::{compile_module, CompileOptions};
use perry_hir::{types::Type, CompareOp, Expr, Function, Module, Param, Stmt, UpdateOp};

fn param(id: u32, ty: Type) -> Param {
    Param {
        id,
        name: format!("p{id}"),
        ty,
        default: None,
        decorators: vec![],
        is_rest: false,
        arguments_object: None,
    }
}

fn function(body: Vec<Stmt>, params: Vec<Param>) -> Function {
    Function {
        id: 1,
        name: "witness".into(),
        type_params: vec![],
        params,
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn ir(module: Module) -> String {
    String::from_utf8(
        compile_module(
            &module,
            CompileOptions {
                emit_ir_only: true,
                ..Default::default()
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn call_callback() -> Stmt {
    Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::LocalGet(2)),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
    })
}

fn loop_body(body: Vec<Stmt>) -> Stmt {
    Stmt::For {
        init: None,
        condition: Some(Expr::Compare {
            op: CompareOp::Lt,
            left: Box::new(Expr::LocalGet(3)),
            right: Box::new(Expr::Integer(4)),
        }),
        update: Some(Expr::Update {
            id: 3,
            op: UpdateOp::Increment,
            prefix: false,
        }),
        body,
    }
}

#[test]
fn unregistered_writes_and_intrinsics_resolve_per_access() {
    crate::temp_root_coverage::under_both_lowerings(|mode| {
        for intrinsic in [false, true] {
            for local in [false, true] {
                let access = if intrinsic {
                    Expr::Call {
                        callee: Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(1)),
                            property: "readUInt32LE".into(),
                            byte_offset: 0,
                        }),
                        args: vec![Expr::Integer(0)],
                        type_args: vec![],
                        byte_offset: 0,
                    }
                } else {
                    Expr::Uint8ArraySet {
                        array: Box::new(Expr::LocalGet(1)),
                        index: Box::new(Expr::Integer(0)),
                        value: Box::new(Expr::Integer(19)),
                    }
                };
                let class = if intrinsic { "Buffer" } else { "Uint8Array" };
                let mut body = vec![];
                if local {
                    body.push(Stmt::Let {
                        id: 1,
                        name: "view".into(),
                        ty: Type::Named(class.into()),
                        mutable: false,
                        init: Some(if intrinsic {
                            Expr::BufferFromArrayBuffer {
                                data: Box::new(Expr::LocalGet(4)),
                                byte_offset: Box::new(Expr::Integer(16)),
                                length: None,
                            }
                        } else {
                            Expr::TypedArrayNew {
                                kind: perry_hir::TYPED_ARRAY_KIND_UINT8,
                                arg: Some(Box::new(Expr::LocalGet(4))),
                            }
                        }),
                    });
                }
                body.extend([call_callback(), Stmt::Expr(access)]);
                let mut params = vec![param(2, Type::Any), param(3, Type::Number)];
                params.push(if local {
                    param(4, Type::Named("ArrayBuffer".into()))
                } else {
                    param(1, Type::Named(class.into()))
                });
                let mut module = Module::new("unregistered.ts");
                module.functions.push(function(body, params));
                let text = ir(module);
                let arm = if intrinsic {
                    text.contains("bytes.numeric.load")
                        || (local
                            && (text.contains("call double @js_method_site_miss")
                                || text.contains("load i32, ptr ")))
                } else {
                    text.contains("u8c.set.store")
                };
                assert!(
                    arm,
                    "{mode}/{intrinsic}/{local}: access arm must be exercised: {text}"
                );
                if intrinsic && !local {
                    // The existing Buffer parameter pre-pass registers before
                    // calls even without indexed reads. Preserve that safe proof.
                    let marker = text
                        .lines()
                        .find(|l| l.contains("; bytes.hoist.roots "))
                        .unwrap();
                    let state = marker
                        .split("state=")
                        .nth(1)
                        .unwrap()
                        .split_whitespace()
                        .next()
                        .unwrap();
                    let dirty = format!("store i8 0, ptr %{state}");
                    let lines: Vec<_> = text.lines().map(str::trim).collect();
                    assert!(
                        lines.iter().enumerate().any(|(i, line)| {
                            (line.contains("call double %") || line.contains("call double @js_closure_call0"))
                                && lines[..i].iter().rev()
                                    .take_while(|l| l.starts_with("store i8 0, ptr "))
                                    .any(|l| *l == dirty)
                        }),
                        "pre-registered Buffer proof must be dirtied before callback"
                    );
                } else {
                    assert!(
                        !text.contains("; bytes.hoist.roots "),
                        "{mode}/{intrinsic}/{local}: unregistered accesses must resolve per access"
                    );
                    assert!(!text.contains("bytes.access.ready"));
                }
            }
        }
    });
}


#[test]
fn loop_writes_register_before_local_initializers_and_callbacks() {
    crate::temp_root_coverage::under_both_lowerings(|mode| {
        for local in [false, true] {
            let write = Stmt::Expr(Expr::Uint8ArraySet {
                array: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::Integer(0)),
                value: Box::new(Expr::Integer(19)),
            });
            let mut loop_stmts = vec![];
            if local {
                loop_stmts.push(Stmt::Let {
                    id: 1, name: "view".into(), ty: Type::Named("Uint8Array".into()),
                    mutable: false,
                    init: Some(Expr::Call {
                        callee: Box::new(Expr::LocalGet(2)),
                        args: vec![], type_args: vec![], byte_offset: 0,
                    }),
                });
            }
            loop_stmts.extend([call_callback(), write]);
            let mut params = vec![param(2, Type::Any), param(3, Type::Number)];
            if !local { params.push(param(1, Type::Named("Uint8Array".into()))); }
            let mut module = Module::new("registered_writes.ts");
            module.functions.push(function(vec![loop_body(loop_stmts)], params));
            let text = ir(module);
            let marker = text.lines().find(|l| l.contains("; bytes.hoist.roots "))
                .unwrap_or_else(|| panic!("{mode}/{local}: loop writes must register in the pre-pass"));
            let state = marker.split("state=").nth(1).unwrap()
                .split_whitespace().next().unwrap();
            let dirty = format!("store i8 0, ptr %{state}");
            let lines: Vec<_> = text.lines().map(str::trim).collect();
            let callbacks: Vec<_> = lines.iter().enumerate()
                .filter(|(_, l)| l.contains("call double %") || l.contains("call double @js_closure_call0")).collect();
            assert!(!callbacks.is_empty(), "callback lowering must be exercised");
            for (i, _) in callbacks {
                assert!(lines[..i].iter().rev()
                    .take_while(|l| l.starts_with("store i8 0, ptr "))
                    .any(|l| *l == dirty),
                    "{mode}/{local}: proof must be registered before every callback, including the initializer");
            }
            assert!(text.contains("bytes.access.ready"));
        }
    });
}

// Follow the owner-brand rejection into the admission branch. Checking only
// the comparisons would stay green if the result stopped guarding the load.
fn assert_special_owner_misses(text: &str) {
    let lines: Vec<_> = text.lines().map(str::trim).collect();
    let result = |line: &str| line.split(" = ").next().unwrap().to_string();
    let shared = lines
        .iter()
        .find(|line| line.contains(" = icmp eq i64 ") && line.ends_with(", 15"))
        .expect("shared owner test");
    let arena = lines
        .iter()
        .find(|line| line.contains(" = icmp eq i64 ") && line.ends_with(", 18"))
        .expect("native owner test");
    assert_eq!(
        shared
            .split("icmp eq i64 ")
            .nth(1)
            .unwrap()
            .split(',')
            .next(),
        arena
            .split("icmp eq i64 ")
            .nth(1)
            .unwrap()
            .split(',')
            .next()
    );
    let special = lines
        .iter()
        .find(|line| line.ends_with(&format!("or i1 {}, {}", result(shared), result(arena))))
        .expect("special owner union");
    let regular = lines
        .iter()
        .find(|line| line.ends_with(&format!("icmp eq i1 {}, false", result(special))))
        .expect("regular owner test");
    let admitted = lines
        .iter()
        .find(|line| {
            line.contains(" = and i1 ") && line.ends_with(&format!(", {}", result(regular)))
        })
        .expect("special owners must guard admission");
    assert!(
        lines.iter().any(
            |line| line.starts_with(&format!("br i1 {},", result(admitted)))
                && line.contains("label %ta.read.slow")
        ),
        "special owner must reach runtime arm: {text}"
    );
    assert!(text.contains("call double @js_dyn_index_get"));
}

#[test]
fn shared_and_native_byte_views_take_the_runtime_arm() {
    crate::temp_root_coverage::under_both_lowerings(|_| {
        for storage in ["sab-u8", "sab-buffer", "native"] {
            let native = storage == "native";
            let buffer = storage == "sab-buffer";
            let mut module = Module::new("special_owner.ts");
            let owner = if native {
                Expr::NativeArenaAlloc(Box::new(Expr::Integer(64)))
            } else {
                Expr::New {
                    class_name: "SharedArrayBuffer".into(),
                    args: vec![Expr::Integer(64)],
                    type_args: vec![],
                    byte_offset: 0,
                    cap_args_appended: 0,
                }
            };
            let view = if native {
                Expr::NativeArenaView {
                    owner: Box::new(Expr::LocalGet(4)),
                    kind: perry_hir::TYPED_ARRAY_KIND_UINT8,
                    byte_offset: Box::new(Expr::Integer(16)),
                    length: Box::new(Expr::Integer(16)),
                }
            } else if buffer {
                Expr::BufferFromArrayBuffer {
                    data: Box::new(Expr::LocalGet(4)),
                    byte_offset: Box::new(Expr::Integer(16)),
                    length: None,
                }
            } else {
                Expr::TypedArrayNew {
                    kind: perry_hir::TYPED_ARRAY_KIND_UINT8,
                    arg: Some(Box::new(Expr::LocalGet(4))),
                }
            };
            module.functions.push(function(
                vec![
                    Stmt::Let {
                        id: 4,
                        name: "owner".into(),
                        ty: Type::Any,
                        mutable: false,
                        init: Some(owner),
                    },
                    Stmt::Let {
                        id: 1,
                        name: "view".into(),
                        ty: Type::Named(if buffer { "Buffer" } else { "Uint8Array" }.into()),
                        mutable: false,
                        init: Some(view),
                    },
                    Stmt::Return(Some(Expr::Call {
                        callee: Box::new(Expr::FuncRef(2)),
                        args: vec![Expr::LocalGet(1), Expr::LocalGet(3)],
                        type_args: vec![],
                        byte_offset: 0,
                    })),
                ],
                vec![param(3, Type::Any)],
            ));
            // Pass the constructed view to an ordinary byte reader. It has no
            // tracked construction descriptor and exercises the plain resolver.
            let mut reader = function(
                vec![Stmt::Return(Some(Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(Expr::LocalGet(3)),
                }))],
                vec![
                    param(
                        1,
                        Type::Named(if buffer { "Buffer" } else { "Uint8Array" }.into()),
                    ),
                    param(3, Type::Any),
                ],
            );
            reader.id = 2;
            reader.name = "read_special".into();
            module.functions.push(reader);
            let text = ir(module);
            assert!(
                text.lines()
                    .any(|line| !line.trim_start().starts_with("declare")
                        && line.contains(if native {
                            "@js_native_arena_view"
                        } else {
                            "@js_shared_array_buffer_new"
                        })),
                "actual {storage} construction must be exercised: {text}"
            );
            assert_special_owner_misses(&text);
        }
    });
}

#[test]
fn reassigned_module_table_cannot_get_a_fixed_receiver() {
    let mut module = Module::new("reassigned.ts");
    module.init.push(Stmt::Let {
        id: 1,
        name: "table".into(),
        ty: Type::Named("Uint8Array".into()),
        mutable: true,
        init: Some(Expr::Uint8ArrayNew(Some(Box::new(Expr::Integer(8))))),
    });
    module.functions.push(function(
        vec![loop_body(vec![
            call_callback(),
            Stmt::Expr(Expr::IndexGet {
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::LocalGet(3)),
            }),
        ])],
        vec![param(2, Type::Any), param(3, Type::Number)],
    ));
    let mut replace = function(
        vec![Stmt::Expr(Expr::LocalSet(
            1,
            Box::new(Expr::Uint8ArrayNew(Some(Box::new(Expr::Integer(16))))),
        ))],
        vec![],
    );
    replace.id = 2;
    replace.name = "replace".into();
    module.functions.push(replace);
    let text = ir(module);
    assert!(
        !text.contains("; bytes.hoist.roots "),
        "reassigned table must re-resolve"
    );
}

#[test]
fn lazy_shared_and_reassigned_negative_controls_turn_red() {
    for (sabotage, witness, message) in [
        (
            "lazy_install",
            "unregistered_writes_and_intrinsics_resolve_per_access",
            "unregistered accesses",
        ),
        (
            "shared_admit",
            "shared_and_native_byte_views_take_the_runtime_arm",
            "special owners must guard admission",
        ),
        (
            "reassigned_admit",
            "reassigned_module_table_cannot_get_a_fixed_receiver",
            "reassigned table must re-resolve",
        ),
    ] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &format!("expr::byte_prepass_tests::{witness}"),
                "--nocapture",
            ])
            .env("PERRY_B4_SABOTAGE", sabotage)
            .output()
            .unwrap();
        let output = format!(
            "{}{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        assert!(output.contains("running 1 test"));
        assert!(
            !child.status.success() && output.contains(message),
            "{sabotage}: wrong failure: {output}"
        );
    }
}
