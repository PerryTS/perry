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
    let mi = pf
        .module_paths
        .iter()
        .position(|p| p == "a.ts")
        .expect("a.ts") as u32;
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
    !v.is_empty()
        && v.iter()
            .all(|x| x.starts_with("T0") && x.ends_with("top=false"))
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
    assert!(
        !q.is_empty()
            && q.iter()
                .all(|x| !x.starts_with("T0") || x.ends_with("top=true")),
        "e.data.q is unknown"
    );
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
    assert!(
        !v.is_empty() && v.iter().all(|x| !x.starts_with("Dead")),
        "the getter's store reaches hh.fn"
    );
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

// W-HOST (LADDER18): the host Worker is a fresh host object no unknown code
// holds. The runtime calls only the listeners the program installed on it
// (`onmessage` / `onerror` / `onmessageerror`, `addEventListener`, `on`),
// with events whose `data` is a structured clone (plain data, never a
// program object); `postMessage` reads its value (getters run) and stores
// nothing into it (perry-stdlib worker_threads worker_pump.rs,
// thread/clone_write.rs; HTML Workers, StructuredSerialize).

const LISTEN: &str = "(globalThis as any).g = new M(); console.log(String((globalThis as any).g.v));";

// R2 (RED on base): an `onmessage` handler that stores a program object on
// a global: the class sites stay proven.
#[test]
fn w3h_r2_onmessage_listener_keeps_realm() {
    let pf = facts(&format!(
        "{CORE}const w = new Worker(new URL(\"./b.ts\", import.meta.url), {{ type: \"module\" }});\n\
         w.onmessage = (e: any) => {{ {LISTEN} }};\n{TAIL}"
    ));
    assert!(all_proven(&pf, "v"), "m.v / g.v proven beside an onmessage handler");
}

// R3 (RED on base): the same through `addEventListener`.
#[test]
fn w3h_r3_add_event_listener_keeps_realm() {
    let pf = facts(&format!(
        "{CORE}const w = new Worker(new URL(\"./b.ts\", import.meta.url), {{ type: \"module\" }});\n\
         w.addEventListener(\"message\", (e: any) => {{ {LISTEN} }});\n{TAIL}"
    ));
    assert!(all_proven(&pf, "v"), "m.v / g.v proven beside addEventListener");
}

// R4 (RED on base): `event.data` is a clone, not a program object: a store
// into it does not reach the class's instances.
#[test]
fn w3h_r4_event_data_is_not_a_program_object() {
    let pf = facts(&format!(
        "{CORE}const w = new Worker(new URL(\"./b.ts\", import.meta.url));\n\
         w.onmessage = (e: any) => {{ e.data.v = \"s\"; e.data.list.push(e.target); }};\n{TAIL}"
    ));
    assert!(all_proven(&pf, "v"), "m.v proven beside a store into event.data");
}

// K4 (KEEP): `postMessage` clones its value: an own getter of the value
// runs, what it stores is seen and the stored callee is called.
#[test]
fn w3h_k4_post_message_runs_getters() {
    let pf = facts(&format!(
        "{CORE}class H {{ fn: any = null; }}\nconst hh = new H();\n\
         const w = new Worker(\"./b.ts\");\n\
         w.postMessage({{ get x() {{ hh.fn = () => String(process.argv[2]); return 1; }} }});\n\
         if (hh.fn !== null) console.log(hh.fn());\n{TAIL}"
    ));
    let v = verdicts(&pf, "fn", false);
    assert!(
        !v.is_empty() && v.iter().all(|x| !x.starts_with("Dead")),
        "the getter's store reaches hh.fn"
    );
}

// K5 (KEEP): every kind of listener runs: what it stores is seen.
#[test]
fn w3h_k5_listeners_run() {
    for reg in [
        "w.onmessage = () => { hh.fn = () => String(process.argv[2]); };",
        "w.onerror = () => { hh.fn = () => String(process.argv[2]); };",
        "w.addEventListener(\"message\", () => { hh.fn = () => String(process.argv[2]); });",
        "w.on(\"message\", () => { hh.fn = () => String(process.argv[2]); });",
        "w.once(\"exit\", () => { hh.fn = () => String(process.argv[2]); });",
    ] {
        let pf = facts(&format!(
            "{CORE}class H {{ fn: any = null; }}\nconst hh = new H();\n\
             const w: any = new Worker(\"./b.ts\");\n{reg}\n\
             if (hh.fn !== null) console.log(hh.fn());\n{TAIL}"
        ));
        let v = verdicts(&pf, "fn", false);
        assert!(
            !v.is_empty() && v.iter().all(|x| !x.starts_with("Dead")),
            "the listener's store reaches hh.fn: {reg}"
        );
    }
}

// K6 (KEEP): a program object stored into the Worker is no escape.
#[test]
fn w3h_k6_store_into_worker_is_no_escape() {
    let pf = facts(&format!(
        "{CORE}const w: any = new Worker(\"./b.ts\");\nconst mm = new M();\nw.foo = mm;\n\
         console.log(String(mm.v));\n{TAIL}"
    ));
    assert!(all_proven(&pf, "v"), "m.v proven beside w.foo = obj");
}

// R5 (RED on the record event): a message event is a MessageEvent, so its
// Event.prototype members are found on its chain: `e.preventDefault()`
// does not throw, and the store after it runs.
#[test]
fn w3h_r5_message_event_has_event_members() {
    let pf = facts(&format!(
        "{CORE}class H {{ n = 0; }}\nconst hh = new H();\n\
         const w = new Worker(new URL(\"./b.ts\", import.meta.url));\n\
         w.onmessage = (e: any) => {{ e.preventDefault(); hh.n = String(e.type).length; }};\n\
         console.log(hh.n);\n{TAIL}"
    ));
    let pd = verdicts(&pf, "preventDefault", false);
    let n = verdicts(&pf, "n", true);
    assert!(!pd.is_empty(), "e.preventDefault is read");
    assert!(
        !n.is_empty() && n.iter().all(|x| !x.starts_with("Dead")),
        "the store after e.preventDefault() runs"
    );
}
