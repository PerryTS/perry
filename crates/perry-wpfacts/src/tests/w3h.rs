// W3HOST (2026-11-16): `new Worker(filename, options)` (HIR WorkerNew) is a
// host construct the model runs exactly: the filename is converted to a
// string, the options are read by [[Get]] (getters run) and cloned, and the
// result is a fresh host Worker object. Its operands no longer reach
// "unmodeled code", so the realm's builtin prototypes stay unwritten
// (HTML Living Standard, Worker constructor; StructuredSerialize; ECMA-262
// 7.1.17 ToString, 10.1.8.1 OrdinaryGet).
use super::*;
use perry_hir::Module;

fn facts(src: &str) -> crate::ProgramFacts {
    let path = "a.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "a", &path).expect("lowers");
    let h = vec![(path, hir)];
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    crate::compute(&list, &cfg)
}

fn verdicts(pf: &crate::ProgramFacts, key: &str, store: bool) -> Vec<String> {
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    let v: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && s.access.is_store() == store)
        .map(|s| format!("{:?} top={}", s.verdict, s.top))
        .collect();
    eprintln!("W3H .{key} store={store} verdicts={v:?}");
    v
}

fn all_proven(pf: &crate::ProgramFacts, key: &str) -> bool {
    let v = verdicts(pf, key, false);
    !v.is_empty() && v.iter().all(|x| x.starts_with("T0") && x.ends_with("top=false"))
}

const CORE: &str = "\"use strict\";\n\
class M { v = 0; }\n\
function h(): number { return 1; }\n\
function f(x: string) { const m = new M(); m.v = x.length; const g = h.bind(null); \
return [1, 2].map((i: number) => m.v + i + g()); }\n";

const TAIL: &str = "console.log(JSON.stringify(f(String(process.argv[2] ?? Math.random()))));\n";

// R1 (RED on base): a Worker construct with a URL filename and an options
// object; the class sites read in a builtin callback stay proven.
#[test]
fn w3h_r1_worker_operands_do_not_write_the_realm() {
    let pf = facts(&format!(
        "{CORE}const w = new Worker(new URL(\"./b.ts\", import.meta.url), {{ type: \"module\" }});\n{TAIL}"
    ));
    assert!(all_proven(&pf, "v"), "m.v proven beside a Worker construct");
}

// K1 (KEEP): the listener's event data is a clone of unknown shape: storing
// it on a global and reading a field of it is never proven.
#[test]
fn w3h_k1_listener_data_stays_unknown() {
    let pf = facts(&format!(
        "{CORE}const w = new Worker(new URL(\"./b.ts\", import.meta.url));\n\
         w.onmessage = (e: any) => {{ (globalThis as any).last = e.data; console.log(String((globalThis as any).last.q)); }};\n{TAIL}"
    ));
    let q = verdicts(&pf, "q", false);
    assert!(!q.is_empty() && q.iter().all(|x| !x.starts_with("T0") || x.ends_with("top=true")), "e.data.q is unknown");
}

// K2 (KEEP): an options getter runs at construction ([[Get]] of the option):
// what it stores is seen; the stored callee is called.
#[test]
fn w3h_k2_options_getter_runs() {
    let pf = facts(&format!(
        "{CORE}class H {{ fn: any = null; }}\nconst hh = new H();\n\
         const w = new Worker(\"./b.ts\", {{ get workerData() {{ hh.fn = () => String(process.argv[2]); return 1; }} }});\n\
         if (hh.fn !== null) console.log(hh.fn());\n{TAIL}"
    ));
    let v = verdicts(&pf, "fn", false);
    assert!(!v.is_empty() && v.iter().all(|x| !x.starts_with("Dead")), "the getter's store reaches hh.fn");
}

// K3 (KEEP): a closure handed to perry/thread spawn that captures an object
// mutated after the call: the object's sites stay sound (not proven by a
// model of the spawn; the runtime runs the closure on copies, accessors
// cross as undefined).
#[test]
fn w3h_k3_spawn_capture_mutated_after() {
    let pf = facts(&format!(
        "import {{ spawn }} from \"perry/thread\";\n{CORE}\
         class C {{ get z(): number {{ return 2; }} }}\nconst c: any = new C();\n\
         spawn(() => c.z);\nc.z2 = String(process.argv[2]);\n{TAIL}"
    ));
    let z = verdicts(&pf, "z", false);
    assert!(!z.is_empty(), "the spawned read exists");
    assert!(all_proven(&pf, "v"), "the core stays proven beside spawn");
}

