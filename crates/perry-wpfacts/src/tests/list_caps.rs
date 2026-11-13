//! `list_caps`: the birth capacity of a `let x = []` from the push sites
//! that reach it.

use crate::FactsConfig;

/// The `# list_cap` probe lines of `src`, without the module, local,
/// template and length columns.
fn lines(src: &str) -> Vec<String> {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let pf = crate::compute(
        &[("t.ts".to_string(), &hir)],
        &FactsConfig {
            diagnostics: true,
            ..Default::default()
        },
    );
    let caps = pf
        .bindings_for("t.ts")
        .expect("module bindings")
        .list_capacities
        .clone();
    let out: Vec<String> = pf
        .rep_probe
        .iter()
        .filter(|l| l.starts_with("# list_cap "))
        .map(|l| {
            let from = l.find("sites=").expect("sites column");
            let len = l.find(" len=").expect("len column");
            let cap = l.find(" cap=").expect("cap column");
            format!("{}{}", &l[from..len], &l[cap..])
        })
        .collect();
    let reserved = out.iter().filter(|l| !l.ends_with("cap=-")).count();
    assert_eq!(
        caps.len(),
        reserved,
        "one export per reserving birth: {out:?}"
    );
    out
}

const RUN: &str = "const RUN = Math.random() < 2 ? 1 : 0;\n";

#[test]
fn a_push_in_a_loop_after_the_birth_reserves_the_cap() {
    let src = format!(
        "{RUN}function f(n: number): number[] {{ const a: number[] = []; \
         for (let i = 0; i < n; i++) a.push(i); return a; }}\nconsole.log(f(9 * RUN).length);"
    );
    assert_eq!(lines(&src), ["sites=1 repeated=1 open=0 single=0 cap=32"]);
}

#[test]
fn a_callee_that_pushes_in_its_own_loop_reaches_the_birth() {
    let src = format!(
        "{RUN}function fill(out: number[], n: number): void {{ let i = 0; \
         while (i < n) {{ out.push(i); i++; }} }}\n\
         function f(n: number): number[] {{ const a: number[] = []; fill(a, n); return a; }}\n\
         console.log(f(9 * RUN).length);"
    );
    assert_eq!(lines(&src), ["sites=1 repeated=1 open=0 single=0 cap=32"]);
}

#[test]
fn straight_line_pushes_are_the_bound() {
    let src = format!(
        "{RUN}function f(x: number): number[] {{ const a: number[] = []; a.push(x); \
         a.push(x, x); if (x > 1) a.push(x); a.push(x, x); return a; }}\n\
         console.log(f(RUN).length);"
    );
    assert_eq!(lines(&src), ["sites=4 repeated=0 open=0 single=6 cap=6"]);
}

#[test]
fn four_pushes_or_fewer_reserve_nothing() {
    let src = format!(
        "{RUN}function f(x: number): number[] {{ const a: number[] = []; a.push(x); \
         a.push(x, x); return a; }}\nconsole.log(f(RUN).length);"
    );
    assert_eq!(lines(&src), ["sites=2 repeated=0 open=0 single=3 cap=-"]);
}

#[test]
fn a_list_born_and_pushed_once_in_the_same_loop_is_single() {
    let src = format!(
        "{RUN}let t = 0;\nfor (let i = 0; i < 9 * RUN; i++) {{ const a: number[] = []; \
         a.push(i); t += a.length; }}\nconsole.log(t);"
    );
    assert_eq!(lines(&src), ["sites=1 repeated=0 open=0 single=1 cap=-"]);
}

#[test]
fn a_callee_push_outside_any_loop_is_open() {
    let src = format!(
        "{RUN}function add(out: number[], v: number): void {{ out.push(v); }}\n\
         function f(n: number): number[] {{ const a: number[] = []; \
         for (let i = 0; i < n; i++) add(a, i); return a; }}\nconsole.log(f(9 * RUN).length);"
    );
    assert_eq!(lines(&src), ["sites=1 repeated=0 open=1 single=0 cap=-"]);
}

#[test]
fn a_list_never_pushed_reserves_nothing() {
    let src = format!(
        "{RUN}function f(n: number): number[] {{ const a: number[] = []; return a; }}\n\
         console.log(f(RUN).length);"
    );
    assert_eq!(lines(&src), ["sites=0 repeated=0 open=0 single=0 cap=-"]);
}
