//! Per-unit CommonJS (design §17.16, DECISIONS 87): the registry record, the
//! exports object and the classes a CommonJS module hands out keep, per unit,
//! what the whole-program solve proves of them. The fixtures are the real
//! `cjs_wrap` output of three modules (`b.js` exports `Base`, `a.js` has
//! `class Main extends Base` and `exports.newInstance = () => new Main()`,
//! `m.js` calls `require("./a").newInstance().run()`), paths shortened to `/t1`.

use super::*;
use std::collections::BTreeSet;

const T1: [(&str, &str); 3] = [
    ("/t1/m.js", include_str!("fixtures/cjs_t1/m.js")),
    ("/t1/a.js", include_str!("fixtures/cjs_t1/a.js")),
    ("/t1/b.js", include_str!("fixtures/cjs_t1/b.js")),
];

/// DECISIONS 115: `r.js` keeps its classes inside the factory (they name
/// the factory's `NONE`); `S extends R` names itself in `static make`, so
/// the HIR binds `S` through a one-element cell (`[undefined]`, DECISIONS
/// 50); `T` holds `S` in `k`; `T.run` makes two `S` and appends one to the
/// other through `R`'s method; `m.js` runs `require("./r").newInstance().run()`.
const T2: [(&str, &str); 2] = [
    ("/t2/m.js", include_str!("fixtures/cjs_t2/m.js")),
    ("/t2/r.js", include_str!("fixtures/cjs_t2/r.js")),
];

/// AWFY Richards as the compiler wraps it: `sa_Richards.js` requires
/// `richards.js` (RBObject and its eight subclasses), which requires
/// `benchmark.js`.
const T3: [(&str, &str); 3] = [
    ("/t3/sa_Richards.js", include_str!("fixtures/cjs_t3/sa_Richards.js")),
    ("/t3/richards.js", include_str!("fixtures/cjs_t3/richards.js")),
    ("/t3/benchmark.js", include_str!("fixtures/cjs_t3/benchmark.js")),
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
    let (pw, pu) = (proven_sites(&w), proven_sites(&u));
    let lost: Vec<_> = pw.difference(&pu).collect();
    assert!(
        lost.is_empty(),
        "per unit {} of whole-program {} proven; lost {:?}\nescaped per unit {:?}\nwhole-program escaped {:?}",
        pu.len(),
        pw.len(),
        lost,
        escaped(&u),
        escaped(&w)
    );
    assert!(escaped(&u).is_empty(), "{:?}", escaped(&u));
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
    assert!(
        lost.is_empty() && classes(&u).is_empty(),
        "per unit {} of whole-program {} proven; lost {:?}\nescaped per unit {:?}\nbroken {:?}",
        pu.len(),
        pw.len(),
        lost,
        escaped(&u),
        u.broken
    );
    // `T.k = S` holds the class and the cell's `undefined` both ways: the
    // reader's import carries the binding's primitives.
    let k_kind = |pf: &ProgramFacts| {
        pf.templates
            .iter()
            .find(|t| t.display == "r.js:T")
            .and_then(|t| t.fields.iter().find(|f| f.key == "k"))
            .map(|f| (f.kind.top, f.kind.prims))
    };
    assert_eq!(k_kind(&u), k_kind(&w), "T.k per unit vs whole-program");
}

#[test]
#[ignore]
fn scratch_cjs() {
    let (w, u) = match std::env::var("CW2_T").as_deref() {
        Ok("3") => both(&T3),
        Ok("2") => both(&T2),
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
    assert_eq!((format!("{uv:?}"), &ur), (format!("{wv:?}"), &wr), "per unit vs whole-program");
}

/// Two units run `run`, each on its own receivers: per unit the site sees
/// the join of both, as the whole program does, never one unit's alone.
#[test]
fn a_published_method_runs_on_the_join_of_its_units_receivers() {
    let ((wv, wr), (uv, ur)) = published_method_site(1);
    assert!(wr.iter().any(|r| r.ends_with(":Main")) && wr.iter().any(|r| r.ends_with(":X")), "{wv:?} {wr:?}");
    assert_eq!((format!("{uv:?}"), &ur), (format!("{wv:?}"), &wr), "per unit vs whole-program");
}

/// A further unit's subclass overriding `go` runs `run` too: per unit the
/// site never binds `Main.go` alone; it sees both receivers, or is not
/// proven.
#[test]
fn a_published_method_runs_on_every_receiver_its_units_publish() {
    let ((wv, wr), (uv, ur)) = published_method_site(2);
    assert!(wr.iter().any(|r| r.ends_with(":Main")) && wr.iter().any(|r| r.ends_with(":X")), "{wv:?} {wr:?}");
    if !matches!(uv, Verdict::No(_)) {
        assert_eq!(ur, wr, "per unit {uv:?} vs whole-program {wv:?}");
    }
    assert!(!(ur.iter().any(|r| r.ends_with(":Main")) && !ur.iter().any(|r| r.ends_with(":X"))), "per unit {uv:?} {ur:?}");
}
