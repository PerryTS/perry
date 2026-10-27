//! DECISIONS 210 (MBW, 2026-10-26): a constructor function another unit
//! constructs. Found by CJL as "a value written into a module binding from
//! a function unit is constructed by another unit without facts": the
//! function local to `make`, published through the binding `rec` that
//! `make` writes, constructed in another module, had `raw` of no kind per
//! unit (a number whole program). The channel that lost it: the reader's
//! `new` of the stand-in made an instance of the stand-in's own `new <f>`
//! template and sank it as the receiver; the instance's escape escaped its
//! constructor (the `.constructor` rule), the census reported the function
//! broken, the owner kept a template nobody constructed. The fix is the
//! class mechanism for constructor functions: a `new` of another unit's
//! contract function constructs its owner's `new <f>` layout; the
//! constructing unit publishes what it passes (`ctor_runs_out`), the owner
//! constructs with the union and publishes the layout; a broken function
//! is constructed by unknown code (unknown arguments, escaped instance).
//! Witnesses on qb9: /root/CJL/tmp/rep2 (ESM) and /root/CJL/tmp/rep (CJS).

use super::*;
use perry_hir::Module;

fn hirs(mods: &[(&str, &str)]) -> Vec<(String, Module)> {
    mods.iter()
        .map(|(name, src)| {
            let path = format!("{name}.ts");
            let module = perry_parser::parse_typescript(src, &path).expect("parses");
            let mut hir = perry_hir::lower_module(&module, name, &path).expect("lowers");
            for imp in &mut hir.imports {
                if let Some(n) = imp.source.strip_prefix("./") {
                    imp.resolved_path = Some(format!("{n}.ts"));
                }
            }
            (path, hir)
        })
        .collect()
}

fn run(h: &[(String, Module)], unit: bool) -> ProgramFacts {
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    // The per-unit dump carries the units' outputs and reports (`# uout`).
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    match unit {
        true => crate::compute_units(&list, &cfg),
        false => crate::compute(&list, &cfg),
    }
}

/// The one constructor-function template (`new <module>:<name>…`) of
/// function `name`.
fn fninst<'a>(pf: &'a ProgramFacts, name: &str) -> &'a crate::TemplateFacts {
    let tag = format!(":{name}");
    let mut it = pf.templates.iter().filter(|t| {
        t.kind == "fninst" && t.display.starts_with("new ") && t.display.contains(&tag)
    });
    let t = it
        .next()
        .unwrap_or_else(|| panic!("new ..:{name} in\n{}", pf.dump_text()));
    assert!(it.next().is_none(), "one template of {name}:\n{}", pf.dump_text());
    t
}

fn kind_text(k: &crate::derive::KindFact) -> String {
    if k.top {
        return "top".into();
    }
    format!(
        "prims={:#x} objs={:?} funcs={} builtins={}",
        k.prims, k.objs, k.funcs, k.builtin_objs
    )
}

/// The claims the per-unit template makes that the whole-program one does
/// not (what `tdiff.py` counts): the owner's rule is that there are none.
fn stronger(u: &crate::TemplateFacts, w: &crate::TemplateFacts) -> Vec<String> {
    let mut out = Vec::new();
    if u.prefix_stable && !w.prefix_stable {
        out.push("prefix_stable".into());
    }
    if u.fixed_layout && !w.fixed_layout {
        out.push("fixed_layout".into());
    }
    if u.escape.is_none() && w.escape.is_some() {
        out.push(format!("escape u=none w={:?}", w.escape));
    }
    for fu in &u.fields {
        let Some(fw) = w.fields.iter().find(|f| f.key == fu.key) else {
            continue;
        };
        for (what, a, b) in [
            ("readonly", fu.readonly, fw.readonly),
            ("definite", fu.definite, fw.definite),
            ("ta_stable", fu.ta_stable, fw.ta_stable),
        ] {
            if a && !b {
                out.push(format!("{} {what}", fu.key));
            }
        }
        let (ku, kw) = (&fu.kind, &fw.kind);
        let kind_stronger = !ku.top
            && (kw.top
                || kw.prims & !ku.prims != 0
                || kw.objs.iter().any(|o| !ku.objs.contains(o))
                || (kw.funcs > 0 && ku.funcs == 0)
                || (kw.builtin_objs > 0 && ku.builtin_objs == 0));
        if kind_stronger {
            out.push(format!(
                "{} kind u={} w={}",
                fu.key,
                kind_text(ku),
                kind_text(kw)
            ));
        }
    }
    out
}

