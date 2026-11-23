//! A data define onto the global object under a key the realm declares as a
//! getter-only accessor (`Object.defineProperty(globalThis, "navigator",
//! { value: {...} })`) replaces the accessor with the program's value. The
//! runtime never reads that key by name (it holds its own getter), so the
//! value goes into the key's global field and stays modeled: reads of
//! `navigator` see it, and calls of its methods keep their receiver.
use super::*;

const PROGRAM: &str = "const g = globalThis as Record<string, unknown>;\n\
     class Vec {\n\
       x: number; y: number;\n\
       constructor(x = 0, y = 0) { this.x = x; this.y = y; }\n\
       add(v: Vec): Vec { this.x += v.x; this.y += v.y; return this; }\n\
     }\n\
     class Nav { ua: string = \"perry\"; beat(): boolean { return false; } }\n\
     const a = new Vec(1, 2);\n\
     a.add(new Vec(3, 4));\n\
     console.log(a.x, a.y);\n";

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    !s.is_empty() && s.iter().all(|s| !matches!(s.verdict, Verdict::No(_)))
}

/// The ladder's r36 shape: the define, then a read of the global and a call
/// of its method, leave the program's sites proven (the read itself is the
/// realm's object or the program's, a mixed receiver, and stays generic).
#[test]
fn global_define_navigator_keeps_sites() {
    let pf = facts(&format!(
        "{PROGRAM}Object.defineProperty(g, \"navigator\", {{ configurable: true, enumerable: true, \
         writable: true, value: new Nav() }});\n\
         const n = navigator as unknown as Nav;\n\
         console.log(n.beat(), n.ua);\n"
    ));
    for key in ["x", "add"] {
        assert!(proven(&pf, key), "{key}\n{}", pf.dump_text());
    }
}

/// Soundness and exactness: a read of the defined global sees the defined
/// value, and nothing unknown.
#[test]
fn global_define_navigator_read_sees_value() {
    let pf = facts(
        "class H { v: any = 0; }\n\
         class O { k: number = 1; }\n\
         Object.defineProperty(globalThis, \"navigator\", { value: new O() });\n\
         const h = new H();\n\
         h.v = navigator;\n\
         console.log(h.v);\n",
    );
    let v = field(tmpl(&pf, "H"), "v");
    assert!(
        !v.kind.top && v.kind.objs.iter().any(|o| o.ends_with(":O")),
        "{}",
        pf.dump_text()
    );
}
