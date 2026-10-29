//! Per-unit mode (decision 151 P1): the other units are unknown compiled
//! code. What a unit publishes reaches them, what they may do comes back.

use super::*;

/// Design §16: the owner performed another unit's new key on one of its
/// literal layouts: the layout adds a key whose field holds an unknown value.
pub(super) fn gains_key(pf: &ProgramFacts, key: &str) -> bool {
    pf.templates.iter().any(|t| {
        t.adds_keys > 0
            && t.fields
                .iter()
                // Rewritten for DECISIONS 73 (principle: a store effect carries the kinds
                // it stores): the field holds the stored primitive, not ⊤.
                .any(|f| f.key == key && !f.readonly && f.writers_post > 0)
    })
}

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    pf.sites
        .iter()
        .filter(|s| s.key == key)
        .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

fn units(src: &str) -> ProgramFacts {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    crate::compute_units(&[("t.ts".to_string(), &hir)], &FactsConfig::default())
}

#[path = "unit_mode/contracts.rs"]
mod contracts;

/// The modules the units belong to (a unit path is `module#uK`, §12.3).
fn modules_of_units(pf: &ProgramFacts) -> Vec<String> {
    let mut m: Vec<String> = pf
        .units
        .iter()
        .map(|(p, _)| crate::decl_units::module_of(p).to_string())
        .collect();
    m.sort();
    m.dedup();
    m
}

/// The work of every unit of module `path` (its declaration units, §12.3).
fn module_work(pf: &ProgramFacts, path: &str) -> u64 {
    let w: Vec<u64> = pf
        .units
        .iter()
        .filter(|(q, _)| crate::decl_units::module_of(q) == path)
        .map(|(_, s)| s.work)
        .collect();
    assert!(!w.is_empty(), "no unit of {path}: {:?}", pf.units);
    w.iter().sum()
}

/// Several modules, one unit each; `./x` imports resolve to `x.ts`.
fn units_of(mods: &[(&str, &str)]) -> ProgramFacts {
    units_with(mods, &FactsConfig::default())
}

fn units_with(mods: &[(&str, &str)], cfg: &FactsConfig) -> ProgramFacts {
    let hirs = hirs_of(mods);
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    crate::compute_units(&list, cfg)
}

/// The same modules solved as one whole program: what per-unit mode must
/// reach (owner 2026-10-23).
pub(super) fn whole_of(mods: &[(&str, &str)]) -> ProgramFacts {
    let hirs = hirs_of(mods);
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    crate::compute(&list, &FactsConfig::default())
}