/// Per unit never stronger than whole program on `new ..:name`, and the
/// construction facts carried: `raw`, the constructor's store of its
/// argument, holds a number in both (not nothing, not anything).
fn constructed_alike(h: &[(String, Module)], name: &str) {
    let (u, w) = (run(h, true), run(h, false));
    let (tu, tw) = (fninst(&u, name), fninst(&w, name));
    let diff = stronger(tu, tw);
    assert!(
        diff.is_empty(),
        "per unit stronger: {diff:?}\nper unit:\n{}\nwhole program:\n{}",
        u.dump_text(),
        w.dump_text()
    );
    let raw = |t: &crate::TemplateFacts| {
        t.fields
            .iter()
            .find(|f| f.key == "raw")
            .map(|f| f.kind.clone())
            .unwrap_or_else(|| panic!("{}.raw", t.display))
    };
    let (ru, rw) = (raw(tu), raw(tw));
    assert!(
        !rw.top && rw.prims & crate::model::prim::NUM != 0,
        "whole program proves raw a number:\n{}",
        w.dump_text()
    );
    assert_eq!(
        kind_text(&ru),
        kind_text(&rw),
        "the construction reaches the owner's layout per unit:\n{}\nwhole program:\n{}",
        u.dump_text(),
        w.dump_text()
    );
}

#[test]
fn a_value_written_into_a_module_binding_from_a_function_unit_is_constructed_by_another_unit_with_its_facts(
) {
    let a = r#"
        export var rec: any;
        function make() {
            function Reply(this: any, res: any) { this.raw = res; }
            rec = { exports: Reply };
        }
        make();
        make();
    "#;
    let main = r#"
        import { rec } from "./a";
        const R = rec.exports;
        const r = new R(2);
        console.log(r.raw);
    "#;
    constructed_alike(&hirs(&[("a", a), ("main", main)]), "Reply");
}

#[test]
fn the_commonjs_record_of_an_unflattened_factory_is_constructed_by_another_unit_with_its_facts(
) {
    // The wrapper's shape in a program that is not closed: the record binding
    // is written by the factory, which the run-once view does not flatten
    // (held beyond its record).
    let a = r#"
        export var __perry_cjs_record: any;
        function __perry_cjs_factory() {
            function Reply(this: any, res: any) { this.raw = res; }
            __perry_cjs_record = { exports: Reply, loaded: false };
            return __perry_cjs_record.exports;
        }
        __perry_cjs_factory();
        __perry_cjs_factory();
    "#;
    let main = r#"
        import { __perry_cjs_record as __perry_cjs_rec_0 } from "./a";
        const R = __perry_cjs_rec_0.exports;
        const r = new R(2);
        console.log(r.raw);
    "#;
    constructed_alike(&hirs(&[("a", a), ("main", main)]), "Reply");
}

#[test]
fn an_exported_constructor_function_constructed_by_another_module_has_its_construction_facts() {
    // The same channel without any binding write: the function itself is
    // the export. Before DECISIONS 210 the reader's `new` broke it per unit.
    let a = r#"
        export function Reply(this: any, res: any) { this.raw = res; this.request = null; }
    "#;
    let main = r#"
        import { Reply } from "./a";
        const r = new Reply(2);
        console.log(r.raw);
    "#;
    constructed_alike(&hirs(&[("a", a), ("main", main)]), "Reply");
}

