//! HUB13: a computed key of known shape runs only the accessors it names
//! (ECMA-262 10.1.8.1 OrdinaryGet / 10.1.9.2: [[Get]]/[[Set]] of key P call
//! only the accessor defined under P). perry-web dom/events.ts:337
//! `target._self()["on" + event.type]` ran every getter of the element
//! chain (`get style`).
use super::*;

fn witness_proven(src: &str) -> bool {
    let pf = facts(src);
    let sites: Vec<_> = pf.sites.iter().filter(|s| s.key == "witness").collect();
    assert_eq!(sites.len(), 1, "{src}\n{}", pf.dump_text());
    let verdict = &sites[0].verdict;
    assert!(
        !matches!(verdict, Verdict::No(why) if why.starts_with("dead site")),
        "{src}: {verdict:?}",
    );
    !matches!(verdict, Verdict::No(_))
}

// The accessors hand `cell` to unknown code: when one runs, `cell.witness`
// is not proven.
const SETUP: &str = r#"
    declare function external(x: unknown): void;
    class Cell { witness = 1; }
    const cell = new Cell();
    class Base { get style(): number { external(cell); return 0; } set style(v: number) { external(cell); } }
    class El extends Base {}
    const s: string = String(process.argv[2]);
"#;

const LIT: &str =
    "{ get style() { external(cell); return 0; }, set style(v: number) { external(cell); } }";

#[test]
fn shaped_key_runs_only_named_accessors() {
    // (receiver, key, proven)
    for (recv, key, proven) in [
        // a class instance: the getter/setter sit on a prototype
        ("new El()", "\"on\" + s", true),
        ("new El()", "\"st\" + s", false),
        ("new El()", "s", false),
        // an object literal's own accessors
        (LIT, "\"on\" + s", true),
        (LIT, "s", false),
    ] {
        for op in ["console.log(e[K]);", "e[K] = 1;"] {
            let body = op.replace('K', key);
            let src = format!("{SETUP}const e: any = {recv};\n{body}\nconsole.log(cell.witness);");
            assert_eq!(witness_proven(&src), proven, "{recv} {body}");
        }
    }
}

#[test]
fn shaped_key_runs_accessor_installed_later() {
    // An accessor defined after the read under a name the shape admits runs
    // (replay); one it excludes does not.
    for (name, proven) in [("onclick", false), ("style", true)] {
        let src = format!(
            "{SETUP}const e: any = new El();\nfunction rd() {{ console.log(e[\"on\" + s]); }}\nrd();\n\
             Object.defineProperty(El.prototype, \"{name}\", {{ get() {{ external(cell); return 0; }}, configurable: true }});\n\
             rd();\nconsole.log(cell.witness);"
        );
        assert_eq!(witness_proven(&src), proven, "{name}");
    }
}
