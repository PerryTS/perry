// BIRTH1 (2026-11-18): RED fixtures for the two store/read reasons of the
// chess web3d rungs, small JS:
// A (rule GS): a named store of a program-owned global onto the global
//    object (`g["window"] = globalThis`, `g` = globalThis) was refused
//    "store: receiver is not a program class instance";
// (B, next commit): a read of a key a class instance never holds, on a class that
//    declares getters of OTHER keys and whose layout is not fixed (a
//    conditional store in a method), was refused "key not a definite birth
//    field": `own_data_or_absent` refused every class with any accessor.
// KEEP: a getter of the key itself still decides the read; a defined
//    global key and an Object.prototype key still refuse rule GS.
use super::*;
use perry_hir::Module;

fn facts(src: &str) -> crate::ProgramFacts {
    facts_cfg(src, None)
}

fn facts_cfg(src: &str, dynamic_code: Option<String>) -> crate::ProgramFacts {
    let path = "a.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "a", &path).expect("lowers");
    let h = [(path, hir)];
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    crate::compute(
        &list,
        &FactsConfig {
            unit_io: true,
            dynamic_code,
            ..Default::default()
        },
    )
}

fn verdicts(pf: &crate::ProgramFacts, key: &str, store: bool) -> Vec<crate::Verdict> {
    let mi = pf
        .module_paths
        .iter()
        .position(|p| p == "a.ts")
        .expect("a.ts") as u32;
    pf.sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && s.access.is_store() == store)
        .map(|s| s.verdict.clone())
        .collect()
}

fn no(v: &crate::Verdict) -> bool {
    matches!(v, crate::Verdict::No(_))
}

#[test]
fn birth1_a_global_user_store() {
    let pf = facts(
        "const g = globalThis as Record<string, unknown>;\n\
         g[\"myWindow\"] = globalThis;\n\
         g[\"myAudio\"] = 3;\n\
         export function f(): unknown { return g[\"myAudio\"]; }\n",
    );
    for k in ["myWindow", "myAudio"] {
        let v = verdicts(&pf, k, true);
        eprintln!("BIRTH1 A .{k} {v:?}");
        assert!(!v.is_empty() && v.iter().all(|v| !no(v)), "{k}: {v:?}");
    }
}

#[test]
fn birth1_a_keep_frozen_global() {
    let pf = facts(
        "const g = globalThis as Record<string, unknown>;\n\
         Object.freeze(globalThis);\n\
         g[\"myWindow\"] = 1;\n",
    );
    let v = verdicts(&pf, "myWindow", true);
    eprintln!("BIRTH1 A-keep {v:?}");
    assert!(v.iter().all(no), "{v:?}");
}

// KEEP (rule GS): code the program cannot see may overwrite a program-owned
// global key: interpreted code (`new Function` of a run-time string) and,
// in unit mode, the stores of another unit. R-GLOBAL (owner 2026-11-18)
// covers only unknown code holding the realm in whole-program mode. The
// interpreter is linked (`dynamic_code`): without it no interpreted code
// runs, and the refusal came from the call of the constructed value holding
// the realm, which replaces no global under R-GLOBAL step 2 (owner
// 2026-11-18, closed world 2026-11-14).
#[test]
fn birth1_a_keep_interpreted_code() {
    let pf = facts_cfg(
        "const g = globalThis as Record<string, unknown>;\n\
         new Function(process.argv[2])();\n\
         g[\"myWindow\"] = 1;\n",
        Some("BIRTH1 interpreted code".into()),
    );
    let v = verdicts(&pf, "myWindow", true);
    eprintln!("BIRTH1 A-keep-interp {v:?}");
    assert!(!v.is_empty() && v.iter().all(no), "{v:?}");
}

#[test]
fn birth1_a_keep_unit_mode() {
    let src = "const g = globalThis as Record<string, unknown>;\n\
               g[\"myWindow\"] = 1;\n";
    let path = "a.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "a", &path).expect("lowers");
    let h = [(path, hir)];
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let pf = crate::compute(
        &list,
        &FactsConfig {
            unit: true,
            unit_io: true,
            ..Default::default()
        },
    );
    let v = verdicts(&pf, "myWindow", true);
    eprintln!("BIRTH1 A-keep-unit {v:?}");
    assert!(v.iter().all(no), "{v:?}");
}

// KEEP (rule B): a getter of the key on a class the receiver's class
// extends decides the read (the chain holds the key).
// KEEP (rule B): a computed member on the class may be the key.
