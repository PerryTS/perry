use super::*;

#[test]
pub(super) fn a_member_function_returning_its_own_layout_is_built_once() {
    // The reader's stand-ins of a layout and of a function its field holds
    // that returns the layout reach each other: each is registered before
    // its members are built (the stack overflowed).
    let a = "const o: any = { k: function () { return o; } }; export function make() { return o; }";
    let b = "import { make } from './a'; const x = make(); const f = x.k; const y = f(); y.k = 1;";
    let pf = bounded(60, move || units_of(&[("a", a), ("b", b)]))
        .expect("D16: the per-unit fixpoint did not end in 60 s");
    // ABM3 (static rule, CWN 10-27): `b`'s store into ⊤ (`y.k = 1`) under
    // `k` writes `k` of every builtin prototype; `a`'s lookups of `k` that
    // end at `Object.prototype` read it, so `a` solves once more: three.
    assert_eq!(pf.rounds, 3, "rounds {:?}", pf.round_work);
}

/// DECISIONS 113: every unit storing under a computed global key writes ⊤
/// into `global:*`, and every unit reading a program-owned global (a
/// `__` key, `builtins::global_user_key`) reads that cell. The
/// reader took one stand-in value per writing unit (effect: ~9500 units,
/// 19k nodes in each reader, quadratic per round); the writes are grouped
/// by contract, so the reader's solve does not grow with the writers.
#[test]
pub(super) fn a_cell_written_alike_by_many_units_is_read_once() {
    let reader_nodes = |n: usize| {
        let srcs: Vec<(String, String)> = (0..n)
            .map(|i| {
                (
                    format!("w{i}"),
                    format!(
                        "(globalThis as any)[String(Math.random())] = {i}; export const v{i} = 1;"
                    ),
                )
            })
            .chain(std::iter::once((
                "r".to_string(),
                "console.log((globalThis as any).__g);".to_string(),
            )))
            .collect();
        let mods: Vec<(&str, &str)> = srcs.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let pf = units_of(&mods);
        let (_, st) = pf
            .units
            .iter()
            .find(|(p, _)| p.starts_with("r.ts"))
            .expect("the reader is a unit");
        (st.nodes, pf.dump_text())
    };
    let (few, _) = reader_nodes(4);
    let (many, dump) = reader_nodes(16);
    assert_eq!(few, many, "{dump}");
}

/// The layouts the units' writes of global `__g` describe at the fixpoint
/// (each unit's own write), when `writers` modules each store an instance
/// of their own class there and `readers` modules each read it (cell
/// `global:__g`) and store an instance of their own.
fn global_cell_layouts(writers: usize, readers: usize) -> (usize, String) {
    let srcs: Vec<(String, String)> = (0..writers)
        .map(|i| {
            (
                format!("w{i}"),
                format!("class W{i} {{ a{i} = {i}; }}\n(globalThis as any).__g = new W{i}();"),
            )
        })
        .chain((0..readers).map(|j| {
            (
                format!("r{j}"),
                format!(
                    "class R{j} {{ r{j} = {j}; }}\nfunction run() {{ const g: any = globalThis; console.log(g.__g.a0); g.__g = new R{j}(); }}\nrun();"
                ),
            )
        }))
        .collect();
    let mods: Vec<(&str, &str)> = srcs.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let pf = units_of(&mods);
    let layouts = pf
        .cells_out
        .iter()
        .filter(|(k, _)| k == "global:__g")
        .map(|(_, c)| c.layouts.len())
        .sum();
    eprintln!("global {writers} writers {readers} readers: {layouts} layouts written");
    (layouts, pf.dump_text())
}

/// A unit's write into a cell is what its own code stores there: the other
/// units' writes it read are their writers', in the cell already (cells
/// only grow, DECISIONS 197). Written back, every reader of a cell was a
/// writer of all of it, and each next reader took one stand-in per such
/// write: one more writer reached every reader once per reader (opencode:
/// 2871 readers of `cjs:*`'s 1929 writes, the facts stage past 52 GB). One
/// more writer adds its one write, with 4 readers as with 16.
#[test]
fn a_cell_reader_does_not_write_back_what_it_read() {
    let extra = |readers: usize| {
        let (few, _) = global_cell_layouts(4, readers);
        let (more, dump) = global_cell_layouts(8, readers);
        (few, more, dump)
    };
    let (f4, m4, _) = extra(4);
    let (f16, m16, dump) = extra(16);
    assert!(m4 > f4, "the writers' instances are in the cell: {f4} {m4}");
    assert_eq!(
        m4 - f4,
        m16 - f16,
        "a writer is written once per reader: 4 readers {f4} -> {m4}, 16 readers {f16} -> {m16}\n{dump}"
    );
}

/// The ⊤ array cells the units write at the fixpoint (each unit's own
/// write), when `throwers` modules each throw an array of their own and
/// `others` modules catch what is thrown and leave it, with interpreted code in the
/// program (it can catch what compiled code throws).
fn thrown_array_tops(throwers: usize, others: usize) -> (usize, String) {
    let srcs: Vec<(String, String)> = (0..throwers)
        .map(|i| {
            (
                format!("t{i}"),
                format!("function f{i}() {{ throw [{i}]; }}\ntry {{ f{i}(); }} catch (e) {{ console.log(0); }}"),
            )
        })
        .chain((0..others).map(|j| {
            (
                format!("o{j}"),
                format!("function run{j}() {{ try {{ console.log({j}); }} catch (e) {{ console.log(0); }} }}\nrun{j}();"),
            )
        }))
        .collect();
    let mods: Vec<(&str, &str)> = srcs.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let mut cfg = FactsConfig::default();
    cfg.dynamic_code_reach.record_global("Function".into());
    let pf = units_with(&mods, &cfg);
    let tops = pf
        .cells_out
        .iter()
        .filter(|(k, c)| k.starts_with("arr:") && c.unknown)
        .count();
    eprintln!("thrown {throwers} throwers {others} others: {tops} array tops written");
    (tops, pf.dump_text())
}

/// DECISIONS 174: what another unit threw reaches a unit through cell
/// `thrown`, and reaches interpreted code as its thrower threw it: the
/// thrower escaped it. Escaped again by every unit that read it, each
/// published ⊤ for every array the program throws (opencode round 3: 127k
/// units ⊤ for ~210 arrays each, 54M cell writes, the facts stage past
/// 43 GB). One more thrower adds its own ⊤s, with 4 other units as with 16.
#[test]
fn a_thrown_value_escapes_in_its_thrower_only() {
    let extra = |others: usize| {
        let (few, _) = thrown_array_tops(4, others);
        let (more, dump) = thrown_array_tops(8, others);
        (few, more, dump)
    };
    let (f4, m4, _) = extra(4);
    let (f16, m16, dump) = extra(16);
    assert!(m4 > f4, "the throwers' arrays escape: {f4} {m4}");
    assert_eq!(
        m4 - f4,
        m16 - f16,
        "a thrown array is ⊤ once per catching unit: 4 others {f4} -> {m4}, 16 others {f16} -> {m16}\n{dump}"
    );
}

