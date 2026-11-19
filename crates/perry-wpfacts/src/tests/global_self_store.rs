//! A program store onto the global object under a named key
//! (`g["globalThis"] = globalThis`) puts its value in that key's global
//! field. A computed read of the global object (`globalThis["on" + type]`)
//! takes the field of each named key it may spell, so that store is not a
//! write of a value the read misses: the read stays modeled, and the call of
//! what it read keeps its receiver.
use super::*;

const PROGRAM: &str = "const g = globalThis as Record<string, unknown>;\n\
     class Vec {\n\
       x: number; y: number;\n\
       constructor(x = 0, y = 0) { this.x = x; this.y = y; }\n\
       add(v: Vec): Vec { this.x += v.x; this.y += v.y; return this; }\n\
     }\n\
     class Target { _self(): unknown { return globalThis; } }\n\
     function fire(t: Target, type: string): void {\n\
       const h = (t._self() as Record<string, unknown>)[\"on\" + type];\n\
       if (typeof h === \"function\") (h as () => void).call(t._self());\n\
     }\n\
     const a = new Vec(1, 2);\n\
     a.add(new Vec(3, 4));\n\
     fire(new Target(), String(Date.now() % 2 ? \"load\" : \"error\"));\n\
     console.log(a.x, a.y);\n";

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    !s.is_empty() && s.iter().all(|s| !matches!(s.verdict, Verdict::No(_)))
}

/// The ladder's r32 shape: the self-store leaves the computed `on*` read
/// of the global object and the call of its result proven.
#[test]
fn global_self_store_keeps_computed_global_read() {
    for tail in ["", "g[\"globalThis\"] = globalThis;\n"] {
        let pf = facts(&format!("{PROGRAM}{tail}"));
        for key in ["call", "x", "add"] {
            assert!(proven(&pf, key), "{tail} / {key}\n{}", pf.dump_text());
        }
    }
}

/// Soundness: a computed read that may spell a named stored key sees the
/// stored value.
#[test]
fn computed_global_read_sees_named_store() {
    let pf = facts(
        "class H { v: any = 0; }\n\
         class O { k: number = 1; }\n\
         const g = globalThis as Record<string, unknown>;\n\
         g[\"onx\"] = new O();\n\
         const h = new H();\n\
         const k: string = String(Date.now() % 2 ? \"x\" : \"y\");\n\
         h.v = g[\"on\" + k];\n\
         console.log(h.v);\n",
    );
    let v = field(tmpl(&pf, "H"), "v");
    assert!(
        v.kind.top || v.kind.objs.iter().any(|o| o.ends_with(":O")),
        "{}",
        pf.dump_text()
    );
}
