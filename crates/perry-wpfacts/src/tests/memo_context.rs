//! Rule C (lane RLC): a function with no `this` term that creates a closure
//! over its own frame (a closure maker: tsc's `memoizeOne`, `memoize`) runs
//! once per call site, and what that run births is per (birth site, call
//! site). One memo's environment (its Map, its callback cell) is then never
//! another memo's, so a memo whose callback returns ⊤ no longer reaches the
//! factory's getters.
//!
//! Per unit the positives stay unproven: memoizeOne is its own declaration
//! unit, its returned closure reaches the caller as a named function
//! (DECISIONS 106) whose result is ⊤, and memoizeOne's own unit solves the
//! union of every caller's callback (design §10.3). Per unit is never
//! stronger than whole, which the negatives check.

use super::call_shadow::{per_unit, whole};

const MEMO_ONE: &str = r#"
function memoizeOne<A extends string | number, T>(callback: (arg: A) => T): (arg: A) => T {
  const map = new Map<string, T>();
  return (arg: A) => {
    const key = `${typeof arg}:${arg}`;
    let value = map.get(key);
    if (value === undefined && !map.has(key)) {
      value = callback(arg);
      map.set(key, value);
    }
    return value!;
  };
}
"#;

const MEMO: &str = r#"
function memoize<T>(callback: () => T): () => T {
  let value: T;
  return () => {
    if (callback) { value = callback(); callback = undefined!; }
    return value;
  };
}
"#;

const FACTORY_ONE: &str = r#"
function createFactory(flags: number) {
  const getPrimary = MAKE((kind: number) => () => ({ kind }));
  const factory = {
    flags,
    createIdentifier(text: string) { return { kind: 80, text }; },
    get createAllType() { return getPrimary(312); },
  };
  return factory;
}
const factory = createFactory(4);
let n = 0;
for (let i = 0; i < 10; i++) n += factory.createAllType().kind + factory.createIdentifier("a").text.length;
console.log(n, factory.flags, typeof getHost(1));
"#;

fn memo_one(host: &str, make: &str) -> String {
    format!("{MEMO_ONE}{host}{}", FACTORY_ONE.replace("MAKE", make))
}

/// Two memos made inside functions (no program-site call reaches either):
/// before rule C both are one shared instance, one Map token, one callback.
#[test]
fn a_memo_made_in_a_function_keeps_its_own_map() {
    let src = memo_one(
        "function mkHost() { return memoizeOne((b: number) => (globalThis as any).host ?? ((x: number) => x)); }\n\
         const getHost = mkHost();\n",
        "memoizeOne",
    );
    assert_eq!(whole(&src).0, true, "whole: createIdentifier must prove");
}

/// A memo at a program site and one in a function (FAC3 acc2 without the
/// ⊤ hook): the `new Map()` births per call site, not per birth site.
#[test]
fn a_memo_at_a_program_site_and_one_in_a_function_have_two_maps() {
    let src = memo_one(
        "const getHost = memoizeOne((b: number) => (globalThis as any).host ?? ((x: number) => x));\n",
        "memoizeOne",
    );
    assert_eq!(whole(&src).0, true, "whole: createIdentifier must prove");
}

/// `memoize`'s captured cell: one cell per call site.
#[test]
fn a_memoize_cell_is_per_call_site() {
    let src = format!(
        "{MEMO}
        function mkHost() {{ return memoize(() => (globalThis as any).host ?? ((x: number) => x)); }}
        const getHost = mkHost();
        function createFactory(flags: number) {{
          const getPrimary = memoize(() => () => ({{ kind: 312 }}));
          const factory = {{
            flags,
            createIdentifier(text: string) {{ return {{ kind: 80, text }}; }},
            get createAllType() {{ return getPrimary(); }},
          }};
          return factory;
        }}
        const factory = createFactory(4);
        let n = 0;
        for (let i = 0; i < 10; i++) n += factory.createAllType().kind + factory.createIdentifier(\"a\").text.length;
        console.log(n, factory.flags, typeof getHost());"
    );
    assert_eq!(whole(&src).0, true, "whole: createIdentifier must prove");
}

/// Both memos come through ONE forwarding site (`mk` -> memoizeOne): still
/// two runs of `mk` (a program site and a call inside createFactory), so two
/// runs of memoizeOne, two Maps, two callbacks.
#[test]
fn a_forwarding_maker_keeps_two_memos_apart() {
    let src = memo_one(
        "function mk(cb: (b: number) => any) { return memoizeOne(cb); }\n\
         const getHost = mk((b: number) => (globalThis as any).host ?? ((x: number) => x));\n",
        "mk",
    );
    assert_eq!(whole(&src).0, true, "whole: createIdentifier must prove");
}

/// Negative: ONE memoizeOne run is both the host reader and the factory's
/// memo; its callback may return ⊤, so the getter's result may be ⊤.
#[test]
fn one_memo_shared_with_a_top_callback_stays_unproven() {
    let src = format!(
        "{MEMO_ONE}
        function mkShared() {{ return memoizeOne((k: number) => k > 400 ? (globalThis as any).host : () => ({{ kind: k }})); }}
        const shared = mkShared();
        const getHost = shared;
        {}",
        FACTORY_ONE.replace("const getPrimary = MAKE((kind: number) => () => ({ kind }));", "const getPrimary = shared;")
    );
    assert_eq!(whole(&src).0, false, "whole: the shared memo stays unproven");
    assert!(!per_unit(&src), "per unit: the shared memo stays unproven");
}

/// Negative: one call site inside a loop makes both memos (one run per
/// site): the site's run holds both callbacks, the ⊤ one reaches the getter.
#[test]
fn one_site_in_a_loop_merges_its_memos() {
    let src = format!(
        "{MEMO_ONE}
        const cbs = [(k: number) => (globalThis as any).host, (k: number) => () => ({{ kind: k }})];
        function mkAll() {{ const out: any[] = []; for (const cb of cbs) out.push(memoizeOne(cb)); return out; }}
        const memos = mkAll();
        const getHost = memos[0];
        {}",
        FACTORY_ONE.replace("const getPrimary = MAKE((kind: number) => () => ({ kind }));", "const getPrimary = memos[1];")
    );
    assert_eq!(whole(&src).0, false, "whole: the merged memos stay unproven");
    assert!(!per_unit(&src), "per unit: the merged memos stay unproven");
}

/// Negative: the memo's cell is written again from ⊤ after it was made
/// (two producers of the cell).
#[test]
fn a_memo_cell_reset_from_top_stays_unproven() {
    let src = r#"
function memoize<T>(callback: () => T): { get: () => T; reset: (c: () => T) => void } {
  let value: T;
  return {
    get: () => { if (callback) { value = callback(); callback = undefined!; } return value; },
    reset: (c: () => T) => { callback = c; },
  };
}
function createFactory(flags: number) {
  const memo = memoize(() => () => ({ kind: 312 }));
  memo.reset((globalThis as any).other ?? (() => () => ({ kind: 312 })));
  const factory = {
    flags,
    createIdentifier(text: string) { return { kind: 80, text }; },
    get createAllType() { return memo.get(); },
  };
  return factory;
}
const factory = createFactory(4);
let n = 0;
for (let i = 0; i < 10; i++) n += factory.createAllType().kind + factory.createIdentifier("a").text.length;
console.log(n, factory.flags);
"#;
    assert_eq!(whole(src).0, false, "whole: the reset cell stays unproven");
    assert!(!per_unit(src), "per unit: the reset cell stays unproven");
}