/// A display names a class, function or array across units (one name, one
/// contract, DECISIONS 106): two modules of one file name in two
/// directories (or two versions of one package) name theirs apart. Named
/// alike, one owner's contract replaced the other's, and their readers
/// were solved again in every round (opencode: 13 units, round 1526).
#[test]
fn modules_of_one_file_name_name_their_contracts_apart() {
    let pf = units_of(&[
        ("a/x", "export class C { k = 1; }\nexport function f(o: any) { return o.k; }"),
        ("b/x", "export class C { s = 'a'; }\nexport function f(o: any) { return o.s; }"),
        (
            "m",
            "import { C as C1, f as f1 } from './a/x';\nimport { C as C2, f as f2 } from './b/x';\nconsole.log(f1(new C1()), f2(new C2()));",
        ),
    ]);
    let cs: std::collections::BTreeSet<&str> = pf
        .templates
        .iter()
        .filter(|t| t.display.ends_with("x.ts:C"))
        .map(|t| t.display.as_str())
        .collect();
    assert_eq!(cs.len(), 2, "{cs:?}\n{}", pf.dump_text());
}

/// Per unit, a CommonJS record's `parent` is the requiring module's record:
/// another module's, another unit's value (unknown). Linked through the
/// registry union, every wrapper read every unit's registry writes
/// (opencode: 14879 units in one dependency level, 1929 writes each); no
/// wrapper that only links its parent reads `cjs:*`.
#[test]
fn linking_a_parent_reads_no_registry_writes() {
    let srcs: Vec<(String, String)> = (0..4)
        .map(|i| {
            (
                format!("m{i}"),
                format!(
                    "class M{i} {{ a{i} = {i}; }}\nfunction factory() {{ const rec: any = {{ exports: new M{i}(), __perry_cjs_record: true, parent: undefined }}; __perry_link_path_module_parent(rec); console.log(rec.parent); __perry_register_path_module(\"/m{i}.js\", rec); }}\nfactory();"
                ),
            )
        })
        .collect();
    let mods: Vec<(&str, &str)> = srcs.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let pf = units_with(
        &mods,
        &FactsConfig {
            unit_io: true,
            ..Default::default()
        },
    );
    let writes = pf
        .unit_io
        .iter()
        .filter(|l| l.starts_with("# uout") && l.contains("cell:cjs:*"))
        .count();
    let reads: Vec<&String> = pf
        .unit_io
        .iter()
        .filter(|l| l.starts_with("# uread") && l.ends_with("cell:cjs:*"))
        .collect();
    assert!(
        writes > 0,
        "the records are registry writes: {:?}",
        pf.unit_io
    );
    assert!(reads.is_empty(), "read through the parent link: {reads:?}");
}

/// Per unit, a computed require's result is another module's exports:
/// another unit's value (unknown). Through the registry union each such
/// unit took a stand-in of every registry write (opencode: 1122 units,
/// each publishing ⊤ for the 3660 arrays it let escape); no unit reads
/// `cjs:*` for one.
#[test]
fn a_computed_require_reads_no_registry_writes() {
    let srcs: Vec<(String, String)> = (0..4)
        .map(|i| {
            (
                format!("m{i}"),
                format!(
                    "class M{i} {{ a{i} = [{i}]; }}\nfunction factory() {{ const rec: any = {{ exports: new M{i}(), __perry_cjs_record: true }}; const m: any = __perry_require_path_module(String(Math.random())); console.log(m.a0); __perry_register_path_module(\"/m{i}.js\", rec); }}\nfactory();"
                ),
            )
        })
        .collect();
    let mods: Vec<(&str, &str)> = srcs.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let pf = units_with(
        &mods,
        &FactsConfig {
            unit_io: true,
            ..Default::default()
        },
    );
    let writes = pf
        .unit_io
        .iter()
        .filter(|l| l.starts_with("# uout") && l.contains("cell:cjs:*"))
        .count();
    let reads: Vec<&String> = pf
        .unit_io
        .iter()
        .filter(|l| l.starts_with("# uread") && l.ends_with("cell:cjs:*"))
        .collect();
    assert!(
        writes > 0,
        "the records are registry writes: {:?}",
        pf.unit_io
    );
    assert!(
        reads.is_empty(),
        "read through a computed require: {reads:?}"
    );
}

/// DECISIONS 114: the driver compares what a unit read with what is
/// published now. Each layout and function contract is digested once per
/// round, so those compares do not grow with the readers (awfy_Richards:
/// 58544 reads of 39 layouts, 125 s a round); a cell is viewed per reader
/// only in a round its writes changed.
#[test]
pub(super) fn the_driver_compares_each_contract_once_per_round() {
    let compares = |n: usize, writer: &str, reader: &str| {
        let srcs: Vec<(String, String)> = std::iter::once(("a".to_string(), writer.to_string()))
            .chain((0..n).map(|i| (format!("r{i}"), reader.replace("I", &i.to_string()))))
            .collect();
        let mods: Vec<(&str, &str)> = srcs.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let pf = units_of(&mods);
        (pf.compares, pf.rounds, pf.dump_text())
    };
    let class = "export class P { x = 1; y = 'p'; } export function mk() { return new P(); }";
    let (few, r1, _) = compares(
        4,
        class,
        "import { mk } from './a'; export const vI = mk().x;",
    );
    let (many, r2, dump) = compares(
        16,
        class,
        "import { mk } from './a'; export const vI = mk().x;",
    );
    assert_eq!(r1, r2, "{dump}");
    assert_eq!(few, many, "layouts and functions: {dump}");
    let global = "(globalThis as any)[String(Math.random())] = 1;";
    let (few, r1, _) = compares(4, global, "export const vI = (globalThis as any).__g;");
    let (many, r2, dump) = compares(16, global, "export const vI = (globalThis as any).__g;");
    assert_eq!(r1, r2, "{dump}");
    // A reader is viewed at most once per round boundary: only a cell
    // whose writes changed is viewed, and the first changed view seeds it.
    let bound = (16 - 4) * (r1 as u64 - 1);
    assert!(
        many - few <= bound,
        "cells: {few} -> {many}, rounds {r1}: {dump}"
    );
}

