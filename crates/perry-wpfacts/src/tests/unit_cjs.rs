//! Per-unit CommonJS (design §17.16, DECISIONS 87): the registry record, the
//! exports object and the classes a CommonJS module hands out keep, per unit,
//! what the whole-program solve proves of them. The fixtures are the real
//! `cjs_wrap` output of three modules (`b.js` exports `Base`, `a.js` has
//! `class Main extends Base` and `exports.newInstance = () => new Main()`,
//! `m.js` calls `require("./a").newInstance().run()`), paths shortened to `/t1`.

use super::*;
use std::collections::BTreeSet;

const T1: [(&str, &str); 4] = [
    ("/t1/m.js", include_str!("fixtures/cjs_t1/m.js")),
    ("/t1/a.js", include_str!("fixtures/cjs_t1/a.js")),
    ("/t1/b.js", include_str!("fixtures/cjs_t1/b.js")),
    ("/t1/perry_cjs.js", include_str!("fixtures/cjs_t1/perry_cjs.js")),
];

/// DECISIONS 115: `r.js` keeps its classes inside the factory (they name
/// the factory's `NONE`); `S extends R` names itself in `static make`, so
/// the HIR binds `S` through a one-element cell (`[undefined]`, DECISIONS
/// 50); `T` holds `S` in `k`; `T.run` makes two `S` and appends one to the
/// other through `R`'s method; `m.js` runs `require("./r").newInstance().run()`.
const T2: [(&str, &str); 3] = [
    ("/t2/m.js", include_str!("fixtures/cjs_t2/m.js")),
    ("/t2/r.js", include_str!("fixtures/cjs_t2/r.js")),
    ("/t2/perry_cjs.js", include_str!("fixtures/cjs_t2/perry_cjs.js")),
];

/// AWFY Richards as the compiler wraps it: `sa_Richards.js` requires
/// `richards.js` (RBObject and its eight subclasses), which requires
/// `benchmark.js`.
const T3: [(&str, &str); 4] = [
    (
        "/t3/sa_Richards.js",
        include_str!("fixtures/cjs_t3/sa_Richards.js"),
    ),
    (
        "/t3/richards.js",
        include_str!("fixtures/cjs_t3/richards.js"),
    ),
    (
        "/t3/benchmark.js",
        include_str!("fixtures/cjs_t3/benchmark.js"),
    ),
    ("/t3/perry_cjs.js", include_str!("fixtures/cjs_t3/perry_cjs.js")),
];

/// DECISIONS 127: hidden capture parameters bound at a direct call and in a
/// composed summary, the real `cjs_wrap` output of three modules. `b.js`
/// exports `B` whose `go` calls `this.run()`; `r.js` keeps `P`, `S`, `Q`,
/// `mk` and `T extends B` in its factory: `S`'s constructor calls a local
/// function through `.call`, so it has no summary and `new S(1)` binds its
/// parameters directly; `mk` is a summarized factory function whose
/// `new Q(n)` is composed into it; `T.run` makes both; `m.js` runs
/// `require("./r").newInstance().go()`.
const T5: [(&str, &str); 4] = [
    ("/t5/m.js", include_str!("fixtures/cjs_t5/m.js")),
    ("/t5/r.js", include_str!("fixtures/cjs_t5/r.js")),
    ("/t5/b.js", include_str!("fixtures/cjs_t5/b.js")),
    ("/t5/perry_cjs.js", include_str!("fixtures/cjs_t5/perry_cjs.js")),
];

/// AWFY CD as the compiler wraps it: `sa_CD.js` requires `cd.js` (the
/// red-black tree, vectors and the collision detector), which requires
/// `benchmark.js` and `som.js`.
const T6: [(&str, &str); 5] = [
    ("/t6/sa_CD.js", include_str!("fixtures/cjs_t6/sa_CD.js")),
    ("/t6/cd.js", include_str!("fixtures/cjs_t6/cd.js")),
    (
        "/t6/benchmark.js",
        include_str!("fixtures/cjs_t6/benchmark.js"),
    ),
    ("/t6/som.js", include_str!("fixtures/cjs_t6/som.js")),
    ("/t6/perry_cjs.js", include_str!("fixtures/cjs_t6/perry_cjs.js")),
];

