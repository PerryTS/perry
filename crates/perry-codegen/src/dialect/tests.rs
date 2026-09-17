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

/// #9050: Apple arm64's hot-TLS reader is value-returning inline asm, not the
/// void-only loop barrier that originally defined this parser branch. Keep
/// both forms live, plus an operand-bearing form that proves the constructed
/// function signature follows the inline-asm argument list.
#[test]
fn typed_and_void_inline_asm_round_trip_with_gc_leaf_attributes() {
    let ir = r#"
declare i64 @"ordinary asm callee"()

define i64 @read_tpidrro() {
entry:
  %r = call i64 asm sideeffect "mrs $0, tpidrro_el0", "=r"() "gc-leaf-function"
  ret i64 %r
}

define i64 @asm_with_operand(i64 %x) {
entry:
  %r = call i64 asm sideeffect "", "=r,r"(i64 %x) "gc-leaf-function"
  ret i64 %r
}

define void @barrier() {
entry:
  call void asm sideeffect "", ""() "gc-leaf-function"
  ret void
}

define i64 @ordinary_call() {
entry:
  %r = call i64 @"ordinary asm callee"()
  ret i64 %r
}
"#;
    let (skeleton, fns) = split_corpus(ir);
    let ctx = Context::create();
    let module = crate::inprocess::parse_ir_text(&ctx, &skeleton, "typed_inline_asm_skel")
        .expect("skeleton parses");
    for function in &fns {
        predeclare_function_from_text(&ctx, &module, function).expect("predeclare");
    }
    for function in &fns {
        add_function_from_text(&ctx, &module, function).unwrap_or_else(|e| panic!("{e:#}"));
    }
    module
        .verify()
        .unwrap_or_else(|e| panic!("verifier rejected native module:\n{}", e.to_string()));

    let printed = module.print_to_string().to_string();
    let typed = printed
        .lines()
        .find(|line| line.contains("mrs $0, tpidrro_el0"))
        .unwrap_or_else(|| panic!("typed inline asm was not constructed:\n{printed}"));
    assert!(
        typed.contains("call i64 asm sideeffect") && typed.contains("#0"),
        "typed inline asm lost its result type, sideeffect flag, or leaf attribute:\n{printed}"
    );
    assert!(
        printed.contains("ret i64 %r"),
        "typed inline asm result is not returned:\n{printed}"
    );
    assert!(
        printed.contains("\"=r,r\"(i64 %x)"),
        "inline-asm argument list did not survive construction:\n{printed}"
    );
    assert!(
        printed.contains("call void asm sideeffect"),
        "void inline-asm barrier regressed:\n{printed}"
    );
    assert!(
        printed.contains("call i64 @\"ordinary asm callee\"()"),
        "ordinary call containing ` asm ` was misclassified:\n{printed}"
    );
    assert!(
        printed.contains("attributes #0 = { \"gc-leaf-function\" }"),
        "inline asm lost its structural gc-leaf-function attribute:\n{printed}"
    );
}

// ---------------------------------------------------------------------------
// #10545: a constant operand must mean what LLVM's own parser says it means
// ---------------------------------------------------------------------------
//
// `types::constant` is where every operand token becomes an LLVM value, on the
// typed-instruction path and the line path alike. A token it builds differently
// from LLVM's assembler fails nothing: the module verifies, runs, and computes
// with a different number — in split modules only, the reader's only default.
// #10545 was exactly that: `i128` literals kept their low 64 bits, so BigInt
// literals wider than 64 bits changed value. The tests below therefore compare
// against LLVM's parse of the SAME text rather than against expectations
// written down here.

use crate::module::LlModule;
use crate::types::{LlvmType, DOUBLE, F32, I1, I128, I16, I32, I64, I8, PTR, VOID};
use inkwell::values::AnyValue;

