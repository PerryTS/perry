//! A program store onto a non-callable built-in namespace object
//! (`performance.now = () => 0`) writes that object's own property: program
//! reads of the key see the stored value, and nothing reaches unknown code.
//! The realm stays the program's, so unrelated sites (`Object.assign`'s
//! argument, the methods of its class) stay proven.
use super::*;
use crate::model::prim;

const VEC: &str = "class Vec {\n\
     x: number; y: number;\n\
     constructor(x = 0, y = 0) { this.x = x; this.y = y; }\n\
     add(v: Vec): Vec { this.x += v.x; this.y += v.y; return this; }\n\
     }\n\
     const a = new Vec(1, 2);\n\
     a.add(new Vec(3, 4));\n\
     console.log(a.x, a.y, Math.sqrt(a.x));\n\
     Object.assign({}, a);\n";

fn all_proven(pf: &ProgramFacts, key: &str) -> bool {
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    !s.is_empty() && s.iter().all(|s| !matches!(s.verdict, Verdict::No(_)))
}

/// The ladder's d03 shape: one store of a program closure onto
/// `performance.now`. The class's sites stay proven, and `Math.sqrt` keeps
/// its intrinsic lowering (a member op, not a site).
#[test]
fn store_onto_performance_keeps_other_sites_proven() {
    for store in [
        "const p = (globalThis as any).performance;\np.now = (): number => 0;",
        "(globalThis as any).performance.now = (): number => 0;",
    ] {
        let pf = facts(&format!("{VEC}{store}\n"));
        let dump = pf.dump_text();
        for key in ["x", "y", "add"] {
            assert!(all_proven(&pf, key), "{store} / {key}\n{dump}");
        }
        assert!(dump.contains("MathSqrt key=sqrt"), "{store}\n{dump}");
    }
}

/// Soundness: a later read of the key sees the program's closure (its
/// result reaches the load), beside the realm's member.
#[test]
fn performance_read_after_store_sees_the_closure() {
    let pf = facts(
        "class H { v: any = 0; }\n\
         const p = (globalThis as any).performance;\n\
         p.now = (): any => \"s\";\n\
         const h = new H();\n\
         h.v = (globalThis as any).performance.now();\n\
         console.log(h.v);\n",
    );
    let k = field(tmpl(&pf, "H"), "v").kind.clone();
    assert!(k.top || k.prims & prim::STR != 0, "{k:?}");
}

/// Control: a computed key may name any member: the store still hands the
/// value to the realm's readers (E20), as before. The stored value is the
/// program object itself, so its fields are no longer proven.
#[test]
fn computed_key_store_onto_performance_still_escapes() {
    let pf = facts(&format!(
        "{VEC}const k: string = String(Date.now() % 2 ? \"now\" : \"mark\");\n\
         ((globalThis as any).performance as any)[k] = a;\n"
    ));
    assert!(!all_proven(&pf, "x"), "{}", pf.dump_text());
}
