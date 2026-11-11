//! VR-3a (`value_records`): a record binding becomes its field locals, on
//! injected facts. Each positive test has a negative control.

use std::sync::Arc;

use perry_hir::{Expr, Module, Stmt};

use super::{rewrite, Census};
use crate::codegen::{ElisionFacts, FactsRecRep, FactsRecordLayout, ProgramFactsView};
use crate::{compile_module, CompileOptions};

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(&src, "vr.ts", &mut cache)
                .expect("parses");
            perry_hir::lower_module(&parsed.module, "vr", "vr.ts").expect("lowers")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread")
}

/// The id of the one `let` named `name`.
fn let_id(m: &Module, name: &str) -> u32 {
    let mut ids = Vec::new();
    perry_hir::tdz_check::for_each_module_stmt_list(m, &mut |list| {
        for s in list {
            if let Stmt::Let { id, name: n, .. } = s {
                if n == name {
                    ids.push(*id);
                }
            }
        }
    });
    assert_eq!(ids.len(), 1, "one let {name}");
    ids[0]
}

/// The facts of the module's one literal-shape class as record template 0,
/// held by the bindings `names` (`(name, opt)`).
fn facts(m: &Module, names: &[(&str, bool)]) -> ElisionFacts {
    let class = m
        .classes
        .iter()
        .find(|c| c.is_literal_shape())
        .expect("a literal-shape class");
    let mut f = ElisionFacts::default();
    f.record_layouts.insert(
        0,
        FactsRecordLayout {
            class_id: Some(class.id),
            keys: class.fields.iter().map(|x| x.name.clone()).collect(),
            numeric: class.fields.iter().map(|_| false).collect(),
        },
    );
    for (name, opt) in names {
        f.record_locals
            .insert(let_id(m, name), FactsRecRep { tmpl: 0, opt: *opt });
    }
    f
}

/// `new __AnonShape_*` births and references of local `id` in `m`.
fn counts(m: &Module, id: Option<u32>) -> (usize, usize) {
    let (mut births, mut refs) = (0, 0);
    perry_hir::tdz_check::for_each_module_expr(m, &mut |e| match e {
        Expr::New { class_name, .. } if class_name.starts_with("__AnonShape_") => births += 1,
        Expr::LocalGet(x) | Expr::LocalSet(x, _) if Some(*x) == id => refs += 1,
        _ => {}
    });
    (births, refs)
}

fn rewritten(src: &str, names: &[(&str, bool)]) -> (Module, Module, Census, u32) {
    let m = lower(src);
    let id = let_id(&m, names[0].0);
    let (out, census, _) = rewrite(&m, &facts(&m, names)).expect("rewritten");
    (m, out, census, id)
}

const READS: &str = "function f(c: boolean): number { const u = { a: 1, b: c }; let s = 0; if (u.b) s += u.a; return s + u.a; }\nconsole.log(f(true));\n";

#[test]
fn field_reads_are_the_field_locals() {
    let (m, out, c, id) = rewritten(READS, &[("u", false)]);
    assert_eq!(counts(&m, Some(id)), (1, 3));
    // No birth and no reference of the binding is left.
    assert_eq!(counts(&out, Some(id)), (0, 0));
    assert_eq!(
        c,
        Census {
            locals: 1,
            field_reads: 3,
            ..Census::default()
        }
    );
    // Negative control: no facts, no rewrite.
    assert!(rewrite(&m, &ElisionFacts::default()).is_none());
}

#[test]
fn a_boundary_use_is_a_materialized_copy() {
    let src = "function f(): number { const u = { a: 1, b: 2 }; console.log(u); console.log(JSON.stringify(u)); return u.a; }\nconsole.log(f());\n";
    let (_, out, c, id) = rewritten(src, &[("u", false)]);
    // The two boundaries each take a fresh copy; the read takes its local.
    assert_eq!(counts(&out, Some(id)), (2, 0));
    assert_eq!((c.materialized, c.field_reads), (2, 1));
}