/// The proven sites, named by module path, offset and key.
fn proven_set(pf: &ProgramFacts) -> std::collections::BTreeSet<String> {
    pf.sites
        .iter()
        .filter(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
        .map(|s| {
            format!(
                "{}@{}.{}",
                pf.module_paths[s.module as usize], s.byte_offset, s.key
            )
        })
        .collect()
}

/// Per unit proves at least what the whole program proves (owner
/// 2026-10-23), and the census breaks nothing. An `override k of C` report
/// breaks nothing: it is the key a subclass of another unit holds, which
/// `C`'s unit reads through its override token (DECISIONS 59); what it
/// costs shows in `lost`.
fn assert_parity(mods: &[(&str, &str)]) {
    assert_parity_but(mods, &[]);
}

/// `assert_parity` where per unit is known to prove exactly `gaps` less
/// than the whole program (each named where it is used): it fails when
/// more is lost, and when a gap closes.
fn assert_parity_but(mods: &[(&str, &str)], gaps: &[&str]) {
    let (pf, wp) = (units_of(mods), whole_of(mods));
    let lost: Vec<_> = proven_set(&wp)
        .difference(&proven_set(&pf))
        .filter(|s| !gaps.contains(&s.as_str()))
        .cloned()
        .collect();
    let closed: Vec<_> = gaps
        .iter()
        .filter(|g| proven_set(&pf).contains(**g) || !proven_set(&wp).contains(**g))
        .collect();
    assert!(closed.is_empty(), "named gaps no longer lost: {closed:?}");
    let broken: Vec<_> = pf
        .broken
        .iter()
        .filter(|d| !d.starts_with("override "))
        .collect();
    assert!(!proven_set(&wp).is_empty(), "{}", wp.dump_text());
    assert!(
        lost.is_empty() && broken.is_empty(),
        "lost {lost:?} broken {broken:?}\n{}",
        pf.dump_text()
    );
}

fn hirs_of(mods: &[(&str, &str)]) -> Vec<(String, perry_hir::Module)> {
    mods.iter()
        .map(|(name, src)| {
            let path = format!("{name}.ts");
            let module = perry_parser::parse_typescript(src, &path).expect("parses");
            let mut hir = perry_hir::lower_module(&module, name, &path).expect("lowers");
            for imp in &mut hir.imports {
                if let Some(n) = imp.source.strip_prefix("./") {
                    imp.resolved_path = Some(format!("{n}.ts"));
                }
            }
            (path, hir)
        })
        .collect()
}

/// The census broke the literal layout born with keys `keys`.
fn literal_broken(pf: &ProgramFacts, keys: &[&str]) -> bool {
    pf.templates
        .iter()
        .filter(|t| t.birth_keys == keys)
        .any(|t| pf.broken.contains(&t.display))
}

fn verdicts(pf: &ProgramFacts, key: &str) -> Vec<String> {
    pf.sites
        .iter()
        .filter(|s| s.key == key)
        .map(|s| format!("{:?}", s.verdict))
        .collect()
}

const DUAL: &str = r#"
    export class D {
        pos: Map<number, number>; neg: Map<number, number>;
        constructor(o: number[]) { this.pos = new Map(); this.neg = new Map(); o.forEach((x) => { this.pos.set(x, x); }); }
        has(k: number) { return this.neg.has(k); }
    }
    "#;

/// The rounds of the larger of two independent pairs solved alone: the
/// breaking pair `ab` and the unrelated `b`/`c` (`DUAL` as `E`).
fn alone_rounds(ab: &[(&str, &str)]) -> usize {
    let e = DUAL.replace("class D", "class E").replace("neg", "other");
    let bc = units_of(&[
        ("b", &e),
        (
            "c",
            "import { E } from './b'; const e = new E([1]); console.log(e.has(1));",
        ),
    ]);
    units_of(ab).round_work.len().max(bc.round_work.len())
}

const BENCH: &str = r#"
    export interface Iters { n: number; warm: number; }
    export function iters(d: number, w: number): Iters { return { n: d, warm: w }; }
    export function header(name: string, it: Iters): void { console.log(name + " n=" + it.n + " warm=" + it.warm); }
    "#;

#[path = "unit_mode/markers.rs"]
mod markers;

/// The points-to entries of the largest unit: `n` exported functions that
/// share a helper storing one argument into the other.
fn marker_store_entries(n: usize) -> u64 {
    let mut src = String::from(
        "class S { static last: any = 0; }\nfunction h(a: any, b: any) { a.x = b; S.last = b; }\nfunction mid(a: any, b: any) { h(a, b); return a.x; }\n",
    );
    for i in 0..n {
        src.push_str(&format!(
            "export function f{i}(a: any, b: any) {{ S.last = a; return mid(a, b); }}\n"
        ));
    }
    // Another module calls every export: each is a contract (§11.1).
    let names: Vec<String> = (0..n).map(|i| format!("f{i}")).collect();
    let calls: Vec<String> = names
        .iter()
        .map(|f| format!("console.log({f}(1, 2));"))
        .collect();
    let user = format!(
        "import {{ {} }} from './t'; {}",
        names.join(", "),
        calls.join(" ")
    );
    let pf = units_of(&[("t", src.as_str()), ("u", user.as_str())]);
    pf.units
        .iter()
        .map(|(_, s)| s.pts_entries)
        .max()
        .unwrap_or(0)
}

/// Pts entries of the largest unit when `n` exported functions pass their
/// argument through a wrapper into a shared helper that stores into it
/// (the contract's escape) and returns it; `S.last` (the class's state: a
/// store through a class name joins it) keeps them in one unit.
fn escaped_marker_entries(n: usize) -> u64 {
    let mut src = String::from(
        "class S { static last: any = 0; }\nfunction h(a: any) { a.k = 1; S.last = 0; return a; }\nfunction mid(a: any) { return h(a); }\n",
    );
    for i in 0..n {
        src.push_str(&format!(
            "export function f{i}(a: any) {{ S.last = {i}; return mid(a); }}\n"
        ));
    }
    let names: Vec<String> = (0..n).map(|i| format!("f{i}")).collect();
    let calls: Vec<String> = names
        .iter()
        .map(|f| format!("console.log({f}(1));"))
        .collect();
    let user = format!(
        "import {{ {} }} from './t'; {}",
        names.join(", "),
        calls.join(" ")
    );
    let pf = units_of(&[("t", src.as_str()), ("u", user.as_str())]);
    pf.units
        .iter()
        .map(|(_, s)| s.pts_entries)
        .max()
        .unwrap_or(0)
}

/// A module whose exported `make` returns, besides its own literal, what
/// `put`'s caller passed: `held` delivers `put`'s marker to `make`'s
/// result before `poke`'s store records the escape (a node's successors
/// are served before its rules). `S`'s statics (the class's state: a store
/// through a class name joins it) keep them in one unit.
const HELD: &str = r#"
    class S { static held: any = 0; static seen = 0; }
    const lit = { n: 1 };
    export function put(p: any) { S.seen = 1; S.held = p; }
    export function poke() { S.seen = 2; S.held.k = 1; }
    export function make(x: any) { S.seen = 3; if (x) { return S.held; } return lit; }
    export function rd() { return lit.n; }
    "#;

/// Pts entries of the largest unit when `n` exported functions pass their
/// argument through a wrapper into a shared helper that only reads it and
/// returns it (no escape: the markers do not read as ⊤).
fn shared_helper_entries(n: usize) -> u64 {
    let mut src = String::from(
        "class S { static last: any = 0; }\nfunction h(a: any) { S.last = a.v; return a; }\nfunction mid(a: any) { return h(a); }\n",
    );
    for i in 0..n {
        src.push_str(&format!(
            "export function f{i}(a: any) {{ S.last = {i}; return mid(a); }}\n"
        ));
    }
    let names: Vec<String> = (0..n).map(|i| format!("f{i}")).collect();
    let calls: Vec<String> = names
        .iter()
        .map(|f| format!("console.log({f}({{ v: 1 }}));"))
        .collect();
    let user = format!(
        "import {{ {} }} from './t'; {}",
        names.join(", "),
        calls.join(" ")
    );
    let pf = units_of(&[("t", src.as_str()), ("u", user.as_str())]);
    pf.units
        .iter()
        .map(|(_, s)| s.pts_entries)
        .max()
        .unwrap_or(0)
}

/// Every function writes `S.last`, so they are one unit (the writers of a
/// binding join its declaration, decl_units.rs).
const JOIN: &str = "class S { static last: any = 0; }\n";

/// Pts entries of the largest unit when `n` exported functions each read
/// their own key of what a shared helper returns, and then any key of it.
fn relative_result_entries(n: usize) -> u64 {
    let mut src = format!(
        "{JOIN}function h(a: any) {{ S.last = 0; return a; }}\nfunction id(a: any) {{ S.last = 1; return h(a); }}\n"
    );
    for i in 0..n {
        src.push_str(&format!(
            "export function f{i}(a: any, k: string) {{ S.last = {i}; const r = id(a); const x = r.k{i}; return [x, r[k]]; }}\n"
        ));
    }
    let names: Vec<String> = (0..n).map(|i| format!("f{i}")).collect();
    let calls: Vec<String> = names
        .iter()
        .map(|f| format!("console.log({f}({{ v: 1 }}, 'v'));"))
        .collect();
    let user = format!(
        "import {{ {} }} from './t'; {}",
        names.join(", "),
        calls.join(" ")
    );
    let pf = units_of(&[("t", src.as_str()), ("u", user.as_str())]);
    pf.units
        .iter()
        .map(|(_, s)| s.pts_entries)
        .max()
        .unwrap_or(0)
}

/// One exported function `both` of `b.ts` (one unit: everything below is
/// its body), whose inner callers share one method instance (a method of a
/// local object: never composed into the callers' summaries), and `m`.
fn both(body: &str, m: &str) -> ProgramFacts {
    let b = format!("export function both(o: any, c: any) {{ {body} }}");
    units_of(&[("b", b.as_str()), ("m", m)])
}

/// Pts entries of the largest unit when `n` other-unit functions let what
/// their argument reaches escape, each called with the head of one list of
/// `n` objects.
fn reach_closure_entries(n: usize) -> u64 {
    let mut b = String::new();
    for i in 0..n {
        b.push_str(&format!(
            "export function keep{i}(p: any) {{ (globalThis as any).k{i} = p.next; }}\n"
        ));
    }
    let names: Vec<String> = (0..n).map(|i| format!("keep{i}")).collect();
    let mut m = format!("import {{ {} }} from './b';\nfunction main() {{\n  const o0: any = {{ next: null, v: 0 }};\n", names.join(", "));
    for i in 1..=n {
        m.push_str(&format!(
            "  const o{i}: any = {{ next: o{}, v: {i} }};\n",
            i - 1
        ));
    }
    for f in &names {
        m.push_str(&format!("  {f}(o{n});\n"));
    }
    m.push_str(&format!("  console.log(o{n}.v);\n}}\nmain(); main();\n"));
    let pf = units_of(&[("b", b.as_str()), ("m", m.as_str())]);
    pf.units
        .iter()
        .map(|(_, s)| s.pts_entries)
        .max()
        .unwrap_or(0)
}

/// ee-first (express, D16): `callback` reads `arguments` and is passed to
/// closed `listener` as `done`; the `onevent` it returns escapes through
/// `ee.on` and calls `done`.
const EE_FIRST: &str = r#"
    export default function first(stuff: any, done: any) {
        var cleanups: any[] = [];
        for (var i = 0; i < stuff.length; i++) {
            var arr = stuff[i];
            var ee = arr[0];
            for (var j = 1; j < arr.length; j++) {
                var event = arr[j];
                var fn = listener(event, callback);
                ee.on(event, fn);
                cleanups.push({ ee: ee, event: event, fn: fn });
            }
        }
        function callback() {
            cleanup();
            done.apply(null, arguments);
        }
        function cleanup() {
            var x;
            for (var i = 0; i < cleanups.length; i++) {
                x = cleanups[i];
                x.ee.removeListener(x.event, x.fn);
            }
        }
        function thunk(fn: any) {
            done = fn;
        }
        thunk.cancel = cleanup;
        return thunk;
    }
    function listener(event: any, done: any) {
        return function onevent(this: any, arg1: any) {
            var args = new Array(arguments.length);
            var ee = this;
            var err = event === "error" ? arg1 : null;
            for (var i = 0; i < args.length; i++) {
                args[i] = arguments[i];
            }
            done(err, ee, event, args);
        };
    }
"#;

/// Runs `f` on its own thread; `None` when it has not returned in `secs`.
fn bounded<T: Send + 'static>(secs: u64, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    // The parser and lowering recurse: a test thread's stack, not 2 MiB.
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let _ = tx.send(f());
        })
        .expect("spawns");
    rx.recv_timeout(std::time::Duration::from_secs(secs)).ok()
}

