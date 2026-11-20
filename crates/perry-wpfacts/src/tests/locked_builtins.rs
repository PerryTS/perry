//! DEC 250, the solver half: a member the image gives an intrinsic
//! prototype (except `constructor`) is never written, and no key is added
//! to `Object.prototype` (the runtime refuses both with a TypeError at the
//! store, `builtin_proto::locked_family` / `add_locked`). A store into ⊤
//! therefore changes no final read (`members::locked_read`): the call of a
//! locked member stays the builtin and keeps its receiver. A store into ⊤
//! may still ADD a key to a prototype the runtime leaves add-open (Map's,
//! Array's under a name) and may replace `constructor`: those reads see it.
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

/// The verdicts of the sites of key `key` (`call`: method calls, else
/// reads), in source order.
fn verdicts(pf: &ProgramFacts, key: &str, call: bool) -> Vec<Verdict> {
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.is_call == call)
        .collect();
    s.sort_by_key(|s| s.byte_offset);
    s.iter().map(|s| s.verdict.clone()).collect()
}

fn proven(v: &[Verdict]) -> bool {
    !v.is_empty() && v.iter().all(|v| !matches!(v, Verdict::No(_)))
}

fn unproven(v: &[Verdict]) -> bool {
    !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::No(_)))
}

/// `<store>` at top level on a value from unknown code, then `<body>` in
/// `main`.
fn program(store: &str, body: &str) -> String {
    format!(
        "const ext: any = (globalThis as any)[process.argv[2]];\n{store}\n\
         function main() {{ {body} }}\nconsole.log(main());"
    )
}

const STORES: [&str; 3] = [
    "ext.push = function () {}; ext.indexOf = 1; ext.hasOwnProperty = 1;",
    "ext[process.argv[3]] = function () {};",
    "ext[process.argv[3]] = function () {}; (Array.prototype as any).push = function () {};",
];

/// Through ⊤ (named, computed) and directly: `push`, `indexOf` of an array
/// stay the builtins, `hasOwnProperty` of a plain object keeps it from
/// escaping (its `o.x` stays proven; the call site itself is no proof on
/// either arm: "method not found on the chain"), and the array does not
/// escape (its later `indexOf` keeps the NativeMember proof, which
/// asks that no unknown code holds the receiver). Red before DEC 250's
/// solver half: the store "may have written" every builtin member.
#[test]
fn a_store_through_top_writes_no_locked_member() {
    let body = "const a: any = [1, 2]; a.push(3); const o: any = { x: 1 }; \
                return a.indexOf(3) + (o.hasOwnProperty(\"x\") ? 1 : 0) + o.x;";
    for store in [""].into_iter().chain(STORES) {
        let pf = one(&program(store, body));
        for (key, call) in [("push", true), ("indexOf", true), ("x", false)] {
            let v = verdicts(&pf, key, call);
            assert!(proven(&v), "{store} {key}: {v:?}\n{}", pf.dump_text());
        }
        let v = verdicts(&pf, "indexOf", true);
        assert!(
            v.iter().all(|v| *v == Verdict::NativeMember),
            "{store}: {v:?}"
        );
    }
}

/// Soundness arms: what the runtime leaves writable stays written.
/// `constructor` of an array through ⊤ (exempt from the lock), a NEW key on
/// `Map.prototype` (no add-locked family) and a non-member name on
/// `Array.prototype`: the call runs what was stored, so its argument
/// escapes and the argument's later read loses its proof. Each against its
/// control without the store.
#[test]
fn a_store_through_top_still_writes_what_the_lock_leaves_open() {
    for (store, body) in [
        (
            "ext.constructor = function (p: any) { p.x = 'w'; };",
            "const a: any = [1]; const p: any = { x: 1 }; const C: any = a.constructor; C(p); return p.x;",
        ),
        (
            "ext.push = function (p: any) { p.x = 'w'; };",
            "const mp: any = new Map(); const p: any = { x: 1 }; mp.push(p); return p.x;",
        ),
        (
            "ext.myHelper = function (p: any) { p.x = 'w'; };",
            "const a: any = [1]; const p: any = { x: 1 }; a.myHelper(p); return p.x;",
        ),
    ] {
        let pf = one(&program("", body));
        let v = verdicts(&pf, "x", false);
        assert!(proven(&v), "control {body}: {v:?}");
        let pf = one(&program(store, body));
        let v = verdicts(&pf, "x", false);
        assert!(unproven(&v), "{store}: {v:?}\n{}", pf.dump_text());
    }
}

/// Soundness arm: a plain object, a class prototype or an `arguments`
/// object whose prototype the program sets to `Array.prototype` (literal
/// `__proto__`, `Object.setPrototypeOf`) passes an add-open prototype, so a
/// name added through ⊤ (`myHelper`) reaches its call: the argument
/// escapes. A plain object's final read holds only while its chain is
/// `Object.prototype` alone.
#[test]
fn a_reparented_plain_object_still_reads_what_top_added() {
    let store = "ext.myHelper = function (p: any) { p.x = 'w'; };";
    for body in [
        "const o: any = { __proto__: Array.prototype }; const p: any = { x: 1 }; o.myHelper(p); return p.x;",
        "const o: any = {}; Object.setPrototypeOf(o, Array.prototype); const p: any = { x: 1 }; o.myHelper(p); return p.x;",
        "class A {} Object.setPrototypeOf(A.prototype, Array.prototype); const o: any = new A(); const p: any = { x: 1 }; o.myHelper(p); return p.x;",
        "function g(..._r: any[]) { const o: any = arguments; Object.setPrototypeOf(o, Array.prototype); const p: any = { x: 1 }; o.myHelper(p); return p.x; } return g(1);",
    ] {
        let pf = one(&program(store, body));
        let v = verdicts(&pf, "x", false);
        assert!(unproven(&v), "{body}: {v:?}\n{}", pf.dump_text());
    }
}