#[test]
fn a_captured_binding_stays_a_heap_object() {
    let src = "function f(): number { const u = { a: 1, b: 2 }; const g = () => u.a; return g(); }\nconsole.log(f());\n";
    let m = lower(src);
    assert!(rewrite(&m, &facts(&m, &[("u", false)])).is_none());
    // Negative control: without the closure it is rewritten.
    let src = "function f(): number { const u = { a: 1, b: 2 }; return u.a; }\nconsole.log(f());\n";
    let m = lower(src);
    assert!(rewrite(&m, &facts(&m, &[("u", false)])).is_some());
}

#[test]
fn an_optional_binding_tracks_presence() {
    let src = "function f(c: boolean): number { const u = c ? { a: 1, b: 2 } : undefined; if (u === undefined) return 0; if (!u) return 1; return u.a + (typeof u === \"object\" ? 1 : 0); }\nconsole.log(f(true), f(false));\n";
    let (_, out, c, id) = rewritten(src, &[("u", true)]);
    assert_eq!(counts(&out, Some(id)), (0, 0));
    assert_eq!(c.materialized, 0, "{c:?}");
    assert_eq!(c.unpacked, 0, "{c:?}");
    assert_eq!(c.field_reads, 1, "{c:?}");
    assert_eq!(c.presence, 3, "{c:?}");
}

#[test]
fn an_assignment_writes_the_fields() {
    // One literal site, assigned in a loop that reads the binding.
    let src = "function f(n: number): number { let u: { a: number; b: number } | undefined = undefined; let s = 0; for (let i = 0; i < n; i++) { u = { a: u ? u.b : 1, b: u ? u.a + i : 2 }; s += u.a; } return s; }\nconsole.log(f(4));\n";
    let (_, out, c, id) = rewritten(src, &[("u", true)]);
    assert_eq!(counts(&out, Some(id)).1, 0);
    // Every value is taken before a field is written: no copy, no birth,
    // and the statement's discarded value takes none either.
    assert_eq!((c.unpacked, c.materialized), (0, 0), "{c:?}");
    assert_eq!(counts(&out, None).0, 0);
    // Negative control: the assignment's value used (`(u = …).a`) is a copy.
    let src = "function f(n: number): number { let u: { a: number; b: number } | undefined = undefined; let s = 0; for (let i = 0; i < n; i++) { s += (u = { a: i, b: 2 }).a; } return s; }\nconsole.log(f(4));\n";
    let (_, _, c, _) = rewritten(src, &[("u", true)]);
    assert_eq!(c.materialized, 1, "{c:?}");
}

/// The module compiles with and without the rewrite (an IR smoke test of
/// every lowering the rewritten forms take).
#[test]
fn the_rewritten_module_compiles() {
    for src in [READS] {
        let m = lower(src);
        let compile = |elision: ElisionFacts| {
            let opts = CompileOptions {
                emit_ir_only: true,
                program_layouts: Arc::new(crate::layout_families::compute(
                    [("vr.ts", &m)],
                    true,
                    &Default::default(),
                )),
                program_facts: ProgramFactsView {
                    elision: Arc::new(elision),
                    ..Default::default()
                },
                ..Default::default()
            };
            String::from_utf8(compile_module(&m, opts).expect("compiles")).expect("UTF-8 IR")
        };
        let on = compile(facts(&m, &[("u", false)]));
        let off = compile(ElisionFacts::default());
        assert_ne!(on, off);
    }
}

// ---- VR-3b: record signature slots (`codegen::record_sig`) ----

use crate::codegen::{FactsRep, FactsSignature};

/// The id of the function named `name`, and its parameter ids.
fn function(m: &Module, name: &str) -> (u32, Vec<u32>) {
    let f = m
        .functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no function {name}"));
    (f.id, f.params.iter().map(|p| p.id).collect())
}

