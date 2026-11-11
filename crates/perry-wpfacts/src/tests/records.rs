//! VR-1 (DESIGN-p4l4 §2): the value-record verdicts, one rule at a time,
//! each with its negative control. Most sources birth their record in the
//! body that uses it; the record signature slots (VR-3b) have their own
//! tests.
use super::facts_with;
use crate::records::RecordReject;
use crate::FactsConfig;

fn on() -> FactsConfig {
    FactsConfig {
        value_records: true,
        ..FactsConfig::default()
    }
}

/// The verdict of the one object-literal template with these birth keys.
fn verdict(src: &str, keys: &[&str], cfg: FactsConfig) -> Result<String, String> {
    let pf = facts_with(src, cfg);
    let ts: Vec<_> = pf
        .templates
        .iter()
        .filter(|t| matches!(t.kind, "lit" | "litclass") && t.birth_keys == keys)
        .collect();
    assert_eq!(ts.len(), 1, "one template {keys:?}: {:#?}", pf.dump_text());
    match &ts[0].record {
        Ok(l) => Ok(l
            .fields
            .iter()
            .map(|(k, r)| format!("{k}:{}", r.name()))
            .collect::<Vec<_>>()
            .join(",")),
        Err(r) => Err(r.why.to_string()),
    }
}

const MK: &str = "function mk(n: number) { return { a: n, b: n > 1 }; }\nfunction use(r: { a: number; b: boolean }): number { return r.b ? r.a : 0; }\n";

/// One literal site of the template, used inside its body.
const U: &str = "const u = { a: Math.random() > 2 ? 1 : 2, b: Math.random() > 2 };\n";

fn rejected(src: &str) -> String {
    verdict(&format!("{U}{src}"), &["a", "b"], on()).expect_err("rejected")
}

fn admitted(src: &str) {
    if let Err(e) = verdict(&format!("{U}{src}"), &["a", "b"], on()) {
        let pf = facts_with(&format!("{U}{src}"), on());
        let lines: Vec<String> = pf
            .dump_text()
            .lines()
            .filter(|l| l.starts_with("# record"))
            .map(String::from)
            .collect();
        panic!("admitted expected, got {e}:\n{src}\n{lines:#?}");
    }
}

#[test]
fn the_switch_is_off_by_default() {
    let src = format!("{U}console.log(u.a);\n");
    assert_eq!(
        verdict(&src, &["a", "b"], FactsConfig::default()),
        Err("Off".into())
    );
    let pf = facts_with(&src, FactsConfig::default());
    assert!(!pf.dump_text().contains("# record "));
}

#[test]
fn a_value_read_and_logged_is_admitted() {
    admitted("let s = 0;\nfor (let i = 0; i < 3; i++) { s += u.b ? u.a : i; }\nconsole.log(s, u, `${u.a}`, \"%o\", u);\n");
    let pf = facts_with(&format!("{U}console.log(u.a, u);\n"), on());
    let text = pf.dump_text();
    assert!(text.contains("ADMITTED fields=a:"), "{text}");
    assert!(
        text.contains("# ledger: records admitted 1 rejected 0"),
        "{text}"
    );
}

/// The signatures with a Record slot, as `(parameters, result)` flags, and
/// the number of typed call sites.
fn record_slots(src: &str, cfg: FactsConfig) -> (Vec<(Vec<bool>, bool)>, usize) {
    use crate::bindings::Rep;
    let pf = facts_with(src, cfg);
    let is = |r: &Rep| matches!(r, Rep::Record(_));
    let mut sigs: Vec<(Vec<bool>, bool)> = pf
        .bindings
        .iter()
        .flat_map(|b| b.signatures.iter())
        .filter(|(_, s)| s.has_record())
        .map(|(_, s)| (s.params.iter().map(is).collect(), is(&s.ret)))
        .collect();
    sigs.sort();
    let calls = pf.bindings.iter().map(|b| b.typed_calls.len()).sum();
    (sigs, calls)
}

fn no_record_signatures() -> FactsConfig {
    FactsConfig {
        no_record_signatures: true,
        ..on()
    }
}

