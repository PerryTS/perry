//! ANIMOOM5 fixtures: a computed store whose key is an unknown string (a
//! regex capture: three.core.js PropertyBinding `_setValue_direct`,
//! `this.targetObject[ this.propertyName ] = buffer[ offset ]`) into every
//! class prototype. Modeled as the `__proto__` setter (B.2.2.1.2), each
//! store re-linked every prototype to every value read back: one prototype
//! cycle over all of them, and the solve did not finish.
//! Rule (OPTIMISTIC, owner 2026-11-16, DECISIONS 217 extended to stores):
//! an unknown string key of a store is never "__proto__". Exact and kept:
//! the named `o.__proto__ = p` / `o["__proto__"] = p`, and a key whose
//! KNOWN strings include "__proto__".
//! Rule (exact, ECMA-262 13.2.5.5 PropertyDefinitionEvaluation): a computed
//! object-literal key `{ [k]: v }` is CreateDataPropertyOrThrow, a
//! definition: it never runs the setter, whatever `k` is. Only the
//! non-computed `{ __proto__: p }` (also spelled `{ "__proto__": p }`)
//! sets the prototype (B.3.1).

use crate::model::{Model, TmplKind, TokKind};

fn solved(src: &str) -> Model {
    let ast = Box::leak(Box::new(perry_parser::parse_typescript(src, "t.ts").unwrap()));
    let hir = Box::leak(Box::new(perry_hir::lower_module(ast, "t", "t.ts").unwrap()));
    let mut g = crate::gen::Gen::new(vec![("t.ts", &*hir)], false, false);
    g.m.max_work = 300_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    g.m
}

fn class_of(m: &Model, t: usize) -> Option<&str> {
    match m.toks[t] {
        TokKind::Proto(tm) => match &m.tmpls[tm as usize].kind {
            TmplKind::Class { name, .. } => Some(name.as_str()),
            _ => None,
        },
        _ => None,
    }
}

/// The program class prototypes among `cls.prototype`'s prototypes.
fn chain(m: &Model, cls: &str) -> Vec<String> {
    let t = (0..m.toks.len())
        .find(|&t| class_of(m, t) == Some(cls))
        .unwrap_or_else(|| panic!("no prototype token of {cls}"));
    let Some(&n) = m.proto_node.get(&(t as u32)) else {
        return Vec::new();
    };
    let mut toks = Vec::new();
    m.nodes[n as usize].pts.sorted_into(&mut toks);
    toks.iter()
        .filter_map(|&p| class_of(m, p as usize).map(str::to_string))
        .collect()
}

fn classes(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        s += &format!("class C{i} {{ f{i}: any = {i}; m{i}(): number {{ return {i}; }} }}\n");
    }
    s += "class P { pm(): number { return 7; } }\n";
    let os: Vec<String> = (0..n).map(|i| format!("C{i}.prototype")).collect();
    s += &format!("const os: any[] = [ {}, new C0() ];\n", os.join(", "));
    s
}

const KEY: &str = "const mt: any = /^(\\w+)\\.(\\w+)$/.exec( 'n' + process.argv.length + '.x' );\nconst k: string = mt[ 2 ];\n";

const TAIL: &str = "const q: any = Object.getPrototypeOf( C5.prototype );\nconsole.log( q === Object.prototype, typeof q.pm );\n";

/// D1 RED: 40 class prototypes, read from and written back under an
/// unknown key: C5.prototype's chain is Object.prototype only, and the
/// solve finishes under a small work bound.
#[test]
fn animoom5_d1_unknown_store_key_never_relinks() {
    let src = format!(
        "{}{KEY}const b: any[] = [];\nos.forEach( ( o: any, i: number ) => {{ b[ i ] = o[ k ]; }} );\nos.forEach( ( o: any, i: number ) => {{ o[ k ] = b[ i ]; }} );\n{TAIL}",
        classes(40)
    );
    let m = solved(&src);
    assert!(!m.aborted, "D1: the solve passed its work bound");
    assert!(chain(&m, "C5").is_empty(), "D1: C5.prototype relinked to {:?}", chain(&m, "C5"));
}

/// D1 KEEP: the named `__proto__` store still relinks (dot form).
#[test]
fn animoom5_d1_keep_named_dot_relinks() {
    let src = format!("{}( C5.prototype as any ).__proto__ = P.prototype;\n{TAIL}", classes(8));
    assert_eq!(chain(&solved(&src), "C5"), vec!["P".to_string()]);
}

/// D1 KEEP: the named `__proto__` store still relinks (string form).
#[test]
fn animoom5_d1_keep_named_string_relinks() {
    let src = format!("{}( C5.prototype as any )[ \"__proto__\" ] = P.prototype;\n{TAIL}", classes(8));
    assert_eq!(chain(&solved(&src), "C5"), vec!["P".to_string()]);
}

/// D1 KEEP: a key whose known strings are { "x", "__proto__" } relinks.
#[test]
fn animoom5_d1_keep_known_set_relinks() {
    let src = format!(
        "{}const k2: string = process.argv.length > 5 ? 'x' : '__proto__';\n( C5.prototype as any )[ k2 ] = P.prototype;\n{TAIL}",
        classes(8)
    );
    assert_eq!(chain(&solved(&src), "C5"), vec!["P".to_string()]);
}

/// D2 RED: a computed literal key is a definition, never the setter: the
/// literal's prototype stays Object.prototype, whatever `k` is.
#[test]
fn animoom5_d2_computed_literal_key_defines() {
    let src = format!(
        "{}const k3: string = process.argv.length > 5 ? 'x' : '__proto__';\nconst lit: any = {{ [ k3 ]: C5.prototype }};\nconst lp: any = Object.getPrototypeOf( lit );\nconsole.log( typeof lp.m5 );\n{TAIL}",
        classes(8)
    );
    let m = solved(&src);
    let lit = lit_protos(&m);
    assert!(lit.is_empty(), "D2: the computed literal key relinked to {lit:?}");
}

/// D2 KEEP: `{ __proto__: p }` and `{ "__proto__": p }` set the prototype.
#[test]
fn animoom5_d2_keep_literal_proto_sets() {
    for form in ["__proto__", "\"__proto__\""] {
        let src = format!(
            "{}const lit: any = {{ {form}: C5.prototype, z: 1 }};\nconsole.log( lit.m5() );\n{TAIL}",
            classes(8)
        );
        let m = solved(&src);
        assert_eq!(lit_protos(&m), vec!["C5".to_string()], "D2 KEEP {form}");
    }
}

/// The program class prototypes among the prototypes of every literal.
fn lit_protos(m: &Model) -> Vec<String> {
    let mut out = Vec::new();
    for (t, k) in m.toks.iter().enumerate() {
        let TokKind::Obj(tm, _) = k else { continue };
        if !matches!(m.tmpls[*tm as usize].kind, TmplKind::Lit { .. }) {
            continue;
        }
        let Some(&n) = m.proto_node.get(&(t as u32)) else { continue };
        let mut toks = Vec::new();
        m.nodes[n as usize].pts.sorted_into(&mut toks);
        out.extend(toks.iter().filter_map(|&p| class_of(m, p as usize).map(str::to_string)));
    }
    out.sort();
    out.dedup();
    out
}
