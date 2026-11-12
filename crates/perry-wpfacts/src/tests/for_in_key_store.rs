//! ENUM3: a store under a for-in key, `T[k] = v` inside `for (k in s)`,
//! writes only keys the for-in may yield: the enumerable String keys of `s`
//! and its chain (ECMA-262 14.7.5.9 EnumerateObjectProperties, 14.7.5.10).
//! Over a program object those are the names written on it, never
//! `Object.prototype.valueOf` or `toString` (non-enumerable, clause 18,
//! 20.1.3), so a built-in function stored that way is never ToPrimitive's
//! callee on `T` (three.js `Material.setValues`). Any value whose keys are
//! unknown keeps the any-String store.
use super::*;

fn mat(lp: &str, subject: &str, pre: &str) -> ProgramFacts {
    facts(&format!(
        "class Mat {{
           color: any = 1;
           name: string = \"m\";
           setValues(v: any) {{ {lp} }}
         }}
         {pre}
         const mat: any = new Mat();
         mat.setValues({subject});
         console.log(`${{mat}}`, mat + \"\", mat.name);"
    ))
}

const LOOP: &str = "for (const k in v) { const nv = v[k]; if (nv === undefined) continue; this[k] = nv; }";

#[test]
fn for_in_key_store_over_a_literal_names_only_its_keys() {
    let pf = mat(LOOP, "{ color: new Function(process.argv[3])() }", "");
    let m = tmpl(&pf, "Mat");
    assert_eq!(m.escape, None, "a for-in key store wrote a builtin into valueOf/toString");
    let name = field(m, "name").kind.clone();
    assert!(!name.top, "mat.name {name:?}");
    // The key alone decides: the stored value need not be `v[k]`.
    let pf = mat(
        "for (const k in v) { this[k] = new Function(process.argv[3])(); }",
        "{ color: 1 }",
        "",
    );
    assert_eq!(tmpl(&pf, "Mat").escape, None);
}

#[test]
fn for_in_key_store_keys_unknown_stay_any() {
    // A JSON.parse object: any key, `valueOf` among them.
    let pf = mat(LOOP, "{ color: new Function(process.argv[3])(), ...JSON.parse(process.argv[2]) }", "");
    let e1 = tmpl(&pf, "Mat").escape.clone();
    // Unknown code's object.
    let pf = mat(LOOP, "new Function(process.argv[2])()", "");
    let e2 = tmpl(&pf, "Mat").escape.clone();
    // A subject written under an unknown key.
    let pf = mat(
        LOOP,
        "s",
        "const s: any = { color: 1 }; s[process.argv[2]] = new Function(process.argv[3])();",
    );
    let e3 = tmpl(&pf, "Mat").escape.clone();
    // The key variable reassigned in the body.
    let pf = mat(
        "for (let k in v) { const nv = v[k]; k = process.argv[2]; this[k] = nv; }",
        "{ color: new Function(process.argv[3])() }",
        "",
    );
    let e4 = tmpl(&pf, "Mat").escape.clone();
    eprintln!("controls {e1:?} {e2:?} {e3:?} {e4:?}");
    assert!(e1.is_some() && e2.is_some() && e3.is_some() && e4.is_some());
}

#[test]
fn for_in_key_store_gains_keys_written_later() {
    // A key the subject gains after the loop's store applied is stored too.
    let pf = facts(
        "class R { v: any; constructor(v: any) { this.v = v; } }
         class K { x = 1; }
         function copy(t: any, s: any): any { for (const k in s) { t[k] = s[k]; } return t; }
         const s: any = { a: 1 };
         const out: any = copy({}, s);
         s.late = new K();
         copy(out, s);
         const r = new R(out.late);
         console.log(r.v);",
    );
    let k = field(tmpl(&pf, "R"), "v").kind.clone();
    assert!(k.objs.iter().any(|o| o.ends_with(":K")), "out.late {k:?}");
}
