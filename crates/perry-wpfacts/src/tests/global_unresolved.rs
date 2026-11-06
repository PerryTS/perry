//! Rule G: an unresolved identifier is `[[Get]](globalThis, NAME)`
//! (`perry-runtime` `error/global_ref.rs`), the same load as
//! `globalThis.NAME`. A name the runtime realm does not declare on the
//! global object or its chain holds exactly what seen stores put there; one
//! nothing stores is absent (a ReferenceError, `typeof` "undefined"), so a
//! guarded call through it hands nothing to unseen code.
use super::*;

fn a_x_proven(src: &str) -> Result<bool, String> {
    let at = src.find("// A_X").expect("named site");
    let line = src[..at].rfind('\n').map_or(0, |p| p + 1) as u32;
    let pf = facts(src);
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| !s.is_call && s.key == "x" && (line..at as u32).contains(&s.byte_offset))
        .collect();
    if sites.is_empty() {
        return Err(format!("missing A_X\n{}", pf.dump_text()));
    }
    Ok(sites.iter().all(|s| !matches!(s.verdict, Verdict::No(_))))
}

/// `prefix` runs first; `add` replaced by a layout change shows a wrong proof.
fn program(prefix: &str) -> String {
    format!(
        r#"
{prefix}
class V3 {{
  x: number; y: number;
  constructor(x: number, y: number) {{ this.x = x; this.y = y; }}
  add(v: V3) {{ this.x += v.x; this.y += v.y; return this; }}
}}
class Obj {{
  position = new V3(0, 0);
  constructor() {{
    if (typeof DEVTOOLS !== "undefined") {{ DEVTOOLS.observe(this); }}
    if (typeof window !== "undefined") {{ window.__REV__ = "1"; }}
  }}
}}
const o = new Obj();
const a = new V3(1, 2), b = new V3(4, 5);
for (let i = 0; i < 3; i++) a.add(b);
console.log(a.x, o.position.y); // A_X
"#
    )
}

const LEAK: &str =
    "{ observe(o: any) { o.position.constructor.prototype.add = function (this: any) { delete this.x; this.q = 1; return this; }; } }";

#[test]
fn a_global_nothing_stores_hands_nothing_to_unseen_code() {
    assert_eq!(a_x_proven(&program("")), Ok(true));
}

/// Twins: every way a program (or code it runs) can put a value under the
/// name keeps the call's receiver reachable by that value; each of these
/// replaces `V3.prototype.add` (node prints `undefined`).
#[test]
fn a_global_something_may_store_keeps_its_value() {
    let mut failures = Vec::new();
    for prefix in [
        format!("(globalThis as any)[\"DEV\" + \"TOOLS\".slice(0)] = {LEAK};"),
        format!("(globalThis as any).DEVTOOLS = {LEAK};"),
        format!("new Function(\"globalThis.DEVTOOLS = {}\")();", LEAK.replace("(o: any)", "(o)").replace("(this: any)", "()").replace('"', "\\\"")),
        format!("Object.defineProperty(globalThis, \"DEVTOOLS\", {{ get() {{ return {LEAK}; }} }});"),
        format!("(Object.prototype as any).DEVTOOLS = {LEAK};"),
        format!("Object.assign(globalThis, {{ DEVTOOLS: {LEAK} }});"),
    ] {
        match a_x_proven(&program(&prefix)) {
            Ok(false) => {}
            Ok(true) => failures.push(prefix),
            Err(e) => failures.push(format!("{prefix}: {}", e.lines().next().unwrap())),
        }
    }
    assert!(failures.is_empty(), "proven although a store may replace add: {failures:?}");
}