#[test]
pub(super) fn a_static_instance_is_a_value_binding_of_its_class() {
    // DECISIONS 173: `P.D` is a binding on the class object, part of P's
    // contract (§11.2), checked as an export binding is: the instance it
    // gives out does not escape with its prototype chain, so `Q`, a
    // subclass of another unit, finds `P` unbroken.
    let b = "export class P { v: number; constructor() { this.v = 1; } get(): number { return this.v; } static D = new P(); }";
    let m = "import { P } from './b'; class Q extends P { w: number = 2; } function main() { const d = P.D; console.log(d.get()); const q = new Q(); console.log(q.v, q.w, q.get()); } main(); main();";
    assert_parity(&[("b", b), ("m", m)]);
}

#[test]
pub(super) fn a_value_no_contract_describes_escapes_alone() {
    // DECISIONS 174: a published union is its members. `pick` holds an `A`
    // or a runtime-made Error, which no contract describes: the Error
    // escapes alone, the binding describes the `A` and says unknown beside
    // it, so `A` stays unbroken.
    let b = "export class A { n: number; constructor() { this.n = 1; } } export const pick: any = Math.random() > 2 ? new A() : new Error('e'); export function own(): number { const a = new A(); return a.n; }";
    let m = "import { pick, own } from './b'; function main() { console.log(pick.n, own()); } main(); main();";
    assert_parity(&[("b", b), ("m", m)]);
    // The thrown values are a cell (174): the catching unit reads the
    // `A` another unit throws beside an Error.
    let b = "export class A { n: number; constructor() { this.n = 1; } } export function f(k: number): number { if (k > 1) throw new A(); if (k < 0) throw new Error('e'); const a = new A(); return a.n; }";
    let m = "import { f } from './b'; function main() { try { console.log(f(2)); } catch (e: any) { console.log(e.n); } } main(); main();";
    assert_parity(&[("b", b), ("m", m)]);
}

#[test]
pub(super) fn a_result_value_no_contract_describes_escapes_alone() {
    // DECISIONS 175: 174's rule for a contract function's result. `mk`
    // returns an `A` or a primitive method's box (`primbox`, on a string of
    // its own: `k` is a marker per unit, no primitive receiver): the box
    // escapes alone, the result describes the `A`: it is given out, not
    // escaped, so `a.n` in `mk` is proven as the whole program proves it.
    let b = "export class A { n: number; constructor() { this.n = 1; } } export function mk(k: number): any { const a = new A(); console.log(a.n); return k > 1 ? a : 'ab'.slice(k); } export function own(): number { const a = new A(); return a.n; }";
    let m = "import { mk, own } from './b'; function main() { const v = mk(2); console.log(v.n, own()); } main(); main();";
    assert_parity(&[("b", b), ("m", m)]);
}

#[test]
pub(super) fn a_callees_rooted_store_on_its_callers_receiver_is_the_callers_flow() {
    // AWFY som.js Dictionary::resize -> transferEntries: `move` runs on
    // `grow`'s receiver and stores what that receiver reaches into what it
    // reaches. The reader replays it on its own object (DECISIONS 179:
    // receiver flows; 181: `d`'s own `D` stand-in is
    // `D2`'s parent, so `D2` runs `d`'s published `move`), as the whole
    // program moves the entries inside one `D`; `E`, a class of a third
    // unit, is not escaped by it.
    let b = "export class E { v: number; constructor(v: number) { this.v = v; } }";
    let d = "import { E } from './b'; export class D { b: any[]; constructor() { this.b = new Array(2); } put(k: number, v: number) { this.b[k] = new E(v); } grow() { const o = this.b; this.b = new Array(4); this.move(o); } move(o: any[]) { this.b[0] = o[0]; } }";
    let m = "import { D } from './d'; class D2 extends D { w: number = 1; } function main() { const x = new D2(); x.put(0, 1); x.grow(); console.log(x.b[0].v, x.w); } main(); main();";
    assert_parity(&[("b", b), ("d", d), ("m", m)]);
}

#[test]
pub(super) fn a_numeric_index_step_of_a_stored_path_is_the_element() {
    // DECISIONS 179/181: `copy` on a `D2` (another unit's subclass) is
    // replayed on the reader's own object, whose `a` holds `E` only: the
    // element read gives `E`, as in the whole program.
    let b = "export class E { v: number; constructor(v: number) { this.v = v; } }";
    let d = "import { E } from './b'; export class D { a: any[]; b: any[]; constructor() { this.a = new Array(2); this.b = new Array(2); } put(v: number) { this.a[0] = new E(v); } copy() { const o = this.a; this.b[0] = o[0]; } }";
    let m = "import { D } from './d'; class D2 extends D { w: number = 1; } function main() { const x = new D2(); x.put(1); x.copy(); console.log(x.b[0].v, x.w); } main(); main();";
    assert_parity(&[("b", b), ("d", d), ("m", m)]);
}

#[test]
pub(super) fn a_string_index_step_of_a_stored_path_reads_any_key() {
    // Soundness of the replay (DECISIONS 179-181): `o[k]` with a string `k`
    // may name a prototype member (`push`): the reader's replay of `move`
    // on its `D2` finds the array's builtin methods beside its elements, as
    // the whole program does. `x.b[0].v` is proven in neither mode.
    let b = "export class E { v: number; constructor(v: number) { this.v = v; } }";
    let d = "import { E } from './b'; export class D { b: any[]; constructor() { this.b = new Array(2); } put(k: number, v: number) { this.b[k] = new E(v); } grow(k: string) { const o = this.b; this.b = new Array(4); this.move(o, k); } move(o: any[], k: string) { this.b[0] = (o as any)[k]; } }";
    let m = "import { D } from './d'; class D2 extends D { w: number = 1; } function main(k: string) { const x = new D2(); x.put(0, 1); x.grow(k); console.log(x.b[0].v, x.w); } main('push'); main('0');";
    let mods = [("b", b), ("d", d), ("m", m)];
    let (wp, pu) = (whole_of(&mods), units_of(&mods));
    let read = |pf: &ProgramFacts| {
        proven_set(pf)
            .iter()
            .any(|s| s.starts_with("m.ts@") && s.ends_with(".v"))
    };
    assert!(!read(&wp), "whole program reads a builtin method beside E");
    assert!(
        !read(&pu),
        "per unit must not prove what the whole program cannot"
    );
}

