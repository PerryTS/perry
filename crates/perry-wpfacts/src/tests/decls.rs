//! DECLS (cliff c09, perry-web dom/tokens.ts `styleDeclaration`): a
//! captured `let decls` that every run of the Proxy traps' helpers shares.
//! `set` runs once per property key (the trap runs per key) and each run's
//! `decls.filter(...)` result goes only into `decls`. Its content is real
//! JS (a get after sets on the SAME object sees every set key's tuple), but
//! the per-run arrays are indistinguishable: nothing holds one run's array
//! without every other run's. Rule SINKBIRTH: a summary call whose result
//! only flows into nodes every run shares births its result as the
//! template run does, one token per birth site.

use crate::model::{Model, TmplKind, TokKind};

fn solved(src: &str) -> Model {
    let ast = Box::leak(Box::new(
        perry_parser::parse_typescript(src, "t.ts").unwrap(),
    ));
    let hir = Box::leak(Box::new(perry_hir::lower_module(ast, "t", "t.ts").unwrap()));
    let mut g = crate::gen::Gen::new(vec![("t.ts", &*hir)], false, false);
    g.m.max_work = 5_000_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    g.m
}

/// perry-web dom/tokens.ts L76-172 (kebab, camel, parseDeclarations,
/// styleDeclaration) verbatim; the program is `TAIL0`.
const STYLE: &str = r##"interface AttributeHost { getAttribute(name: string): string | null; setAttribute(name: string, value: string): void; removeAttribute(name: string): void; }
// ---------------------------------------------------------------- style

const kebab = (name: string): string =>
  name.startsWith("--") ? name : name.replace(/^(webkit|moz|ms)(?=[A-Z])/, "-$1").replace(/[A-Z]/g, (c) => "-" + c.toLowerCase());
const camel = (name: string): string =>
  name.startsWith("--") ? name : name.replace(/^-(webkit|moz|ms)-/, "$1-").replace(/-([a-z])/g, (_m, c: string) => c.toUpperCase());

/** Split a declaration block into [property, value, important] (semicolons in strings and parentheses don't end one). */
function parseDeclarations(text: string): [string, string, boolean][] {
  const out: [string, string, boolean][] = [];
  let depth = 0, quote = "", start = 0;
  const parts: string[] = [];
  for (let i = 0; i < text.length; i++) {
    const c = text.charAt(i);
    if (quote !== "") { if (c === "\\") i++; else if (c === quote) quote = ""; continue; }
    if (c === "\"" || c === "'") quote = c;
    else if (c === "(") depth++;
    else if (c === ")") depth--;
    else if (c === ";" && depth === 0) { parts.push(text.slice(start, i)); start = i + 1; }
  }
  parts.push(text.slice(start));
  for (const p of parts) {
    const colon = p.indexOf(":");
    if (colon < 0) continue;
    const name = p.slice(0, colon).trim();
    let value = p.slice(colon + 1).trim();
    if (name === "" || value === "") continue;
    let important = false;
    const imp = /!\s*important\s*$/i.exec(value);
    if (imp !== null) { important = true; value = value.slice(0, imp.index).trim(); }
    out.push([name.startsWith("--") ? name : name.toLowerCase(), value, important]);
  }
  return out;
}

/**
 * The style attribute as an object: `style.backgroundColor = "red"`,
 * `style.setProperty("--x", "1")`, `style.cssText`. A Proxy, so any
 * camelCase or kebab-case property name works.
 */
interface CSSStyleDeclaration {
  cssText: string;
  readonly length: number;
  item(i: number): string;
  getPropertyValue(name: string): string;
  getPropertyPriority(name: string): string;
  setProperty(name: string, value: string | null, priority?: string): void;
  removeProperty(name: string): string;
  [property: string]: unknown;
}

function styleDeclaration(host: AttributeHost | null, initial: [string, string, boolean][] = [], readOnly = false): CSSStyleDeclaration {
  let decls: [string, string, boolean][] = initial;
  let seen: string | null = null;
  const read = (): [string, string, boolean][] => {
    if (host === null) return decls;
    const attr = host.getAttribute("style") ?? "";
    if (attr !== seen) { seen = attr; decls = parseDeclarations(attr); }
    return decls;
  };
  const write = (): void => {
    if (readOnly) throw new Error("NoModificationAllowedError: computed style is read-only");
    const text = decls.map(([n, v, imp]) => `${n}: ${v}${imp ? " !important" : ""};`).join(" ");
    seen = text;
    if (host !== null) { if (text === "") host.removeAttribute("style"); else host.setAttribute("style", text); }
  };
  const get = (name: string): string => { const d = read().find((x) => x[0] === name); return d === undefined ? "" : d[1]; };
  const set = (name: string, value: string | null, important = false): void => {
    read();
    decls = decls.filter((x) => x[0] !== name);
    if (value !== null && String(value) !== "") decls.push([name, String(value), important]);
    write();
  };
  const api = {
    get cssText(): string { return read().map(([n, v, imp]) => `${n}: ${v}${imp ? " !important" : ""};`).join(" "); },
    set cssText(text: string) { decls = parseDeclarations(String(text)); write(); },
    get length(): number { return read().length; },
    item: (i: number): string => read()[i]?.[0] ?? "",
    getPropertyValue: (name: string): string => get(name.startsWith("--") ? name : name.toLowerCase()),
    getPropertyPriority: (name: string): string => (read().find((x) => x[0] === name)?.[2] === true ? "important" : ""),
    setProperty: (name: string, value: string | null, priority?: string): void => set(name.startsWith("--") ? name : name.toLowerCase(), value, priority === "important"),
    removeProperty: (name: string): string => { const old = get(name); set(name, null); return old; },
  };
  return new Proxy(api as unknown as CSSStyleDeclaration, {
    get(target, key) {
      if (typeof key !== "string") return (target as unknown as Record<symbol, unknown>)[key as unknown as symbol];
      if (key in api) return (api as unknown as Record<string, unknown>)[key];
      if (/^\d+$/.test(key)) return api.item(Number(key));
      return get(kebab(key));
    },
    set(_target, key, value) {
      if (typeof key !== "string") return false;
      if (key === "cssText") { api.cssText = String(value); return true; }
      set(kebab(key), value === null || value === undefined ? null : String(value));
      return true;
    },
    has(_target, key) { return typeof key === "string" && (key in api || read().some((d) => d[0] === kebab(key))); },
  });
}
__TAIL__
"##;