/// `(type, token)` for every constant operand form perry-codegen emits in a
/// function body. Globals, `c"…"` byte strings and aggregate initializers are
/// deliberately absent: they live in the module skeleton, which LLVM's own
/// parser reads (`native_emit`), never this reader.
fn emitted_constant_forms() -> Vec<(LlvmType, String)> {
    let mut forms: Vec<(LlvmType, String)> = Vec::new();
    macro_rules! push {
        ($ty:expr, $toks:expr $(,)?) => {
            forms.extend($toks.iter().map(|t: &&str| ($ty, t.to_string())))
        };
    }
    push!(I1, &["true", "false", "0", "1"]);
    // Narrow operands spelled past their signed range (`and i8 %f, 128`).
    push!(I8, &["0", "127", "128", "255", "-1", "-128"]);
    push!(I16, &["1024", "32767", "65535", "-32768"]);
    push!(
        I32,
        &["16000000", "2147483647", "-2147483648", "4294967295", "-1"],
    );
    push!(
        I64,
        &[
            "-1",
            "9218868437227405312",
            "9223372036854775807",
            "-9223372036854775808",
            "12345678901234567890",
            "18446744073709551615",
            "ptrtoint (ptr @constant_forms_global to i64)",
            "undef",
            "poison",
        ],
    );
    // `NativeRep::SmallBigInt` spells its literal with `i128::to_string`.
    for v in [
        0i128,
        -1,
        64,
        (1 << 64) - 1,
        1 << 64,
        -(1 << 64),
        1 << 70,
        -98_765_432_109_876_543_210_987_654_321,
        i128::MAX,
        i128::MIN,
    ] {
        forms.push((I128, v.to_string()));
    }
    // LLVM also accepts the unsigned spelling of a full-width word.
    forms.push((I128, u128::MAX.to_string()));
    // `nanbox::double_literal` is the emitter's only decimal spelling; cover
    // each class it distinguishes (signed zero, non-finite hex, shortest
    // round-trip digits, the longest expansions).
    for v in [
        0.0,
        -0.0,
        1.5,
        -1.0,
        0.1,
        1e21,
        1e300,
        f64::MAX,
        f64::MIN_POSITIVE,
        5e-324,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
    ] {
        forms.push((DOUBLE, crate::nanbox::double_literal(v)));
    }
    // NaN-boxed tag words and raw bit patterns, including a signalling NaN.
    push!(
        DOUBLE,
        &[
            "0x7FFC000000000001",
            "0x7FFD000000000000",
            "0x7FFF000000000000",
            "0xFFF8000000000000",
            "0x7FF0000000000001",
            "0x7FF4000000000000",
        ],
    );
    push!(
        F32,
        &["1.5", "-0.0", "0x3FF8000000000000", "0x7FF0000000000000"]
    );
    push!(PTR, &["null", "@constant_forms_global", "undef"]);
    push!(
        "<2 x i64>",
        &[
            "<i64 207232172546, i64 0>",
            "<i64 -1, i64 18446744073709551615>",
            "zeroinitializer",
        ],
    );
    push!(
        "<4 x i32>",
        &["<i32 0, i32 0, i32 0, i32 0>", "zeroinitializer", "poison"]
    );
    forms
}

/// One sink call per form: calls are never constant-folded by the C-API
/// builder, so the printed operand is exactly the constant that was built.
fn constant_forms_module() -> LlModule {
    let forms = emitted_constant_forms();
    let mut m = LlModule::new(crate::codegen::default_target_triple());
    m.add_global("constant_forms_global", I64, "0");
    for (i, (ty, _)) in forms.iter().enumerate() {
        m.declare_function(&format!("constant_sink_{i}"), VOID, &[*ty]);
    }
    let f = m.define_function("constant_forms", VOID, vec![]);
    let entry = f.create_block("entry");
    for (i, (ty, tok)) in forms.iter().enumerate() {
        entry.call_void(&format!("constant_sink_{i}"), &[(*ty, tok.as_str())]);
    }
    entry.ret_void();
    m
}

fn print_function(module: &inkwell::module::Module<'_>, name: &str) -> String {
    module
        .get_function(name)
        .unwrap_or_else(|| panic!("@{name} missing"))
        .print_to_string()
        .to_string()
}

fn assert_same_function_print(label: &str, llvm: &str, reader: &str) {
    let diffs: Vec<String> = llvm
        .lines()
        .zip(reader.lines())
        .filter(|(a, b)| a != b)
        .map(|(a, b)| format!("  LLVM:   {}\n  reader: {}", a.trim(), b.trim()))
        .collect();
    assert!(
        diffs.is_empty() && llvm.lines().count() == reader.lines().count(),
        "{label} built constants that differ from LLVM's own parse of the same \
         text:\n{}",
        diffs.join("\n")
    );
}