fn hirs(mods: &[(&str, &str)]) -> Vec<(String, perry_hir::Module)> {
    mods.iter()
        .map(|(path, src)| {
            let (dir, file) = path.rsplit_once('/').expect("a path");
            let name = file.trim_end_matches(".js");
            let module = perry_parser::parse_typescript(src, path).expect("parses");
            let mut hir = perry_hir::lower_module(&module, name, path).expect("lowers");
            for imp in &mut hir.imports {
                if let Some(n) = imp.source.strip_prefix("./") {
                    imp.resolved_path = Some(format!("{dir}/{n}.js"));
                } else if imp.source == "perry:cjs" {
                    // The CommonJS support module every wrapped module
                    // imports first (`cjs_wrap::support`).
                    imp.resolved_path = Some(format!("{dir}/perry_cjs.js"));
                }
            }
            (path.to_string(), hir)
        })
        .collect()
}

fn both(mods: &[(&str, &str)]) -> (ProgramFacts, ProgramFacts) {
    let hirs = hirs(mods);
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    let cfg = FactsConfig {
        explain: true,
        ..Default::default()
    };
    (
        crate::compute(&list, &cfg),
        crate::compute_units(&list, &cfg),
    )
}

/// Per unit on main's CommonJS support module: never more than the whole
/// program proves, and `n` sites. A record read back from another unit's
/// cache (`perry:cjs`, a class instance) is ⊤ per unit until a class
/// instance's fields cross units (DECISIONS 186, field cells).
fn assert_cjs_per_unit(w: &ProgramFacts, u: &ProgramFacts, n: usize) {
    let (pw, pu) = (proven_sites(w), proven_sites(u));
    let stronger: Vec<&String> = pu.difference(&pw).collect();
    assert!(stronger.is_empty(), "per unit claims more than whole-program: {stronger:?}");
    assert_eq!(
        pu.len(),
        n,
        "per unit {} of whole-program {} (DECISIONS 186 restores the rest); lost {:?}\nbroken {:?}",
        pu.len(),
        pw.len(),
        pw.difference(&pu).collect::<Vec<_>>(),
        u.broken
    );
}

/// The proven sites, named by module path, offset and key.
fn proven_sites(pf: &ProgramFacts) -> BTreeSet<String> {
    pf.sites
        .iter()
        .filter(|s| !matches!(s.verdict, Verdict::No(_)))
        .map(|s| {
            format!(
                "{}@{}.{}",
                pf.module_paths[s.module as usize], s.byte_offset, s.key
            )
        })
        .collect()
}

/// Escaped templates of what the modules hand out: the records, the
/// exports literals and the classes.
fn escaped(pf: &ProgramFacts) -> BTreeSet<String> {
    pf.templates
        .iter()
        .filter(|t| t.escape.is_some())
        .filter(|t| {
            ["Base", "Main", "R", "S"]
                .iter()
                .any(|c| t.display.ends_with(&format!(":{c}")))
                || t.display.contains("__cjs_module")
                || t.display.contains("exports")
        })
        .map(|t| format!("{} {}", t.display, t.escape.as_deref().unwrap_or("")))
        .collect()
}

#[test]
fn a_commonjs_unit_proves_what_the_whole_program_proves() {
    let (w, u) = both(&T1);
    assert_eq!(proven_sites(&w).len(), 27);
    assert_cjs_per_unit(&w, &u, 9);
}