/// The byte offsets of the direct calls of function `fid`.
fn calls_of(m: &Module, fid: u32) -> Vec<u32> {
    let mut out = Vec::new();
    perry_hir::tdz_check::for_each_module_expr(m, &mut |e| {
        if let Expr::Call {
            callee,
            byte_offset,
            ..
        } = e
        {
            if matches!(callee.as_ref(), Expr::FuncRef(id) if *id == fid) {
                out.push(*byte_offset);
            }
        }
    });
    out
}

/// `facts` plus a typed signature for function `name`: its record
/// parameters (`params`: per parameter, a record of template 0 and whether
/// it may be absent) and result, every call of it a typed call.
fn with_signature(
    m: &Module,
    mut f: ElisionFacts,
    name: &str,
    params: &[Option<bool>],
    ret: Option<bool>,
) -> ElisionFacts {
    let (fid, ids) = function(m, name);
    assert_eq!(ids.len(), params.len());
    let rec = |opt: bool| FactsRecRep { tmpl: 0, opt };
    for (id, p) in ids.iter().zip(params) {
        if let Some(opt) = p {
            f.record_locals.insert(*id, rec(*opt));
        }
    }
    f.signatures.insert(
        fid,
        FactsSignature {
            params: params
                .iter()
                .map(|p| {
                    if p.is_some() {
                        FactsRep::Boxed
                    } else {
                        FactsRep::F64
                    }
                })
                .collect(),
            ranges: vec![None; params.len()],
            ret: if ret.is_some() {
                FactsRep::Boxed
            } else {
                FactsRep::F64
            },
            this_class: None,
            zero_ok: false,
            record_params: params.iter().map(|p| p.map(rec)).collect(),
            record_ret: ret.map(rec),
        },
    );
    for offset in calls_of(m, fid) {
        assert_ne!(offset, 0, "a call offset");
        f.typed_calls.insert(offset, fid);
    }
    for l in f.record_layouts.values_mut() {
        l.numeric = vec![true; l.keys.len()];
    }
    f
}

fn ir(m: &Module, elision: ElisionFacts) -> String {
    let opts = CompileOptions {
        emit_ir_only: true,
        program_layouts: Arc::new(crate::layout_families::compute(
            [("vr.ts", m)],
            true,
            &Default::default(),
        )),
        program_facts: ProgramFactsView {
            elision: Arc::new(elision),
            ..Default::default()
        },
        ..Default::default()
    };
    let text = String::from_utf8(compile_module(m, opts).expect("compiles")).expect("UTF-8 IR");
    if let Ok(dir) = std::env::var("PERRY_TEST_IR_OUT") {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::fs::write(format!("{dir}/vr-{}-{n}.ll", std::process::id()), &text).expect("writes");
    }
    text
}

/// The `define` line of the `$sig` whose symbol contains `name`.
fn sig_define<'a>(ir: &'a str, name: &str) -> Option<&'a str> {
    ir.lines()
        .find(|l| l.starts_with("define") && l.contains(name) && l.contains("$sig("))
}

const PARAM: &str = "function use(r: { a: number; b: number }, k: number): number { let s = 0; for (let i = 0; i < k; i++) s += r.a + r.b; return s; }\nfunction f(n: number): number { const u = { a: n, b: 2 }; return use(u, n) + use(u, 3); }\nconsole.log(f(1), f(2));\n";

