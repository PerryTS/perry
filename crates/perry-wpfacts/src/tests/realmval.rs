//! REALMVAL: a read of a realm member is the value its declaration gives
//! (the generated realm table, from the runtime's `realm_decl!` sites), never
//! ⊤; a computed key reads the union of the declared members it may spell
//! and the program's stores.
use super::*;
use crate::model::prim;
use perry_hir::realm_manifest::RealmKind;

/// The kind of each `h.k<i> = reads[i]` after `prelude`.
fn kinds(prelude: &str, reads: &[String]) -> Vec<crate::derive::KindFact> {
    let mut src = String::from("class H {");
    for i in 0..reads.len() {
        src += &format!(" k{i}: any = 0;");
    }
    src += " }\nconst h = new H();\n";
    src += prelude;
    for (i, r) in reads.iter().enumerate() {
        src += &format!("h.k{i} = {r};\nconsole.log(h.k{i});\n");
    }
    let pf = facts(&src);
    let t = tmpl(&pf, "H");
    (0..reads.len())
        .map(|i| field(t, &format!("k{i}")).kind.clone())
        .collect()
}

/// The reads of `reads` that hold ⊤.
fn top_reads(prelude: &str, reads: Vec<String>) -> Vec<String> {
    let ks = kinds(prelude, &reads);
    reads
        .into_iter()
        .zip(ks)
        .filter(|(_, k)| k.top)
        .map(|(r, _)| r)
        .collect()
}

fn own_keys(owner: &str) -> Vec<&'static str> {
    crate::builtins::members::realm_entry(owner)
        .unwrap_or_else(|| panic!("{owner} declared"))
        .own
        .iter()
        .filter(|p| !p.symbol && p.key != "__proto__")
        .map(|p| p.key)
        .collect()
}

/// `globalThis[k]` under a key the solver knows nothing of: the declared
/// members (NaN: a number, `undefined`), the program's store
/// (`globalThis.myG = 1`) and `undefined` (absent), not ⊤ (was: loads.rs
/// `globalThis.[computed]`, the c09/m2 stop).
#[test]
fn realmval_global_computed_reads_declared_and_program_values() {
    let ks = kinds(
        "(globalThis as any).myG = 1;\nconst k: string = String(Date.now());\n",
        &["(globalThis as any)[k]".to_string()],
    );
    assert!(!ks[0].top, "{:?}", ks[0]);
    assert_eq!(
        ks[0].prims & (prim::NUM | prim::UNDEF),
        prim::NUM | prim::UNDEF,
        "{:?}",
        ks[0]
    );
}

/// Every member `%Global%` owns reads its declared value, by
/// `globalThis.k` and by the bare name.
#[test]
fn realmval_every_global_member_has_a_value() {
    let keys = own_keys("%Global%");
    assert!(keys.len() > 100, "{keys:?}");
    let mut reads: Vec<String> = keys
        .iter()
        .map(|k| format!("(globalThis as any)[{k:?}]"))
        .collect();
    reads.extend(
        keys.iter()
            .filter(|k| !matches!(**k, "eval"))
            .map(|k| format!("(({k}) as any)")),
    );
    let tops = top_reads("", reads);
    assert!(tops.is_empty(), "⊤ reads: {tops:?}");
}

/// Every member of `process` and of every node module the realm declares
/// reads its declared value (a primitive of its kind, a host string array
/// or record, a native function, a namespace, a declared getter's result);
/// a computed key reads their union.
#[test]
fn realmval_every_host_member_has_a_value() {
    let mut reads: Vec<String> = own_keys("process")
        .iter()
        .map(|k| format!("(process as any)[{k:?}]"))
        .collect();
    reads.push("(process as any)[k]".into());
    let mut prelude = String::from("const k: string = String(Date.now());\n");
    let mods: Vec<&'static str> = crate::native_effects::realm()
        .iter()
        .filter(|e| {
            e.kind == RealmKind::Namespace
                && e.name.starts_with("node:")
                && !e.name.contains(['.', '@', '/'])
                && !e.name.starts_with("node:__")
        })
        .map(|e| e.name)
        // A module `perry compile` resolves (HIR's builtin module set; an
        // npm-provided namespace such as `node:cheerio` lowers only from its
        // package).
        .filter(|md| {
            let src = format!("import * as M from {md:?};\nconsole.log(M);\n");
            let module = perry_parser::parse_typescript(&src, "t.ts").expect("parses");
            perry_hir::lower_module(&module, "t", "t.ts").is_ok()
        })
        .collect();
    assert!(mods.len() > 20, "{mods:?}");
    for (i, md) in mods.iter().enumerate() {
        prelude += &format!("import * as M{i} from {md:?};\n");
        for key in own_keys(md) {
            reads.push(format!("(M{i} as any)[{key:?}]"));
        }
        reads.push(format!("(M{i} as any)[k]"));
    }
    let tops = top_reads(&prelude, reads);
    assert!(tops.is_empty(), "{} ⊤ reads: {tops:?}", tops.len());
}

/// `Blob.prototype.type` is a getter whose declaration returns a string
/// (proto_methods.rs `Blob.prototype.type.<get>`, `returns(prim(str))`).
#[test]
fn realmval_blob_type_reads_its_declared_result() {
    let ks = kinds("", &["new Blob([]).type".to_string()]);
    assert!(!ks[0].top, "{:?}", ks[0]);
    assert_eq!(ks[0].prims & prim::STR, prim::STR, "{:?}", ks[0]);
}

/// STOP3 (c09 stop, builtins.rs:430 `argument of builtin Module("Atomics")
/// .toJSON`): a named-key call on a host namespace object calls what [[Get]]
/// of the key reads. `Atomics` and `Intl` own no `toJSON`/`foo` (the read is
/// `undefined`, the call a TypeError: no value), and `toString` is
/// `Object.prototype.toString` (a string): no result is ⊤.
#[test]
fn stop3_host_namespace_call_calls_the_read_value() {
    let reads: Vec<String> = [
        "(Atomics as any).toJSON()",
        "(Atomics as any).foo(1)",
        "(Intl as any).toJSON()",
        "(Atomics as any).toString()",
        "(process as any).toJSON()",
        "(globalThis as any).toJSON()",
        // NSPRIM: ToPrimitive looks up `@@toPrimitive` first (7.1.1), a
        // named key read like any other: absent, so `valueOf`/`toString`.
        "+(Atomics as any)",
        "`${Atomics as any}`",
        "(Intl as any) + \"\"",
        "+(process as any)",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(top_reads("", reads), Vec::<String>::new());
}

/// STOP5 (c09 rule-wide): a named-key call on the global object or
/// `process` calls what [[Get]] reads: the callee a program stored there
/// runs (its result is the argument, a number), and a key neither the realm
/// declares nor the program stores has no callee (no result).
#[test]
fn stop5_host_object_call_runs_the_stored_callee() {
    let prelude = "(globalThis as any).idg = (x: any) => x;\n\
                   (process as any).idp = (x: any) => x;\n";
    let ks = kinds(
        prelude,
        &[
            "(globalThis as any).idg(7)".to_string(),
            "(process as any).idp(7)".to_string(),
        ],
    );
    for k in &ks {
        assert!(!k.top, "{k:?}");
        assert_eq!(k.prims & prim::NUM, prim::NUM, "{k:?}");
    }
    assert_eq!(
        top_reads(
            "",
            vec![
                "(process as any).someAbsentKey(1)".to_string(),
                "(globalThis as any).someAbsentKey(1)".to_string(),
            ]
        ),
        Vec::<String>::new()
    );
}