#[path = "unit_mode/shared_values.rs"]
mod shared_values;

/// Pts entries of the largest unit when `n` exported functions store their
/// argument into one cell through helpers of their own, and `n` readers
/// each read their own key of what the cell holds.
fn shared_cell_entries(n: usize) -> u64 {
    // The cell is a static of `S` and every function names `S`: one unit (a
    // cell of its own declaration unit would be another unit's literal).
    let mut src = "class S { static last: any = 0; static cell: any = { s: null }; }\n".to_string();
    for i in 0..n {
        src.push_str(&format!(
            "function h{i}(a: any) {{ S.last = {i}; S.cell.s = a; }}\nexport function f{i}(a: any) {{ S.last = {i}; h{i}(a); }}\nexport function g{i}() {{ S.last = {i}; const x = S.cell.s; return x.k{i}; }}\n"
        ));
    }
    let names: Vec<String> = (0..n)
        .flat_map(|i| [format!("f{i}"), format!("g{i}")])
        .collect();
    let calls: Vec<String> = (0..n)
        .map(|i| format!("f{i}({{ k{i}: 1 }}); console.log(g{i}());"))
        .collect();
    let user = format!(
        "import {{ {} }} from './t'; {}",
        names.join(", "),
        calls.join(" ")
    );
    let pf = units_of(&[("t", src.as_str()), ("u", user.as_str())]);
    pf.units
        .iter()
        .map(|(_, s)| s.pts_entries)
        .max()
        .unwrap_or(0)
}

