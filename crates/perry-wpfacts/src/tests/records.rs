//! VR-1 (DESIGN-p4l4 §2): the value-record verdicts, one rule at a time,
//! each with its negative control.
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

fn rejected(src: &str) -> String {
    verdict(&format!("{MK}{src}"), &["a", "b"], on()).expect_err("rejected")
}

fn admitted(src: &str) {
    if let Err(e) = verdict(&format!("{MK}{src}"), &["a", "b"], on()) {
        let pf = facts_with(&format!("{MK}{src}"), on());
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
    let src = format!("{MK}console.log(use(mk(1)));\n");
    assert_eq!(
        verdict(&src, &["a", "b"], FactsConfig::default()),
        Err("Off".into())
    );
    let pf = facts_with(&src, FactsConfig::default());
    assert!(!pf.dump_text().contains("# record "));
}

#[test]
fn a_value_passed_read_and_logged_is_admitted() {
    admitted("let s = 0;\nfor (let i = 0; i < 3; i++) { const r = mk(i); s += use(r); }\nconsole.log(s, mk(5), `${mk(2).a}`, \"%o\", mk(3));\n");
    let pf = facts_with(&format!("{MK}console.log(use(mk(1)));\n"), on());
    let text = pf.dump_text();
    assert!(text.contains("ADMITTED fields=a:"), "{text}");
    assert!(
        text.contains("# ledger: records admitted 1 rejected 0"),
        "{text}"
    );
}

#[test]
fn identity_rejects() {
    assert_eq!(
        rejected("const x = mk(1), y = mk(2);\nconsole.log(x === y);\n"),
        "R4Identity(===)"
    );
    assert_eq!(
        rejected("const x = mk(1), y = mk(2);\nconsole.log(x == y);\n"),
        "R4Identity(==)"
    );
    assert_eq!(
        rejected("const x = mk(1);\nconsole.log(Object.is(x, x));\n"),
        "R4Identity(Object.is)"
    );
    assert_eq!(
        rejected("const x = mk(1), y = mk(2);\nswitch (x) { case y: console.log(1); }\n"),
        "R4Identity(switch)"
    );
    // Negative control: a presence test is no identity.
    admitted("const x: { a: number; b: boolean } | undefined = Math.random() > 2 ? undefined : mk(1);\nconsole.log(x === undefined, x !== undefined ? use(x) : 0);\n");
}

#[test]
fn a_write_after_birth_rejects() {
    assert_eq!(
        rejected("const x = mk(1);\n(x as any).a = 2;\nconsole.log(use(x));\n"),
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
        rejected("try { throw mk(1); } catch (e) { console.log(e); }\n"),
        "R5Throw"
    );
}

#[test]
fn an_untyped_call_rejects() {
    let r = rejected("function use2(r: { a: number; b: boolean }) { return r.a * 2; }\nconst hs = [use, use2];\nlet s = 0;\nfor (let i = 0; i < 2; i++) s += hs[i](mk(i));\nconsole.log(s);\n");
    assert!(
        r.starts_with("R5UntypedCall(") || r.starts_with("R5StoreHeap("),
        "{r}"
    );
}

#[test]
fn a_retaining_native_rejects() {
    let r =
        rejected("const m = new Map<number, unknown>();\nm.set(1, mk(1));\nconsole.log(m.size);\n");
    assert!(
        r.starts_with("R5StoreHeap(") || r.starts_with("R5Native("),
        "{r}"
    );
}

#[test]
fn a_record_array_waits_for_the_intrinsics() {
    let r = rejected("const xs: { a: number; b: boolean }[] = [];\nconst u = mk(1);\nxs.push(u);\nconsole.log(xs.length);\n");
    assert_eq!(r, "R6Op(push)");
    let pf = facts_with(&format!("{MK}const xs: {{ a: number; b: boolean }}[] = [];\nconst u = mk(1);\nxs.push(u);\nconsole.log(xs.length);\n"), on());
    assert!(
        pf.dump_text().contains("# record-array "),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn json_without_a_function_replacer_is_a_boundary() {
    admitted("console.log(JSON.stringify(mk(1)), JSON.stringify(mk(2), null, 2), JSON.stringify(mk(3), [\"a\"]));\n");
    assert_eq!(
        rejected("console.log(JSON.stringify(mk(1), (k, v) => v));\n"),
        "R5Protocol(JsonReplacer)"
    );
}

#[test]
fn a_boundary_wrapper_is_admitted() {
    admitted("const u = mk(1);\nconsole.log({ u });\nconsole.log(\"pv\", { ply: 3, u });\nconsole.log([u, 4]);\n");
    // Negative control: a wrapper the program reads is a heap object.
    let r = rejected("const u = mk(1);\nconst w = { u };\nconsole.log(w.u.a);\n");
    assert!(r.starts_with("R5StoreHeap("), "{r}");
}

#[test]
fn a_logging_helper_with_a_boxed_parameter_is_admitted() {
    // Owner ruling 2026-11-10 (§4.4): the record is materialized at the call.
    admitted("function show(x: unknown) { console.log(x); }\nshow(mk(1));\nshow(5);\n");
    admitted("function show(x: unknown) { console.log(x); }\nconst c = Math.random() > 2;\nshow(mk(1));\nshow(c ? 5 : \"s\");\n");
}

#[test]
fn a_retaining_helper_rejects() {
    assert_eq!(
        rejected("const saved: unknown[] = [];\nfunction keep(x: unknown) { saved.push(x); }\nkeep(mk(1));\nkeep(5);\nconsole.log(saved.length);\n"),
        "R5RetainingHelper(keep, retain)"
    );
    assert_eq!(
        rejected("let last: unknown;\nfunction same(x: unknown) { return x === last; }\nconsole.log(same(mk(1)), same(5));\n"),
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
    let src = format!("{MK}const g: any = globalThis;\nconsole.log(use(mk(1)), g[\"ev\" + \"al\"](\"1\"), eval(\"2\"));\n");
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
