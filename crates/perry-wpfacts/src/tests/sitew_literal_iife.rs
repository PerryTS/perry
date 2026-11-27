//! Rule SITEW (ECMA-262 13.2.5.5, 15.4.4): the get/set closures of an
//! object literal are created in the running execution context, once per
//! evaluation of the literal. The HIR lowers a literal with accessors to an
//! IIFE over the literal's birth; that IIFE is summarized like any closure,
//! so what its accessors capture from the enclosing run (`read`, `decls`)
//! stays a term of that run. RED on cliff-trunk10 e227d44179: the IIFE was
//! never summarized, every capture through it was pinned, and the result of
//! `read()` in each run of `styleDeclaration` was one shared node holding
//! every run's declarations (c09 tokens.ts:142, N x N `.find` work).
use super::*;

const SRC: &str = r#"
function parse(text: string): [string, string][] { const out: [string, string][] = []; out.push([text, text]); return out; }
interface Host { getAttribute(n: string): string | null; setAttribute(n: string, v: string): void; }
function styleDeclaration(host: Host | null, initial: [string, string][] = []): any {
  let decls: [string, string][] = initial;
  let seen: string | null = null;
  const read = (): [string, string][] => { if (host === null) return decls; const a = host.getAttribute("style") ?? ""; if (a !== seen) { seen = a; decls = parse(a); } return decls; };
  const get = (name: string): string => { const d = read().find((x) => x[0] === name); return d === undefined ? "" : d[1]; };
  const set = (name: string, value: string): void => { read(); decls = decls.filter((x) => x[0] !== name); decls.push([name, value]); host?.setAttribute("style", name); };
  const api = {
    get length(): number { return read().length; },
    set cssText(text: string) { decls = parse(String(text)); },
    getPropertyValue: (n: string): string => get(n),
    setProperty: (n: string, v: string): void => set(n, v),
  };
  return new Proxy(api, {
    get(t, key) { if (typeof key !== "string") return undefined; if (key in api) return (api as any)[key]; return get(key); },
    set(_t, key, v) { if (typeof key !== "string") return false; set(key, String(v)); return true; },
  });
}
class El implements Host {
  attrs = new Map<string, string>();
  _style: any = null;
  getAttribute(n: string): string | null { return this.attrs.get(n) ?? null; }
  setAttribute(n: string, v: string): void { this.attrs.set(n, v); }
  get style(): any { if (this._style === null) this._style = styleDeclaration(this); return this._style; }
}
class A extends El {}
class B extends El {}
class C extends El {}
const els: El[] = [new A(), new B(), new C()];
for (const e of els) { e.style.color = "red"; e.style.cssText = "top: 1"; console.log(e.style.width, e.style.length); }
"#;

/// Every `.find`/`.filter` receiver in the arrows `get` and `set` is a
/// term of the run (a slot), not one node all runs share.
#[test]
fn sitew_literal_accessor_captures_stay_per_run() {
    let ast = perry_parser::parse_typescript(SRC, "t.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").unwrap();
    let (g, _, _) = crate::generate(
        &[("t.ts", &hir)],
        &FactsConfig::default(),
        None,
        None,
        None,
        crate::Hold::Midway,
        std::time::Instant::now(),
    );
    let mut seen = 0;
    for (f, fi) in g.m.funcs.iter().enumerate() {
        let n = &fi.name;
        if !(n.contains(":get#") || n.contains(":set#")) {
            continue;
        }
        let Some(s) = g.m.sum.of.get(&(f as u32)) else { continue };
        for (h, c) in &s.cons {
            if let crate::model::Cons::MCall { .. } = c {
                seen += 1;
                assert!(
                    h & crate::summary::SLOT != 0,
                    "{n}: method-call receiver {h} is shared by every run: {}",
                    g.m.sum.stats
                );
            }
        }
    }
    assert!(seen >= 2, "no .find/.filter receiver in a summary of get/set ({seen})");
}