/// Pts entries of the largest unit when `n` escaped literals (each stored
/// into ⊤ and read back beside it) are merged into one value that `n`
/// arrays hold.
fn escaped_beside_top_entries(n: usize) -> u64 {
    let mut src = format!("{JOIN}");
    for i in 0..n {
        src.push_str(&format!(
            "function l{i}(o: any) {{ S.last = {i}; return o.l{i} || (o.l{i} = {{ v: {i} }}); }}\n"
        ));
    }
    src.push_str("export function f(x: any, c: boolean) { S.last = -1; let y: any = l0(x);\n");
    for i in 1..n {
        src.push_str(&format!("if (c) y = l{i}(x);\n"));
    }
    for i in 0..n {
        src.push_str(&format!("const z{i} = [y];\n"));
    }
    let zs: Vec<String> = (0..n).map(|i| format!("z{i}.length")).collect();
    src.push_str(&format!("return {}; }}\n", zs.join(" + ")));
    let user = "import { f } from './t'; console.log(f((globalThis as any).u, true));";
    let pf = units_of(&[("t", src.as_str()), ("u", user)]);
    pf.units
        .iter()
        .map(|(_, s)| s.pts_entries)
        .max()
        .unwrap_or(0)
}

/// Pts entries of the largest unit when `n` exported functions store their
/// argument into a cell each, one function returns any of the cells, and
/// `n` readers each read their own key of what it returns.
fn result_holder_entries(n: usize) -> u64 {
    // The cells are statics of `S` (one unit with the functions); `pick` is
    // a method of a local object, never composed into its callers, so its
    // result node is the one every caller reads.
    let cells: Vec<String> = (0..n)
        .map(|i| format!("static c{i}: any = {{ s: null }};"))
        .collect();
    let mut src = format!("class S {{ static last: any = 0; {} }}\n", cells.join(" "));
    for i in 0..n {
        src.push_str(&format!(
            "export function f{i}(a: any) {{ S.last = {i}; S.c{i}.s = a; }}\n"
        ));
    }
    // `pick` returns its parameter `d` when no cell matches: its result is
    // a slot of its instance's run (it depends on a parameter), which the
    // cells' markers cross into.
    src.push_str("const P = { pick(k: number, d: any): any { S.last = -1;\n");
    for i in 0..n {
        src.push_str(&format!("if (k === {i}) return S.c{i}.s;\n"));
    }
    src.push_str("return d; } };\n");
    for i in 0..n {
        src.push_str(&format!(
            "export function g{i}(k: number, a: any) {{ S.last = {i}; const x = P.pick(k, a); return x.k{i}; }}\n"
        ));
    }
    let names: Vec<String> = (0..n)
        .flat_map(|i| [format!("f{i}"), format!("g{i}")])
        .collect();
    let calls: Vec<String> = (0..n)
        .map(|i| format!("f{i}({{ k{i}: 1 }}); console.log(g{i}({i}, {{ k{i}: 2 }}));"))
        .collect();
    let user = format!(
        "import {{ {} }} from './t'; {}",
        names.join(", "),
        calls.join(" ")
    );
    let pf = units_of(&[("t", src.as_str()), ("u", user.as_str())]);
    pf.units
        .iter()
        .map(|(_, s)| s.pts_entries)
        .max()
        .unwrap_or(0)
}