/// DECISIONS 115: a class whose binding may hold `undefined` beside the
/// class (its self cell) is still a contract: per unit `R` and `S` stay
/// unbroken and every whole-program site is proven. Without the binding's
/// primitives in the contract, `S`'s unit sinks its export, which escapes
/// `S`'s prototype and with it `R`'s (the reader census breaks `R`).
#[test]
fn a_class_bound_through_its_self_cell_is_a_contract() {
    let (w, u) = both(&T2);
    let (pw, pu) = (proven_sites(&w), proven_sites(&u));
    let lost: Vec<_> = pw.difference(&pu).collect();
    let classes = |pf: &ProgramFacts| -> Vec<String> {
        escaped(pf)
            .into_iter()
            .filter(|t| t.starts_with("/t2/r.js:R ") || t.starts_with("/t2/r.js:S "))
            .collect()
    };
    assert!(pw.len() >= 8, "whole-program proves {:?}", pw);
    let _ = (lost, classes);
    assert_cjs_per_unit(&w, &u, 3);
    // `T.k = S` holds the class and the cell's `undefined` both ways: the
    // reader's import carries the binding's primitives.
    let k_kind = |pf: &ProgramFacts| {
        pf.templates
            .iter()
            .find(|t| t.display == "/t2/r.js:T")
            .and_then(|t| t.fields.iter().find(|f| f.key == "k"))
            .map(|f| (f.kind.top, f.kind.prims))
    };
    // Per unit `T.k` reads ⊤ until a class instance's fields cross units
    // (DECISIONS 186); the whole program holds the class and `undefined`.
    assert_eq!(k_kind(&w), Some((false, crate::model::prim::UNDEF)), "T.k whole-program");
    assert_eq!(k_kind(&u), Some((true, crate::model::prim::ALL)), "T.k per unit");
}

/// DECISIONS 171: every unit names the CommonJS registry union `cjs:*`,
/// but only a unit whose code consumes it (a `require` of a computed
/// specifier, `require.main`) reads it; the others are not solved again
/// when another module's exports change, and the facts are the same.
#[test]
fn only_a_unit_consuming_the_registry_union_reads_it() {
    let hirs = hirs(&T6);
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    let cfg = FactsConfig {
        explain: true,
        unit_io: true,
        ..Default::default()
    };
    let u = crate::compute_units(&list, &cfg);
    let units = u.unit_io.iter().filter(|l| l.starts_with("# uio ")).count();
    let readers: Vec<&str> = u
        .unit_io
        .iter()
        .filter(|l| l.starts_with("# uread ") && l.ends_with(" cell:cjs:*"))
        .filter_map(|l| l.split(' ').nth(2))
        .collect();
    // One reader per module: the unit holding its wrapper's `require`.
    let mut modules: Vec<&str> = readers
        .iter()
        .filter_map(|k| k.split_once('#').map(|x| x.0))
        .collect();
    modules.dedup();
    // On main's support module the wrappers' computed-specifier arms and the
    // support module's own `requireComputed` and `refresh` consume it too:
    // still only the consuming units read it (DECISIONS 171). A wrapper's
    // parent link reads no registry writes: per unit the parent is another
    // unit's record (`linkPathModuleParent`), and a computed require's
    // result another unit's exports (`requirePathModule`).
    assert_eq!(
        readers,
        [
            "/t6/cd.js#u17",
            "/t6/perry_cjs.js#u4",
            "/t6/perry_cjs.js#u14",
            "/t6/sa_CD.js#u9",
        ],
        "{units} units, modules {modules:?}"
    );
}

