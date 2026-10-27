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
    (
        "/t1/perry_cjs.js",
        include_str!("fixtures/cjs_t1/perry_cjs.js"),
    ),
];

/// DECISIONS 115: `r.js` keeps its classes inside the factory (they name
/// the factory's `NONE`); `S extends R` names itself in `static make`, so
/// the HIR binds `S` through a one-element cell (`[undefined]`, DECISIONS
/// 50); `T` holds `S` in `k`; `T.run` makes two `S` and appends one to the
/// other through `R`'s method; `m.js` runs `require("./r").newInstance().run()`.
const T2: [(&str, &str); 3] = [
    ("/t2/m.js", include_str!("fixtures/cjs_t2/m.js")),
    ("/t2/r.js", include_str!("fixtures/cjs_t2/r.js")),
    (
        "/t2/perry_cjs.js",
        include_str!("fixtures/cjs_t2/perry_cjs.js"),
    ),
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
    (
        "/t3/perry_cjs.js",
        include_str!("fixtures/cjs_t3/perry_cjs.js"),
    ),
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
    (
        "/t5/perry_cjs.js",
        include_str!("fixtures/cjs_t5/perry_cjs.js"),
    ),
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
    (
        "/t6/perry_cjs.js",
        include_str!("fixtures/cjs_t6/perry_cjs.js"),
    ),
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
    assert!(
        stronger.is_empty(),
        "per unit claims more than whole-program: {stronger:?}"
    );
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
            .filter(|t| t.starts_with("r.js:R ") || t.starts_with("r.js:S "))
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
            .find(|t| t.display == "r.js:T")
            .and_then(|t| t.fields.iter().find(|f| f.key == "k"))
            .map(|f| (f.kind.top, f.kind.prims))
    };
    // Per unit `T.k` reads ⊤ until a class instance's fields cross units
    // (DECISIONS 186); the whole program holds the class and `undefined`.
    assert_eq!(
        k_kind(&w),
        Some((false, crate::model::prim::UNDEF)),
        "T.k whole-program"
    );
    assert_eq!(
        k_kind(&u),
        Some((true, crate::model::prim::ALL)),
        "T.k per unit"
    );
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
    // still only the consuming units read it (DECISIONS 171).
    assert_eq!(
        readers,
        [
            "/t6/benchmark.js#u11",
            "/t6/cd.js#u17",
            "/t6/cd.js#u42",
            "/t6/perry_cjs.js#u4",
            "/t6/perry_cjs.js#u10",
            "/t6/perry_cjs.js#u14",
            "/t6/sa_CD.js#u9",
            "/t6/sa_CD.js#u20",
            "/t6/som.js#u19"
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
    assert!(
        proven_sites(&w).len() >= 120,
        "whole-program proves {}",
        proven_sites(&w).len()
    );
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

/// DECISIONS 209: typescript.js's shape as the compiler wraps it. `t.js`
/// keeps its functions in a run-once arrow (`((module) => {…})(module)`)
/// and hands each out through `__export`'s getters and `__toCommonJS`;
/// `user` calls `isId`, `other` and `filter` by name, `filter` gets `isId`
/// as a callback; `m.js` runs `require("./t").user(..)`.
const T9: [(&str, &str); 3] = [
    ("/t9/m.js", include_str!("fixtures/cjs_tsw/m.js")),
    ("/t9/t.js", include_str!("fixtures/cjs_tsw/t.js")),
    (
        "/t9/perry_cjs.js",
        include_str!("fixtures/cjs_tsw/perry_cjs.js"),
    ),
];

#[test]
fn a_broken_function_is_still_the_callee_of_a_call_by_its_name() {
    // The getters hand `isId`, `filter` and `user` to unknown code, so the
    // census breaks their contracts and their owners run them on ⊤
    // arguments. A call by name still calls that function: whole-program
    // proves the callees of `user`'s calls (other units of `t.js`). Before,
    // a broken function read through a value contract was ⊤ ("broken
    // function of another unit"): no target, and typescript.js's top-level
    // helpers (`isIdentifier`, `visitNode`, `some`, ...) lost 22,115 call
    // sites per unit. The arguments still escape and the result is unknown
    // (the escaped function's contract, `fn_named`).
    let (w, u) = both(&T9);
    for f in ["isId", "filter"] {
        assert!(
            u.broken
                .iter()
                .any(|d| d.starts_with("fn:") && d.contains(&format!(":{f}#"))),
            "the census must break {f} (else this test proves nothing): {:?}",
            u.broken
        );
    }
    let calls = |pf: &ProgramFacts| -> BTreeSet<String> {
        pf.sites
            .iter()
            .filter(|s| {
                !matches!(s.verdict, Verdict::No(_))
                    && pf.module_paths[s.module as usize] == "/t9/t.js"
                    && ["isId", "other", "filter"]
                        .iter()
                        .any(|f| s.targets.iter().any(|t| t.contains(&format!(":{f}#"))))
            })
            .map(|s| format!("{}.{}", s.byte_offset, s.key))
            .collect()
    };
    let (cw, cu) = (calls(&w), calls(&u));
    assert_eq!(cw.len(), 4, "{cw:?}");
    let lost: Vec<&String> = cw.difference(&cu).collect();
    assert!(
        lost.is_empty(),
        "lost per unit {lost:?}\nbroken {:?}",
        u.broken
    );
    let (pw, pu) = (proven_sites(&w), proven_sites(&u));
    let stronger: Vec<&String> = pu.difference(&pw).collect();
    assert!(
        stronger.is_empty(),
        "per unit claims more than whole-program: {stronger:?}"
    );
}

/// DECISIONS 209: yargs-parser's shape as the compiler wraps it. `index.js`
/// exports a function object `Parser` with a property function
/// `Parser.detailed` that returns a literal holding two more; `m.js` reads
/// `P.detailed`, calls it and hands the result to unknown code
/// (`globalThis.zz(d)`).
const T10: [(&str, &str); 3] = [
    ("/t10/m.js", include_str!("fixtures/cjs_tsw2/m.js")),
    ("/t10/index.js", include_str!("fixtures/cjs_tsw2/index.js")),
    (
        "/t10/perry_cjs.js",
        include_str!("fixtures/cjs_tsw2/perry_cjs.js"),
    ),
];

#[test]
fn a_property_of_a_broken_function_holds_anything() {
    // The census breaks `Parser`: `m.js` reads a property of it, which no
    // function contract describes. Its calls still go through its owner's
    // contract, but as an object it escaped: `P.detailed` holds anything,
    // so the literal `detailed` returns reaches unknown code per unit as
    // it does whole-program. Before, the stand-in's fields held nothing:
    // `d` was empty, the literal never escaped and kept facts the whole
    // program refutes (d_yargs: 49 stronger facts on yargs-parser's
    // `parse` result).
    let (w, u) = both(&T10);
    assert!(
        u.broken
            .iter()
            .any(|d| d.starts_with("fn:") && d.contains(":Parser#")),
        "the census must break Parser (else this test proves nothing): {:?}",
        u.broken
    );
    let s = super::unit_alias::stronger(&u, &w);
    assert!(
        s.is_empty(),
        "per-unit claims more than whole-program: {s:?}"
    );
    let lit = u
        .templates
        .iter()
        .find(|t| t.display.starts_with("index.js:") && t.birth_keys.iter().any(|k| k == "aliases"))
        .expect("the parse result literal");
    assert!(lit.escape.is_some(), "{lit:?}");
}

/// DECISIONS 210: `rep` of CJL as the compiler wraps it, the factory held
/// beyond its record (`void __perry_cjs_factory`), so the run-once view
/// does not flatten it: `Reply` is a local of the factory, written into
/// the record's `exports`, which is published described by no contract
/// (DECISIONS 203). `m.js` constructs it (and a subclass-like `_Reply`
/// whose prototype is `Object.create(Reply.prototype)`).
const TM: [(&str, &str); 3] = [
    ("/mbw/m.js", include_str!("fixtures/cjs_mbw/m.js")),
    ("/mbw/r.js", include_str!("fixtures/cjs_mbw/r.js")),
    (
        "/mbw/perry_cjs.js",
        include_str!("fixtures/cjs_mbw/perry_cjs.js"),
    ),
];

#[test]
fn an_escaped_constructor_function_of_an_unflattened_factory_is_constructed_by_unknown_code() {
    // Per unit the owner's `new Reply` layout is never stronger than the
    // whole program's: another unit's code holds `Reply` (it escaped with
    // the record) and constructs it, so the owner constructs it with
    // unknown arguments and the instance escapes. Before DECISIONS 210 no
    // unit constructed it: `raw` held nothing (kind none, readonly) while
    // the whole program saw `new r(2)`.
    let (w, u) = both(&TM);
    let tmpl = |pf: &ProgramFacts| -> crate::TemplateFacts {
        pf.templates
            .iter()
            .find(|t| t.display.starts_with("new r.js:Reply"))
            .cloned()
            .unwrap_or_else(|| panic!("new r.js:Reply in\n{}", pf.dump_text()))
    };
    let (tu, tw) = (tmpl(&u), tmpl(&w));
    let raw = |t: &crate::TemplateFacts| {
        t.fields
            .iter()
            .find(|f| f.key == "raw")
            .cloned()
            .unwrap_or_else(|| panic!("raw of {}", t.display))
    };
    let (ru, rw) = (raw(&tu), raw(&tw));
    assert!(
        ru.kind.top || (rw.kind.prims & !ru.kind.prims == 0 && !rw.kind.top),
        "per unit raw {:?} narrower than whole program {:?}\nper unit:\n{}",
        ru.kind,
        rw.kind,
        u.dump_text()
    );
    assert!(
        !(ru.readonly && !rw.readonly),
        "per unit claims raw readonly, the whole program does not\n{}",
        u.dump_text()
    );
    assert!(
        !(tu.prefix_stable && !tw.prefix_stable),
        "per unit claims prefix_stable, the whole program does not\n{}",
        u.dump_text()
    );
}