/// The unit of `f` (a store) and `r` (a read of a builtin function value's
/// member, then a read of the result): whether the read site holds ⊤.
fn builtin_fn_read_top(store: &str) -> bool {
    let b = format!("{JOIN}export function f(x: any, c: boolean) {{ S.last = 1; const g: any = Math.max; {store} }}\nexport function r() {{ S.last = 2; const m: any = Math.min; const y: any = m.foo; return y.v; }}");
    let m = "import { f, r } from './b'; function main() { f((globalThis as any).u, true); console.log(r()); } main(); main();";
    let pf = units_of(&[("b", b.as_str()), ("m", m)]);
    let s = pf
        .sites
        .iter()
        .find(|s| s.key == "v")
        .unwrap_or_else(|| panic!("{}", pf.dump_text()));
    s.top
}

/// RTX bug 3: `h` stores its argument into `saved.p`, a literal of another
/// declaration unit, and `b` returns it: `b`'s result is the `Box` given to
/// `a`, which the reader writes.
const SAVED_B: &str = "const saved: any = { p: null };\nfunction h(p: any) { if (!saved.p) saved.p = p; return saved.p; }\nexport function a(p: any) { return h(p); }\nexport function b(p: any) { return h(p); }";
const SAVED_M: &str = "import { a, b } from './b'; class Box { v: any = 1; } const o = new Box(); a(o); const r = b(0); r.v = 'x'; console.log(o.v);";