/// The program: an element-like owner whose `style` getter makes the
/// declaration once, and a method that sets three keys through it, run on
/// two elements. Each trap call runs in its caller's run (a nested site),
/// so every `set` run has its own copy of the `filter` call: on 791b8df6fb
/// one array per run (3), each holding every tuple.
const TAIL0: &str = "class El { st: CSSStyleDeclaration | null = null; get style(): CSSStyleDeclaration { if (this.st === null) this.st = styleDeclaration(null); return this.st; } paint(c: string) { this.style.color = c; this.style.width = \"1px\"; this.style.height = \"2px\"; } }\nconst e = new El();\ne.paint(\"red\");\nconst f = new El();\nf.paint(\"blue\");\nconsole.log(e.style.color, f.style.width);";

/// The tokens of each `Array@call` site template (a builtin's array born
/// at a call), by template.
fn site_arrays(m: &Model) -> Vec<(u32, Vec<u32>)> {
    let mut out: Vec<(u32, Vec<u32>)> = Vec::new();
    for (t, k) in m.toks.iter().enumerate() {
        let TokKind::Obj(tm, _) = *k else { continue };
        if !matches!(
            m.tmpls[tm as usize].kind,
            TmplKind::Arr {
                module: u32::MAX,
                ..
            }
        ) {
            continue;
        }
        match out.iter_mut().find(|e| e.0 == tm) {
            Some(e) => e.1.push(t as u32),
            None => out.push((tm, vec![t as u32])),
        }
    }
    out
}

fn elems(m: &Model, t: u32) -> Vec<u32> {
    match m.field_any.get(&t) {
        Some(&n) => m.nodes[n as usize].pts.to_sorted(),
        None => Vec::new(),
    }
}

#[test]
fn decls_filter_result_is_one_object_per_closure_context() {
    let m = solved(&STYLE.replace("__TAIL__", TAIL0));
    for (t, k) in m.toks.iter().enumerate() {
        let nm = m.tok_name(t as u32);
        if nm.contains("lit#")
            || nm.contains("Array@")
            || nm.contains("fn:t.ts:set")
            || nm.contains("fn:t.ts:get")
        {
            eprintln!("DECLS tok {t} {k:?} {nm}");
        }
    }
    eprintln!("DECLS insts {}", m.sum.insts.len());
    let arrs = site_arrays(&m);
    for (tm, ts) in &arrs {
        eprintln!(
            "DECLS tmpl {} {} toks {:?}",
            tm, m.tmpls[*tm as usize].display, ts
        );
        for &t in ts {
            let e: Vec<String> = elems(&m, t).iter().map(|&e| m.tok_name(e)).collect();
            eprintln!("DECLS   tok {t} {:?} elems {e:?}", m.toks[t as usize]);
        }
    }
    // The filter result: the site array holding the tuples.
    let tuple_of = |m: &Model, t: u32| m.tok_name(t).contains("[lit#");
    let filt: Vec<&(u32, Vec<u32>)> = arrs
        .iter()
        .filter(|(_, ts)| {
            ts.iter()
                .any(|&t| elems(&m, t).iter().any(|&e| tuple_of(&m, e)))
        })
        .collect();
    assert_eq!(
        filt.len(),
        1,
        "one filter site template holds the tuples: {arrs:?}"
    );
    // The arrays the runs birth (the site's template token holds none).
    let ts: Vec<u32> = filt[0]
        .1
        .iter()
        .copied()
        .filter(|&t| elems(&m, t).iter().any(|&e| tuple_of(&m, e)))
        .collect();
    // Positive: every set key's tuple is in the array (set after set keeps
    // the earlier keys: `set`'s `[name, String(value), important]`, one per
    // key run, holding the String value).
    let holds_str = |m: &Model, e: u32| {
        m.field_any
            .get(&e)
            .is_some_and(|&fa| m.nodes[fa as usize].prim & crate::model::prim::STR != 0)
    };
    for &t in &ts {
        let tuples: Vec<u32> = elems(&m, t)
            .into_iter()
            .filter(|&e| tuple_of(&m, e) && holds_str(&m, e))
            .collect();
        eprintln!(
            "DECLS filter tok {} {} tuples {:?}",
            t,
            m.tok_name(t),
            tuples
        );
        assert!(
            tuples.len() >= 3,
            "every key's tuple reaches the filter result {t}: {tuples:?}"
        );
    }
    // Rule SITEW: a literal-IIFE result is born per closure context, so the
    // filter result is one array per styleDeclaration closure context (3 in
    // this test). e.style and f.style are different objects with different
    // decls; key runs within one context fold to one token. One array across
    // ALL contexts would encode the old imprecision (decls shared/pinned).
    assert_eq!(
        ts.len(),
        3,
        "one filter result per styleDeclaration closure context: {ts:?}"
    );
}
