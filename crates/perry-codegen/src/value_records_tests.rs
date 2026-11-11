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
    let (out, census) = rewrite(&m, &facts(&m, names)).expect("rewritten");
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
