//! W3HOST4 (2026-11-16). Item 1: the `process` data keys the runtime always
//! answers with one primitive type (`execPath`, `argv0`, `title`: String;
//! `pid`, `ppid`: Number; perry-runtime process.rs `process_property`,
//! os.rs `js_process_pid` / `js_process_ppid`, process/attributes.rs
//! `js_process_title`) are declared with that type in the realm manifest
//! (`runtime(process.execPath, string)`); a read of such a key is a value of
//! that type unless a write of the key may reach it (ECMA-262 6.1, 10.1.8.1
//! OrdinaryGet on the process object's own data property).
use super::*;

fn facts_of(src: &str) -> ProgramFacts {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::compute(&list, &cfg)
}

/// The verdict of the first site `.key` after `marker`, and the dump.
fn verdict(src: &str, key: &str, marker: &str) -> (Verdict, String) {
    let pf = facts_of(src);
    let at = src.find(marker).expect("marker") as u32;
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("site .{key} after {marker:?} in\n{}", pf.dump_text()));
    (s.verdict.clone(), pf.dump_text())
}

/// The reasons of the unknown values the solve created (explain on) and
/// whether unknown code holds the realm.
fn top_origins(src: &str) -> (Vec<String>, bool) {
    let ast = Box::leak(Box::new(
        perry_parser::parse_typescript(src, "t.ts").unwrap(),
    ));
    let hir = Box::leak(Box::new(perry_hir::lower_module(ast, "t", "t.ts").unwrap()));
    let mut g = crate::gen::Gen::new(vec![("t.ts", &*hir)], false, true);
    g.m.max_work = 50_000_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    (g.m.top_vals.keys().cloned().collect(), g.m.realm_held)
}

// 1-R1 (RED on base): execPath / argv0 / title read as Strings, nothing unknown.
#[test]
fn w3k_1_r1_process_string_keys_are_strings() {
    let src = "const e = process.execPath; const a = process.argv0; const t = process.title;\n\
               const d = e.slice(0, e.lastIndexOf(\"/\") + 1);\n\
               console.log(d.length, a.length, t.toUpperCase());\n";
    let (tops, held) = top_origins(src);
    assert!(
        !tops.iter().any(|t| t.starts_with("process.")),
        "no unknown process value: {tops:?}"
    );
    assert!(!held, "realm not held");
    let (v, dump) = verdict(src, "lastIndexOf", "e.lastIndexOf");
    assert_eq!(
        v,
        Verdict::NativeMember,
        "String.prototype.lastIndexOf\n{dump}"
    );
}

// 1-K0 (KEEP: the bare `process.pid` lowers to its own HIR read): pid / ppid
// read as Numbers.
#[test]
fn w3k_1_k0_process_pid_is_a_number() {
    let src = "const p = process.pid; const q = process.ppid;\nconsole.log(p.toFixed(0), q.toFixed(0));\n";
    let (tops, held) = top_origins(src);
    assert!(
        !tops.iter().any(|t| t.starts_with("process.")),
        "no unknown process value: {tops:?}"
    );
    assert!(!held, "realm not held");
    let (v, dump) = verdict(src, "toFixed", "p.toFixed");
    // A Number receiver (no unknown value): the primitive-receiver verdict.
    assert_eq!(
        format!("{v:?}"),
        "No(\"primitive receiver\")",
        "a Number\n{dump}"
    );
    assert!(
        dump.contains(".toFixed call=1 fixed=1 recv={} top=0"),
        "{dump}"
    );
}

const OBJ: &str = "class O { v = 1; }\nconst o: any = new O();\n";

// 1-K1 (KEEP): a program store of the key (taken by argv) is what the read sees.
#[test]
fn w3k_1_k1_program_store_then_read() {
    let src = format!(
        "{OBJ}if (process.argv[2]) (process as any).execPath = o;\n\
         const r: any = process.execPath;\nconsole.log(r.v);\n"
    );
    let (v, dump) = verdict(&src, "v", "r.v");
    assert_ne!(
        v,
        Verdict::NativeMember,
        "the stored object may be read\n{dump}"
    );
    assert_ne!(v, Verdict::Dead, "the read runs\n{dump}");
    let (tops, _) = top_origins(&src);
    assert!(!tops.is_empty() || dump.contains("O"), "{tops:?}\n{dump}");
}

// 1-K2 (KEEP): `Object.defineProperty( process, "pid", { get } )`: the
// getter's result is what the read sees.
#[test]
fn w3k_1_k2_define_property_pid() {
    let src = format!(
        "{OBJ}Object.defineProperty(process, \"pid\", {{ get() {{ return o; }} }});\n\
         const r: any = process.pid;\nconsole.log(r.v);\n"
    );
    let (v, dump) = verdict(&src, "v", "r.v");
    assert_ne!(
        v,
        Verdict::NativeMember,
        "the getter's object may be read\n{dump}"
    );
    assert_ne!(v, Verdict::Dead, "the read runs\n{dump}");
}

// 1-K3 (KEEP): a store under an unknown key may write `title`.
#[test]
fn w3k_1_k3_unknown_key_store() {
    let src = format!(
        "{OBJ}(process as any)[process.argv[2] ?? String(Math.random())] = o;\n\
         const r: any = process.title;\nconsole.log(r.v);\n"
    );
    let (v, dump) = verdict(&src, "v", "r.v");
    assert_ne!(
        v,
        Verdict::NativeMember,
        "the stored object may be read\n{dump}"
    );
    assert_ne!(v, Verdict::Dead, "the read runs\n{dump}");
}