/// VR-3b (§5.2): a record crosses a typed call in the callee's Record
/// signature slot; a Boxed helper parameter receives a copy (owner ruling
/// §4.4) and is no such slot.
#[test]
fn a_record_crosses_a_typed_call_in_a_record_slot() {
    let src = format!("{MK}console.log(use(mk(1)));\n");
    assert!(verdict(&src, &["a", "b"], on()).is_ok());
    let (sigs, calls) = record_slots(&src, on());
    // `mk` returns the record, `use` takes it; both calls are typed.
    assert_eq!(sigs, vec![(vec![false], true), (vec![true], false)]);
    assert_eq!(calls, 2);
    // Negative control: record signatures off, the slot rejects (VR-3a).
    let r = verdict(&src, &["a", "b"], no_record_signatures())
        .expect_err("a record result of a typed call");
    assert!(
        r.starts_with("R5UntypedCall(") && r.contains("record signatures are off"),
        "{r}"
    );
    assert_eq!(record_slots(&src, no_record_signatures()).0, vec![]);
    let src = format!("{U}function use2(r: {{ a: number; b: boolean }}): number {{ return r.a; }}\nconsole.log(use2(u), use2(u));\n");
    assert!(verdict(&src, &["a", "b"], on()).is_ok());
    let r = verdict(&src, &["a", "b"], no_record_signatures()).expect_err("an argument");
    assert!(
        r.starts_with("R5UntypedCall(") && r.contains("arg(0)"),
        "{r}"
    );
    // Negative control: a Boxed helper parameter is a copy, not a slot.
    let src = format!("{U}function show(x: unknown) {{ console.log(x); }}\nshow(u);\nshow(5);\n");
    assert!(verdict(&src, &["a", "b"], on()).is_ok());
    assert_eq!(record_slots(&src, on()).0, vec![]);
}

/// An optional record parameter is a Record slot with `opt`; a call that
/// is no typed call of one body still rejects (B3), and the negative
/// control `vruntyped` ignores exactly that rule.
#[test]
fn an_untyped_call_of_a_record_rejects() {
    let src = format!("{U}function use3(r: {{ a: number; b: boolean }}): number {{ return r.a; }}\nfunction other(r: {{ a: number; b: boolean }}): number {{ return r.b ? 1 : 2; }}\nconst fs = [use3, other];\nconsole.log(fs[u.a](u), use3(u));\n");
    let r = verdict(&src, &["a", "b"], on()).expect_err("an untyped call");
    assert!(r.starts_with("R5UntypedCall("), "{r}");
    let sabotaged = FactsConfig {
        sabotage: Some("vruntyped".into()),
        ..on()
    };
    assert!(verdict(&src, &["a", "b"], sabotaged).is_ok());
}

#[test]
fn identity_rejects() {
    assert_eq!(rejected("console.log(u === u);\n"), "R4Identity(===)");
    assert_eq!(rejected("console.log(u == u);\n"), "R4Identity(==)");
    assert_eq!(
        rejected("console.log(Object.is(u, u));\n"),
        "R4Identity(Object.is)"
    );
    assert_eq!(
        rejected("switch (u) { case u: console.log(1); }\n"),
        "R4Identity(switch)"
    );
    // A FinalizationRegistry keeps its target and its token by identity.
    const FR: &str = "const fr = new FinalizationRegistry((_h: any) => {});\n";
    assert_eq!(
        rejected(&format!("{FR}fr.register(u, 1);\nconsole.log(u.a);\n")),
        "R4Identity(collection-key)"
    );
    assert_eq!(
        rejected(&format!(
            "{FR}const k = {{}};\nfr.register(k, 1, u);\nconsole.log(u.a);\n"
        )),
        "R4Identity(collection-key)"
    );
    assert_eq!(
        rejected(&format!("{FR}console.log(fr.unregister(u), u.a);\n")),
        "R4Identity(collection-key)"
    );
    // Negative control: a presence test is no identity.
    let src = "const x: { a: number; b: boolean } | undefined = Math.random() > 2 ? undefined : { a: 1, b: true };\nconsole.log(x === undefined, x !== undefined ? x.a : 0);\n";
    assert!(verdict(src, &["a", "b"], on()).is_ok(), "{src}");
}

#[test]
fn a_write_after_birth_rejects() {
    assert_eq!(
        rejected("(u as any).a = 2;\nconsole.log(u.a);\n"),
        "R2Write(a)"
    );
}

#[test]
fn a_pointer_field_rejects() {
    let src = "const o = { a: 1, b: [1] };\nconsole.log(o.a);\n";
    assert!(verdict(src, &["a", "b"], on())
        .unwrap_err()
        .starts_with("R3Pointer(b:"));
}

