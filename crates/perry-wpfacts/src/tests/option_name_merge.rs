//! tsc's `setOptionValue` shape (commandLineParser.ts): option names are
//! string constants, `options[d.name]` reads the parsed options (which hold
//! `Object` among their values) and `(result ??= assign({}, options))[d.name]
//! = value` stores into a new options object that a template literal later
//! turns into a primitive.

use super::*;
use crate::model::Key;

fn src(n: usize, tail: &str) -> String {
    let decls: Vec<String> = (0..n).map(|i| format!("{{ name: \"opt{i}\" }}")).collect();
    format!(
        r#"
        declare const w: boolean;
        function assign(t: any, ...args: any[]) {{ for (const arg of args) {{ if (arg === undefined) continue; for (const p in arg) t[p] = arg[p]; }} return t; }}
        function getOwn(m: any, k: string) {{ return m[k]; }}
        const decls: any[] = [{}];
        class Svc {{ size = 3; }}
        const box: any = {{ svc: new Svc() }};
        const options: any = {{ opt0: "x", opt1: box, opt2: Object }};
        let result: any;
        for (const d of decls) {{
            const value = options[d.name];
            const v2 = w ? value : getOwn(value, d.name);
            if (value !== undefined) {{ (result ??= assign({{}}, options))[d.name] = v2; }}
        }}
        {tail}
        console.log(box.svc.size);
        "#,
        decls.join(", ")
    )
}

fn witness(s: &str) -> (bool, String) {
    let pf = facts(s);
    let d = pf.dump_text();
    let ok = pf
        .sites
        .iter()
        .filter(|x| x.key == "size" || x.key == "svc")
        .all(|x| !matches!(x.verdict, Verdict::No(_)));
    (ok, d)
}

#[test]
fn option_name_merge_report() {
    for (tag, n, tail) in [
        ("n3", 3, "console.log(`${result}`);"),
        ("n12", 12, "console.log(`${result}`);"),
        ("n12-noprim", 12, ""),
    ] {
        let (ok, d) = witness(&src(n, tail));
        let esc: Vec<&str> = d.lines().filter(|l| l.contains("escape")).take(8).collect();
        eprintln!("SEED2 {tag}: facts_kept={ok}\n  {}", esc.join("\n  "));
    }
}

#[test]
fn constant_option_names_past_eight_store_named_slots() {
    // Control within the old bound, then tsc's case past it.
    for n in [3, 12, 40] {
        let (ok, d) = witness(&src(n, "console.log(`${result}`);"));
        assert!(ok, "{n} option names: box escaped through ToPrimitive of result\n{d}");
    }
}

#[test]
fn constant_option_names_without_to_primitive_keep_facts() {
    let (ok, d) = witness(&src(12, ""));
    assert!(ok, "{d}");
}

/// An any-String key (`String(...)`) never names a symbol-keyed property:
/// `o[Symbol.toPrimitive]()` never calls what `o[k] = v`
/// wrote.
fn sym_src(key: &str) -> String {
    format!(
        r#"
        declare const w: boolean;
        function getOwn(m: any, k: any) {{ return m[k]; }}
        class Svc {{ size = 3; }}
        const box: any = {{ svc: new Svc() }};
        const o: any = {{}};
        const k: any = {key};
        o[k] = getOwn(Object, String(Date.now()));
        o.box = box;
        o[Symbol.toPrimitive]("default");
        console.log(box.svc.size);
        "#
    )
}

#[test]
fn any_string_key_store_keeps_facts() {
    let (ok, d) = witness(&sym_src("String(Date.now())"));
    assert!(ok, "any-String key store read back under Symbol.toPrimitive\n{d}");
}

/// The predicate itself: the any-String shape may name every string, a
/// Number key among them, and no well-known symbol; no shape (`Key::Any(0)`)
/// may name a symbol too.
#[test]
fn any_string_shape_never_names_a_symbol() {
    let mut m = crate::model::Model::default();
    let s = m.any_string_shape();
    assert_ne!(s, 0);
    let wk = format!("{}toPrimitive", crate::builtins::WK_PREFIX);
    assert!(!m.key_may_name_str(Key::Any(s), &wk));
    assert!(m.key_may_name_str(Key::Any(0), &wk));
    for n in ["constructor", "toString", "__proto__", "0", crate::model::ELEM, crate::model::NUM_KEY] {
        assert!(m.key_may_name_str(Key::Any(s), n), "{n}");
    }
}