#[test]
pub(super) fn a_published_methods_parameter_holds_its_callers_arguments() {
    // DECISIONS 148: `o.items` on `take`'s parameter is proven in the whole
    // program (o is m's literal). Per unit, m publishes what it passes to
    // the method it runs (`method_args_out`, the 107 rule for parameters),
    // and d's parameter holds that literal's stand-in beside its marker.
    let b = "export class E { v: number; constructor(v: number) { this.v = v; } }";
    let d = "export class D { b: any[]; constructor() { this.b = new Array(2); } take(o: any) { this.b[0] = o.items[0]; } }";
    let m = "import { D } from './d'; import { E } from './b'; function main() { const x = new D(); x.take({ items: [new E(1)] }); console.log(x.b[0].v); } main(); main();";
    let mods = [("b", b), ("d", d), ("m", m)];
    let (pu, wp) = (units_of(&mods), whole_of(&mods));
    assert!(proven(&wp, "items"), "{}", wp.dump_text());
    assert_eq!(
        verdicts(&pu, "items"),
        verdicts(&wp, "items"),
        "{}",
        pu.dump_text()
    );
    assert!(
        proven_set(&pu).is_subset(&proven_set(&wp)),
        "{}",
        pu.dump_text()
    );
}

#[test]
pub(super) fn a_published_methods_parameter_holds_its_callers_arguments_with_a_rest_parameter() {
    // DECISIONS 148 for a method the solve does not summarize (a rest
    // parameter): its contract publication (`escape_fn_params`) puts what
    // the running units pass beside the marker as the summarized one does.
    let b = "export class E { v: number; constructor(v: number) { this.v = v; } }";
    let d = "export class D { b: any[]; constructor() { this.b = new Array(2); } take(o: any, ...more: any[]) { this.b[0] = o.items[0]; } }";
    let m = "import { D } from './d'; import { E } from './b'; function main() { const x = new D(); x.take({ items: [new E(1)] }); console.log(x.b[0].v); } main(); main();";
    let mods = [("b", b), ("d", d), ("m", m)];
    let (pu, wp) = (units_of(&mods), whole_of(&mods));
    assert!(proven(&wp, "items"), "{}", wp.dump_text());
    assert_eq!(
        verdicts(&pu, "items"),
        verdicts(&wp, "items"),
        "{}",
        pu.dump_text()
    );
    assert!(
        proven_set(&pu).is_subset(&proven_set(&wp)),
        "{}",
        pu.dump_text()
    );
}

#[test]
pub(super) fn a_published_methods_parameter_some_caller_passes_unknown_stays_a_marker() {
    // DECISIONS 148 guard: n passes a value no contract describes
    // (`JSON.parse`) for `q`, so the joined argument there is unknown and
    // `q` stays the bare marker, while `o` (both callers pass literals)
    // holds them: per unit proves no `.list` the whole program cannot
    // (soundness: never stronger than the whole program), and `.items`.
    let b = "export class E { v: number; constructor(v: number) { this.v = v; } }";
    let d = "export class D { b: any[]; constructor() { this.b = new Array(2); } take(o: any, q: any) { this.b[0] = o.items[0]; this.b[1] = q.list[0]; } }";
    let m = "import { D } from './d'; import { E } from './b'; import { run } from './n'; function main() { const x = new D(); x.take({ items: [new E(1)] }, { list: [new E(2)] }); run(x); console.log(x.b[0].v); } main(); main();";
    let n = "export function run(x: any) { x.take({ items: [1] }, JSON.parse(process.argv[2] || '{\"list\":[1]}')); }";
    let mods = [("b", b), ("d", d), ("n", n), ("m", m)];
    let (pu, wp) = (units_of(&mods), whole_of(&mods));
    assert!(!proven(&wp, "list"), "{}", wp.dump_text());
    assert!(!proven(&pu, "list"), "{}", pu.dump_text());
    assert_eq!(
        proven(&pu, "items"),
        proven(&wp, "items"),
        "{}",
        pu.dump_text()
    );
    assert!(
        proven_set(&pu).is_subset(&proven_set(&wp)),
        "{}",
        pu.dump_text()
    );
}

#[test]
#[ignore = "DECISIONS 186 open: a stand-in layout's fields hold unknown values per unit"]
pub(super) fn a_field_of_a_callers_literal_read_in_a_published_method_is_described() {
    // DECISIONS 186 (found with 148): after 148, `o` holds m's literal's
    // stand-in, but its field `items` reads as unknown (a stand-in's fields
    // hold unknown values), so d's write into D's array cell is unknown and
    // m loses `x.b[0].v`, which the whole program proves.
    let b = "export class E { v: number; constructor(v: number) { this.v = v; } }";
    let d = "export class D { b: any[]; constructor() { this.b = new Array(2); } take(o: any) { this.b[0] = o.items[0]; } }";
    let m = "import { D } from './d'; import { E } from './b'; function main() { const x = new D(); x.take({ items: [new E(1)] }); console.log(x.b[0].v); } main(); main();";
    assert_parity(&[("b", b), ("d", d), ("m", m)]);
}

#[test]
pub(super) fn a_published_cell_holding_another_units_prototype_terminates() {
    // DECISIONS 180 (found with it): a cell holding a stand-in published
    // onward (another unit's class prototype) is undescribed, and its
    // escape is a no-op: only an escape that took effect is a change of
    // the unit's contract check, so the check ends.
    let b = "export class E { v: number; constructor(v: number) { this.v = v; } }";
    let d = "import { E } from './b'; export let p: any = E.prototype; export function set(q: any) { p = q; }";
    let m = "import { p, set } from './d'; import { E } from './b'; function main() { set(Object.getPrototypeOf(new E(1))); console.log(p.constructor, new E(2).v); } main(); main();";
    let pu = units_of(&[("b", b), ("d", d), ("m", m)]);
    assert!(!pu.sites.is_empty());
}

#[test]
pub(super) fn a_primitive_method_container_crosses_units_as_a_container() {
    // DECISIONS 184: `parts` returns `split`'s array (a `primbox`). Its
    // contract describes it (cell `box:<name>`, as an array's, DECISIONS
    // 94), so `m` holds a container of its own kind, not an unknown value:
    // `forEach` calls the closure with primitives and `a` stays this unit's,
    // as in the whole program.
    let b = "export function parts(s: string): any { return s.split(','); }";
    let m = "import { parts } from './b'; class A { n = 0; inc(x: any) { this.n = this.n + x.length; } } function main() { const a = new A(); const p = parts(process.argv.length > 9 ? 'a' : 'a,b'); p.forEach((x: any) => a.inc(x)); console.log(a.n); } main(); main();";
    assert_parity(&[("b", b), ("m", m)]);
}

