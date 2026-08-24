//! Reader tests, split out of `dialect/mod.rs` for the 2000-line cap.

use super::*;

fn split_corpus(text: &str) -> (String, Vec<String>) {
    let mut skeleton = String::new();
    let mut fns = Vec::new();
    let mut cur: Option<String> = None;
    for line in text.lines() {
        if line.starts_with("define ") {
            cur = Some(String::new());
        }
        match cur.as_mut() {
            Some(f) => {
                f.push_str(line);
                f.push('\n');
                if line == "}" {
                    fns.push(cur.take().unwrap());
                }
            }
            None => {
                skeleton.push_str(line);
                skeleton.push('\n');
            }
        }
    }
    (skeleton, fns)
}

/// Build every `define` in `text` through the reader and verify the module.
/// Returns the instruction count so callers can assert the corpus was real.
fn roundtrip_ir(text: &str, module_name: &str) -> usize {
    let (skeleton, fns) = split_corpus(text);
    let ctx = Context::create();
    let module =
        crate::inprocess::parse_ir_text(&ctx, &skeleton, module_name).expect("skeleton parses");
    for f in &fns {
        predeclare_function_from_text(&ctx, &module, f)
            .unwrap_or_else(|e| panic!("predeclare: {e:#}"));
    }
    let mut n = 0usize;
    for f in &fns {
        n += add_function_from_text(&ctx, &module, f).unwrap_or_else(|e| panic!("{e:#}"));
    }
    module
        .verify()
        .unwrap_or_else(|e| panic!("verifier rejected native module:\n{}", e.to_string()));
    n
}

/// Every function in a real perry-emitted corpus file must construct
/// natively and pass the LLVM verifier. This is the reader's primary
/// gate: a form it cannot express fails here, not in a user build.
fn corpus_roundtrip(path: &str) {
    // The corpora are tracked in-tree alongside this reader, so a missing
    // file is a broken checkout, not a branch without artifacts. Skipping
    // would make the reader's primary gate pass vacuously — precisely the
    // failure mode the Linux bring-up had to rule out by hand.
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("corpus file {path} is not readable: {e}"));
    let n = roundtrip_ir(&text, "corpus_skel");
    assert!(
        n > 1000,
        "expected a real corpus, built only {n} instructions"
    );
}

#[test]
fn corpus_spike() {
    corpus_roundtrip(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../experiments/llvm-inprocess-spike/spike_text.ll"
    ));
}

/// #7302: a try/catch/finally corpus — invoke edges, landing pads,
/// the personality clause on the define, and the inline continuation
/// labels an invoke split leaves behind. Without this the reader's EH
/// support would be exercised only by the async spike, which has one
/// shape (the async rejection boundary) and no nesting.
#[test]
fn corpus_exception_handling() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../experiments/llvm-inprocess-spike/eh_text.ll"
    );
    // Assert the subject is LIVE: a corpus that lost its EH forms would
    // still round-trip, and would silently stop testing invoke.
    let text = std::fs::read_to_string(path).expect("eh corpus readable");
    assert!(
        text.matches("invoke ").count() >= 20,
        "eh corpus has no invoke edges left"
    );
    assert!(text.contains("landingpad"), "eh corpus has no landing pad");
    assert!(
        text.contains("personality ptr @perry_eh_personality"),
        "eh corpus lost its personality clause"
    );
    corpus_roundtrip(path);
}

#[test]
fn corpus_batch_kernel() {
    corpus_roundtrip(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../experiments/llvm-inprocess-spike/batch_kernel.ll"
    ));
}

// ---------------------------------------------------------------------------
// #8228: LIVE emit -> re-parse, not a frozen corpus
// ---------------------------------------------------------------------------
//
// The three `corpus_*` tests above are the reader's primary gate, and they are
// all **snapshots**: `.ll` files checked in under `experiments/`. A form the
// emitters started producing *after* those files were captured is invisible to
// them, and the reader is only the DEFAULT for split (multi-unit) modules
// (`native_emit::native_units_mode`) — every gap/parity fixture is a single
// unit and keeps the text path. So a new emission form could ship, be silently
// untested by every per-PR job, and first fail in a user build of a large app.
//
// That is exactly what #8204 did: it added the `<2 x i64>` object-header-image
// compose, the reader had no `insertelement` case, and the fall-through
// binary-op arm reported `bad binary op \`insertelement\` operands` on the five
// biggest modules of the Next App Route fixture — the only build big enough to
// split, and one that is tag-gated.
//
// `compiled_module_ir_round_trips_through_the_reader` closes the class rather
// than the instance: it compiles a real module through the real emitters and
// re-parses every function it produced, so the NEXT new form fails here.

use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Class, ClassField, Expr, Module, ModuleInitKind, Stmt};

/// The header-image compose the reader must accept. Asserted present before
/// the round-trip so a fixture that stopped exercising the inline allocator
/// fails loudly instead of passing vacuously (CLAUDE.md: a gate must assert
/// its subject was live).
const HEADER_IMAGE_COMPOSE: &str = "insertelement <2 x i64> <i64 ";