#[test]
fn a_method_of_the_constructed_instance_runs_on_it_in_the_owner() {
    // The prototype's functions are the layout's methods: the reader's call
    // runs the owner's `send` on the instance, whose post-birth store
    // (`sent`) is a writer of the owner's layout, as in the whole program.
    let a = r#"
        export var rec: any;
        function make() {
            function Reply(this: any, res: any) { this.raw = res; }
            Reply.prototype.send = function (this: any) { this.sent = true; return this.raw; };
            rec = { exports: Reply };
        }
        make();
        make();
    "#;
    let main = r#"
        import { rec } from "./a";
        const R = rec.exports;
        const r = new R(2);
        console.log(r.send());
    "#;
    let h = hirs(&[("a", a), ("main", main)]);
    constructed_alike(&h, "Reply");
    let u = run(&h, true);
    let t = fninst(&u, "Reply");
    assert!(
        t.fields.iter().any(|f| f.key == "sent"),
        "send ran on the instance per unit:\n{}",
        u.dump_text()
    );
}

#[test]
fn a_store_into_another_units_constructed_instance_breaks_its_layout() {
    // A reader's use beyond the layout is its census report: the owner's
    // instance escapes, so the layout claims nothing the store refutes.
    let a = r#"
        export var rec: any;
        function make() {
            function Reply(this: any, res: any) { this.raw = res; }
            rec = { exports: Reply };
        }
        make();
        make();
    "#;
    let main = r#"
        import { rec } from "./a";
        const R = rec.exports;
        const r = new R(2);
        r.extra = 1;
        console.log(r.raw, r.extra);
    "#;
    let h = hirs(&[("a", a), ("main", main)]);
    let (u, w) = (run(&h, true), run(&h, false));
    let (tu, tw) = (fninst(&u, "Reply"), fninst(&w, "Reply"));
    let diff = stronger(tu, tw);
    assert!(
        diff.is_empty(),
        "per unit stronger: {diff:?}\nper unit:\n{}\nwhole program:\n{}",
        u.dump_text(),
        w.dump_text()
    );
    assert!(
        u.broken.iter().any(|d| d == &tu.display),
        "the store is reported against the layout:\n{}",
        u.dump_text()
    );
}

#[test]
fn a_broken_constructor_function_is_constructed_by_unknown_code() {
    // A reader uses the function beyond its contract (hands it to unknown
    // code): the census breaks it, unknown code holds it and may construct
    // it with anything. The owner's layout then claims no kind nobody wrote:
    // constructed with unknown arguments, the instance escaped.
    let a = r#"
        export var rec: any;
        function make() {
            function Reply(this: any, res: any) { this.raw = res; }
            rec = { exports: Reply };
        }
        make();
        make();
    "#;
    let main = r#"
        import { rec } from "./a";
        const R = rec.exports;
        (globalThis as any).keep = R;
        const r = new R(2);
        console.log(r.raw);
    "#;
    let h = hirs(&[("a", a), ("main", main)]);
    let (u, w) = (run(&h, true), run(&h, false));
    let (tu, tw) = (fninst(&u, "Reply"), fninst(&w, "Reply"));
    assert!(
        u.broken.iter().any(|d| d.starts_with("fn:") && d.contains(":Reply")),
        "unknown code holding it breaks the function:\n{}",
        u.dump_text()
    );
    let diff = stronger(tu, tw);
    assert!(
        diff.is_empty(),
        "per unit stronger: {diff:?}\nper unit:\n{}\nwhole program:\n{}",
        u.dump_text(),
        w.dump_text()
    );
    let raw = tu.fields.iter().find(|f| f.key == "raw").expect("raw");
    assert!(
        raw.kind.top || raw.kind.prims & crate::model::prim::NUM != 0,
        "raw is never silently empty per unit:\n{}",
        u.dump_text()
    );
}

#[test]
fn a_property_stored_into_another_units_constructor_function_breaks_nothing() {
    // `R.tag = 1` writes a property of the function, not its layout: the
    // owner let its properties (all but `prototype`) leave with it, so the
    // store is no census report and the construction keeps its facts (the
    // CommonJS wrapper's `require.resolve = resolve` across units).
    let a = r#"
        export var rec: any;
        function make() {
            function Reply(this: any, res: any) { this.raw = res; }
            rec = { exports: Reply };
        }
        make();
        make();
    "#;
    let main = r#"
        import { rec } from "./a";
        const R = rec.exports;
        R.tag = 1;
        const r = new R(2);
        console.log(r.raw, R.tag);
    "#;
    let h = hirs(&[("a", a), ("main", main)]);
    let u = run(&h, true);
    assert!(
        !u.broken.iter().any(|d| d.contains(":Reply")),
        "an expando store breaks nothing:\n{}",
        u.dump_text()
    );
    constructed_alike(&h, "Reply");
}

