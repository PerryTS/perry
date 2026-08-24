use perry_codegen::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, Expr, Function, Module, Param, Stmt, TYPED_ARRAY_KIND_UINT32};

#[path = "native_proof_support/mod.rs"]
mod native_proof_support;
use native_proof_support::{artifact_env_lock, artifact_for_module, NativeRepsEnv};

const VALUES: u32 = 100;
const KEY: u32 = 101;
const BASE_TMP: u32 = 102;
const KEY_TMP: u32 = 103;

fn param(id: u32, name: &str) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn rmw_function(rhs: Expr) -> Function {
    let read = Expr::IndexGet {
        object: Box::new(Expr::LocalGet(BASE_TMP)),
        index: Box::new(Expr::LocalGet(KEY_TMP)),
    };
    Function {
        id: 7,
        name: "bump".to_string(),
        type_params: Vec::new(),
        params: vec![param(VALUES, "values"), param(KEY, "key")],
        return_type: Type::Number,
        body: vec![
            // The exact immutable aliases emitted by
            // `hoist_compound_member_assign` for `values[key] += rhs`.
            Stmt::Let {
                id: BASE_TMP,
                name: "__cmpd_base_test".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::LocalGet(VALUES)),
            },
            Stmt::Let {
                id: KEY_TMP,
                name: "__cmpd_key_test".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::LocalGet(KEY)),
            },
            Stmt::Expr(Expr::IndexSet {
                object: Box::new(Expr::LocalGet(BASE_TMP)),
                index: Box::new(Expr::LocalGet(KEY_TMP)),
                value: Box::new(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(read),
                    right: Box::new(rhs),
                }),
            }),
            Stmt::Return(Some(Expr::Integer(0))),
        ],
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

fn rmw_module(name: &str, rhs: Expr) -> Module {
    let mut module = Module::new(name);
    module.functions.push(rmw_function(rhs));
    module.init = vec![
        Stmt::Let {
            id: 1,
            name: "values".to_string(),
            ty: Type::Named("Uint32Array".to_string()),
            mutable: false,
            init: Some(Expr::TypedArrayNew {
                kind: TYPED_ARRAY_KIND_UINT32,
                arg: Some(Box::new(Expr::Integer(4))),
            }),
        },
        Stmt::Let {
            id: 2,
            name: "keys".to_string(),
            ty: Type::Array(Box::new(Type::Any)),
            mutable: false,
            init: Some(Expr::Array(vec![Expr::Integer(0)])),
        },
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::FuncRef(7)),
            args: vec![
                Expr::LocalGet(1),
                Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(2)),
                    index: Box::new(Expr::Integer(0)),
                },
            ],
            type_args: Vec::new(),
            byte_offset: 0,
        }),
    ];
    module
}

fn compile_ir(module: Module) -> String {
    String::from_utf8(
        compile_module(
            &module,
            CompileOptions {
                emit_ir_only: true,
                ..CompileOptions::default()
            },
        )
        .expect("module compiles"),
    )
    .expect("LLVM IR is UTF-8")
}

fn function_containing<'a>(ir: &'a str, marker: &str) -> &'a str {
    let start = ir
        .match_indices("define ")
        .find(|(start, _)| {
            let end = ir[*start..]
                .find('\n')
                .map_or(ir.len(), |offset| start + offset);
            ir[*start..end].contains(marker)
        })
        .map(|(start, _)| start)
        .unwrap_or_else(|| panic!("function containing `{marker}` not found:\n{ir}"));
    let rest = &ir[start..];
    let end = rest.find("\n}\n").map_or(rest.len(), |offset| offset + 3);
    &rest[..end]
}

fn block_containing<'a>(function: &'a str, marker: &str) -> &'a str {
    let start = function
        .lines()
        .scan(0usize, |offset, line| {
            let start = *offset;
            *offset += line.len() + 1;
            Some((start, line))
        })
        .find(|(_, line)| line.contains(marker) && line.trim_end().ends_with(':'))
        .map(|(start, _)| start)
        .unwrap_or_else(|| panic!("block containing `{marker}` not found:\n{function}"));
    let rest = &function[start..];
    let end = rest.find("\n\n").unwrap_or(rest.len());
    &rest[..end]
}