#[test]
fn a_large_template_rejects() {
    let keys: Vec<String> = (0..17).map(|i| format!("k{i}")).collect();
    let body: Vec<String> = keys.iter().map(|k| format!("{k}: 1")).collect();
    let src = format!("const o = {{ {} }};\nconsole.log(o.k0);\n", body.join(", "));
    let ks: Vec<&str> = keys.iter().map(|s| s.as_str()).collect();
    assert_eq!(verdict(&src, &ks, on()), Err("R0Size(17)".into()));
}

#[test]
fn throw_rejects() {
    assert_eq!(
        rejected("try { throw u; } catch (e) { console.log(e); }\n"),
        "R5Throw"
    );
}

#[test]
fn an_untyped_call_rejects() {
    let r = rejected("function use2(r: { a: number; b: boolean }) { return r.a * 2; }\nfunction use3(r: { a: number; b: boolean }) { return r.a * 3; }\nconst hs = [use2, use3];\nlet s = 0;\nfor (let i = 0; i < 2; i++) s += hs[i](u);\nconsole.log(s);\n");
    assert!(
        r.starts_with("R5UntypedCall(") || r.starts_with("R5StoreHeap("),
        "{r}"
    );
}

#[test]
fn a_retaining_native_rejects() {
    let r = rejected("const m = new Map<number, unknown>();\nm.set(1, u);\nconsole.log(m.size);\n");
    assert!(
        r.starts_with("R5StoreHeap(") || r.starts_with("R5Native("),
        "{r}"
    );
}

const XS: &str = "const xs: { a: number; b: boolean }[] = [];\nxs.push(u);\n";

#[test]
fn a_record_array_is_admitted_for_its_intrinsics() {
    // VR-4b: push, pop, length, an index read and for-of are the
    // intrinsics; the array and its record are admitted together.
    admitted(&format!("{XS}console.log(xs.length);\n"));
    admitted(&format!(
        "{XS}const last = xs.pop();\nconsole.log(last === undefined ? 0 : last.a);\n"
    ));
    admitted(&format!(
        "{XS}const x = xs[0];\nconsole.log(x === undefined ? 0 : x.a);\n"
    ));
    admitted(&format!(
        "{XS}let s = 0;\nfor (const x of xs) s += x.a;\nconsole.log(s);\n"
    ));
    let pf = facts_with(&format!("{U}{XS}console.log(xs.length);\n"), on());
    let text = pf.dump_text();
    assert!(text.contains("# record-array "), "{text}");
    assert!(text.contains(" ADMITTED"), "{text}");
    // The binding and its birth are what codegen lowers as a record array.
    assert!(text.contains("# record_array module=0 local="), "{text}");
    assert!(text.contains("# record_array module=0 birth-local="), "{text}");
    // Off: nothing is exported.
    let pf = facts_with(
        &format!("{U}{XS}console.log(xs.length);\n"),
        FactsConfig::default(),
    );
    assert!(!pf.dump_text().contains("# record_array "));
}

#[test]
fn a_record_array_operation_without_an_intrinsic_rejects() {
    // Each is an operation codegen has no record-array form for.
    let r = |src: &str| rejected(&format!("{XS}{src}"));
    for (src, why) in [
        ("xs[3] = u;\nconsole.log(xs.length);\n", "R6Store"),
        ("xs.length = 0;\nconsole.log(xs.length);\n", "R6Store"),
        ("console.log(xs.slice().length);\n", "R6"),
        ("console.log(xs.map((x) => x.a).length);\n", "R"),
        ("console.log([...xs].length);\n", "R6"),
        ("xs.reverse();\nconsole.log(xs.length);\n", "R6"),
        ("console.log(xs);\n", "R6ArrayHolder"),
    ] {
        let got = r(src);
        assert!(got.starts_with(why), "{src}: {got}");
    }
}

#[test]
fn a_record_array_is_born_empty_at_its_holder() {
    // A literal with elements, and a literal that is not the whole value
    // of its holder, have no record-array birth.
    let r = rejected("const xs: { a: number; b: boolean }[] = [u];\nconsole.log(xs.length);\n");
    assert!(r.starts_with("R6ArrayHolder(birth"), "{r}");
    let r = rejected("function mk(): { a: number; b: boolean }[] { return []; }\nconst xs = mk();\nxs.push(u);\nconsole.log(xs.length);\n");
    assert!(r.starts_with("R6ArrayHolder(birth"), "{r}");
}

