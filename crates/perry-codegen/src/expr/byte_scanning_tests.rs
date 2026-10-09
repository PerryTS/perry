//! Byte reads share typed-element lowering, including unproven keys.
use crate::{compile_module, CompileOptions};
use perry_hir::{types::Type, Expr, Function, Module, Param, Stmt};

fn read_ir(class: &str, dedicated: bool, global: bool) -> String {
    read_ir_shape(class, dedicated, global, Expr::LocalGet(2), false)
}

fn read_ir_shape(class: &str, dedicated: bool, global: bool, key: Expr, addition: bool) -> String {
    let mut module = Module::new("byte_scan.ts");
    let param = |id, ty| Param {
        id,
        name: format!("p{id}"),
        ty,
        default: None,
        decorators: vec![],
        is_rest: false,
        arguments_object: None,
    };
    let object = Box::new(Expr::LocalGet(1));
    let index = Box::new(key);
    let read = if dedicated {
        Expr::Uint8ArrayGet {
            array: object,
            index,
        }
    } else {
        Expr::IndexGet { object, index }
    };
    let read = if addition {
        Expr::Binary {
            op: perry_hir::BinaryOp::Add,
            left: Box::new(read),
            right: Box::new(Expr::Integer(1)),
        }
    } else {
        read
    };
    if global {
        module.init.push(Stmt::Let {
            id: 1,
            name: "table".into(),
            ty: Type::Named(class.into()),
            mutable: false,
            init: Some(Expr::Uint8ArrayNew(Some(Box::new(Expr::Integer(256))))),
        });
    }
    module.functions.push(Function {
        id: 1,
        name: "read".into(),
        type_params: vec![],
        params: if global {
            vec![param(2, Type::Any)]
        } else {
            vec![param(1, Type::Named(class.into())), param(2, Type::Any)]
        },
        return_type: Type::Any,
        body: if global {
            vec![Stmt::While {
                condition: Expr::Bool(true),
                body: vec![
                    Stmt::Expr(Expr::Call {
                        callee: Box::new(Expr::LocalGet(2)),
                        args: vec![],
                        type_args: vec![],
                        byte_offset: 0,
                    }),
                    Stmt::Return(Some(read)),
                ],
            }]
        } else {
            vec![Stmt::Return(Some(read))]
        },
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    });
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

#[test]
fn byte_reads_with_unknown_keys_use_the_common_checked_element_load() {
    crate::temp_root_coverage::under_both_lowerings(|mode| {
        for (class, dedicated, global) in [
            ("Uint8Array", true, false),
            ("Uint8Array", false, false),
            ("Buffer", false, false),
            ("Uint8Array", true, true),
        ] {
            let ir = read_ir(class, dedicated, global);
            assert!(
                ir.contains("ta.read.load"),
                "{mode}/{class}/{dedicated}/{global}: no shared load"
            );
            assert!(
                ir.contains("ta.read.oob"),
                "canonical OOB must produce undefined inline"
            );
            assert!(
                ir.contains("call double @js_dyn_index_get"),
                "a lying receiver or noncanonical key keeps boxed [[Get]]"
            );
            assert!(
                !ir.contains("call double @js_typed_array_index_get_dynamic"),
                "the duplicate byte dispatcher must be gone"
            );
            if global {
                let marker = ir
                    .lines()
                    .find(|l| l.contains("; bytes.hoist.roots "))
                    .expect("constructed module table owner proof");
                let state = marker
                    .split("state=")
                    .nth(1)
                    .unwrap()
                    .split_whitespace()
                    .next()
                    .unwrap();
                assert!(
                    ir.matches(&format!("store i8 0, ptr %{state}")).count() >= 2,
                    "a call before the first read must already invalidate the global proof"
                );
                let fn_ir = crate::testing::root_slots::enclosing_function(&ir, marker);
                let roots = crate::testing::root_slots::bound_slots(fn_ir);
                for field in ["receiver=", "owner="] {
                    let slot = format!(
                        "%{}",
                        marker
                            .split(field)
                            .nth(1)
                            .unwrap()
                            .split_whitespace()
                            .next()
                            .unwrap()
                    );
                    assert!(
                        roots.contains_key(&slot)
                            || ir.contains(&format!("{slot} = alloca ptr addrspace(1)")),
                        "{mode}: global {field}{slot} stays a GC root"
                    );
                }
            }
        }
    });
}

#[test]
fn admitted_byte_reads_have_no_opaque_memory_clobber() {
    let ir = read_ir("Int8Array", false, false);
    assert!(ir.contains("load i8"));
    assert!(
        !ir.contains("movzbl"),
        "ordinary admitted storage must use an LLVM load, not an opaque x86 barrier"
    );
}

#[test]
fn byte_scanning_reuses_the_number_loop_scope() {
    use perry_hir::{CompareOp, UpdateOp};
    crate::temp_root_coverage::under_both_lowerings(|mode| {
        for while_loop in [false, true] {
            for live_length in [false, true] {
                let mut module = Module::new("scan_index.ts");
                let param = |id, ty| Param {
                    id,
                    name: format!("p{id}"),
                    ty,
                    default: None,
                    decorators: vec![],
                    is_rest: false,
                    arguments_object: None,
                };
                module.functions.push(Function {
                    id: 1,
                    name: "scan".into(),
                    type_params: vec![],
                    params: vec![
                        param(1, Type::Named("Uint8Array".into())),
                        param(2, Type::Number),
                    ],
                    return_type: Type::Number,
                    body: vec![
                        Stmt::For {
                            init: None,
                            condition: Some(Expr::Compare {
                                op: CompareOp::Lt,
                                left: Box::new(Expr::LocalGet(2)),
                                right: Box::new(Expr::Integer(8)),
                            }),
                            update: Some(Expr::Update {
                                id: 2,
                                op: UpdateOp::Increment,
                                prefix: false,
                            }),
                            body: vec![Stmt::Expr(Expr::Uint8ArrayGet {
                                array: Box::new(Expr::LocalGet(1)),
                                index: Box::new(Expr::LocalGet(2)),
                            })],
                        },
                        Stmt::Return(Some(Expr::LocalGet(2))),
                    ],
                    is_async: false,
                    is_generator: false,
                    is_strict: true,
                    is_exported: true,
                    captures: vec![],
                    decorators: vec![],
                    was_plain_async: false,
                    was_unrolled: false,
                });
                if live_length {
                    let Stmt::For {
                        condition: Some(Expr::Compare { right, .. }),
                        ..
                    } = &mut module.functions[0].body[0]
                    else {
                        unreachable!()
                    };
                    *right = Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(1)),
                        property: "length".into(),
                        byte_offset: 0,
                    });
                }
                if while_loop {
                    let Stmt::For {
                        condition,
                        update,
                        mut body,
                        ..
                    } = module.functions[0].body.remove(0)
                    else {
                        unreachable!()
                    };
                    body.push(Stmt::Expr(update.unwrap()));
                    module.functions[0].body.insert(
                        0,
                        Stmt::While {
                            condition: condition.unwrap(),
                            body,
                        },
                    );
                }
                let ir = String::from_utf8(
                    compile_module(
                        &module,
                        CompileOptions {
                            emit_ir_only: true,
                            ..Default::default()
                        },
                    )
                    .unwrap(),
                )
                .unwrap();
                assert!(
                    ir.contains("for.number_locals.fast.preheader"),
                    "{mode}: byte index must share the existing one-time Number admission"
                );
                let start = ir
                    .find("\nfor.number_locals_fast.cond")
                    .expect("fast loop condition");
                let end = ir[start..]
                    .find("\nfor.number_locals_slow.cond")
                    .map(|n| start + n)
                    .unwrap_or(ir.len());
                let fast = &ir[start..end];
                assert!(fast.contains("fcmp olt double") && fast.contains("fadd double"));
                let condition_value = fast
                    .lines()
                    .find_map(|line| {
                        line.split_once("fcmp olt double ")
                            .map(|(_, rest)| rest.split(',').next().unwrap().trim())
                    })
                    .expect("native induction comparison");
                let definition = format!("{condition_value} = load double, ptr ");
                let slot = ir
                    .lines()
                    .find_map(|line| line.trim().strip_prefix(&definition))
                    .expect("the comparison reads the Number scope slot")
                    .split(',')
                    .next()
                    .unwrap()
                    .trim();
                assert!(
                    fast.lines().map(str::trim).any(|line| {
                        line.starts_with("store double ")
                            && line.split_once(", ptr ").is_some_and(|(_, target)| {
                                target.split(',').next().unwrap().trim() == slot
                            })
                    }),
                    "{mode}: ++ must update the same unboxed slot the loop reads"
                );

                assert!(
                    fast.contains("load volatile i32, ptr @PERRY_GC_POLL_ARMED"),
                    "scanner back edges keep the armed GC poll"
                );
                assert!(
                    !fast.contains("@js_numeric_step"),
                    "admitted index stays Number through every loop write"
                );
                if !live_length {
                    assert!(!fast.contains("@js_rel_lt"));
                } else {
                    assert!(
                        fast.contains("@js_value_length_property_key_ic_f64"),
                        "length overrides retain full property semantics"
                    );
                }
                assert!(
                    ir.contains("for.number_locals_slow"),
                    "annotation lies retain the existing generic loop"
                );
            }
        }
    });
}