#[test]
fn constant_operands_match_llvms_own_parse_on_typed_and_line_paths() {
    let m = constant_forms_module();
    let forms = emitted_constant_forms().len();
    let function = m
        .deduped_function_refs()
        .into_iter()
        .find(|f| f.name == "constant_forms")
        .expect("fixture function");
    let text = function.to_ir();
    let header = text.lines().next().expect("define header");

    // Reference: LLVM's assembler over the complete module text.
    let llvm_ctx = Context::create();
    let llvm_module = crate::inprocess::parse_ir_text(&llvm_ctx, &m.to_ir(), "forms_llvm")
        .expect("LLVM parses the fixture");
    let llvm = print_function(&llvm_module, "constant_forms");
    assert_eq!(
        llvm.matches("call void @constant_sink_").count(),
        forms,
        "fixture lost sink calls, so some forms would go uncompared:\n{llvm}"
    );

    // Typed path: `FnStream::item`, what split modules stream.
    let typed_ctx = Context::create();
    let typed_module = crate::inprocess::parse_ir_text(&typed_ctx, &m.skeleton_ir(), "forms_typed")
        .expect("skeleton parses");
    let mut stream = FnStream::begin(&typed_ctx, &typed_module, header).expect("begin");
    function
        .for_each_final_item::<anyhow::Error>(&mut |item| stream.item(&item))
        .unwrap_or_else(|e| panic!("typed construction: {e:#}"));
    let (typed, _) = stream.finish().expect("finish");
    assert!(typed >= forms, "only {typed} typed instructions were built");
    typed_module
        .verify()
        .unwrap_or_else(|e| panic!("verifier rejected typed module:\n{}", e.to_string()));
    assert_same_function_print(
        "typed path",
        &llvm,
        &print_function(&typed_module, "constant_forms"),
    );

    // Line path: what personality/stack-map functions stream.
    let line_ctx = Context::create();
    let line_module = crate::inprocess::parse_ir_text(&line_ctx, &m.skeleton_ir(), "forms_line")
        .expect("skeleton parses");
    predeclare_function_from_text(&line_ctx, &line_module, &text).expect("predeclare");
    add_function_from_text(&line_ctx, &line_module, &text)
        .unwrap_or_else(|e| panic!("line construction: {e:#}"));
    line_module
        .verify()
        .unwrap_or_else(|e| panic!("verifier rejected line module:\n{}", e.to_string()));
    assert_same_function_print(
        "line path",
        &llvm,
        &print_function(&line_module, "constant_forms"),
    );
}

/// #10545 from the real emitter: a BigInt literal that fits `i128` lowers to
/// `NativeRep::SmallBigInt` and is boxed by splitting it into two words
/// (`trunc i128 C to i64`, `ashr i128 C, 64`). The C-API builder folds both at
/// construction, so the built call's operands ARE the words the runtime gets.
#[test]
fn wide_bigint_literal_words_survive_native_construction() {
    let literals: [i128; 3] = [
        1 << 70,
        -98_765_432_109_876_543_210_987_654_321,
        i128::MIN + 1,
    ];
    let mut m = Module::new("wide_bigint_literals.ts");
    m.init = literals
        .iter()
        .enumerate()
        .map(|(i, v)| Stmt::Let {
            id: 4100 + i as u32,
            name: format!("b{i}"),
            ty: Type::BigInt,
            mutable: false,
            init: Some(Expr::BigInt(v.to_string())),
        })
        .collect();
    m.init_kind = ModuleInitKind::Eager;
    let opts = CompileOptions {
        emit_ir_only: true,
        is_entry_module: true,
        ..Default::default()
    };
    let ir = String::from_utf8(compile_module(&m, opts).expect("module compiles"))
        .expect("LLVM IR is UTF-8");
    for v in literals {
        assert!(
            ir.contains(&format!("i128 {v}")),
            "fixture no longer lowers {v}n to an i128 operand, so this test \
             would construct nothing relevant:\n{ir}"
        );
    }

    let (skeleton, fns) = split_corpus(&ir);
    let ctx = Context::create();
    let module = crate::inprocess::parse_ir_text(&ctx, &skeleton, "wide_bigint_skel")
        .expect("skeleton parses");
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
    for v in literals {
        let words = format!(
            "@js_bigint_from_i128_parts(i64 {}, i64 {})",
            v as i64,
            (v >> 64) as i64
        );
        assert!(
            printed.contains(&words),
            "{v}n was not boxed from its two's-complement words `{words}`:\n{printed}"
        );
    }
}