#[test]
pub(super) fn a_primitive_method_box_in_another_units_storage_keeps_its_elements_described() {
    // DECISIONS 184 (AWFY Json: json.js#u18 appends `input.substring(..)`
    // to som.js's Vector): `m` stores a `primbox` and an instance of `a`'s
    // `P` into `a`'s Vector storage. The storage cell's write describes the
    // box (cell `box:<name>`), so `a` reads no ⊤ element: `storage.slice`
    // (a named read that sees the elements, Key::Any) calls no unknown
    // callee, the storage and its `P` stay unescaped, as in the whole
    // program.
    let a = "export class P { x: number; constructor(x: number) { this.x = x; } get(): number { return this.x; } } export class Vector { storage: any[]; n: number; constructor() { this.storage = new Array(2); this.n = 0; } append(e: any) { if (this.n >= this.storage.length) { const s = this.storage.slice(); this.storage = new Array(this.n * 2); for (let i = 0; i < this.n; i++) this.storage[i] = s[i]; } this.storage[this.n] = e; this.n = this.n + 1; } at(i: number): any { return this.storage[i]; } }";
    let m = "import { P, Vector } from './a'; class J { input: string; v: Vector; constructor(s: string) { this.input = s; this.v = new Vector(); } run() { for (let i = 0; i < 4; i++) { this.v.append(new P(i)); this.v.append(this.input.substring(i, i + 1)); } return this.v.at(0).get(); } } function main() { const j = new J(process.argv.length > 9 ? 'abcdef' : 'xyzuvw'); console.log(j.run()); } main(); main();";
    // Gap: `m` reads `this.v.at(0)` as ⊤ (the element `at` returns is
    // `m`'s own value in `a`'s storage), so no unit publishes `P` as the
    // receiver of `get`: per unit `this.x` there is E0 over `P` (the
    // guarded body), the whole program T0. It was "proven" before only by
    // a proof over no receiver (`tests/proof_receivers.rs`).
    assert_parity_but(&[("a", a), ("m", m)], &["a.ts@91.x"]);
}

#[test]
pub(super) fn a_nested_initializer_and_its_hoisted_var_are_one_emitted_binding() {
    let src = r#"
        const loaded = (function() {
            try { var imported = { value: 123 }; } catch (unused) {}
            return { get: () => imported.value };
        })();
        console.log(loaded.get());
    "#;
    let pf = units(src);
    assert!(pf.broken.is_empty(), "{:?}", pf.broken);
    // The one binding is published once even though its hoisted seed and
    // nested initializer are distinct init statements after run-once flatten.
    let bindings = pf.bindings_for("t.ts").unwrap();
    let mut ids: Vec<_> = bindings.locals.iter().map(|(id, _)| *id).collect();
    let count = ids.len();
    assert!(count > 0, "the captured module binding must be live");
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count);
}

// MRK: a marker is a token only inside its run; it crosses into a result,
// storage or another run as a member of that node's own marker.

#[test]
pub(super) fn an_inner_helper_given_two_objects_holds_no_marker() {
    // No marker arrives at `h`'s parameter (both callers pass objects of
    // their own), so it holds the two objects alone and `p.x` is proven on
    // both layouts: a marker there would be a receiver no fact covers.
    let b = "class A { x: any = 1; a: any = 0; }\nclass B { x: any = 2; b: any = 0; }\nexport function run() { function h(p: any) { return p.x; } return h(new A()) + h(new B()); }";
    let pf = units_of(&[
        ("b", b),
        ("m", "import { run } from './b'; console.log(run());"),
    ]);
    assert_eq!(verdicts(&pf, "x"), ["T0Set"], "{}", pf.dump_text());
}

#[test]
pub(super) fn an_inner_helper_published_through_a_literal_keeps_its_marker() {
    // `h` is published as a field of `api`: its parameter's marker stays a
    // token in its run, so its contract still returns parameter 0 and the
    // caller's store through the result is a store into its own `Box`, not
    // into the `Crate` it never passed. (In a function: a module-scope value
    // is ⊤ to the reader's declaration unit, §12.3.)
    let b = "function h(p: any) { return p; }\nexport const api = { h };";
    let m = "import { api } from './b'; class Box { v: any = 1; } class Crate { v: any = 1; } function main() { const o = new Box(); const c = new Crate(); api.h(o).v = 'x'; console.log(o.v, c.v); } main(); main();";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
    assert!(
        field(tmpl(&pf, "Crate"), "v").readonly,
        "{}",
        pf.dump_text()
    );
}