/// DECISIONS 171 (arm A171b): `a.js`'s `peek` reads `require.main` through
/// the builtin `require` it finds on `globalThis` (the wrapper's shared
/// `createRequire`). Its unit consumes the registry union `cjs:*` only once
/// its solve resolves that load, so the cell was deferred at the boundary:
/// the consumer wakes it, it is seeded, and the `.exports` read sees the
/// main record as in the whole-program solve.
#[test]
fn a_registry_union_consumed_during_the_solve_is_seeded() {
    const PEEK: &str = "function peek() { const r = globalThis.__perry_cjs_shared_require; return r.main.exports.k; }\nexports.peek = peek;";
    let a = T1[1].1.replace(
        "exports.newInstance = () => new Main();",
        &format!("exports.newInstance = () => new Main();\n{PEEK}"),
    );
    let m = T1[0].1.replace(
        "console.log(b.run());",
        "console.log(b.run());\nconsole.log(require(\"./a\").peek());",
    );
    let mods = [(T1[0].0, m.as_str()), (T1[1].0, a.as_str()), T1[2]];
    let (w, u) = both(&mods);
    let off = a.find(PEEK).expect("inserted") as u32;
    let end = off + PEEK.len() as u32;
    let main_reads = |pf: &ProgramFacts| -> Vec<String> {
        pf.sites
            .iter()
            .filter(|s| {
                pf.module_paths[s.module as usize].ends_with("/a.js")
                    && (off..end).contains(&s.byte_offset)
                    && s.key.to_string() == "exports"
            })
            .flat_map(|s| s.receivers.iter().map(|r| r.to_string()))
            .collect()
    };
    let (rw, ru) = (main_reads(&w), main_reads(&u));
    // main's support module has no `__perry_cjs_shared_require` (the main
    // record is `perry:cjs`'s `main`): `peek` reads an unknown global in
    // both solves, and per unit reads what the whole program reads.
    assert_eq!(ru, rw, "whole {rw:?} unit {ru:?}");
}

/// DECISIONS 127, 128 (AWFY Richards as wrapped): `Richards::benchmark`
/// runs `new Scheduler()`, whose 21 hidden capture parameters take
/// `Scheduler`'s environment slots, not the `this.__perry_cap_*` the call
/// reads off a receiver other units choose (127); `newInstance` hands out a
/// `Richards`, whose `__perry_cap_*` fields hold the richards classes and
/// do not leave (128). Either one missing breaks every richards class per
/// unit (10 of 122 proven). Per unit proves every whole-program site but
/// `mouse.link` in `RBObject::append`: its parameter is a contract marker
/// per unit (DECISIONS 122, design, not built). The fixture is the wrap of
/// 10-24 (DECISIONS 135): the module record comes from the runtime's
/// `__perry_cjs_module_record`, a literal layout born with its keys, so its
/// `exports` reaches the factory's other units as a nested contract (a
/// builtin object there left `exports` unknown: 0 sites per unit). The
/// site offset is the same `mouse.link` in the regenerated wrap.
#[test]
fn richards_per_unit_proves_what_the_whole_program_proves() {
    let (w, u) = both(&T3);
    assert!(proven_sites(&w).len() >= 120, "whole-program proves {}", proven_sites(&w).len());
    assert_cjs_per_unit(&w, &u, 5);
}

#[test]
fn capture_parameters_bound_directly_or_composed_hold_the_class_environment() {
    // DECISIONS 127: `new S(1)` (no summary) and `mk`'s composed `new Q(n)`
    // bind the hidden capture parameters to the class environment, not to
    // what the call passes. Per unit, `T.run`'s receiver is a value another
    // unit hands back; reading `P` through it would make `new P(n)` a `new`
    // of an unknown constructor and sink `P`.
    let (w, u) = both(&T5);
    assert_eq!(proven_sites(&w).len(), 32);
    assert_cjs_per_unit(&w, &u, 5);
}

#[test]
#[ignore]
fn scratch_cjs() {
    let (w, u) = match std::env::var("CW2_T").as_deref() {
        Ok("3") => both(&T3),
        Ok("2") => both(&T2),
        Ok("5") => both(&T5),
        Ok("6") => both(&T6),
        _ => both(&T1),
    };
    for (n, pf) in [("WHOLE", &w), ("UNIT", &u)] {
        eprintln!("===== {n}");
        for t in &pf.templates {
            eprintln!("T {} kind={} escape={:?}", t.display, t.kind, t.escape);
        }
        for s in &pf.sites {
            eprintln!(
                "S {}@{}.{} {:?} {:?}",
                pf.module_paths[s.module as usize], s.byte_offset, s.key, s.verdict, s.receivers
            );
        }
        eprintln!("broken {:?}", pf.broken);
        for l in pf.dump_text().lines().filter(|l| l.starts_with('#')) {
            eprintln!("D {}", &l[..l.len().min(400)]);
        }
    }
}

