//! BNSRECV: a member read or method call whose receiver is one builtin
//! namespace object or constructor (`Math.cos`, `Promise.all(...)`,
//! `Array.prototype`).
//! The realm creates each such member as an own property of the namespace
//! (ECMA-262 9.3.2 CreateIntrinsics; 21.3.2 Function Properties of the Math
//! Object; 27.2.4 Properties of the Promise Constructor; 23.1.2.4
//! Array.prototype): a read is one own data lookup (10.1.8.1 OrdinaryGet)
//! that finds the realm's own value while nothing has written, deleted or
//! redefined the key. The runtime's lock (perry-abi lock.rs `LockHolder`)
//! holds builtin PROTOTYPES only: no namespace object and no constructor is
//! a holder, so the runtime admits `Math.cos = f`, `delete Math.cos` and a
//! defineProperty there. The claim therefore needs every write the program
//! can make to be in the solve (`native_written`: a store of the key, a
//! store under a computed key, a write through ⊤) and no code the solve
//! does not see (unknown code holding the realm, interpreted code).
//! Compiled code: a direct call of a namespace static is the intrinsic
//! (perry-hir devirtualizes it unless `patched_builtins` records a write);
//! a value read is a generic property get on the namespace object.
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

/// Verdicts of the sites of `key` (calls when `call`), in source order.
fn at(pf: &ProgramFacts, key: &str, call: bool) -> Vec<String> {
    let mut s: Vec<_> = pf.sites.iter().filter(|s| s.key == key && s.is_call == call).collect();
    s.sort_by_key(|s| (s.module, s.byte_offset));
    let v: Vec<String> = s.iter().map(|s| format!("{:?} {:?}", s.verdict, s.receivers)).collect();
    eprintln!("BNSRECV {key} call={call}: {v:?}");
    v
}

fn all(v: &[String], k: &str) -> bool {
    !v.is_empty() && v.iter().all(|v| v.split(' ').next() == Some(k))
}
fn none_proven(v: &[String]) -> bool {
    !v.is_empty() && v.iter().all(|v| v.starts_with("No"))
}

// ---------------------------------------------------------------- shapes

#[test]
fn n1_namespace_static_read() {
    let pf = one("const cos = Math.cos;\nconsole.log(cos(1));\n");
    let v = at(&pf, "cos", false);
    assert!(all(&v, "NativeMember"), "{v:?}");
    assert!(v.iter().all(|v| v.contains("builtin:Math")), "{v:?}");
}

#[test]
fn n1_namespace_static_call() {
    let pf = one(
        "Promise.all([Promise.resolve(1)]).then((v) => console.log(v));\n",
    );
    assert!(all(&at(&pf, "all", true), "NativeMember"));
    assert!(all(&at(&pf, "resolve", true), "NativeMember"));
}

#[test]
fn n2_constructor_prototype_read() {
    let pf = one("const p = Array.prototype;\nconsole.log(typeof p);\n");
    assert!(all(&at(&pf, "prototype", false), "NativeMember"));
}

// -------------------------------------------------------------- controls

/// The program replaces the member: the runtime admits it (no namespace is
/// a `LockHolder`), the read finds the intrinsic or the program's function.
#[test]
fn c1_member_replaced() {
    let pf = one(
        "(Math as any).cos = (x: number) => x + 1;\n\
         const cos = Math.cos;\nconsole.log(cos(1));\n",
    );
    assert!(none_proven(&at(&pf, "cos", false)));
}

/// A store under a computed key that may be `cos`.
#[test]
fn c2_computed_store() {
    let pf = one(
        "const k = String(Date.now());\n(Math as any)[k] = (x: number) => x + 1;\n\
         const cos = Math.cos;\nconsole.log(cos(1));\n",
    );
    assert!(none_proven(&at(&pf, "cos", false)));
}

/// `delete Math.cos` (admitted: the property is configurable, 21.3.2).
#[test]
fn c3_member_deleted() {
    let pf = one(
        "delete (Math as any).cos;\n\
         const cos = Math.cos;\nconsole.log(cos);\n",
    );
    assert!(none_proven(&at(&pf, "cos", false)));
}

/// An accessor defined over the member.
#[test]
fn c4_member_redefined() {
    let pf = one(
        "Object.defineProperty(Math, \"cos\", { get() { return 1; } });\n\
         const cos = Math.cos;\nconsole.log(cos);\n",
    );
    assert!(none_proven(&at(&pf, "cos", false)));
}

/// Unknown code holds the realm: `Math` is no replace-locked holder, so
/// unknown code may replace `Math.cos` and the solve does not replay it.
/// (A replace-locked holder, a builtin prototype's image key, keeps its
/// claim: native_member.rs, `lock::refuses` lock.rs:97.)
#[test]
fn c5_realm_held() {
    let pf = one(
        "declare function sink(x: any): void;\nsink(0);\n\
         const cos = Math.cos;\nconsole.log(cos(1));\n",
    );
    assert!(none_proven(&at(&pf, "cos", false)));
    assert!(none_proven(&at(&pf, "all", true)) || at(&pf, "all", true).is_empty());
}

/// Interpreted code can name `Math` and write any member.
#[test]
fn c6_interpreted_code() {
    let pf = one(
        "eval(process.argv[2]);\nconst cos = Math.cos;\nconsole.log(cos(1));\n",
    );
    assert!(none_proven(&at(&pf, "cos", false)));
}

/// The namespace mixed with a program object: two receivers, no claim.
#[test]
fn c7_mixed_with_program_object() {
    let pf = one(
        "const o: any = process.argv[2] ? Math : { cos: (x: number) => x };\n\
         console.log(o.cos(1));\n",
    );
    assert!(none_proven(&at(&pf, "cos", true)));
}

/// `.call` on a value that may be a builtin function or a non-function.
#[test]
fn c8_call_on_function_or_object() {
    let pf = one(
        "const f: any = process.argv[2] ? Array.prototype.slice : { call: () => 1 };\n\
         console.log(f.call([1, 2]));\n",
    );
    assert!(none_proven(&at(&pf, "call", true)));
}