#[test]
fn a_store_into_another_units_constructor_function_prototype_breaks_it() {
    // `R.prototype.x = ...` before the owner published the layout (or a
    // replaced `R.prototype`) changes the chain the owner's instances
    // have: the census reports it and the owner's layout claims nothing
    // the store refutes.
    let a = r#"
        export var rec: any;
        function make() {
            function Reply(this: any, res: any) { this.raw = res; }
            rec = { exports: Reply };
        }
        make();
        make();
    "#;
    let main = r#"
        import { rec } from "./a";
        const R = rec.exports;
        R.prototype = { raw: "s" };
        const r = new R(2);
        console.log(r.raw);
    "#;
    let h = hirs(&[("a", a), ("main", main)]);
    let (u, w) = (run(&h, true), run(&h, false));
    assert!(
        u.broken.iter().any(|d| d.starts_with("fn:") && d.contains(":Reply")),
        "replacing the prototype breaks the function:\n{}",
        u.dump_text()
    );
    let diff = stronger(fninst(&u, "Reply"), fninst(&w, "Reply"));
    assert!(
        diff.is_empty(),
        "per unit stronger: {diff:?}\nper unit:\n{}\nwhole program:\n{}",
        u.dump_text(),
        w.dump_text()
    );
}

/// Per unit never stronger than whole program on `new ..:name`, the layout
/// stays described (no census report against it), and field `key` carries
/// the kinds the whole program finds.
fn field_alike(h: &[(String, Module)], name: &str, key: &str) {
    let (u, w) = (run(h, true), run(h, false));
    let (tu, tw) = (fninst(&u, name), fninst(&w, name));
    let diff = stronger(tu, tw);
    assert!(
        diff.is_empty(),
        "per unit stronger: {diff:?}\nper unit:\n{}\nwhole program:\n{}",
        u.dump_text(),
        w.dump_text()
    );
    let kind = |pf: &ProgramFacts, t: &crate::TemplateFacts| {
        t.fields
            .iter()
            .find(|f| f.key == key)
            .map(|f| kind_text(&f.kind))
            .unwrap_or_else(|| panic!("{}.{key}:\n{}", t.display, pf.dump_text()))
    };
    assert_eq!(
        kind(&u, tu),
        kind(&w, tw),
        "{key} per unit as whole program:\nper unit:\n{}\nwhole program:\n{}",
        u.dump_text(),
        w.dump_text()
    );
}

#[test]
fn an_instance_handed_back_to_its_owner_is_the_owners_own_layout() {
    // Another unit (`hold`) constructs the owner's function and writes the
    // instance into a module binding the owner's unit (`make`'s) reads: the
    // binding's value contract describes the `new Reply` layout, which in
    // its owner is its own template (`Gen::own_layout`), whose facts this
    // solve makes. `bump`'s store `n` is a writer of the owner's layout, as
    // in the whole program; into a stand-in of its own layout it would be
    // a census report against the layout.
    let a = r#"
        export var rec: any;
        export var held: any;
        function make() {
            function Reply(this: any, res: any) { this.raw = res; }
            rec = { exports: Reply, bump: function () { held.n = 1; } };
        }
        function hold() {
            held = new rec.exports(2);
            rec.bump();
        }
        make();
        make();
        hold();
    "#;
    let main = r#"
        import { held } from "./a";
        console.log(held.raw);
    "#;
    let h = hirs(&[("a", a), ("main", main)]);
    let u = run(&h, true);
    assert!(
        !u.broken.iter().any(|d| d.contains(":Reply")),
        "the owner's store into its own layout breaks nothing:\n{}",
        u.dump_text()
    );
    field_alike(&h, "Reply", "n");
}