fn compile_artifact(module: Module) -> serde_json::Value {
    let _lock = artifact_env_lock();
    struct ArtifactDir(std::path::PathBuf);
    impl Drop for ArtifactDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let dir = ArtifactDir(std::env::temp_dir().join(format!(
        "perry_typed_array_rmw_8692_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )));
    std::fs::create_dir_all(&dir.0).unwrap();
    let _env = NativeRepsEnv::install(&dir.0, false);
    let name = module.name.clone();
    compile_module(
        &module,
        CompileOptions {
            emit_ir_only: true,
            ..CompileOptions::default()
        },
    )
    .expect("module compiles with artifact recording");
    artifact_for_module(&dir.0, &name)
}

#[test]
fn specialized_uint32_dynamic_index_rmw_has_a_call_free_fast_arm_and_fallback() {
    let ir = compile_ir(rmw_module("typed_array_rmw_8692.ts", Expr::Integer(1)));
    let specialized = function_containing(&ir, "$spec_ta5x4");
    let load = block_containing(specialized, "ta.rmw.load");
    let store = block_containing(specialized, "ta.rmw.store");
    let fallback = block_containing(specialized, "ta.rmw.full_fallback");

    assert!(
        specialized.contains("@PERRY_TA_VIEW_GUARD")
            && specialized.contains("@PERRY_TA_KIND_CACHE"),
        "the selected RMW must retain explicit inline-storage and kind guards:\n{specialized}"
    );
    assert!(
        load.contains("load i32") && load.contains("uitofp i32") && load.contains("fadd"),
        "the hot read/add arm must stay in native numeric SSA:\n{load}"
    );
    assert!(
        store.contains("store i32"),
        "Uint32 conversion and the direct backing-store write must remain in the guarded store arm:\n{store}"
    );
    for helper in [
        "js_typed_array_index_get_dynamic",
        "js_dynamic_string_or_number_add",
        "js_typed_array_index_set_dynamic",
    ] {
        assert!(
            !load.contains(helper) && !store.contains(helper),
            "the guarded fast arm must not call `{helper}`:\n{load}\n{store}"
        );
    }
    assert!(
        fallback.contains("js_typed_array_index_get_dynamic")
            && fallback.contains("js_dynamic_string_or_number_add"),
        "guard failure must retain the semantic get/add fallback:\n{fallback}\n{specialized}"
    );
    assert!(
        specialized.contains("ta.rmw.set_fallback")
            && specialized.contains("call double @js_dyn_index_set"),
        "post-RHS invalidation must retain a set-only continuation fallback:\n{specialized}"
    );
}

#[test]
fn native_artifact_reports_selected_guards_and_both_fallbacks() {
    let artifact = compile_artifact(rmw_module(
        "typed_array_rmw_artifact_8692.ts",
        Expr::Integer(1),
    ));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "TypedArrayRmw"
                && record["consumer"] == "TypedArrayRmw.guarded_direct_uint32_add"
                && record["access_mode"] == "checked_native"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| note == "typed_array_rmw=selected")
                        && notes.iter().any(|note| {
                            note == "post_rhs_fallback=generic_set_without_repeating_rhs"
                        })
                        && notes
                            .iter()
                            .any(|note| note == "post_rhs_receiver=reload_gc_visible_local")
                })
        }),
        "selected RMW guard/fallback evidence missing:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "TypedArrayRmw"
                && record["consumer"] == "TypedArrayRmw.explicit_fallback"
                && record["access_mode"] == "dynamic_fallback"
        }),
        "explicit dynamic fallback evidence missing:\n{artifact:#}"
    );
}

#[test]
fn native_artifact_explains_rejection_for_a_noncanonical_rhs() {
    let artifact = compile_artifact(rmw_module(
        "typed_array_rmw_rejected_8692.ts",
        Expr::String("x".to_string()),
    ));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "TypedArrayRmw"
                && record["consumer"] == "TypedArrayRmw.rejected"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes
                        .iter()
                        .any(|note| note == "typed_array_rmw_rejection=rhs_not_canonical_number")
                })
        }),
        "RMW rejection reason missing from explain-lowering/native artifacts:\n{artifact:#}"
    );
}
