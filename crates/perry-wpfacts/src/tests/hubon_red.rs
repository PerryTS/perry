//! HUBON1 red fixture (c09 VBUF1 first-arrival chain): perry-web dom/events.ts:337
//! `(target._self() as Record<string, unknown>)["on" + event.type]` on an element family
//! whose `style` getter returns a Proxy (dom/tokens.ts:159). The key is `"on" + String`:
//! ECMA-262 13.15.3 makes it a String that starts with "on", 10.1.8.1 OrdinaryGet runs only
//! the accessor defined under that key, so `style` never runs and its Proxy never reaches
//! the result. Control: `o[t]` with `t` unknown may name `style`.
//! RED on base be688fccda: once the program defines an accessor under a computed name
//! (`numeric_accessor`, here `Object.defineProperty(reg, s, {get})`), the index read's
//! number-key part (`plain_key_from`) walks the chain under ANY key, though ToPropertyKey of
//! `"on" + x` is never a canonical numeric String (7.1.19, 7.1.21).
use super::*;

fn witness_proven(src: &str) -> bool {
    let pf = facts(src);
    let sites: Vec<_> = pf.sites.iter().filter(|s| s.key == "witness").collect();
    assert_eq!(sites.len(), 1, "{src}\n{}", pf.dump_text());
    let verdict = &sites[0].verdict;
    assert!(
        !matches!(verdict, Verdict::No(why) if why.starts_with("dead site")),
        "{src}: {verdict:?}",
    );
    !matches!(verdict, Verdict::No(_))
}

// The style Proxy's get trap hands `cell` to unknown code: once the read's
// result may hold the Proxy, a member read of it runs the trap and
// `cell.witness` is not proven.
fn src(key: &str) -> String {
    format!(
        r#"
declare function external(x: unknown): void;
class Cell {{ witness = 1; }}
const cell = new Cell();
function styleDeclaration(el: unknown): Record<string, unknown> {{
  const api = {{ el }};
  return new Proxy(api, {{
    get(t: object, k: string | symbol): unknown {{ external(cell); return undefined; }},
    set(t: object, k: string | symbol, v: unknown): boolean {{ return true; }},
  }}) as Record<string, unknown>;
}}
class Ev {{ type: string; constructor(t: string) {{ this.type = t; }} }}
function EventTargetBase() {{
  return class {{
    _l: unknown[] = [];
    _self(): unknown {{ return this; }}
  }};
}}
class EventTarget extends EventTargetBase() {{}}
class Node extends EventTarget {{ nodeType = 1; }}
class Element extends Node {{
  onclick: unknown = null;
  private _style: Record<string, unknown> | null = null;
  get style(): Record<string, unknown> {{ if (this._style === null) this._style = styleDeclaration(this); return this._style; }}
  get tagName(): string {{ return "DIV"; }}
}}
const el = new Element();
el.style.left = "1px";
function invoke(target: EventTarget, event: Ev): void {{
  const handler = (target._self() as Record<string, unknown>)[{key}];
  if (handler !== null && handler !== undefined) console.log((handler as Record<string, unknown>).foo);
}}
const s: string = String(process.argv[2]);
const reg: Record<string, unknown> = {{}};
Object.defineProperty(reg, s, {{ get(): number {{ return 1; }}, configurable: true }});
console.log(reg[s]);
invoke(el, new Ev(s));
console.log(cell.witness);
"#
    )
}

#[test]
fn hubon_on_prefix_read_skips_style_getter() {
    assert!(witness_proven(&src("\"on\" + event.type")));
}

#[test]
fn hubon_unknown_key_read_control() {
    assert!(!witness_proven(&src("event.type")));
}
