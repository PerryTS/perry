//! DEC 250 on a member read off a primitive or read directly from
//! `Object.prototype`: a store into an unknown value may add a key to a
//! builtin prototype, but it never replaces an image member the lock
//! refuses to change (`members::locked_read`). Such a read is final: it
//! takes no opaque member and is not replayed when a key is written (as a
//! call of it already is, `prim_method`). `constructor` and a key a
//! prototype may still gain are not final.

use super::*;
use crate::model::prim;

/// One store into a value unknown code returns, under a computed key: it
/// may write any key of any builtin prototype the lock leaves open.
fn facts_of(src: &str) -> ProgramFacts {
    let src = format!(
        r#"
        export {{}};
        class Svc {{ size = 3; }}
        const f: any = (globalThis as any)[process.argv[2] ?? "Object"];
        const u: any = f(process.argv[3]);
        u[process.argv[4] ?? "z"] = (q: any) => q;
        {src}
        "#
    );
    let ast = perry_parser::parse_typescript(&src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    perry_transform::transform_generators(&mut hir);
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    compute(&[("t.ts".to_string(), &hir)], &cfg)
}

/// The `size` read on `x` (a `Svc` or what member read `m` gives): `.0`
/// `m` took the opaque member (⊤ and any primitive, the value a store into
/// ⊤ may have written: every primitive kind reaches `x`), `.1` the read is
/// refused for a receiver from unmodeled code.
fn reads_top(member: &str) -> (bool, bool, String) {
    let pf = facts_of(&format!(
        "const m: any = {member}; const x: any = process.argv[5] ? new Svc() : m; console.log(x.size);"
    ));
    let s = site(&pf, "size");
    let any = prim::NUM | prim::STR | prim::BOOL | prim::NULL | prim::UNDEF | prim::BIGINT | prim::SYMBOL;
    let opaque = s.receiver_prims & any == any;
    let top = matches!(&s.verdict, Verdict::No(w) if w.contains("unmodeled code"));
    let r = format!("{:?} {:#x} {:?}", s.receivers, s.receiver_prims, s.verdict);
    (opaque, top, r)
}

/// node: prints `undefined` or `3`. `length` of a string: an image member
/// of `String.prototype`, locked.
#[test]
fn a_locked_primitive_member_read_is_final() {
    let (opaque, top, r) = reads_top("\"abc\".length");
    assert!(!opaque && !top, "{r}");
}

/// node: prints `undefined` or `3`. `Object.prototype.toString`: an image
/// member of the add-locked `Object.prototype`. (The read stays refused:
/// `Object.prototype` is the token that stands for every builtin
/// prototype, and the global `Object` read is open here.) A plain
/// `builtin-fn` receiver is no sign of the opaque member: the open
/// `Object.prototype` may be any primitive, and `toString` of a bigint is
/// a builtin function value without a declared identity.
#[test]
fn a_locked_object_prototype_member_read_is_final() {
    let (opaque, _, r) = reads_top("Object.prototype.toString");
    assert!(!opaque, "{r}");
}

/// Controls: a key `String.prototype` may gain, and the exempt
/// `constructor`, may be what the store wrote: the read is ⊤.
#[test]
fn unlocked_member_reads_stay_open() {
    for member in ["\"abc\".zzq", "Object.prototype.constructor"] {
        let (_, top, r) = reads_top(member);
        assert!(top, "{member}: {r}");
    }
}