#[test]
pub(super) fn a_boundary_parameters_field_read_in_a_helper_is_proven() {
    // `join` is closed (decision 13): its callers' literal is beside its
    // parameter's marker. The inner `rd` reads `i.prev`: its parameter's
    // marker stands for `join`'s, all of whose members are closed, so the
    // site is proven on the callers' layout.
    let src = r#"
        function mkIndent() { return { prev: 'a', base: 'b' }; }
        function join(xs: any, indent: any) { function rd(i: any) { return i.prev; } return xs + rd(indent) + indent.base; }
        function main(x: any) { const ind = mkIndent(); console.log(join(x, ind)); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(!verdicts(&pf, "prev").is_empty(), "{}", pf.dump_text());
    assert!(
        proven(&pf, "prev") && proven(&pf, "base"),
        "{}",
        pf.dump_text()
    );
}

#[test]
pub(super) fn a_helpers_result_never_holds_another_helpers_argument() {
    // `H.h1` and `H.h2` both store their argument into one cell, whose
    // marker stands for both; `h1` returns its own argument. The cell's
    // marker never enters `h1`'s run as `h2`'s family, so `a`'s result is
    // `a`'s argument alone: the store through `both`'s result reaches the
    // `Box`, not the `Crate`.
    let pf = both(
        "const keep: any = { p: null }; const H = { h1(p: any) { keep.p = p; return p; }, h2(p: any) { keep.p = p; return p; } }; function a(p: any) { return H.h1(p); } function b(p: any) { return H.h2(p); } b(c); return a(o);",
        "import { both } from './b'; class Box { v: any = 1; } class Crate { v: any = 1; } const r = both(new Box(), new Crate()); r.v = 'x'; console.log(r.v);",
    );
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
    assert!(
        field(tmpl(&pf, "Crate"), "v").readonly,
        "{}",
        pf.dump_text()
    );
}

#[test]
pub(super) fn what_a_cell_holds_is_used_as_its_members() {
    // `f` stores its argument into a cell; `g` hands the cell's value to
    // unknown code. The cell holds its own marker with `f`'s argument as a
    // member, so the escape at `g` is an escape of `f`'s parameter, and the
    // caller's `o.v` is not proven. (One unit: `mk`'s body. With the cell a
    // declaration of its own, `f`'s store into another unit's literal is
    // bug 3, unsound on main.)
    let b = "export function mk() { const cell: any = { s: null }; return { f(x: any) { cell.s = x; }, g() { (globalThis as any).k = cell.s; } }; }";
    let m = "import { mk } from './b'; class Box { v: any = 1; } function main() { const api = mk(); const o = new Box(); api.f(o); api.g(); (globalThis as any).k.v = 'x'; console.log(o.v); } main(); main();";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(!verdicts(&pf, "v").is_empty(), "{}", pf.dump_text());
    assert!(!proven(&pf, "v"), "{}", pf.dump_text());
}

#[test]
pub(super) fn a_parameter_a_cell_gives_back_is_another_calls() {
    // `f` returns what an earlier call stored: another call's argument, not
    // this one's. `n` passes a `Crate` first, `m` a `Box` and stores through
    // the result. Read as "returns parameter 0" `m` would store into its own
    // `Box` while the result is `n`'s `Crate`, which would stay read-only
    // (unsound). The result's marker stands for another call's argument: the
    // result is another unit's value and the argument leaves through it. (The
    // cell is a static of `S`, so it is in `f`'s unit: bug 3 otherwise.)
    let b = "class S { static last: any = 0; static cell: any = { s: null }; }\nexport function f(x: any) { S.last = 1; const o = S.cell.s; S.cell.s = x; return o; }";
    let n = "import { f } from './b'; class Crate { w: any = 0; v: any = 2; } function main() { const c = new Crate(); f(c); console.log(c.v); } main(); main();";
    let m = "import { f } from './b'; class Box { v: any = 1; } function main() { const r = f(new Box()); r.v = 'x'; console.log(r.v); } main(); main();";
    let pf = units_of(&[("b", b), ("n", n), ("m", m)]);
    assert!(
        !field(tmpl(&pf, "Crate"), "v").readonly,
        "{}",
        pf.dump_text()
    );
}

#[test]
pub(super) fn a_written_parameter_is_each_callers_own() {
    // `first` writes its parameter (`n = id(n)`): its binding holds the
    // marker, the variable what `id` returns of it, a token of its run. So
    // `top` returns its own argument: the store through its result is a
    // store into the caller's `Box`, not into the `Crate` passed to `top2`.
    let b = format!("{JOIN}function id(x: any) {{ S.last = 0; return x; }}\nfunction first(n: any) {{ S.last = 1; n = id(n); return n; }}\nexport function top(x: any) {{ S.last = 2; return first(x); }}\nexport function top2(x: any) {{ S.last = 3; return first(x); }}");
    let b = b.as_str();
    let m = "import { top, top2 } from './b'; class Box { v: any = 1; } class Crate { v: any = 2; } function main() { const o = new Box(); const c = new Crate(); top(o).v = 'x'; top2(c); console.log(o.v, c.v); } main(); main();";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
    assert!(
        field(tmpl(&pf, "Crate"), "v").readonly,
        "{}",
        pf.dump_text()
    );
}

#[test]
pub(super) fn a_shared_cell_holds_one_marker() {
    // MRK: every helper's argument crosses into the cell as a member of its
    // one marker; each reader gets that marker and its own key's reach.
    // With the families themselves in the cell each reader would get every
    // helper's family and its key of each: the square of the helpers.
    let (e8, e16, e32) = (
        shared_cell_entries(8),
        shared_cell_entries(16),
        shared_cell_entries(32),
    );
    let (d1, d2) = (e16 - e8, e32 - e16);
    assert!(
        d1 > 0 && d2 < 3 * d1,
        "entries {e8} {e16} {e32}: quadratic in the helpers"
    );
}

#[test]
pub(super) fn a_primitive_method_on_a_receiver_beside_top_is_that_top() {
    // `x` is another unit's value: ⊤ with every primitive kind beside it.
    // `x.trim()` is a call of an unknown function value (the arguments
    // escape, the result is ⊤); the primitive method's summary of the same
    // call says nothing more. Its container of primitives would stand
    // beside the literal in `f`'s result and, no literal shape, sink the
    // result: the literal escaped for no reason.
    let b = "export function f(x: any, c: boolean) { const r = x.trim(); const o = { v: 1 }; return c ? r : o; }";
    let m = "import { f } from './b'; console.log(f((globalThis as any).u, true).v);";
    let pf = units_of(&[("b", b), ("m", m)]);
    let lit = pf
        .templates
        .iter()
        .find(|t| t.display.starts_with("b.ts:__AnonShape"))
        .unwrap_or_else(|| panic!("{}", pf.dump_text()));
    assert!(lit.escape.is_none(), "{}", pf.dump_text());
}

#[test]
pub(super) fn an_escaped_value_beside_top_is_that_top() {
    // Each `l{i}` literal is stored into ⊤ (it escapes) and read back beside
    // ⊤; merged into `y`, the `n` literals ride ⊤ into every array that
    // holds `y`: the square of `n`. Beside ⊤ an escaped value says nothing a
    // reader can use (its layout is escaped, its fields are ⊤), so it is
    // left out there and `y` is ⊤ alone.
    let (e8, e16, e32) = (
        escaped_beside_top_entries(8),
        escaped_beside_top_entries(16),
        escaped_beside_top_entries(32),
    );
    // Merged head: per unit `f`'s literals stored into its argument are its
    // contract's store flows (DECISIONS 118/151), not escapes, so beside the
    // callers' ⊤ they stay and ride into every array: n² until a store into a
    // parameter only ⊤ is passed for escapes there (DECISIONS 192, open).
    // Rewritten for #13294 (decision 151: every unit replays the shared
    // objects' writes): (939, 2563, 8115) -> (964, 2612, 8212); for
    // DECISIONS 210 (the published `f`'s prototype stays its own: two
    // entries fewer at every `n`) -> (962, 2610, 8210); for the builtin
    // namespaces' prototype chain and every array's named-key node
    // (`Model::named_any`, beside its elements, which a store under a
    // computed key fills) -> the pin below (the same n² growth).
    assert_eq!(
        (e8, e16, e32),
        (1068, 2948, 9396),
        "entries {e8} {e16} {e32}"
    );
}

#[test]
pub(super) fn a_written_parameter_without_a_summary_is_each_callers_own() {
    // `up` reads `arguments`, so it has no summary: one body for every call,
    // a run of its own whose parameters get bindings the calls write. It
    // writes its parameter (`n = n.parent`): the variable holds the marker
    // and what it reaches under `parent`, two tokens of its run, so `top`'s
    // result is what the caller's argument reaches. Read as the marker alone
    // (the parameter node's marker standing for both) the result would be
    // the caller's `Child` and the store through it would miss the `Parent`.
    let b = format!("{JOIN}function up(n: any) {{ S.last = 0; if (arguments.length > 1) return null; n = n.parent; return n; }}\nexport function top(x: any) {{ S.last = 1; return up(x); }}\nexport function top2(x: any) {{ S.last = 2; return up(x); }}");
    let b = b.as_str();
    let m = "import { top, top2 } from './b'; class Parent { v: any = 1; } class Child { v: any = 1; parent: any = new Parent(); } class Crate { v: any = 2; parent: any = null; } function main() { const c = new Child(); top(c).v = 'x'; top2(new Crate()); console.log(c.v); } main(); main();";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(
        !field(tmpl(&pf, "Parent"), "v").readonly,
        "{}",
        pf.dump_text()
    );
    assert!(
        field(tmpl(&pf, "Crate"), "v").readonly,
        "{}",
        pf.dump_text()
    );
}

#[test]
pub(super) fn a_result_holds_one_marker() {
    // MRK: `pick` returns what any of `n` cells holds; its result holds one
    // marker standing for the cells' markers, and each reader gets that one.
    // With the cells' markers returned as themselves each reader would get
    // all `n` and its key of each: the square of the cells.
    let (e8, e16, e32) = (
        result_holder_entries(8),
        result_holder_entries(16),
        result_holder_entries(32),
    );
    let (d1, d2) = (e16 - e8, e32 - e16);
    assert!(
        d1 > 0 && d2 < 3 * d1,
        "entries {e8} {e16} {e32}: quadratic in the cells"
    );
}

#[test]
pub(super) fn a_store_beside_top_writes_no_builtin_function_value() {
    // `h` holds another unit's value (⊤) and `Math.max` (the opaque builtin
    // function value, one token for every one). `Math.max` reaches `h`
    // first (a constant), ⊤ later; once the solve settles `h` holds ⊤, so the
    // store into it is a store into a value from unmodeled code, not into a
    // builtin function value, and `Math.min.foo` keeps its builtin members:
    // `y.v` holds no ⊤. Decided when the store first ran, it marked every
    // builtin function value written.
    assert!(
        !builtin_fn_read_top("const h: any = c ? x : g; h.k = 1;"),
        "a store into ⊤ marked builtin function values written"
    );
    // A store into a builtin function value itself still marks them written
    // (one token for every one): the read is ⊤.
    assert!(
        builtin_fn_read_top("g.k = 1;"),
        "a store into a builtin function value must mark them written"
    );
}

#[test]
pub(super) fn a_value_stored_into_another_units_literal_escapes() {
    let pf = units_of(&[("b", SAVED_B), ("m", SAVED_M)]);
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
}

#[test]
pub(super) fn a_value_stored_into_another_units_literal_is_written_in_the_whole_program() {
    let pf = whole_of(&[("b", SAVED_B), ("m", SAVED_M)]);
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
}

#[test]
pub(super) fn a_parameter_stored_through_a_result_into_another_units_literal_escapes() {
    // `get()` gives `saved`, another declaration unit's literal: `a`'s store
    // is a store effect, and the field holds an unknown value (an exported
    // function's parameter). The shared body's observation node and the
    // instance's slot both store `p`: each escapes, so `a`'s argument leaves
    // and `Box.v`, written through `b()`, is not read-only.
    let b = "const saved: any = { p: null };\nfunction get() { return saved; }\nexport function a(p: any) { get().p = p; }\nexport function b() { return saved.p; }";
    let m = "import { a, b } from './b'; class Box { v: any = 1; } const o = new Box(); a(o); const r = b(); r.v = 'x'; console.log(o.v);";
    let mods = [("b", b), ("m", m)];
    assert!(!field(tmpl(&whole_of(&mods), "Box"), "v").readonly);
    let pf = units_of(&mods);
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
}

#[test]
pub(super) fn a_closure_stores_into_another_units_literal() {
    // `f` and `g` are a literal's methods; the cell they share is a
    // declaration unit of its own. What `f` stores, `g` hands to unknown
    // code: `o.v` is written.
    let b = "const cell: any = { s: null };\nexport function mk() { return { f(x: any) { cell.s = x; }, g() { (globalThis as any).k = cell.s; } }; }";
    let m = "import { mk } from './b'; class Box { v: any = 1; } function main() { const api = mk(); const o = new Box(); api.f(o); api.g(); (globalThis as any).k.v = 'x'; console.log(o.v); } main(); main();";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
}

#[test]
pub(super) fn an_arrow_stores_into_another_units_literal() {
    // An exported arrow (a closure of the module scope) stores its argument
    // into `saved.p`; `b()` gives it back and the reader writes it.
    let b = "const saved: any = { p: null };\nexport const put = (p: any) => { saved.p = p; };\nexport function b() { return saved.p; }";
    let m = "import { put, b } from './b'; class Box { v: any = 1; } const o = new Box(); put(o); const r = b(); r.v = 'x'; console.log(o.v);";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
}

#[test]
pub(super) fn a_store_into_another_units_literal_carries_the_stored_kinds() {
    // The field `saved.p` holds what `a` stores (DECISIONS 73): per unit
    // its kinds are never fewer than the whole program's.
    let b = "const saved: any = { p: null };\nexport function a() { saved.p = 5; }\nexport function c() { return saved.p; }";
    let m = "import { a, c } from './b'; a(); console.log(c());";
    let mods = [("b", b), ("m", m)];
    let (pf, w) = (units_of(&mods), whole_of(&mods));
    let lit = |pf: &ProgramFacts| -> crate::derive::FieldFacts {
        let t = pf
            .templates
            .iter()
            .find(|t| t.display.starts_with("b.ts:__AnonShape"))
            .unwrap_or_else(|| panic!("{}", pf.dump_text()));
        field(t, "p").clone()
    };
    let (fu, fw) = (lit(&pf), lit(&w));
    assert!(
        kinds_cover(&fu, &fw),
        "{:?} vs {:?}\n{}",
        fu.kind,
        fw.kind,
        pf.dump_text()
    );
    assert!(!fu.readonly, "{}", pf.dump_text());
}

#[test]
pub(super) fn a_store_into_the_units_own_literal_publishes_nothing() {
    // The literal is `f`'s own: the store is no store effect, nothing
    // leaves, and `o.s.w` is proven as in the whole program.
    let b = "class Inner { w: number = 1; }\nexport function f() { const o: any = { s: null }; o.s = new Inner(); return o.s.w; }";
    let m = "import { f } from './b'; console.log(f());";
    let mods = [("b", b), ("m", m)];
    let (pf, w) = (units_of(&mods), whole_of(&mods));
    assert_eq!(verdicts(&pf, "w"), verdicts(&w, "w"), "{}", pf.dump_text());
    assert!(proven(&pf, "w"), "{}", pf.dump_text());
    assert!(tmpl(&pf, "Inner").escape.is_none(), "{}", pf.dump_text());
}

#[test]
pub(super) fn a_store_deep_in_closed_locals_reaches_the_outer_callers_argument() {
    // DECISIONS 195: the closed locals of `run` are called only in `a`, so
    // they carry no parameter markers; the store `x.k2 = 2` two calls deep
    // lands on what their callers pass, `run`'s own parameter marker, and
    // `run`'s contract tells `b` its literal is written under k0..k2 with
    // numbers: `b`'s reads stay proven as in the whole program.
    let [(an, a), (bn, b)] = closed_chain(3);
    let mods = [(an, a.as_str()), (bn, b.as_str())];
    assert_parity(&mods);
    let pf = units_of(&mods);
    for k in ["k0", "k1", "k2"] {
        assert!(proven(&pf, k), "{k}\n{}", pf.dump_text());
    }
}

#[test]
pub(super) fn a_closed_local_called_only_in_its_unit_costs_its_own_stores() {
    // DECISIONS 195: the closed locals of `run` carry no parameter markers
    // (no other unit can call them), so a store through one lands on what
    // its callers pass; it costs less than the same chain of a literal's
    // methods, whose parameters take argument markers (DECISIONS 44). With
    // markers on the closed locals (as before 195) the chain cost more than
    // the methods (n = 64: 6,986 vs 5,285; now 2,702).
    let n = 64;
    let [(an, a), (bn, b)] = closed_chain(n);
    let closed = module_work(&units_of(&[(an, a.as_str()), (bn, b.as_str())]), "a.ts");
    let mut m = String::from("export function run(o: any) {\n  const h = {\n");
    for i in 0..n {
        let next = match i + 1 < n {
            true => format!(" h.f{}(x);", i + 1),
            false => String::new(),
        };
        m += &format!("    f{i}(x: any) {{ x.k{i} = {i};{next} }},\n");
    }
    m += "  };\n  h.f0(o);\n}\n";
    let methods = module_work(&units_of(&[("a", m.as_str()), (bn, b.as_str())]), "a.ts");
    assert!(
        closed * 4 < methods * 3,
        "closed {closed} methods {methods}"
    );
}

#[test]
pub(super) fn a_method_run_on_an_argument_marker_makes_one_context() {
    // DECISIONS 201: `go` runs on the published method's receiver, an
    // argument marker (DECISIONS 44); its arrow captures `this` in a closure
    // context, storage that takes its own marker, and runs `go` again on it.
    // A context per marker receiver made a new instance, context and marker
    // per lap without end (ajv `compileAsync`: 3.8M instances, the unit over
    // 24 GB). A marker is no object of this unit: its runs share one context.
    let a = r#"
        export class A {
          refs: any = {};
          compileAsync(s: any) {
            return go.call(this, s);
            function go(x: any): any {
              const body = () => { check.call(this, x); return go.call(this, x); };
              return body();
            }
            function check(e: any) { if (this.refs[e]) throw new Error("loaded"); }
          }
        }
        "#;
    let b = "import { A } from \"./a\";\nconsole.log(new A().compileAsync(1));\n";
    let cfg = FactsConfig {
        max_work: Some(5_000_000),
        ..FactsConfig::default()
    };
    let pf = units_with(&[("a", a), ("b", b)], &cfg);
    let us: Vec<_> = pf
        .units
        .iter()
        .filter(|(q, _)| crate::decl_units::module_of(q) == "a.ts")
        .collect();
    assert!(!us.is_empty(), "no unit of a.ts");
    for (q, s) in us {
        assert!(
            !s.aborted && s.instances < 64,
            "{q}: aborted {} instances {}",
            s.aborted,
            s.instances
        );
    }
}

#[test]
pub(super) fn another_units_json_of_a_parameter_runs_to_json_and_ends() {
    // `ser` serializes its parameter: on the marker, the JSON protocol's
    // reads are what the marker reaches (one touch of the reach, not one
    // marker per level), and `w.toJSON`, two fields down, runs.
    let b = "export function ser(p: any) { return JSON.stringify(p); }";
    let m = "import { ser } from './b'; function main() { const w = { n: 1, toJSON() { this.n = 'x'; return 0; } }; const o = { a: { b: w } }; console.log(ser(o), w.n); } main(); main();";
    let pf = units_with(
        &[("b", b), ("m", m)],
        &FactsConfig {
            max_work: Some(2_000_000),
            ..FactsConfig::default()
        },
    );
    assert!(!pf.stats.aborted, "{}", pf.dump_text());
    let w = pf
        .templates
        .iter()
        .find(|t| t.birth_keys == ["n", "toJSON"])
        .expect("w literal");
    assert!(!field(w, "n").readonly, "{}", pf.dump_text());
}

#[test]
pub(super) fn a_published_literal_field_may_hold_another_units_layout() {
    // ts.Diagnostics (diagnosticInformationMap.generated.ts): `diag()`'s
    // result layout is its unit's; the `Diagnostics` literal of another
    // unit holds such records in its fields. Forwarded with the owner's
    // contract (DECISIONS 79), nothing of the literal's unit leaves through
    // them: the field is described by that layout, the layout stays whole
    // and a reader of `Diagnostics.A.code` reads its kind. Was: the field
    // was sunk, the records escaped, the census broke the layout and every
    // round sank it again.
    let b = "function diag(code: number, message: string) { return { code, message }; } export const Diagnostics = { A: diag(1, 'a'), B: diag(2, 'b') };";
    let m = "import { Diagnostics } from './b'; function main() { const d = Diagnostics.A; console.log(d.code + 1, Diagnostics.B.message); } main();";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(
        !literal_broken(&pf, &["code", "message"]),
        "{}",
        pf.dump_text()
    );
    for k in ["code", "message"] {
        for s in pf.sites.iter().filter(|s| s.key == k) {
            assert!(!s.top, ".{k} reads ⊤\n{}", pf.dump_text());
        }
    }
}

/// Per unit, the boundary's store standing in for other units' stores into
/// what this unit gives them is no write of the intrinsics: each unit
/// publishes its own stores into ⊤, and every unit replays them under their
/// key. A class key read on a string stays absent until some unit stores
/// into ⊤ under it.
#[test]
pub(super) fn the_boundary_store_writes_no_intrinsic_member() {
    let b = "export function poke(t: any) { t.inner = function () { return 1; }; }";
    let m = |w: &str| {
        format!(
            "import {{ poke }} from './b';
            class A {{ a = 1; }}
            class M {{ m(o: any) {{ return o.a; }} }}
            class N {{ inner: any; constructor() {{ this.inner = new M(); }} }}
            declare const k: number;
            {w}
            const v: any = k > 1 ? new N() : \"text\";
            const i = v.inner;
            if (i) console.log(i.m(new A()));"
        )
    };
    let proven_m = |pf: &ProgramFacts| {
        let s: Vec<_> = pf.sites.iter().filter(|s| s.key == "m").collect();
        !s.is_empty()
            && s.iter()
                .all(|s| !s.top && matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
    };
    let pf = units_of(&[("b", b), ("m", &m(""))]);
    assert!(proven_m(&pf), "{}", pf.dump_text());
    let pf = units_of(&[("b", b), ("m", &m("poke((globalThis as any).u);"))]);
    assert!(!proven_m(&pf), "{}", pf.dump_text());
}