#[test]
fn dropping_the_global_owner_root_turns_the_scanning_witness_red() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "expr::byte_scanning_tests::byte_reads_with_unknown_keys_use_the_common_checked_element_load", "--nocapture"])
        .env("PERRY_B4_SABOTAGE", "hoist_owner").output().unwrap();
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(output.contains("running 1 test"));
    assert!(
        !child.status.success() && output.contains("global owner="),
        "dropping the global owner must fail its live-root invariant: {output}"
    );
}

#[test]
fn constructed_table_binding_is_invariant_while_storage_remains_live() {
    let ir = read_ir("Uint8Array", true, true);
    let lines: Vec<_> = ir.lines().collect();
    assert!(
        lines.windows(2).any(|pair| {
            if !pair[0].contains(" = icmp eq i8 ") || !pair[0].ends_with(", 1") {
                return false;
            }
            let value = pair[0].split('=').next().unwrap().trim();
            pair[1].contains(&format!("and i1 {value}, true"))
        }),
        "the existing stable-binding flag must remove the receiver-bits comparison"
    );
    assert!(ir.contains("bytes.access.revalidate"));
    assert!(ir.contains("; bytes.hoist.roots "));
}

#[test]
fn byte_result_numeric_facts_require_a_proven_receiver() {
    crate::temp_root_coverage::under_both_lowerings(|mode| {
        for class in ["Uint8Array", "Buffer"] {
            let ir = read_ir_shape(class, true, false, Expr::Integer(0), true);
            assert!(ir.contains("call double @js_dynamic_string_or_number_add"),
                "{mode}/{class}: an erased receiver annotation cannot rule out string concatenation");
        }
    });
}

#[test]
fn a_boxed_nested_key_cannot_establish_numeric_byte_result_facts() {
    let key = Expr::Uint8ArrayGet {
        array: Box::new(Expr::LocalGet(1)),
        index: Box::new(Expr::Integer(512)),
    };
    let ir = read_ir_shape("Uint8Array", true, true, key, true);
    assert!(
        ir.contains("call double @js_dynamic_string_or_number_add"),
        "the inner byte read can yield undefined, which names an arbitrary outer property"
    );
}
