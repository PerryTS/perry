//! A literal of this unit handed to another unit's closed class's
//! constructor (`new App({ a: 1, b: "x" })`): it leaves as a handed
//! literal, described key by key by the constructor's argument contract
//! (`ctor_args_out`), as one handed to a closed function does
//! (`Gen::closed_arg_literals`). It escapes only once a census report
//! broke its layout (the owner used it beyond the layout).

use super::*;

/// `new App({ a, b })`: the literal passed inline (the chess app's
/// `new ChessApp({ level, human, fen, color, unicode })`).
const MAIN: &str = r#"import { App } from "./a";
function run(): void {
    let a = 1;
    let b = "x";
    if (process.argv.length > 5) { a = 2; b = "y"; }
    const app = new App({ a, b });
    console.log(app.sum());
}
run();"#;

/// The literal held in a binding the caller reads again.
const MAIN_HELD: &str = r#"import { App } from "./a";
function run(): void {
    const o = { a: 1, b: "x" };
    const app = new App(o);
    console.log(app.sum(), o.a, o.b);
}
run();"#;

fn app(body: &str) -> String {
    format!(
        r#"export interface AppOptions {{
    a: number;
    b: string;
}}
export class App {{
    a: number;
    b: string;
    constructor(options: AppOptions) {{
        {body}
        this.a = options.a;
        this.b = options.b;
    }}
    sum(): string {{ return this.a + this.b; }}
}}"#
    )
}

/// The sites of module `path` reading `key`.
fn sites_of<'a>(pf: &'a ProgramFacts, path: &str, key: &str) -> Vec<&'a crate::SiteFacts> {
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && pf.module_paths[s.module as usize] == path)
        .collect();
    assert!(!v.is_empty(), "no {path} site {key}\n{}", pf.dump_text());
    v
}

fn all_proven(pf: &ProgramFacts, path: &str, key: &str) -> bool {
    sites_of(pf, path, key)
        .iter()
        .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

fn never_stronger(mods: &[(&str, &str)], what: &str) -> (ProgramFacts, ProgramFacts) {
    let (u, w) = (units_of(mods), whole_of(mods));
    let stronger: Vec<_> = proven_set(&u).difference(&proven_set(&w)).cloned().collect();
    assert!(stronger.is_empty(), "{what}: per unit stronger than whole: {stronger:?}\n{}", u.dump_text());
    (u, w)
}

#[test]
fn a_literal_handed_to_a_closed_ctor_is_described() {
    let a = app("");
    for main in [MAIN, MAIN_HELD] {
        let mods = [("a", a.as_str()), ("main", main)];
        let (u, w) = never_stronger(&mods, "plain");
        assert!(u.broken.is_empty(), "{:?}\n{}", u.broken, u.dump_text());
        for k in ["a", "b"] {
            assert!(all_proven(&w, "a.ts", k), "whole a.ts {k}\n{}", w.dump_text());
            assert!(all_proven(&u, "a.ts", k), "unit a.ts {k}\n{}", u.dump_text());
        }
    }
}

/// What the constructor does to the literal beyond reading it is never
/// hidden from the caller: per unit never stronger than whole.
#[test]
fn a_closed_ctor_using_a_handed_literal_stays_sound() {
    for body in [
        "(options as any).a = \"s\";",
        "(options as any).z = 1;",
        "delete (options as any).b;",
        "(globalThis as any).keep = options;",
        "(this as any).o = options; (this as any).o.a = \"t\";",
        "Object.defineProperty(options, \"a\", { get() { return \"g\"; } });",
    ] {
        let a = app(body);
        for main in [MAIN, MAIN_HELD] {
            let mods = [("a", a.as_str()), ("main", main)];
            let (u, _) = never_stronger(&mods, body);
            // A use beyond the layout is the owner's census report: the
            // caller's literal escapes in its unit.
            if body.contains("globalThis") {
                assert!(
                    u.broken.iter().any(|d| d.starts_with("main.ts:")),
                    "{body}: {:?}\n{}",
                    u.broken,
                    u.dump_text()
                );
                let lit = u
                    .templates
                    .iter()
                    .find(|t| t.display.starts_with("main.ts:__AnonShape") && t.birth_keys == ["a", "b"])
                    .unwrap_or_else(|| panic!("{body}: the literal\n{}", u.dump_text()));
                assert!(lit.escape.is_some(), "{body}\n{}", u.dump_text());
            }
        }
    }
}
