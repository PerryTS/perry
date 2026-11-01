//! Rule C (lane RLC): a function with no `this` term that creates a closure
//! over its own frame (a closure maker: tsc's `memoizeOne`, `memoize`) runs
//! once per call site, and what that run births is per (birth site, call
//! site). One memo's environment (its Map, its callback cell) is then never
//! another memo's, so a memo whose callback returns ⊤ no longer reaches the
//! factory's getters.

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
    assert!(per_unit(&src), "per unit: createIdentifier must prove");
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
    assert!(per_unit(&src), "per unit: createIdentifier must prove");
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
    assert!(per_unit(&src), "per unit: createIdentifier must prove");
}

/// Negative: both memos come from ONE call site (`mk`), so that site's
/// run holds both callbacks; the ⊤ one reaches the factory's getter.
#[test]
fn one_call_site_making_two_memos_merges_them() {
    let src = memo_one(
        "function mk(cb: (b: number) => any) { return memoizeOne(cb); }\n\
         const getHost = mk((b: number) => (globalThis as any).host ?? ((x: number) => x));\n",
        "mk",
    );
    assert_eq!(whole(&src).0, false, "whole: the merged memo stays unproven");
    assert!(!per_unit(&src), "per unit: the merged memo stays unproven");
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