#[test]
fn a_record_parameter_is_its_lanes() {
    let m = lower(PARAM);
    let base = facts(&m, &[("u", false)]);
    let on = with_signature(&m, base.clone(), "use", &[Some(false), None], None);
    let (out, c, abi) = rewrite(&m, &on).expect("rewritten");
    assert_eq!((c.params, c.passed, c.materialized), (1, 2, 0), "{c:?}");
    let (fid, ids) = function(&m, "use");
    let f = &abi.fns[&fid];
    // Two lane lets at the body's start, no presence lane.
    assert_eq!((f.prologue_at, f.prologue_len, f.ret), (0, 2, false));
    let lanes = f.params[0].as_ref().expect("the record parameter's lanes");
    assert_eq!((lanes.present, lanes.fields.len()), (None, 2));
    assert!(f.params[1].is_none());
    // The parameter is read only by the prologue.
    assert_eq!(counts(&out, Some(ids[0])).1, 2);
    let text = ir(&m, on);
    let define = sig_define(&text, "use").expect("use$sig");
    // Two Number lanes and `k`.
    assert_eq!(define.matches("double %arg").count(), 3, "{define}");
    // Negative control: without the signature the record crosses boxed (a
    // copy at each call), and there is no `$sig` with lanes.
    let (_, c, abi) = rewrite(&m, &base).expect("rewritten");
    assert_eq!((c.params, c.passed, c.materialized), (0, 0, 2), "{c:?}");
    assert!(abi.fns.is_empty());
    assert!(sig_define(&ir(&m, base), "use").is_none());
}

const RESULT: &str = "function mk(n: number) { if (n > 5) return { a: n, b: 0 }; return { a: n, b: n + 1 }; }\nfunction f(n: number): number { const v = mk(n); return v.a + v.b + mk(n + 1).a; }\nconsole.log(f(1), f(7));\n";

#[test]
fn a_record_result_is_an_aggregate() {
    let m = lower(RESULT);
    let base = facts(&m, &[("v", false)]);
    let on = with_signature(&m, base.clone(), "mk", &[None], Some(false));
    let (_, c, abi) = rewrite(&m, &on).expect("rewritten");
    assert_eq!((c.returns, c.calls, c.materialized), (1, 2, 0), "{c:?}");
    assert_eq!(abi.aggs.len(), 2);
    let text = ir(&m, on);
    let define = sig_define(&text, "mk").expect("mk$sig");
    assert!(define.contains("[3 x double]"), "{define}");
    assert!(text.contains("insertvalue [3 x double]"));
    assert!(text.contains("extractvalue [3 x double]"));
    // The boxed entry of a record-returning function has no caller.
    assert!(text.contains("call void @llvm.trap()"));
    // Negative control: without the signature the result is a boxed object,
    // read key by key.
    let (_, c, _) = rewrite(&m, &base).expect("rewritten");
    assert_eq!((c.returns, c.calls), (0, 0), "{c:?}");
    assert!(c.unpacked >= 1, "{c:?}");
    assert!(!ir(&m, base).contains("[3 x double]"));
}

#[test]
fn an_optional_record_parameter_has_a_presence_lane() {
    let src = "function use(r: { a: number; b: number } | undefined): number { return r === undefined ? 0 : r.a + r.b; }\nfunction f(n: number): number { const u = n > 1 ? { a: n, b: 2 } : undefined; return use(u) + use(undefined); }\nconsole.log(f(1), f(2));\n";
    let m = lower(src);
    let on = with_signature(&m, facts(&m, &[("u", true)]), "use", &[Some(true)], None);
    let (_, c, abi) = rewrite(&m, &on).expect("rewritten");
    assert_eq!((c.params, c.passed, c.materialized), (1, 1, 0), "{c:?}");
    let (fid, _) = function(&m, "use");
    assert!(abi.fns[&fid].params[0]
        .as_ref()
        .is_some_and(|l| l.present.is_some()));
    let text = ir(&m, on);
    let define = sig_define(&text, "use").expect("use$sig");
    assert_eq!(define.matches("double %arg").count(), 3, "{define}");
    // The boxed entry admits `undefined` only.
    assert!(text.contains("signature.record_absent"));
    assert!(text.contains("call void @llvm.trap()"));
}

