//! PXGET: a Proxy trap that 10.5.x calls with the key P of one access
//! (get 10.5.8 step 8, set 10.5.9 step 8, has 10.5.7 step 8,
//! deleteProperty 10.5.10 step 8, getOwnPropertyDescriptor 10.5.5 step 8)
//! runs once per key shape of the access (Named(k) or Any) for its handler
//! context, not once for every access of the handler. RED on 85cc9b0648:
//! the get trap ran one instance, so the any-key get `p[k]` (the
//! Object.prototype methods, `valueOf` among them) flowed into the named
//! get `p.box`, and `b.w = 2` stored onto `Object.prototype.valueOf`
//! (PERFNOW122 edge E2, tokens.ts get#45).
use super::*;
use crate::model::prim;

// The tokens.ts shape: the handler is made in a function (a closure trap)
// and reads a captured object. Observed on 85cc9b0648: a module-level
// handler whose trap reads `t[k]` or a captured object returns per call
// already, so it does not show the merge; this shape does.
const SRC: &str = r#"
function mk(): any {
  const api: any = { box: { w: 1 } };
  const store: any = {};
  return new Proxy(api, {
    get(t: any, key: any) { if (typeof key !== "string") return t[key]; if (key in api) return api[key]; return store[key]; },
    set(_t: any, key: any, v: any) { store[key] = v; return true; },
  });
}
const p: any = mk();
const anyKey: string = process.argv[2];
const junk = p[anyKey];
const b = p.box;
b.w = 2;
const vo: any = ({} as any).valueOf;
const holder: any = { bv: null, jv: null, wv: null };
holder.bv = b;
holder.jv = junk;
holder.wv = vo.w;
console.log(holder.bv, holder.jv, holder.wv);
"#;

fn kind(pf: &ProgramFacts, key: &str) -> crate::derive::KindFact {
    let fs: Vec<_> = pf
        .templates
        .iter()
        .flat_map(|t| &t.fields)
        .filter(|f| f.key == key)
        .collect();
    assert!(!fs.is_empty(), "no field {key}");
    fs[0].kind.clone()
}

/// GREEN: the named get returns the target's `box` object only.
#[test]
fn pxget_named_get_sees_its_own_key_only() {
    let pf = facts(SRC);
    let b = kind(&pf, "bv");
    assert!(
        !b.top && b.funcs == 0 && b.builtin_objs == 0 && b.objs.len() == 1,
        "b = p.box holds only {{w}}: {} {:?}\n{}",
        b.desc(),
        b.objs,
        pf.dump_text()
    );
}

/// GREEN: `b.w = 2` stores onto `{w}`, never onto `Object.prototype.valueOf`.
#[test]
fn pxget_named_store_not_onto_value_of() {
    let pf = facts(SRC);
    let w = kind(&pf, "wv");
    assert!(
        w.prims & prim::NUM == 0,
        "valueOf.w holds no number: {}\n{}",
        w.desc(),
        pf.dump_text()
    );
}

/// Guard: the any-key get still returns everything it can: the target's
/// `box` object and the `Object.prototype` methods.
#[test]
fn pxget_any_key_get_keeps_every_value() {
    let pf = facts(SRC);
    let j = kind(&pf, "jv");
    assert!(
        j.top || ((j.funcs > 0 || j.builtin_objs > 0) && !j.objs.is_empty()),
        "p[anyKey] holds box and the prototype methods: {} {:?}\n{}",
        j.desc(),
        j.objs,
        pf.dump_text()
    );
}