/// DECISIONS 107: `b.js` `Base.run() { return this.go(); }`, run by `m.js`
/// on `a.js`'s `Main`, and by case: 1, `c.js` runs it on its own
/// `X extends Base` (two units' receivers join); 2, `m.js` runs it on
/// `c.js`'s `X extends Main` overriding `go`. The `this.go()` site's
/// (verdict, receivers) both ways.
fn published_method_site(case: u8) -> ((Verdict, Vec<String>), (Verdict, Vec<String>)) {
    let m = match case {
        1 => "import { newMain } from \"./a\";\nimport { runX } from \"./c\";\nconsole.log(newMain().run() + runX());\n",
        2 => "import { newMain } from \"./a\";\nimport { newX } from \"./c\";\nconsole.log(newMain().run() + newX().run());\n",
        _ => "import { newMain } from \"./a\";\nconsole.log(newMain().run());\n",
    };
    let mut mods = vec![
        ("/t1/b.js", "export class Base { run() { return this.go(); } }\n"),
        (
            "/t1/a.js",
            "import { Base } from \"./b\";\nexport class Main extends Base { go() { return 1; } }\nexport function newMain() { return new Main(); }\n",
        ),
        ("/t1/m.js", m),
    ];
    if case == 1 {
        mods.push((
            "/t1/c.js",
            "import { Base } from \"./b\";\nexport class X extends Base { go() { return 2; } }\nexport function runX() { return new X().run(); }\n",
        ));
    }
    if case == 2 {
        mods.push((
            "/t1/c.js",
            "import { Main } from \"./a\";\nexport class X extends Main { go() { return 2; } }\nexport function newX() { return new X(); }\n",
        ));
    }
    let (w, u) = both(&mods);
    let site = |pf: &ProgramFacts| {
        let s = pf
            .sites
            .iter()
            .find(|s| pf.module_paths[s.module as usize] == "/t1/b.js" && s.key == "go")
            .expect("b.js this.go() site");
        let mut r = s.receivers.clone();
        r.sort();
        (s.verdict.clone(), r)
    };
    (site(&w), site(&u))
}

/// The receivers of `run` are the instances the running unit published:
/// per unit the site is proven over `Main`, as in the whole program.
#[test]
fn a_published_method_runs_on_the_receivers_its_units_publish() {
    let ((wv, wr), (uv, ur)) = published_method_site(0);
    assert!(!matches!(wv, Verdict::No(_)), "whole-program {wv:?} {wr:?}");
    assert_eq!(
        (format!("{uv:?}"), &ur),
        (format!("{wv:?}"), &wr),
        "per unit vs whole-program"
    );
}

/// Two units run `run`, each on its own receivers: per unit the site sees
/// the join of both, as the whole program does, never one unit's alone.
#[test]
fn a_published_method_runs_on_the_join_of_its_units_receivers() {
    let ((wv, wr), (uv, ur)) = published_method_site(1);
    assert!(
        wr.iter().any(|r| r.ends_with(":Main")) && wr.iter().any(|r| r.ends_with(":X")),
        "{wv:?} {wr:?}"
    );
    assert_eq!(
        (format!("{uv:?}"), &ur),
        (format!("{wv:?}"), &wr),
        "per unit vs whole-program"
    );
}

/// A further unit's subclass overriding `go` runs `run` too: per unit the
/// site never binds `Main.go` alone; it sees both receivers, or is not
/// proven.
#[test]
fn a_published_method_runs_on_every_receiver_its_units_publish() {
    let ((wv, wr), (uv, ur)) = published_method_site(2);
    assert!(
        wr.iter().any(|r| r.ends_with(":Main")) && wr.iter().any(|r| r.ends_with(":X")),
        "{wv:?} {wr:?}"
    );
    if !matches!(uv, Verdict::No(_)) {
        assert_eq!(ur, wr, "per unit {uv:?} vs whole-program {wv:?}");
    }
    assert!(
        !(ur.iter().any(|r| r.ends_with(":Main")) && !ur.iter().any(|r| r.ends_with(":X"))),
        "per unit {uv:?} {ur:?}"
    );
}