fn cell_class() -> Class {
    Class {
        id: 3,
        name: "Cell".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![ClassField {
            name: "v".to_string(),
            key_expr: None,
            ty: Type::Number,
            init: None,
            is_private: false,
            is_readonly: false,
            decorators: Vec::new(),
        }],
        constructor: None,
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

/// `while (…) { const c = new Cell(1) }` — a `new` in a loop, which is what
/// admits the inline bump allocator and therefore the per-class header image.
/// The result is bound rather than discarded: a discarded value takes a
/// different lowering path (#7590).
fn cell_loop_module() -> Module {
    let mut m = Module::new("dialect_roundtrip.ts");
    m.classes = vec![cell_class()];
    m.init = vec![Stmt::While {
        condition: Expr::Bool(false),
        body: vec![Stmt::Let {
            id: 4001,
            name: "c".to_string(),
            ty: Type::Named("Cell".to_string()),
            mutable: false,
            init: Some(Expr::New {
                class_name: "Cell".to_string(),
                args: vec![Expr::Number(1.0)],
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
            }),
        }],
    }];
    m.init_kind = ModuleInitKind::Eager;
    m
}

#[test]
fn compiled_module_ir_round_trips_through_the_reader() {
    let opts = CompileOptions {
        emit_ir_only: true,
        is_entry_module: true,
        ..Default::default()
    };
    let ir = String::from_utf8(compile_module(&cell_loop_module(), opts).expect("module compiles"))
        .expect("LLVM IR is UTF-8");
    assert!(
        ir.contains(HEADER_IMAGE_COMPOSE),
        "fixture no longer emits the inline-allocator header image, so this \
         test would round-trip nothing relevant:\n{ir}"
    );
    let n = roundtrip_ir(&ir, "emit_roundtrip");
    assert!(n > 0, "round-tripped an empty module");
}

#[test]
fn acquire_atomic_load_round_trips_through_the_reader() {
    let ir = r#"
@gate = external global i8

define i8 @load_gate() {
entry:
  %value = load atomic i8, ptr @gate acquire, align 1
  ret i8 %value
}
"#;
    assert_eq!(roundtrip_ir(ir, "acquire_load"), 2);
}

/// The `format!` templates `expr/channel.rs` emits for its `<4 x i32>` SIMD
/// byte-channel reduction, in emission order. The fixture below is BUILT from
/// these strings rather than duplicating them, and each is asserted to still
/// be present verbatim in that file's source — so if the emitter's text
/// changes, this test's fixture changes with it or the test fails, and it can
/// never quietly test a shape codegen stopped producing.
const CHANNEL_TEMPLATES: &[&str] = &[
    "{} = insertelement <4 x i32> {}, i32 {}, i32 {}",
    "{} = insertelement <4 x i32> poison, i32 {}, i32 0",
    "{} = shufflevector <4 x i32> {}, <4 x i32> poison, <4 x i32> zeroinitializer",
    "{} = mul <4 x i32> {}, {}",
    "{} = add <4 x i32> {}, {}",
    "{} = extractelement <4 x i32> {}, i32 {}",
];

/// Substitute `args` into a `format!`-style template's `{}` holes.
fn instantiate(template: &str, args: &[&str]) -> String {
    let mut parts = template.split("{}");
    let mut out = parts.next().unwrap_or_default().to_string();
    for (i, tail) in parts.enumerate() {
        out.push_str(args.get(i).unwrap_or_else(|| {
            panic!("template `{template}` needs more than {} args", args.len())
        }));
        out.push_str(tail);
    }
    out
}

#[test]
fn channel_reduction_vector_forms_round_trip() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/expr/channel.rs"))
        .expect("channel.rs is readable");
    for t in CHANNEL_TEMPLATES {
        assert!(
            src.contains(&format!("\"{t}\"")),
            "expr/channel.rs no longer emits `{t}` — update CHANNEL_TEMPLATES \
             so this fixture keeps matching the emitter"
        );
    }
    let zeros = "<i32 0, i32 0, i32 0, i32 0>";
    let mut body = vec![
        "  %b0 = load i8, ptr %acc".to_string(),
        "  %z0 = zext i8 %b0 to i32".to_string(),
        // Lane 0 seeded from the constant zero vector, lane 1 chained off a
        // register — both operand shapes the emitter produces.
        format!(
            "  {}",
            instantiate(CHANNEL_TEMPLATES[0], &["%v0", zeros, "%z0", "0"])
        ),
        format!(
            "  {}",
            instantiate(CHANNEL_TEMPLATES[0], &["%v1", "%v0", "%z0", "1"])
        ),
        format!("  {}", instantiate(CHANNEL_TEMPLATES[1], &["%k0", "3"])),
        format!("  {}", instantiate(CHANNEL_TEMPLATES[2], &["%ks", "%k0"])),
        format!(
            "  {}",
            instantiate(CHANNEL_TEMPLATES[3], &["%mv", "%v1", "%ks"])
        ),
        format!(
            "  {}",
            instantiate(CHANNEL_TEMPLATES[0], &["%a0", zeros, "%z0", "0"])
        ),
        format!(
            "  {}",
            instantiate(CHANNEL_TEMPLATES[4], &["%na", "%a0", "%mv"])
        ),
        format!(
            "  {}",
            instantiate(CHANNEL_TEMPLATES[5], &["%l0", "%na", "0"])
        ),
        "  store i32 %l0, ptr %acc".to_string(),
        "  ret void".to_string(),
    ];
    body.insert(0, "entry:".to_string());
    let ir = format!(
        "define void @channel_probe(ptr %acc) {{\n{}\n}}\n",
        body.join("\n")
    );
    let n = roundtrip_ir(&ir, "channel_roundtrip");
    assert!(
        n >= body.len() - 1,
        "built only {n} instructions from:\n{ir}"
    );
}

/// #8175: `preserve_nonecc` on a define header, a call site, and an invoke
/// site all construct natively with the real LLVM convention — asserted on
/// LLVM's own printed form, which only shows the token when the convention
/// was actually set on the value (a dropped call-site convention would be a
/// silent define/call mismatch, i.e. UB, so this must not rely on perry's
/// emitter being the only writer).
#[test]
fn preserve_none_constructs_on_define_call_and_invoke() {
    let ctx = Context::create();
    let skeleton = "declare i32 @perry_eh_personality(i32, i32, i64, ptr, ptr)\n";
    let module = crate::inprocess::parse_ir_text(&ctx, skeleton, "preserve_none_skel")
        .expect("skeleton parses");
    let fns = [
        "define internal preserve_nonecc double @callee$pn_i32(i32 %arg0) {\n\
         entry.0:\n\
         \x20 %r1 = sitofp i32 %arg0 to double\n\
         \x20 ret double %r1\n\
         }\n",
        "define double @caller(double %arg0) {\n\
         entry.0:\n\
         \x20 %r1 = call preserve_nonecc double @callee$pn_i32(i32 7)\n\
         \x20 ret double %r1\n\
         }\n",
        "define double @trycaller(double %arg0) personality ptr @perry_eh_personality {\n\
         entry.0:\n\
         \x20 %r1 = invoke preserve_nonecc double @callee$pn_i32(i32 7) to label %eh.cont1 \
         unwind label %lpad.0\n\
         eh.cont1:\n\
         \x20 ret double %r1\n\
         lpad.0:\n\
         \x20 %lp = landingpad { ptr, i32 } catch ptr null\n\
         \x20 ret double 0.0\n\
         }\n",
    ];
    for f in &fns {
        predeclare_function_from_text(&ctx, &module, f).expect("predeclare");
    }
    for f in &fns {
        add_function_from_text(&ctx, &module, f).unwrap_or_else(|e| panic!("{e:#}"));
    }
    module
        .verify()
        .unwrap_or_else(|e| panic!("verifier rejected native module:\n{}", e.to_string()));
    let printed = module.print_to_string().to_string();
    assert!(
        printed.contains("define internal preserve_nonecc double @\"callee$pn_i32\"")
            || printed.contains("define internal preserve_nonecc double @callee$pn_i32"),
        "function value lost its calling convention:\n{printed}"
    );
    assert_eq!(
        printed.matches("call preserve_nonecc double").count(),
        1,
        "call site lost its calling convention:\n{printed}"
    );
    assert_eq!(
        printed.matches("invoke preserve_nonecc double").count(),
        1,
        "invoke site lost its calling convention:\n{printed}"
    );
}

/// #8596: a transitive-leaf direct call inside `try` is an invoke, and LLVM's
/// call-site attribute sits between the argument list and `to label`. The
/// split-module native reader must carry it onto the CallBase or RS4GC silently
/// restores a statepoint that the text path removed.
#[test]
fn gc_leaf_attribute_constructs_on_invoke() {
    let ctx = Context::create();
    let skeleton = "declare void @pure()\n\
                    declare i32 @perry_eh_personality(i32, i32, i64, ptr, ptr)\n";
    let module = crate::inprocess::parse_ir_text(&ctx, skeleton, "leaf_invoke_skel")
        .expect("skeleton parses");
    let function = "define void @trycaller() personality ptr @perry_eh_personality {\n\
                    entry:\n\
                    \x20 invoke void @pure() \"gc-leaf-function\" to label %ok unwind label %pad\n\
                    ok:\n\
                    \x20 ret void\n\
                    pad:\n\
                    \x20 %lp = landingpad { ptr, i32 } catch ptr null\n\
                    \x20 ret void\n\
                    }\n";
    predeclare_function_from_text(&ctx, &module, function).expect("predeclare");
    add_function_from_text(&ctx, &module, function).unwrap_or_else(|e| panic!("{e:#}"));
    module
        .verify()
        .unwrap_or_else(|e| panic!("verifier rejected native module:\n{}", e.to_string()));
    let printed = module.print_to_string().to_string();
    let invoke = printed
        .lines()
        .find(|line| line.contains("invoke void @pure"))
        .unwrap_or_else(|| panic!("no invoke in constructed module:\n{printed}"));
    assert!(
        invoke.contains("#0") && printed.contains("attributes #0 = { \"gc-leaf-function\" }"),
        "invoke lost its gc-leaf-function call-site attribute:\n{printed}"
    );
}