#[test]
fn a_self_recursive_record_function_has_a_record_sig() {
    let src = "function use(r: { a: number; b: number }, d: number): number { if (d > 0) return use(r, d - 1); return r.a + r.b; }\nfunction mk(n: number, d: number): { a: number; b: number } { if (d > 0) return mk(n + 1, d - 1); return { a: n, b: 2 }; }\nfunction f(n: number): number { const u = mk(n, 1); return use(u, 2); }\nconsole.log(f(1), f(2));\n";
    let m = lower(src);
    let on = with_signature(
        &m,
        facts(&m, &[("u", false)]),
        "use",
        &[Some(false), None],
        None,
    );
    let on = with_signature(&m, on, "mk", &[None, None], Some(false));
    let (_, c, _) = rewrite(&m, &on).expect("rewritten");
    assert_eq!((c.params, c.returns, c.materialized), (1, 1, 0), "{c:?}");
    let text = ir(&m, on);
    let define = sig_define(&text, "use").expect("use$sig");
    assert_eq!(define.matches("double %arg").count(), 3, "{define}");
    let define = sig_define(&text, "mk").expect("mk$sig");
    assert!(define.contains("[3 x double]"), "{define}");
}

/// The instructions that birth an instance of a literal-shape class.
fn births(ir: &str) -> Vec<&str> {
    ir.lines()
        .filter(|l| {
            l.starts_with("  ")
                && (l.contains("perry_class_header_image") || l.contains("@js_object_alloc_class"))
        })
        .collect()
}

/// Every form a record takes around a typed call lowers: a call's result
/// passed straight to a record slot, a conditional of calls returned, an
/// optional result tested and read, a parameter reassigned in the callee, a
/// discarded call, a call in a loop condition and a field of a call.
#[test]
fn the_record_call_forms_compile() {
    let src = "function mk(n: number, d: number): { a: number; b: number } { if (d > 0) return mk(n + 1, d - 1); return { a: n, b: 2 }; }\n\
function pick(n: number): { a: number; b: number } { return n > 2 ? mk(n, 0) : mk(n, 1); }\n\
function maybe(n: number): { a: number; b: number } | undefined { if (n > 3) return undefined; if (n > 2) return; return mk(n, 0); }\n\
function use(r: { a: number; b: number }, d: number): number { if (d > 0) { r = mk(r.a, 0); return use(r, d - 1); } return r.a + r.b; }\n\
function f(n: number): number {\n  let s = use(mk(n, 1), 2) + use(pick(n), 0) + mk(n, 2).b;\n  mk(n, 0);\n  const v = maybe(n);\n  if (v !== undefined) s += v.a;\n  let w = mk(n, 0);\n  while (mk(s, 0).a < 10) { s += w.b; w = pick(s); }\n  return s + w.a;\n}\n\
console.log(f(1), f(3), f(5));\n";
    let m = lower(src);
    let mut on = facts(&m, &[("v", true), ("w", false)]);
    for (name, params, ret) in [
        ("mk", vec![None, None], Some(false)),
        ("pick", vec![None], Some(false)),
        ("maybe", vec![None], Some(true)),
        ("use", vec![Some(false), None], None),
    ] {
        on = with_signature(&m, on, name, &params, ret);
    }
    let (_, c, abi) = rewrite(&m, &on).expect("rewritten");
    assert_eq!((c.params, c.returns), (1, 3), "{c:?}");
    assert_eq!(c.materialized, 0, "{c:?}");
    assert_eq!(c.unpacked, 0, "{c:?}");
    assert!(abi.fns.len() == 4 && c.calls >= 12, "{c:?}");
    let text = ir(&m, on);
    for name in ["mk", "pick", "maybe"] {
        let define = sig_define(&text, name).unwrap_or_else(|| panic!("{name}$sig"));
        assert!(define.contains("[3 x double]"), "{define}");
    }
    assert!(sig_define(&text, "use").is_some());
    // No copy is born anywhere: every record crosses as lanes.
    assert_eq!(births(&text), Vec::<&str>::new(), "a copy is born");
    // Negative control: without the signatures the literal is born.
    let off = ir(&m, facts(&m, &[("v", true), ("w", false)]));
    assert!(!births(&off).is_empty());
}