#[test]
fn a_record_array_receiver_is_a_binding_or_a_named_read() {
    // An operation on a temporary: codegen types no receiver there.
    let r = rejected(&format!(
        "{XS}const ys: {{ a: number; b: boolean }}[] = [];\nys.push(u);\nconsole.log((Math.random() > 2 ? xs : ys).length);\n"
    ));
    assert!(r.starts_with("R6ArrayHolder(temporary"), "{r}");
    // A named read is typed.
    admitted("class C { xs: { a: number; b: boolean }[] = []; }\nconst o = new C();\no.xs = [];\no.xs.push(u);\nconsole.log(o.xs.length);\n");
    // An object literal's property is no birth holder.
    let r = rejected("const o = { xs: [] as { a: number; b: boolean }[] };\no.xs.push(u);\nconsole.log(o.xs.length);\n");
    assert!(r.starts_with("R6ArrayHolder(birth"), "{r}");
}

#[test]
fn a_binding_of_record_arrays_and_other_arrays_rejects() {
    // One body serves both representations: the join rejects (B1).
    let r = rejected(&format!(
        "{XS}function len(a: unknown[]): number {{ return a.length; }}\nconsole.log(len(xs), len([1, 2]));\n"
    ));
    assert!(r.starts_with("R6Join"), "{r}");
}

#[test]
fn json_without_a_function_replacer_is_a_boundary() {
    admitted(
        "console.log(JSON.stringify(u), JSON.stringify(u, null, 2), JSON.stringify(u, [\"a\"]));\n",
    );
    assert_eq!(
        rejected("console.log(JSON.stringify(u, (k, v) => v));\n"),
        "R5Protocol(JsonReplacer)"
    );
}

#[test]
fn a_boundary_wrapper_is_admitted() {
    admitted("console.log({ u });\nconsole.log(\"pv\", { ply: 3, u });\nconsole.log([u, 4]);\n");
    // Negative control: a wrapper the program reads is a heap object.
    let r = rejected("const w = { u };\nconsole.log(w.u.a);\n");
    assert!(r.starts_with("R5StoreHeap("), "{r}");
}

#[test]
fn a_logging_helper_with_a_boxed_parameter_is_admitted() {
    // Owner ruling 2026-11-10 (§4.4): the record is materialized at the call.
    admitted("function show(x: unknown) { console.log(x); }\nshow(u);\nshow(5);\n");
    admitted("function show(x: unknown) { console.log(x); }\nconst c = Math.random() > 2;\nshow(u);\nshow(c ? 5 : \"s\");\n");
}

#[test]
fn a_retaining_helper_rejects() {
    assert_eq!(
        rejected("const saved: unknown[] = [];\nfunction keep(x: unknown) { saved.push(x); }\nkeep(u);\nkeep(5);\nconsole.log(saved.length);\n"),
        "R5RetainingHelper(keep, retain)"
    );
    assert_eq!(
        rejected("let last: unknown;\nfunction same(x: unknown) { return x === last; }\nconsole.log(same(u), same(5));\n"),
        "R5RetainingHelper(same, identity)"
    );
}

#[test]
fn interpreted_code_admits_nothing() {
    let cfg = FactsConfig {
        value_records: true,
        dynamic_code: Some("eval".into()),
        ..FactsConfig::default()
    };
    let src = format!("{U}const g: any = globalThis;\nconsole.log(u.a, g[\"ev\" + \"al\"](\"1\"), eval(\"2\"));\n");
    let v = verdict(&src, &["a", "b"], cfg);
    assert!(
        matches!(v, Err(ref e) if e.starts_with("R0Interp(") || e.starts_with("R5")),
        "{v:?}"
    );
}

#[test]
fn the_reject_list_prints_every_reason() {
    // The ledger names each reason with its arguments.
    let r = RecordReject::R5RetainingHelper {
        f: "keep".into(),
        why: "retain".into(),
    };
    assert_eq!(r.to_string(), "R5RetainingHelper(keep, retain)");
    assert_eq!(RecordReject::Cascade(7).to_string(), "Cascade(7)");
}

