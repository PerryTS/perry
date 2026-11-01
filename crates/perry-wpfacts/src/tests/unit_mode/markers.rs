use super::*;

#[test]
pub(super) fn a_function_value_passed_to_a_closed_callee_keeps_its_arguments() {
    // Decision 19 (tssrc moduleNameResolver: `loadModuleFromFileNoPackageId`
    // passed as `loader` to closed `tryLoadModuleUsingOptionalResolutionSettings`,
    // which calls it with `state`): `load` is referenced only as a direct
    // callee or as an argument at a call of a closed function, so it is
    // closed too; `run` (its own unit) publishes what its calls of the
    // function value pass, and `state.flag` holds on `mkState`'s literal.
    let src = r#"
        function mkState() { return { flag: 'a', host: 'b' }; }
        function load(x: any, state: any) { return x + state.flag + state.host; }
        function run(x: any, loader: any, state: any) { return loader(x, state); }
        function main(x: any) { const st = mkState(); console.log(run(x, load, st)); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(pf.units.len() >= 4, "{}", pf.dump_text());
    assert!(
        proven(&pf, "flag") && proven(&pf, "host"),
        "{}",
        pf.dump_text()
    );
    assert!(!verdicts(&pf, "flag").is_empty(), "{}", pf.dump_text());
    // A test must be able to fail: with no published arguments the
    // parameters of `load` hold nothing and the reads are vacuously T0 on
    // no layout; the proof must name the literal the calls of `run` pass.
    assert!(
        pf.sites
            .iter()
            .filter(|s| s.key == "flag" || s.key == "host")
            .all(|s| !s.receivers.is_empty()),
        "{}",
        pf.dump_text()
    );
    assert!(pf.broken.is_empty(), "{:?}", pf.broken);
    // Soundness: the callee lets the function value escape (unknown code
    // may call it with anything): the census breaks it, its parameters ⊤.
    let src = r#"
        function mkState() { return { flag: 'a', host: 'b' }; }
        function load(x: any, state: any) { return x + state.flag + state.host; }
        function run(x: any, loader: any, state: any) { (globalThis as any).l = loader; return loader(x, state); }
        function main(x: any) { const st = mkState(); console.log(run(x, load, st)); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(!proven(&pf, "flag"), "{}", pf.dump_text());
    // Soundness: the callee returns it; its caller calls it with a number.
    let src = r#"
        function mkState() { return { flag: 'a', host: 'b' }; }
        function load(x: any, state: any) { return x + state.flag + state.host; }
        function run(x: any, loader: any, state: any) { loader(x, state); return loader; }
        function main(x: any) { const st = mkState(); const f = run(x, load, st); console.log(f(x, 5)); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(!proven(&pf, "flag"), "{}", pf.dump_text());
    // Soundness: the callee passes it to unknown code (a ⊤ callee; a
    // modeled builtin such as forEach calls it with what it models).
    let src = r#"
        function mkState() { return { flag: 'a', host: 'b' }; }
        function load(x: any, state: any) { return x + state.flag + state.host; }
        function run(x: any, loader: any, state: any) { (globalThis as any).hook(loader); return loader(x, state); }
        function main(x: any) { const st = mkState(); console.log(run(x, load, st)); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(!proven(&pf, "flag"), "{}", pf.dump_text());
    // Soundness: a closed function its own unit passes to a closed callee
    // of another unit. Rewritten for decision 23 (which reverses 22a): it
    // is described there, so the callee's calls of it publish what they
    // pass and its parameter holds that too; `run` passes another literal,
    // so `.flag` is proven on no single layout. (A number, the old case,
    // is a primitive receiver: T0 judges object receivers only, as for the
    // undefined every closed parameter holds.)
    let src = r#"
        function mkState() { return { flag: 'a', host: 'b' }; }
        function run(x: any, loader: any, state: any) { return loader(x, { other: 5 }); }
        function load(x: any, state: any) { if (x > 5) return run(x - 1, load, state); return x + state.flag; }
        function main(x: any) { const st = mkState(); console.log(load(x, st)); }
        main(1); main(10);
        "#;
    let pf = units(src);
    assert!(!proven(&pf, "flag"), "{}", pf.dump_text());
}

#[test]
pub(super) fn an_own_function_value_passed_to_a_closed_callee_is_described() {
    // Decision 23 (tssrc moduleNameResolver: tryResolve's own arrow
    // `const loader` passed to closed `tryLoadModuleUsingOptionalResolutionSettings`,
    // which another unit also calls with a sibling's function): the local
    // literal is closed too, so `run`'s parameter stays described for every
    // caller; `load` keeps its arguments and so does the local arrow.
    let src = r#"
        function mkState() { return { flag: 'a', host: 'b' }; }
        function load(x: any, state: any) { return x + state.flag; }
        function run(x: any, loader: any, state: any) { return loader(x, state); }
        function viaLocal(x: any) { const st = mkState(); const loader = (y: any, s: any) => y + s.host; return run(x, loader, st); }
        function main(x: any) { const st = mkState(); console.log(run(x, load, st)); console.log(viaLocal(x)); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(pf.units.len() >= 5, "{}", pf.dump_text());
    assert!(pf.broken.is_empty(), "{:?}", pf.broken);
    assert!(
        !verdicts(&pf, "flag").is_empty() && !verdicts(&pf, "host").is_empty(),
        "{}",
        pf.dump_text()
    );
    assert!(
        proven(&pf, "flag") && proven(&pf, "host"),
        "{}",
        pf.dump_text()
    );
    // A test must be able to fail: the proofs name the literal passed.
    assert!(
        pf.sites
            .iter()
            .filter(|s| s.key == "flag" || s.key == "host")
            .all(|s| !s.receivers.is_empty()),
        "{}",
        pf.dump_text()
    );
    // Soundness: the callee lets the local escape (unknown code may call
    // it with anything): broken in the census, its parameters ⊤.
    let src = r#"
        function mkState() { return { flag: 'a', host: 'b' }; }
        function run(x: any, loader: any, state: any) { (globalThis as any).l = loader; return loader(x, state); }
        function viaLocal(x: any) { const st = mkState(); const loader = (y: any, s: any) => y + s.host; return run(x, loader, st); }
        viaLocal(1); viaLocal(2);
        "#;
    let pf = units(src);
    assert!(
        !verdicts(&pf, "host").is_empty() && !proven(&pf, "host"),
        "{}",
        pf.dump_text()
    );
    // Soundness: its own function stores it (a reference that is no call
    // and no argument of a closed call): not closed, unknown as before.
    let src = r#"
        function mkState() { return { flag: 'a', host: 'b' }; }
        function run(x: any, loader: any, state: any) { return loader(x, state); }
        function viaLocal(x: any) { const st = mkState(); const loader = (y: any, s: any) => y + s.host; (globalThis as any).k = loader; return run(x, loader, st); }
        viaLocal(1); viaLocal(2);
        "#;
    let pf = units(src);
    assert!(
        !verdicts(&pf, "host").is_empty() && !proven(&pf, "host"),
        "{}",
        pf.dump_text()
    );
    // Two closed locals of one name are two functions (tssrc: three
    // `loader`s in moduleNameResolver.ts): the one a callee lets escape is
    // broken, the other keeps its arguments (each passes its own literal:
    // the escaping call lets its argument escape too).
    let src = r#"
        function mkA() { return { host: 'b', port: 1 }; }
        function mkB() { return { flag: 'a' }; }
        function run(x: any, loader: any, state: any) { return loader(x, state); }
        function leak(x: any, loader: any, state: any) { (globalThis as any).l = loader; return loader(x, state); }
        function a(x: any) { const st = mkA(); const loader = (y: any, s: any) => y + s.host; return run(x, loader, st); }
        function b(x: any) { const st = mkB(); const loader = (y: any, s: any) => y + s.flag; return leak(x, loader, st); }
        a(1); a(2); b(1); b(2);
        "#;
    let pf = units(src);
    assert!(
        !verdicts(&pf, "flag").is_empty() && !proven(&pf, "flag"),
        "{}",
        pf.dump_text()
    );
    assert!(
        !verdicts(&pf, "host").is_empty() && proven(&pf, "host"),
        "{}",
        pf.dump_text()
    );
    assert!(
        pf.sites
            .iter()
            .filter(|s| s.key == "host")
            .all(|s| !s.receivers.is_empty()),
        "{}",
        pf.dump_text()
    );
    // Soundness: the callee calls it with another literal for `s`.
    let src = r#"
        function mkState() { return { flag: 'a', host: 'b' }; }
        function run(x: any, loader: any, state: any) { loader(x, { other: 5 }); return loader(x, state); }
        function viaLocal(x: any) { const st = mkState(); const loader = (y: any, s: any) => y + s.host; return run(x, loader, st); }
        viaLocal(1); viaLocal(2);
        "#;
    let pf = units(src);
    assert!(
        !verdicts(&pf, "host").is_empty() && !proven(&pf, "host"),
        "{}",
        pf.dump_text()
    );
}

#[test]
pub(super) fn a_unit_is_ordered_after_the_units_owning_the_names_it_imports() {
    // Decision 21. `a` and `b` import each other, but no name cycle exists:
    // `useD` (b) names `D`, `R` (a) names `useD`, `D` names nothing. By
    // whole modules every unit of `a` comes after every unit of `b` and back,
    // so the driver solved `useD` before `D` and `d.has` read no contract
    // (⊤). Ordered by the owners of the names, `D`'s contract exists first.
    let a = r#"
        import { useD } from './b';
        export class D { pos: Map<number, number>; constructor() { this.pos = new Map(); } has(k: number) { return this.pos.has(k); } }
        export class R { run() { return useD(); } }
        "#;
    let b =
        "import { D } from './a'; export function useD() { const d = new D(); return d.has(1); }";
    let pf = units_of(&[
        ("a", a),
        ("b", b),
        ("m", "import { R } from './a'; console.log(new R().run());"),
    ]);
    let mi = pf.module_paths.iter().position(|p| p == "b.ts").unwrap() as u32;
    let site = pf
        .sites
        .iter()
        .find(|s| s.module == mi && s.key == "has")
        .expect("the call in b");
    assert_eq!(site.verdict, crate::Verdict::T0Mono, "{}", pf.dump_text());
}

#[test]
pub(super) fn a_closed_function_its_own_unit_lets_escape_has_top_parameters() {
    // Decision 22(b): a closed function's publication is no escape, any
    // other escape is real and gives ⊤ parameters. `run` and `load` call
    // each other (one unit, no census between them); `run` stores the
    // function value it is passed, so unknown code may call `load` with
    // anything: `state.flag` is not proven on `mkState`'s literal. (Its
    // arguments still name the literal, so the closed contract alone would
    // prove it: the escape must override it.)
    let src = r#"
        function mkState() { return { flag: 'a', host: 'b' }; }
        function run(x: any, loader: any, state: any) { (globalThis as any).l = loader; if (x < 0) return load(x, state); return loader(x, state); }
        function load(x: any, state: any) { if (x > 5) return run(x - 1, load, state); return x + state.flag; }
        function main(x: any) { const st = mkState(); console.log(load(x, st)); }
        main(1); main(10);
        "#;
    let pf = units(src);
    assert!(
        !verdicts(&pf, "flag").is_empty() && !proven(&pf, "flag"),
        "{}",
        pf.dump_text()
    );
    // The same without the store: proven, so the case above can fail.
    let pf = units(&src.replace("(globalThis as any).l = loader; ", ""));
    assert!(
        proven(&pf, "flag")
            && pf
                .sites
                .iter()
                .filter(|s| s.key == "flag")
                .all(|s| !s.receivers.is_empty()),
        "{}",
        pf.dump_text()
    );
}

#[test]
pub(super) fn a_closed_function_without_a_summary_its_own_unit_lets_escape_has_top_parameters() {
    // Decision 22(b) on the call path (`Model::escape_fn_params`): the twin
    // of the test above for a closed function the solver does not summarize.
    // `load` calls a local function through `.call`, which no summary rule
    // covers (summary refused: "call … by no rule"), so unknown code calling
    // `load` reaches its plain parameters. `run` stores `load`: a real escape,
    // ⊤ parameters, `state.flag` not proven on `mkState`'s literal.
    let src = r#"
        function mkState() { return { flag: 'a', host: 'b' }; }
        function run(x: any, loader: any, state: any) { (globalThis as any).l = loader; if (x < 0) return load(x, state); return loader(x, state); }
        function load(x: any, state: any): any { if (x > 5) return run(x - 1, load, state); function z(this: any, y: any) { return this.host + y; } z.call(state, x); return x + state.flag; }
        function main(x: any) { const st = mkState(); console.log(load(x, st)); }
        main(1); main(10);
        "#;
    let pf = units(src);
    assert!(
        !verdicts(&pf, "flag").is_empty() && !proven(&pf, "flag"),
        "{}",
        pf.dump_text()
    );
    // The same without the store: proven, so the case above can fail.
    let pf = units(&src.replace("(globalThis as any).l = loader; ", ""));
    assert!(
        proven(&pf, "flag")
            && pf
                .sites
                .iter()
                .filter(|s| s.key == "flag")
                .all(|s| !s.receivers.is_empty()),
        "{}",
        pf.dump_text()
    );
}

#[test]
pub(super) fn a_function_a_tdz_check_reads_stays_closed() {
    // effect Cron.ts: `parse` calls the module-scope arrow `parseSegment`
    // defined after it, so every call carries a TDZ check
    // (`js_tdz_check_binding(parseSegment, ..)`). The check only tests the
    // slot for the dead-zone hole and yields undefined: it is no reference
    // to the function value, so `parseSegment` is closed (decision 13) and
    // its parameter holds what its calls pass (`opts.max` on the literals).
    let src = r#"
        const secondOptions = { min: 0, max: 59 };
        const hourOptions = { min: 0, max: 23 };
        export const parse = (s: string) => {
          return [parseSegment(s, secondOptions), parseSegment(s, hourOptions)];
        };
        const parseSegment = (input: string, opts: any): number => {
          return opts.max - opts.min + input.length;
        };
        console.log(parse("a"));
        "#;
    let pf = units(src);
    assert!(
        proven(&pf, "max")
            && pf
                .sites
                .iter()
                .filter(|s| s.key == "max")
                .all(|s| !s.receivers.is_empty()),
        "{}",
        pf.dump_text()
    );
    // A real reference besides the calls lets it escape: not proven, so the
    // case above can fail.
    let pf = units(&src.replace(
        "console.log(parse(\"a\"));",
        "console.log(parse(\"a\")); (globalThis as any).f = parseSegment;",
    ));
    assert!(
        !verdicts(&pf, "max").is_empty() && !proven(&pf, "max"),
        "{}",
        pf.dump_text()
    );
}

#[test]
pub(super) fn a_literal_passed_to_a_closed_function_carries_its_field_functions() {
    // tssrc program.ts: `createProgram` passes the literal `{ compilerHost:
    // host, getSymlinkCache, toPath, ... }` to the closed
    // `updateHostForUseSourceOfProjectReferenceRedirect`, which calls
    // `host.toPath(..)`. The argument contract (decision 13) describes the
    // literal's field functions as a value contract does (`note_field_fns`):
    // the callee calls them through their contract, the literal does not
    // escape there, so its layout is not broken and `host.base` holds.
    let src = r#"
        function update(host: any) { const k = host.base; return host.toPath(k) + host.base; }
        function main(x: any) { const h = { base: 'b', toPath: (s: string) => s + x }; console.log(update(h)); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(pf.units.len() >= 2, "{}", pf.dump_text());
    assert!(
        proven(&pf, "base")
            && pf
                .sites
                .iter()
                .filter(|s| s.key == "base")
                .all(|s| !s.receivers.is_empty()),
        "{}",
        pf.dump_text()
    );
    assert!(pf.broken.is_empty(), "{:?}", pf.broken);
    // The callee storing a computed key into the literal breaks its layout
    // (rewritten for design §16: a named new key is now the owner's shape
    // transition): `base` is not proven, so the case above can fail. (Principle: the census
    // decides; the old negative, `globalThis.f = h.toPath` in the caller,
    // no longer keeps a field function out of the contract, decision 29.)
    let pf = units(&src.replace("const k = host.base;", "const k = host.base; host[k] = 1;"));
    assert!(
        !verdicts(&pf, "base").is_empty() && !proven(&pf, "base"),
        "{}",
        pf.dump_text()
    );
    // Here `toPath` escaped in the caller before it became a contract
    // function (through the callee's previous contract: what its argument
    // reaches escapes). It still gets parameter markers then: the
    // `fn_contract_of` debug assertion checks every contract function's
    // parameters carry them (sabotage arm 76).
}

#[test]
pub(super) fn parallel_levels_give_the_serial_facts() {
    // Decision 32: `z` and `b` import nothing (level 0), `c` and `e` import
    // only `z` (level 1), `m` imports `c` (level 2). A level is solved in parallel against one frozen
    // input: the facts are those of the serial solve, and `z`'s contract
    // still reaches its importers (the call in `c` is proven through it).
    let mods = [
        ("z", DUAL),
        ("b", "export const k = 2;"),
        ("c", "import { D } from './z'; const d = new D([3]); console.log(d.has(3)); export const n = 3;"),
        ("e", "import { D } from './z'; const d = new D([4]); console.log(d.has(4));"),
        ("m", "import { D } from './z'; import { k } from './b'; import { n } from './c'; console.log(new D([k, n]).has(5));"),
    ];
    let run = |jobs| {
        let pf = units_with(
            &mods,
            &FactsConfig {
                jobs,
                ..FactsConfig::default()
            },
        );
        let facts = format!(
            "{:?}\n{:?}\n{:?} {:?} {}",
            pf.templates, pf.sites, pf.broken, pf.round_work, pf.rounds
        );
        (facts, pf)
    };
    let (serial, _) = run(1);
    let (parallel, pf) = run(4);
    let ci = pf.module_paths.iter().position(|p| p == "c.ts").unwrap() as u32;
    let site = pf
        .sites
        .iter()
        .find(|s| s.module == ci && s.key == "has")
        .expect("the call in c");
    assert_eq!(site.verdict, Verdict::T0Mono, "{}", pf.dump_text());
    assert!(pf.broken.is_empty(), "{}", pf.dump_text());
    assert_eq!(serial, parallel);
}

#[test]
pub(super) fn the_object_key_follows_what_an_importer_observes() {
    // Design §13.2: a module's object-cache key (`digest_for`) holds the
    // templates its sites observe, contracted imports included. A layout
    // change in `a` that `m` reads changes `m`'s key; an edit `m` does not
    // observe keeps it, so `m`'s object is reused.
    let user = "import { P } from './a'; const p = new P(); console.log(p.x);";
    let a = "export class P { x: number; constructor() { this.x = 1; } }\n";
    let grown =
        "export class P { x: number; y: number; constructor() { this.x = 1; this.y = 2; } }\n";
    let other = format!("{a}export function helper() {{ const o = {{ k: 1 }}; return o.k; }}\n");
    let base = units_of(&[("a", a), ("m", user)]);
    let grown = units_of(&[("a", grown), ("m", user)]);
    let other = units_of(&[("a", &other), ("m", user)]);
    assert_ne!(
        base.digest_for("a.ts"),
        other.digest_for("a.ts"),
        "{}",
        other.dump_text()
    );
    assert_ne!(
        base.digest_for("m.ts"),
        grown.digest_for("m.ts"),
        "{}",
        grown.dump_text()
    );
    assert_eq!(
        base.digest_for("m.ts"),
        other.digest_for("m.ts"),
        "{}",
        other.dump_text()
    );
}

#[test]
pub(super) fn a_store_into_an_argument_keeps_no_field_of_the_marker() {
    // DECISIONS 41: a parameter marker is another unit's value; a store into
    // it is its contract's escape and keeps no field. With a field, the
    // shared helper `h` stores every caller's `b` marker into every
    // caller's `a` marker: the unit grows with the square of the callers.
    let (e8, e16, e32) = (
        marker_store_entries(8),
        marker_store_entries(16),
        marker_store_entries(32),
    );
    let (d1, d2) = (e16 - e8, e32 - e16);
    assert!(
        d2 < 3 * d1,
        "entries {e8} {e16} {e32}: quadratic in the callers"
    );
}

#[test]
pub(super) fn an_escaped_marker_is_the_top_beside_it() {
    // DECISIONS 42: once a contract records an argument's escape, its
    // consumer sinks the argument and reads nothing else of it, so the
    // marker says no more than the ⊤ beside it. Delivered anyway, the
    // shared `h` returns every caller's marker to every caller (`mid` is
    // copied per caller, `h` is not): the unit grows with the square of
    // the callers.
    let (e8, e16, e32) = (
        escaped_marker_entries(8),
        escaped_marker_entries(16),
        escaped_marker_entries(32),
    );
    let (d1, d2) = (e16 - e8, e32 - e16);
    assert!(
        d2 < 3 * d1,
        "entries {e8} {e16} {e32}: quadratic in the callers"
    );
}

#[test]
pub(super) fn an_escaped_marker_in_a_result_is_another_units_value() {
    // DECISIONS 42, the reader: `put`'s marker reads as ⊤ in `make`'s
    // result (another unit's value), so the result stays checked and
    // `lit` is given out by its layout, not escaped with the result.
    let m = "import { put, poke, make, rd } from './b'; put({ a: 1 }); poke(); const r = make(0); console.log(r.n, rd());";
    let pf = units_of(&[("b", HELD), ("m", m)]);
    let lit = pf
        .templates
        .iter()
        .find(|t| t.birth_keys == ["n"])
        .unwrap_or_else(|| panic!("lit in\n{}", pf.dump_text()));
    assert!(lit.escape.is_none(), "{}", pf.dump_text());
}

#[test]
pub(super) fn an_escaped_marker_of_another_function_is_no_result_transfer() {
    // DECISIONS 42, the contract: `put`'s marker in `make`'s result is
    // ⊤, not `make`'s parameter 0 coming back. Read as `put`'s parameter
    // index, the caller's `o` would come back as `r` and the store would
    // reach `Box.v`.
    let m = "import { put, poke, make } from './b'; class Box { v: any = 1; } put({ a: 1 }); poke(); function main() { const o = new Box(); const r = make(o); r.v = 'x'; console.log(o.v); } main(); main();";
    let pf = units_of(&[("b", HELD), ("m", m)]);
    assert!(field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
}

#[test]
pub(super) fn a_shared_body_holds_one_marker_per_parameter() {
    // DECISIONS 44: a parameter node holds its argument marker in place of
    // the markers its callers pass. Without it the shared `h` returns every
    // caller's marker to every caller (`mid` is copied per caller, `h` is
    // not), and nothing escapes, so DECISIONS 42 does not apply: the unit
    // grows with the square of the callers.
    let (e8, e16, e32) = (
        shared_helper_entries(8),
        shared_helper_entries(16),
        shared_helper_entries(32),
    );
    let (d1, d2) = (e16 - e8, e32 - e16);
    assert!(
        d2 < 3 * d1,
        "entries {e8} {e16} {e32}: quadratic in the callers"
    );
}

#[test]
pub(super) fn a_use_in_a_shared_body_is_a_use_of_each_callers_argument() {
    // DECISIONS 44: the argument marker's uses are its members' uses. `keep`
    // reaches `h` through `mid` (shared with `keep2`); `h` lets what its
    // argument reaches escape, so the caller's `o.inner` is not its own.
    let b = "function h(p: any) { (globalThis as any).k = p.inner; }\nfunction mid(p: any) { h(p); }\nexport function keep(p: any) { mid(p); }\nexport function keep2(p: any) { mid(p); }";
    let m = "import { keep, keep2 } from './b'; function main() { const o = { inner: { v: 1 } }; keep(o); keep2({ inner: 0 }); (globalThis as any).k.v = 'x'; console.log(o.inner.v); } main(); main();";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(!proven(&pf, "v"), "{}", pf.dump_text());
}

#[test]
pub(super) fn a_keyed_use_in_a_recursion_is_a_use_of_each_callers_argument() {
    // Design §18.4 with DECISIONS 44 when the shared bodies recurse: `h`
    // and `mid` pass their argument to each other, so their argument
    // markers stand for each other (one cycle of the member graph). `h`
    // lets `p.inner` escape; the keyed use reaches `keep`'s parameter
    // through the cycle (settled after the solve), so the caller's
    // `o.inner` is not its own.
    let b = "function h(p: any, n: number) { if (n > 0) mid(p, n - 1); (globalThis as any).k = p.inner; }\nfunction mid(p: any, n: number) { h(p, n); }\nexport function keep(p: any) { mid(p, 2); }\nexport function keep2(p: any) { mid(p, 2); }";
    let m = "import { keep, keep2 } from './b'; function main() { const o = { inner: { v: 1 } }; keep(o); keep2({ inner: 0 }); (globalThis as any).k.v = 'x'; console.log(o.inner.v); } main(); main();";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(!proven(&pf, "v"), "{}", pf.dump_text());
}

#[test]
pub(super) fn a_computed_key_use_in_a_shared_body_is_a_use_of_each_callers_argument() {
    // DECISIONS 44 under design §18.4: a named key reaches through its keyed
    // reach marker; a computed key through the marker's reach marker, which
    // an argument marker forwards to each marker it stands for. `h` lets
    // `p[k]` escape, so the caller's `o.inner` is not its own.
    let b = "function h(p: any, k: string) { (globalThis as any).k = p[k]; }\nfunction mid(p: any, k: string) { h(p, k); }\nexport function keep(p: any, k: string) { mid(p, k); }\nexport function keep2(p: any, k: string) { mid(p, k); }";
    let m = "import { keep, keep2 } from './b'; function main() { const o = { inner: { v: 1 } }; keep(o, 'inner'); keep2({ inner: 0 }, 'inner'); (globalThis as any).k.v = 'x'; console.log(o.inner.v); } main(); main();";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(!proven(&pf, "v"), "{}", pf.dump_text());
}

#[test]
pub(super) fn an_argument_a_shared_body_returns_comes_back_to_the_caller() {
    // DECISIONS 44: a result holding an argument marker holds the markers
    // it stands for. `id` returns its argument through the shared `h`, so
    // the caller's store through the result is a store into its own `Box`.
    let b = "function h(p: any) { return p; }\nfunction mid(p: any) { return h(p); }\nexport function id(p: any) { return mid(p); }\nexport function id2(p: any) { return mid(p); }";
    let m = "import { id, id2 } from './b'; class Box { v: any = 1; } const o = new Box(); const r = id(o); id2(0); r.v = 'x'; console.log(o.v);";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
}

#[test]
pub(super) fn a_shared_body_returns_each_caller_its_own_argument() {
    // DECISIONS 44 (b): the shared `h` (and `id`) return their argument
    // marker; at each call's result it is that caller's own argument. Without
    // it every caller gets the shared marker back, reads its own key of it
    // (one keyed reach marker per caller on the shared marker) and then any
    // key of it (all of them): the unit grows with the square of the callers.
    let (e8, e16, e32) = (
        relative_result_entries(8),
        relative_result_entries(16),
        relative_result_entries(32),
    );
    let (d1, d2) = (e16 - e8, e32 - e16);
    assert!(
        d1 > 0 && d2 < 3 * d1,
        "entries {e8} {e16} {e32}: quadratic in the callers"
    );
}

#[test]
pub(super) fn a_caller_gets_back_only_what_it_passed() {
    // DECISIONS 44 (b): `id` and `id2` return their argument through the
    // shared `H.h`. `id`'s result is `id`'s argument alone, so `both`
    // returns `o`, not `c`, and the store through it is a store into the
    // `Box`, not into the `Crate`.
    let pf = both(
        "const H = { h(p: any) { return p; } }; function id(p: any) { return H.h(p); } function id2(p: any) { return H.h(p); } id2(c); return id(o);",
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
pub(super) fn an_argument_a_closure_keeps_is_every_callers() {
    // DECISIONS 44 (b): `H.h` keeps its first argument in a closure and
    // returns it on every later call, so `b`'s result is `a`'s argument.
    // The marker entered a closure cell (it left its run): its family comes
    // back as every caller's argument, and `both` may return `o`.
    let pf = both(
        "let saved: any = null; const H = { h(p: any) { if (!saved) saved = () => p; return saved(); } }; function a(p: any) { return H.h(p); } function b(p: any) { return H.h(p); } a(o); return b(c);",
        "import { both } from './b'; class Box { v: any = 1; } both(new Box(), 0).v = 'x'; console.log(1);",
    );
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
}

#[test]
pub(super) fn an_argument_the_heap_keeps_is_every_callers() {
    // DECISIONS 44 (b): as above through an object field: the marker entered
    // the heap (it left its run).
    let pf = both(
        "const keep: any = { p: null }; const H = { h(p: any) { if (!keep.p) keep.p = p; return keep.p; } }; function a(p: any) { return H.h(p); } function b(p: any) { return H.h(p); } a(o); return b(c);",
        "import { both } from './b'; class Box { v: any = 1; } both(new Box(), 0).v = 'x'; console.log(1);",
    );
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
}

#[test]
pub(super) fn a_recursion_returns_what_its_argument_reaches() {
    // DECISIONS 44 (b) through a recursion: `W.walk` returns its argument
    // or, through itself, what its argument reaches under `next`. At the
    // recursive call's result the keyed reach marker of `next` is the
    // caller's `n.next`, and a reach marker reaches itself under every key,
    // so the substitution ends. `both` may return the `Tail`.
    let pf = both(
        "const W = { walk(n: any): any { return n.next ? W.walk(n.next) : n; } }; function last(p: any) { return W.walk(p); } function last2(p: any) { return W.walk(p); } last2(c); return last(o);",
        "import { both } from './b'; class Head { v: any = 1; next: any = null; } class Tail { v: any = 1; } const h = new Head(); h.next = new Tail(); both(h, 0).v = 'x'; console.log(h.v);",
    );
    assert!(
        !field(tmpl(&pf, "Tail"), "v").readonly,
        "{}",
        pf.dump_text()
    );
    assert!(pf.units.iter().all(|(_, s)| !s.aborted), "{:?}", pf.units);
}

#[test]
pub(super) fn a_written_parameter_is_not_the_callers_argument() {
    // DECISIONS 44 (b): `U.up` writes its parameter (`n = n.parent`), so its
    // marker stands for the caller's `n.parent` too and is never replaced
    // by the caller's argument alone: `both` may return the `Parent`.
    let pf = both(
        "const U = { up(n: any) { n = n.parent; return n; } }; function top(p: any) { return U.up(p); } function top2(p: any) { return U.up(p); } top2(c); return top(o);",
        "import { both } from './b'; class Parent { v: any = 1; } class Child { v: any = 1; parent: any = new Parent(); } both(new Child(), 0).v = 'x'; console.log(1);",
    );
    assert!(
        !field(tmpl(&pf, "Parent"), "v").readonly,
        "{}",
        pf.dump_text()
    );
}

#[test]
pub(super) fn an_argument_marker_that_arrives_late_is_lifted() {
    // DECISIONS 44 (b): `b` passes `H.h` the result of `H.g`, whose marker
    // arrives after `H.h`'s result already holds its family (from `a`'s
    // call): the call's argument rule lifts it then. `both` returns `c`.
    let pf = both(
        "const H = { h(p: any) { return p; }, g(p: any) { return p; } }; function a(p: any) { return H.h(p); } function b(p: any) { return H.h(H.g(p)); } a(o); return b(c);",
        "import { both } from './b'; class Box { v: any = 1; } both(0, new Box()).v = 'x'; console.log(1);",
    );
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
}

#[test]
pub(super) fn another_units_function_returns_each_caller_its_own_argument() {
    // DECISIONS 44 (b) through another unit's function: `pass` (unit `p`)
    // returns its argument and touches what it reaches under `k` (its
    // stand-in's touch node takes nothing back). Its stand-in is one body
    // for every call: its parameter's marker is the caller's argument at
    // each call's result, so `id`'s result is `id`'s argument alone.
    let p = "export function pass(x: any) { const s = '' + x.k; return x; }";
    let c = format!("{JOIN}import {{ pass }} from './p';\nfunction h(q: any) {{ S.last = 0; return pass(q); }}\nexport function id(q: any) {{ S.last = 1; return h(q); }}\nexport function id2(q: any) {{ S.last = 2; return h(q); }}");
    let m = "import { id, id2 } from './c'; class Box { v: any = 1; } class Crate { v: any = 1; } const o = new Box(); const c = new Crate(); const r = id(o); id2(c); r.v = 'x'; console.log(o.v, c.v);";
    let pf = units_of(&[("p", p), ("c", c.as_str()), ("m", m)]);
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
    assert!(
        field(tmpl(&pf, "Crate"), "v").readonly,
        "{}",
        pf.dump_text()
    );
}

#[test]
pub(super) fn a_result_an_argument_reaches_is_the_callers_reach() {
    // An argument marker's keyed reach reads as what its members reach: `up`
    // (its own unit, a closed function) returns `x.parent`, which its
    // argument marker holds as a keyed reach marker of itself. Read as
    // nothing, `up`'s contract would say it returns its argument alone and
    // the store through `top`'s result would miss the `Parent`.
    let b = "function up(x: any) { x = x.parent; return x; }\nexport function top(x: any) { return up(x); }\nexport function top2(x: any) { return up(x); }";
    let m = "import { top, top2 } from './b'; class Parent { v: any = 1; } class Child { v: any = 1; parent: any = new Parent(); } const c = new Child(); top2(0); top(c).v = 'x'; console.log(c.v);";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(
        !field(tmpl(&pf, "Parent"), "v").readonly,
        "{}",
        pf.dump_text()
    );
}

#[test]
pub(super) fn an_aborted_unit_checks_no_contracts() {
    // 113-CWL3: past the bound a unit claims nothing (sound), so checking
    // its contracts only spends time: a walk over every argument marker's
    // members per result token. checker.ts#u18 spent 93 s there after its
    // 25 s solve hit the bound.
    let (b, m) = (
        "export function get(p: any) { const q = { v: p.k }; return q; }",
        "import { get } from './b'; console.log(get({ k: 1 }).v);",
    );
    let of = |pf: &ProgramFacts| {
        pf.units
            .iter()
            .find(|(p, _)| p.starts_with("b.ts"))
            .map(|(_, s)| (s.aborted, s.checks))
            .expect("the unit of get")
    };
    let free = of(&units_of(&[("b", b), ("m", m)]));
    assert!(
        !free.0 && free.1 >= 1,
        "unbounded, the unit checks its contracts: {free:?}"
    );
    let cfg = FactsConfig {
        max_work: Some(10),
        ..Default::default()
    };
    let bounded = of(&units_with(&[("b", b), ("m", m)], &cfg));
    assert!(bounded.0, "the bound held: {bounded:?}");
    assert_eq!(bounded.1, 0, "an aborted unit checked its contracts");
}

#[test]
pub(super) fn what_other_units_let_escape_of_reached_values_is_one_closure() {
    // DECISIONS 45: an escape is transitive, so the unit's one escape node
    // takes the arguments' fields. A closure per contract parameter holds
    // the whole list once per callee: the unit grows with the square.
    let (e8, e16, e32) = (
        reach_closure_entries(8),
        reach_closure_entries(16),
        reach_closure_entries(32),
    );
    // Linear: doubling the callees again adds twice as much (a closure per
    // parameter: 258 418 738 became 376 784 1984, d2 = 2.9 d1).
    let (d1, d2) = (e16 - e8, e32 - e16);
    assert!(
        4 * d2 <= 9 * d1,
        "entries {e8} {e16} {e32}: quadratic in the callees"
    );
}

#[test]
pub(super) fn another_units_touch_of_a_deep_reached_value_runs_its_protocol() {
    // DECISIONS 45: a touch is not transitive; the unit's touch node closes
    // over the fields of what it holds. `show` converts `p.a.b`, two fields
    // down: the literal `w`'s `valueOf` runs on `w` and writes `n`. (A class
    // method would not do: its own unit writes the field whether or not
    // anyone calls it.)
    let b = "export function show(p: any) { return '' + p.a.b; }";
    let m = "import { show } from './b'; function main() { const w = { n: 1, valueOf() { this.n = 'x'; return 0; } }; const o = { a: { b: w } }; show(o); console.log(w.n); } main(); main();";
    let pf = units_of(&[("b", b), ("m", m)]);
    let w = pf
        .templates
        .iter()
        .find(|t| t.birth_keys == ["n", "valueOf"])
        .expect("w literal");
    assert!(!field(w, "n").readonly, "{}", pf.dump_text());
}

#[test]
pub(super) fn an_argument_contract_ascends_so_the_units_reach_a_fixpoint() {
    // D16 (express, ee-first): `callback` reads `arguments` (its argument
    // contract is ⊤) and is passed to closed `listener` as `done`. With ⊤
    // from `first`, `listener.done` is unknown, so no call reaches
    // `callback` and its contract drops to none; then `callback` is closed,
    // its function contract is published, `listener.done` holds it, the
    // escaped `onevent` calls it: ⊤ again. Rebuilt from the latest solves
    // only, the contract cycled with period 3 and the driver never ended.
    // Joined with last round's (it only grows), the run ends: rounds 1-3
    // pass the contracts on, round 4 changes nothing. Bounded by the test,
    // so a driver that cycles fails here instead of hanging the suite.
    // DECISIONS 140: `onevent`'s `done(err, ee, event, args)` is a call
    // through `listener`'s parameter, published as one more contract input
    // that ascends (D16: every input joins with last round's): it is passed
    // on one round later, so round 5 is the one that changes nothing.
    // DECISIONS 147: `listener`'s result `onevent` is a function handed to
    // the caller's unit, now a contract function (its markers, its own
    // contract) rather than a sunk result: one more input passed on a round
    // later, so round 6 is the one that changes nothing.
    let m = r#"import first from "./ee";
        const t = first([[process, "exit"]], function (err: any) { console.log(err); });
        t.cancel();"#;
    let pf = bounded(60, move || units_of(&[("ee", EE_FIRST), ("m", m)]))
        .expect("D16: the per-unit fixpoint did not end in 60 s");
    // DEC223 keeps callable identity described while its entry stays unknown,
    // so the same ascending join now settles in three rounds. Bound unchanged.
    assert_eq!(pf.rounds, 3, "rounds {:?}", pf.round_work);
}

#[test]
pub(super) fn a_store_into_a_class_field_is_a_store_effect() {
    // CW2 C (binarytrees_arena per unit): `make` (its own unit) does
    // `a.size++` and the top level `s.size = 0` on an `Arena` of the class
    // unit. `size` is a definite data field of every Arena from its birth,
    // so the store adds no key and opens nothing: a store effect (decision
    // 17), as for a literal layout. The class is not broken, and `.left` /
    // `.right` hold on its layout in every unit.
    let src = r#"
        class Arena {
          left: Int32Array; right: Int32Array; size: number = 0;
          constructor(n: number) { this.left = new Int32Array(n); this.right = new Int32Array(n); }
        }
        function make(a: Arena, d: number): number {
          let l = -1; let r = -1;
          if (d > 0) { l = make(a, d - 1); r = make(a, d - 1); }
          const i = a.size++; a.left[i] = l; a.right[i] = r; return i;
        }
        function check(a: Arena, i: number): number {
          const l = a.left[i]; if (l < 0) return 1; return 1 + check(a, l) + check(a, a.right[i]);
        }
        const s = new Arena(64);
        const t = make(s, 3);
        console.log(check(s, t));
        s.size = 0;
        console.log(check(s, make(s, 2)));
        "#;
    let pf = units(src);
    assert!(pf.units.len() >= 3, "{}", pf.dump_text());
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    for k in ["left", "right"] {
        assert!(
            !verdicts(&pf, k).is_empty() && proven(&pf, k),
            "{k}\n{}",
            pf.dump_text()
        );
    }
    // The owner performs the effect: `size` has writers after birth.
    assert!(
        !field(tmpl(&pf, "Arena"), "size").readonly,
        "{}",
        pf.dump_text()
    );
    assert!(
        field(tmpl(&pf, "Arena"), "left").readonly,
        "{}",
        pf.dump_text()
    );
    // Soundness: the owner reads what the other unit stored. `get` (a method
    // of the class unit) reads `this.v.key`; the constructor put `{ key: 1 }`
    // there, but `put` stored another layout: `.key` must not hold on it.
    let src = r#"
        class Box { v: any; constructor() { this.v = { key: 1 }; } get() { return this.v.key; } }
        function put(b: Box, x: any) { b.v = { other: x, key: 's' }; }
        function main(x: any) { const b = new Box(); put(b, x); console.log(b.get()); }
        main(1); main(2);
        "#;
    let pf = units(src);
    // Rewritten for NSS (a store effect carries the stored value's
    // contract): `Box` is not broken; `put`'s literal may be.
    assert!(
        !pf.broken.iter().any(|d| d == "t.ts:Box"),
        "{:?}",
        pf.broken
    );
    assert!(
        !verdicts(&pf, "key").is_empty() && !proven(&pf, "key"),
        "{}",
        pf.dump_text()
    );
    // Soundness: a store of a key the class does not hold from birth adds a
    // key (the layout changes): a census report, the class is broken.
    let src = r#"
        class C { x: number = 1; }
        function grow(c: C) { (c as any).y = 2; }
        function main() { const c = new C(); grow(c); console.log(c.x); }
        main(); main();
        "#;
    let pf = units(src);
    assert!(
        pf.broken.iter().any(|d| d.ends_with(":C")),
        "{:?}\n{}",
        pf.broken,
        pf.dump_text()
    );
}

#[test]
pub(super) fn a_closed_class_constructor_holds_what_its_news_pass() {
    // Design §18.13 (binarytrees_arena per unit): `Arena` is no export, and
    // every naming of it is a `new`, in units other than its own. Its
    // constructor's `n` holds the numbers those `new`s pass, not ⊤ (which
    // might be a SharedArrayBuffer): `left`/`right` are typed arrays over
    // their own buffers, `ta_stable` as in the whole-program solve.
    let src = r#"
        class Arena {
          left: Int32Array; right: Int32Array; size: number = 0;
          constructor(n: number) { this.left = new Int32Array(n); this.right = new Int32Array(n); }
        }
        function make(a: Arena, d: number): number {
          let l = -1; let r = -1;
          if (d > 0) { l = make(a, d - 1); r = make(a, d - 1); }
          const i = a.size++; a.left[i] = l; a.right[i] = r; return i;
        }
        function check(a: Arena, i: number): number {
          const l = a.left[i]; if (l < 0) return 1; return 1 + check(a, l) + check(a, a.right[i]);
        }
        function fresh(k: number): Arena { return new Arena(1 << k); }
        const s = fresh(6);
        console.log(check(s, make(s, 3)));
        "#;
    let pf = units(src);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    for k in ["left", "right"] {
        assert!(
            field(tmpl(&pf, "Arena"), k).ta_stable,
            "{k}\n{}",
            pf.dump_text()
        );
    }
    // Soundness: another unit constructs it through an instance's
    // `constructor` with a shared buffer. The stand-in class object is the
    // instance's `constructor` there, so the construction reaches the
    // stand-in constructor's parameters and the owner sees the buffer.
    let src = r#"
        class Arena {
          left: Int32Array;
          constructor(n: any) { this.left = new Int32Array(n); }
        }
        function fresh(k: number): Arena { return new Arena(k); }
        function twin(a: Arena): number { const C: any = a.constructor; const t = new C(new SharedArrayBuffer(64)); return t.left[0]; }
        function main() { const s = fresh(8); console.log(s.left[0] + twin(s)); }
        main();
        "#;
    let pf = units(src);
    // Not vacuous: the class holds its contract (nothing reports it).
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    assert!(
        !field(tmpl(&pf, "Arena"), "left").ta_stable,
        "{}",
        pf.dump_text()
    );
    // Across modules: `b` gives instances out (no export of the class); `m`
    // constructs one through `constructor` with a shared buffer. Today `m`'s
    // census already reports the class there (its stand-in escapes), so the
    // constructor's parameters are ⊤ twice over; `ctor_args_out` would carry
    // the buffer if it did not.
    let b = r#"
        class Arena {
          left: Int32Array;
          constructor(n: any) { this.left = new Int32Array(n); }
        }
        export function fresh(k: number): Arena { return new Arena(k); }
        export function first(a: Arena): number { return a.left[0]; }
        "#;
    let m = r#"
        import { fresh, first } from './b';
        function main() {
          const s = fresh(8);
          const C: any = s.constructor;
          const t = new C(new SharedArrayBuffer(64));
          console.log(first(s) + first(t));
        }
        main();
        "#;
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(
        !field(tmpl(&pf, "Arena"), "left").ta_stable,
        "{}",
        pf.dump_text()
    );
}

#[test]
pub(super) fn a_published_method_has_no_receiver_before_its_readers_publish_one() {
    // DECISIONS 123 setting: `A::run` is published; its receivers are what
    // its readers publish (107), none before they have (round 1). It stores
    // another unit's instance into its receiver: per unit `B` holds its
    // contract and `b.v` is proven, as in the whole program (parity
    // witness; arm A123 does not fail here, this shape's round-1 store does
    // not reach `B`'s stand-in: A123's catching test is the cjs_t3 one).
    let mods = [
        ("c", "export class B { v: number = 1; }\n"),
        (
            "b",
            "import { B } from './c';\nexport class A { keep: any = null; run(): number { const b = new B(); this.keep = b; return b.v; } }\n",
        ),
        ("m", "import { A } from './b';\nconst a = new A();\nconsole.log(a.run());\n"),
    ];
    let w = whole_of(&mods);
    let u = units_of(&mods);
    assert!(proven(&w, "v"), "{}", w.dump_text());
    assert!(
        !u.broken.iter().any(|d| d.ends_with(":B")),
        "{:?}",
        u.broken
    );
    assert!(proven(&u, "v"), "{:?}\n{}", u.broken, u.dump_text());
}

#[test]
pub(super) fn a_reader_that_exposes_a_typed_array_field_breaks_ta_stable() {
    // DECISIONS 126: Arena's typed-array fields leave described (the owner's
    // typed arrays), so the reader's uses count: `peek` reads `a.left.buffer`
    // (B1, the buffer can be detached or wrapped), its census reports the
    // field and the owner drops `ta_stable` for `left`, as the whole-program
    // solve does. `right`, which no reader exposes, stays stable (the test is
    // not vacuous: the description holds where nothing breaks it).
    let src = r#"
        class Arena {
          left: Int32Array; right: Int32Array;
          constructor(n: number) { this.left = new Int32Array(n); this.right = new Int32Array(n); }
        }
        function fresh(k: number): Arena { return new Arena(1 << k); }
        function peek(a: Arena): number { const b: any = a.left.buffer; return b.byteLength + a.right[0]; }
        const s = fresh(6);
        console.log(peek(s));
        "#;
    let w = whole_of(&[("t", src)]);
    let u = units(src);
    for pf in [&w, &u] {
        assert!(
            !field(tmpl(pf, "Arena"), "left").ta_stable,
            "{}",
            pf.dump_text()
        );
        assert!(
            field(tmpl(pf, "Arena"), "right").ta_stable,
            "{}",
            pf.dump_text()
        );
    }
    assert!(
        u.broken.iter().any(|d| d.starts_with("ta left of ")),
        "{:?}",
        u.broken
    );
    // A value a reader stores under a named key of the typed array is one
    // the owner's solve does not see: reported too (the field is sunk), so
    // the owner's flow through `left.tag` stays sound.
    let src = r#"
        class Arena {
          left: Int32Array; right: Int32Array;
          constructor(n: number) { this.left = new Int32Array(n); this.right = new Int32Array(n); }
          tag(): any { return (this.left as any).tag; }
        }
        function fresh(k: number): Arena { return new Arena(1 << k); }
        function mark(a: Arena): number { (a.left as any).tag = "x"; return a.right[0]; }
        const s = fresh(6);
        console.log(mark(s), s.tag());
        "#;
    let u = units(src);
    assert!(
        u.broken.iter().any(|d| d.starts_with("ta left of ")),
        "{:?}\n{}",
        u.broken,
        u.dump_text()
    );
    // ABM3: `mark` stores into ⊤ under `tag` in its first solve, before
    // `Arena`'s contract describes `a.left`; that write stays published (the
    // driver's shared writes only grow). Under the static rule (CWN 10-27) a
    // computed-key protocol touch of `right` in the reader finds
    // `Object.prototype`'s written `tag`: an unknown callee with `right` as
    // its receiver, so `right` is reported too. Imprecise, not unsound.
    assert!(
        !field(tmpl(&u, "Arena"), "right").ta_stable,
        "{}",
        u.dump_text()
    );
}

#[test]
pub(super) fn a_store_effect_carries_the_kinds_it_stores() {
    // DECISIONS 73/74 (binarytrees_arena per unit): `make` does `a.size++`
    // on another unit's Arena. The store effect carries the stored kinds and
    // the reader reads the field's published kinds, so `size` stays a number
    // in its owner, as in the whole-program solve (not ⊤).
    let src = r#"
        class Arena {
          left: Int32Array; right: Int32Array; size: number = 0;
          constructor(n: number) { this.left = new Int32Array(n); this.right = new Int32Array(n); }
        }
        function make(a: Arena, d: number): number {
          let l = -1; let r = -1;
          if (d > 0) { l = make(a, d - 1); r = make(a, d - 1); }
          const i = a.size++; a.left[i] = l; a.right[i] = r; return i;
        }
        function check(a: Arena, i: number): number {
          const l = a.left[i]; if (l < 0) return 1; return 1 + check(a, l) + check(a, a.right[i]);
        }
        function fresh(k: number): Arena { return new Arena(1 << k); }
        const s = fresh(6);
        console.log(check(s, make(s, 3)));
        "#;
    let pf = units(src);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    let k = &field(tmpl(&pf, "Arena"), "size").kind;
    assert!(
        !k.top && k.objs.is_empty() && k.prims == crate::model::prim::NUM,
        "{k:?}\n{}",
        pf.dump_text()
    );
    // Soundness: another unit stores a string into it; the owner's field
    // holds strings too, and a reader adding to it may make strings.
    let src = r#"
        class Arena {
          size: number = 0;
          constructor() {}
        }
        function bump(a: Arena): void { (a as any).size = "x"; }
        function grow(a: Arena): number { const i = a.size++; return i; }
        function fresh(): Arena { return new Arena(); }
        const s = fresh();
        bump(s);
        console.log(grow(s));
        "#;
    let pf = units(src);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    let k = &field(tmpl(&pf, "Arena"), "size").kind;
    assert!(
        !k.top && k.prims & crate::model::prim::STR != 0,
        "{k:?}\n{}",
        pf.dump_text()
    );
    // An object stored by another unit widens the field as before.
    let src = r#"
        class Arena {
          size: number = 0;
          constructor() {}
        }
        function bump(a: Arena): void { (a as any).size = {}; }
        function fresh(): Arena { return new Arena(); }
        const s = fresh();
        bump(s);
        console.log(s.size);
        "#;
    let pf = units(src);
    let k = &field(tmpl(&pf, "Arena"), "size").kind;
    assert!(k.top || !k.objs.is_empty(), "{k:?}\n{}", pf.dump_text());
}

#[test]
pub(super) fn an_escaped_arguments_keyed_reach_reads_as_unknown() {
    // DECISIONS 42 with design §18.4: `f` lets its argument escape, so what
    // it returns from `p.inner` may be anything unknown code stored there:
    // the caller's `r.v` has ⊤ among its receivers.
    // Two orders: the escape is seen before the read of `p.inner`, or (through
    // `keep`, another unit's function) after it.
    for b in [
        "export function f(p: any) { const x = p.inner; (globalThis as any).g = p; return x; }",
        "export function keep(q: any) { (globalThis as any).g = q; }\nexport function f(p: any) { const x = p.inner; keep(p); return x; }",
    ] {
        let m = "import { f } from './b'; function main() { const o = { inner: { v: 1 } }; const r = f(o); console.log(r.v); } main(); main();";
        let pf = units_of(&[("b", b), ("m", m)]);
        let s: Vec<_> = pf.sites.iter().filter(|s| s.key == "v").collect();
        assert!(!s.is_empty() && s.iter().all(|s| s.top), "{}", pf.dump_text());
    }
}

#[test]
pub(super) fn an_escaped_keyed_reach_in_a_result_is_another_units_value() {
    // DECISIONS 42 with design §18.4: `put` keeps what its argument reaches
    // under `inner` in `held`, and that value escapes: (1) with the argument
    // itself, (2) under the key, through shared helpers (argument markers).
    // `make`'s result is then another unit's value (⊤), not `make`'s own
    // parameter 0's reach coming back.
    for b in [
        r#"
        let held: any = 0;
        export function put(p: any) { held = p.inner; (globalThis as any).g = p; }
        export function make(x: any) { if (x) { return held; } return 0; }
        "#,
        r#"
        let held: any = 0;
        function h(s: any) { (globalThis as any).g = s.inner; }
        function mid(q: any) { h(q); }
        export function put(p: any) { held = p.inner; mid(p); }
        export function put2(p: any) { mid(p); }
        export function make(x: any) { if (x) { return held; } return 0; }
        "#,
    ] {
        let m = "import { put, make } from './b'; put({ inner: { a: 1 } }); function main() { const o = { w: { q: 1 } }; const r = make(o); console.log(r.q); } main(); main();";
        let pf = units_of(&[("b", b), ("m", m)]);
        let s: Vec<_> = pf.sites.iter().filter(|s| s.key == "q").collect();
        assert!(
            !s.is_empty() && s.iter().all(|s| s.top),
            "{}",
            pf.dump_text()
        );
    }
}

#[test]
pub(super) fn a_closed_parameter_binding_holds_its_contract_classes_only() {
    // DECISIONS 76 (binarytrees_arena per unit): `make`'s and `check`'s `a`
    // hold the contract's `Arena` plus the parameter markers beside it
    // (DECISIONS 74, 75). A marker beside its values adds no value of its
    // own, so the binding is not `other`: codegen gets an unchecked receiver
    // proof and reads `a.left` / `a.right` / `a.size` at their slots.
    let src = r#"
        class Arena {
          left: Int32Array; right: Int32Array; size: number = 0;
          constructor(n: number) { this.left = new Int32Array(n); this.right = new Int32Array(n); }
        }
        function make(a: Arena, d: number): number {
          let l = -1; let r = -1;
          if (d > 0) { l = make(a, d - 1); r = make(a, d - 1); }
          const i = a.size++; a.left[i] = l; a.right[i] = r; return i;
        }
        function check(a: Arena, i: number): number {
          const l = a.left[i]; if (l < 0) return 1; return 1 + check(a, l) + check(a, a.right[i]);
        }
        const s = new Arena(64);
        console.log(check(s, make(s, 3)));
        "#;
    let pf = units(src);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    let held: Vec<_> = pf.bindings[0]
        .locals
        .iter()
        .filter(|(_, b)| !b.class_ids.is_empty())
        .collect();
    // Not vacuous: `s` and both parameters hold the class.
    assert!(held.len() >= 3, "{held:?}");
    for (id, b) in held {
        assert!(!b.other, "local {id}: {b:?}\n{}", pf.dump_text());
        // DECISIONS 77: every call passes `a`, so it is never undefined
        // (no nullish receiver test, `a.size++` updates the slot).
        assert!(
            b.prims & crate::model::prim::UNDEF == 0,
            "local {id}: {b:?}\n{}",
            pf.dump_text()
        );
    }
    // Negative: another unit's call passes fewer arguments; the missing
    // parameter holds undefined (the solver's rule at the stand-in).
    let src = r#"
        class Arena { size: number = 0; }
        function f(a: Arena, d: number): number { return a.size + d; }
        const s = new Arena();
        console.log(f(s));
        "#;
    let pf = units(src);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    assert!(
        pf.bindings[0]
            .locals
            .iter()
            .any(|(_, b)| b.class_ids.is_empty() && b.prims & crate::model::prim::UNDEF != 0),
        "{:?}\n{}",
        pf.bindings[0].locals,
        pf.dump_text()
    );
}

#[test]
pub(super) fn a_returned_literal_carries_its_field_functions() {
    // DECISIONS 78 (tssrc completions.ts:1376 `uniqueNames.has`, :1503
    // `printer.printAndFormatNode`): a contract function returns a literal of
    // its unit whose fields hold its functions. The result is the same layout
    // contract as a published literal (`note_field_fns`): the caller calls
    // `has` through its contract, so the literal does not escape as the
    // receiver of an unknown call and its layout is not broken.
    let src = r#"
        function mk(m: Set<string>) {
          return { has: (k: string) => m.has(k), add: (k: string) => { m.add(k); } };
        }
        function use(x: string) { const r = mk(new Set()); r.add(x); return r.has(x); }
        console.log(use("a"), use("b"));
        "#;
    let pf = units(src);
    assert!(pf.units.len() >= 2, "{}", pf.dump_text());
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    assert!(
        proven(&pf, "has") && proven(&pf, "add"),
        "{}",
        pf.dump_text()
    );
    // A field function returning its own literal (a builder): the result
    // layout's field functions name the same layout again, and the reader
    // makes its stand-in once (tssrc overflowed the stack without that).
    let src = r#"
        function mk(m: Set<string>) {
          const o = { add: (k: string) => { m.add(k); return o; }, has: (k: string) => m.has(k) };
          return o;
        }
        function use(x: string) { return mk(new Set()).add(x).add("y").has(x); }
        console.log(use("a"), use("b"));
        "#;
    let pf = units(src);
    assert!(pf.units.len() >= 2, "{}", pf.dump_text());
    assert!(proven(&pf, "has"), "{}", pf.dump_text());
}

#[test]
pub(super) fn a_forwarded_result_layout_leaves_no_fields() {
    // DECISIONS 79 (tssrc tsbuildPublic.ts: `createSolutionBuilder` returns
    // `createSolutionBuilderWorker(..)`, a literal whose closure returns the
    // `info` literal of a third unit). The forwarding unit's result is the
    // worker's layout, a stand-in forwarded with its owner's contract: its
    // fields do not leave there, so `info`'s layout is not broken and the
    // reader's `i.project` is proven.
    let src = r#"
        function next(n: number) { return { project: "p" + n, status: n }; }
        function worker(n: number) {
          return { getNext: (k: number) => next(n + k), close: () => n };
        }
        function create(n: number) { return worker(n); }
        function use(n: number) { const i = create(n).getNext(1); return i.project + i.status; }
        console.log(use(1), use(2));
        "#;
    let pf = units(src);
    assert!(pf.units.len() >= 4, "{}", pf.dump_text());
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    assert!(
        proven(&pf, "project") && proven(&pf, "status"),
        "{}",
        pf.dump_text()
    );
}

#[test]
pub(super) fn a_store_effect_ascends_so_the_units_reach_a_fixpoint() {
    // D16, the store channel (decision 17): `b` calls the function field
    // `k` of `a`'s literal and stores into the result, `obj` itself: a
    // store effect on `obj.k`. `a` performs it (an unknown value), so `k`
    // no longer holds only a function and `b` reads it as unknown: no
    // store. Rebuilt from the latest solves only, the effect would drop,
    // `k` would hold the function again, and the driver would cycle.
    // Joined with last round's, the effect stays: two rounds (three since
    // ABM3, below).
    let a = "function k() { return o; } const o = { k: k }; export const obj = o;";
    let b = "import { obj } from './a'; const f = (obj as any).k; const y = f(); y.k = 1;";
    let pf = bounded(60, move || units_of(&[("a", a), ("b", b)]))
        .expect("D16: the per-unit fixpoint did not end in 60 s");
    // ABM3 (static rule, CWN 10-27): `b`'s store into ⊤ (`y.k = 1`) under
    // `k` writes `k` of every builtin prototype; `a`'s lookups of `k` that
    // end at `Object.prototype` read it, so `a` solved once more: three.
    // DECISIONS 234: `k` is an own definite key of `o`, which shadows the
    // builtin prototypes: `a`'s read is the field's, two rounds again.
    assert_eq!(pf.rounds, 2, "rounds {:?}", pf.round_work);
    // `a` keeps performing the store: `k` is written after construction.
    let o = pf
        .templates
        .iter()
        .find(|t| t.birth_keys == ["k"])
        .expect("the literal");
    assert!(!field(o, "k").readonly, "{}", pf.dump_text());
}

#[test]
fn a_closed_callback_in_the_callers_scc_is_published() {
    let src = r#"
        function invoke(cb: any) { return cb(0); }
        function first(x: any): any { if (x) return root(x - 1); return 1; }
        function second(x: any): any { if (x) return root(x - 1); return 2; }
        function root(x: any): any {
            if (x) return first(x - 1) + second(x - 1);
            return invoke(first) + invoke(second);
        }
        console.log(root(1));
    "#;
    let pf = units(src);
    let calls: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| {
            s.is_call
                && s.targets
                    .iter()
                    .any(|t| t.contains(":first") || t.contains(":second"))
        })
        .collect();
    assert!(!calls.is_empty());
    let callback_calls: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.is_call && s.byte_offset == src.find("cb(0)").unwrap() as u32 + 1)
        .collect();
    assert!(!callback_calls.is_empty(), "{}", pf.dump_text());
    assert!(
        callback_calls
            .iter()
            .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set)),
        "{}",
        pf.dump_text()
    );
    assert!(
        callback_calls.iter().all(|s| s.targets.len() == 2),
        "{}",
        pf.dump_text()
    );
    // A later unknown holder may invoke the callee with any callback.
    let sabotage = src.replace(
        "console.log(root(1));",
        "console.log(root(1)); (globalThis as any).leak = invoke;",
    );
    let refused = units(&sabotage);
    let observed: Vec<_> = refused
        .sites
        .iter()
        .filter(|s| s.is_call && s.byte_offset == src.find("cb(0)").unwrap() as u32 + 1)
        .collect();
    assert!(!observed.is_empty());
    assert!(
        observed.iter().all(|s| matches!(s.verdict, Verdict::No(_))),
        "{}",
        refused.dump_text()
    );
}