/// The kinds per unit cover the whole program's: ⊤, or every primitive
/// kind and layout the whole program finds.
fn kinds_cover(u: &crate::derive::FieldFacts, w: &crate::derive::FieldFacts) -> bool {
    u.kind.top
        || (u.kind.prims & w.kind.prims == w.kind.prims
            && w.kind.objs.iter().all(|o| u.kind.objs.contains(o)))
}

/// `run(o)` in `a` hands its argument down a chain of `n` closed locals
/// (decision 23: every reference a direct call, none passed as a value);
/// each stores its own key into it. `b` passes a literal and reads every key.
fn closed_chain(n: usize) -> [(&'static str, String); 2] {
    let mut a = String::from("export function run(o: any) {\n");
    for i in (0..n).rev() {
        let next = match i + 1 < n {
            true => format!(" f{}(x);", i + 1),
            false => String::new(),
        };
        a += &format!("  const f{i} = (x: any) => {{ x.k{i} = {i};{next} }};\n");
    }
    a += "  f0(o);\n}\n";
    let keys: Vec<String> = (0..n).map(|i| format!("k{i}: -1")).collect();
    let reads: Vec<String> = (0..n).map(|i| format!("o.k{i}")).collect();
    let b = format!(
        "import {{ run }} from './a';\nconst o = {{ {} }};\nrun(o);\nconsole.log({});\n",
        keys.join(", "),
        reads.join(" + ")
    );
    [("a", a), ("b", b)]
}

#[path = "unit_mode/escaped_results.rs"]
mod escaped_results;

#[path = "unit_mode/return_transfers.rs"]
mod return_transfers;

#[path = "unit_mode/pub_callable.rs"]
mod pub_callable;