/// VR-2 (§1.1): a binding of only an admitted record's instances is
/// `Rep::Record`; with `undefined` beside them it is `opt`.
#[test]
fn an_admitted_record_binding_is_a_record_rep() {
    use crate::bindings::{Enc, Rep};
    let src = "const r = { a: 1, b: true };\nconst q: { a: number; b: boolean } | undefined = Math.random() > 2 ? undefined : { a: 2, b: false };\nconsole.log(r.a, q === undefined ? 0 : q.a);\n".to_string();
    let reps = super::array_reps::reps_with(&src, on());
    let r = reps["r"];
    assert!(matches!(r, Rep::Record(x) if !x.opt), "{r:?}");
    assert!(
        matches!(reps["q"], Rep::Record(x) if x.opt),
        "{:?}",
        reps["q"]
    );
    // VR-3b: a record's slot is its lanes.
    assert_eq!(r.encoding(), Enc::Record);
    assert!(r.name().starts_with("rec<"));
    // Negative controls: the switch off, and a rejected template.
    let off = super::array_reps::reps_with(&src, FactsConfig::default());
    assert_eq!(off["r"], Rep::Boxed);
    let src = "const r = { a: 1, b: true };\nconsole.log(r === r, r.a);\n".to_string();
    assert_eq!(super::array_reps::reps_with(&src, on())["r"], Rep::Boxed);
}

#[test]
fn a_record_join_is_its_template_only() {
    use crate::bindings::{RecRep, Rep};
    let a = Rep::Record(RecRep {
        tmpl: 3,
        opt: false,
    });
    let ao = Rep::Record(RecRep { tmpl: 3, opt: true });
    let b = Rep::Record(RecRep {
        tmpl: 4,
        opt: false,
    });
    assert_eq!(Rep::join(a, ao), ao);
    assert_eq!(Rep::join(a, a), a);
    assert_eq!(Rep::join(a, b), Rep::Boxed);
    assert_eq!(Rep::join(a, Rep::F64), Rep::Boxed);
    assert_eq!(Rep::join(Rep::Boxed, a), Rep::Boxed);
}

#[test]
fn a_captured_record_rejects() {
    // Closure capture by value is VR-7: a captured local is a heap cell.
    let r = rejected("const f = () => u.a;\nconsole.log(f());\n");
    assert_eq!(r, "R5StoreHeap(closure capture)");
}

#[test]
fn a_mixed_argument_of_a_logging_helper_is_admitted() {
    // Owner ruling 2026-11-10: `show(c ? 5 : u)` materializes at the call.
    admitted("function show(x: unknown) { console.log(x); }\nconst c = Math.random() > 2;\nshow(c ? 5 : u);\nshow(u);\nconsole.log(u.a);\n");
}

#[test]
fn a_retaining_helper_is_named_before_what_it_causes() {
    let r = rejected("const saved: unknown[] = [];\nfunction keep(x: unknown) { saved.push(x); }\nkeep(u);\nkeep(5);\nconsole.log(saved.length, saved[0], saved[1]);\n");
    assert_eq!(r, "R5RetainingHelper(keep, retain)");
}

const CHESS: &str = "interface U { a: number; b: number }\nclass G {\n  private played: U[] = [];\n  reset(): void { this.played = []; }\n  mk(n: number): U { return { a: n, b: 2 }; }\n  un(u: U): number { return u.a; }\n  play(n: number): void { this.played.push(this.mk(n)); }\n  undo(): boolean { const last = this.played.pop(); if (last === undefined) return false; this.un(last); return true; }\n  hist(): number[] { const r: number[] = []; for (const u of this.played) r.push(u.a); return r; }\n  pv(n: number): number { const undos: U[] = []; for (let i = 0; i < n; i++) { undos.push(this.mk(i)); } let s = 0; for (let i = undos.length - 1; i >= 0; i--) { const u = undos[i]; s += this.un(u); } return s; }\n}\nconst g = new G(); g.reset(); g.play(1); console.log(g.undo(), g.hist().length, g.pv(3));\n";

#[test]
fn a_chess_like_undo_stack_is_a_record_array() {
    // A field's stack (birth by its initializer and by a store, push of a
    // call's result, pop, for-of) and a local one (push, length, index).
    let pf = facts_with(CHESS, on());
    let text = pf.dump_text();
    let lines: Vec<&str> = text.lines().filter(|l| l.starts_with("# record")).collect();
    let has = |needle: &str| lines.iter().any(|l| l.contains(needle));
    assert!(has("ADMITTED fields=a:f64,b:f64"), "{lines:#?}");
    assert!(!lines.iter().any(|l| l.starts_with("# record-array") && l.contains("REJECTED")), "{lines:#?}");
    for needle in [
        "birth-field=",
        "birth-store=",
        "birth-local=",
        ".played of=",
        " local=",
    ] {
        assert!(
            lines.iter().any(|l| l.starts_with("# record_array") && l.contains(needle)),
            "{needle}: {lines:#?}"
        );
    }
}
