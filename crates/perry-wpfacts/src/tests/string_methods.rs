//! String methods and RegExp methods read their operands by the spec's
//! algorithm: the operand is ToString'd (`valueOf`/`toString`), or its
//! well-known-symbol method (`@@replace`, `@@split`, `@@match`,
//! `@@matchAll`, `@@search`) is called with the string; no other method of
//! it runs. `Svc.leak` is never reached by those; a `Hook` with the symbol
//! method hands itself to unknown code when the string method calls it.

use super::*;

fn verdict(body: &str) -> Verdict {
    let src = format!(
        r#"
        class Svc {{ size = 3; leak() {{ (globalThis as any).hostTake(this); }} }}
        function g(a: any) {{ return a.size; }}
        const s: any = "a-b";
        const t: string = "a-b";
        const re = /b/;
        {body}
        "#
    );
    site(&facts(&src), "size").verdict.clone()
}

fn hook(sym: &str) -> String {
    format!(
        "class Hook {{ size = 3; [Symbol.{sym}](..._a: any[]): any {{ (globalThis as any).hostTake(this); return 'r'; }} }}"
    )
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// `expr` on a string with `new Svc()` as the operand: node prints `3`.
fn reads_only(expr: &str) {
    let v = verdict(&format!("const o: any = new Svc(); {expr}; console.log(g(o));"));
    assert!(proven(&v), "{expr}: {v:?}");
}

/// `expr` on a string with a `Hook` of `sym` as the operand: node runs the
/// hook (`hostTake(this)` stores through `this`), so the object escapes.
fn calls_hook(expr: &str, sym: &str) {
    let v = verdict(&format!(
        "{} const o: any = new Hook(); {expr}; console.log(g(o));",
        hook(sym)
    ));
    assert!(!proven(&v), "{expr}: {v:?}");
}

macro_rules! op {
    ($name:ident, $expr:expr, $sym:expr) => {
        mod $name {
            #[test]
            fn reads_only() {
                super::reads_only($expr);
            }
            #[test]
            fn calls_the_symbol_method() {
                super::calls_hook($expr, $sym);
            }
        }
    };
}

// Unknown receiver type: the call is a method call on a primitive.
op!(replace_any, r#"s.replace(o, "x")"#, "replace");
op!(replace_all_any, r#"s.replaceAll(o, "x")"#, "replace");
op!(split_any, "s.split(o)", "split");
op!(match_any, "s.match(o)", "match");
op!(match_all_any, "s.matchAll(o)", "matchAll");
op!(search_any, "s.search(o)", "search");
// Typed string receiver: HIR lowers these to dedicated expressions.
op!(replace_typed, r#"t.replace(o, "x")"#, "replace");
op!(split_typed, "t.split(o)", "split");
op!(match_typed, "t.match(o)", "match");
op!(match_all_typed, "t.matchAll(o)", "matchAll");

/// Operands that are only coerced: no method of theirs but `valueOf` and
/// `toString` runs. node prints `3`.
#[test]
fn coerced_operands_read_only() {
    for e in [
        "s.indexOf(o)",
        "s.lastIndexOf(o)",
        "s.includes(o)",
        "s.startsWith(o)",
        "s.endsWith(o)",
        "s.charAt(o)",
        "s.charCodeAt(o)",
        "s.slice(o, o)",
        "s.substring(o)",
        "s.padStart(o, o)",
        "s.concat(o)",
        "s.localeCompare(o)",
    ] {
        reads_only(e);
    }
}

/// `valueOf` is what ToNumber/ToString runs: node stores through `this`.
#[test]
fn coerced_operand_runs_its_value_of() {
    let v = verdict(
        "class Hook { size = 3; valueOf() { (globalThis as any).hostTake(this); return 1; } }
         const o: any = new Hook(); s.charAt(o); console.log(g(o));",
    );
    assert!(!proven(&v), "{v:?}");
}

/// `includes(x)` tests IsRegExp(x): `@@match` is read, and a getter runs.
#[test]
fn includes_reads_symbol_match() {
    let v = verdict(
        "class Hook { size = 3; get [Symbol.match](): any { (globalThis as any).hostTake(this); return undefined; } }
         const o: any = new Hook(); s.includes(o); console.log(g(o));",
    );
    assert!(!proven(&v), "{v:?}");
}

/// The replace callback is called by the runtime with strings, numbers and
/// the groups object, and its result is ToString'd: returning an object
/// does not hand it to unknown code. node prints `3`.
#[test]
fn replace_callback_gets_match_strings() {
    let v = verdict("const o: any = new Svc(); s.replace(re, () => o); console.log(g(o));");
    assert!(proven(&v), "{v:?}");
}

/// A callback called by a described RegExp `@@replace` (a pattern from a
/// variable of unknown type).
#[test]
fn replace_callback_with_any_pattern() {
    let v = verdict(
        "const p: any = Math.random() > 2 ? 'b' : re;
         const o: any = new Svc(); s.replace(p, () => o); console.log(g(o));",
    );
    assert!(proven(&v), "{v:?}");
}

/// The callback result is ToString'd: its `toString` runs.
#[test]
fn replace_callback_result_is_to_stringed() {
    let v = verdict(
        "class Hook { size = 3; toString() { (globalThis as any).hostTake(this); return 'x'; } }
         const o: any = new Hook(); s.replace(re, () => o); console.log(g(o));",
    );
    assert!(!proven(&v), "{v:?}");
}

/// RegExp methods coerce their string operand, and run the object's `exec`.
#[test]
fn regexp_test_and_exec_read_only() {
    reads_only("re.test(o)");
    reads_only("re.exec(o)");
}

#[test]
fn regexp_test_runs_a_replaced_exec() {
    let v = verdict(
        "const o = new Svc(); (re as any).exec = function (_x: string) { (globalThis as any).hostTake(o); return null; };
         re.test('b'); console.log(g(o));",
    );
    assert!(!proven(&v), "{v:?}");
}

#[test]
fn regexp_test_with_untouched_exec() {
    let v = verdict("const o = new Svc(); re.test('b'); console.log(g(o));");
    assert!(proven(&v), "{v:?}");
}

/// `new RegExp(x)` ToString's `x` and reads `source` and `flags`: no other
/// method of `x` runs. node prints `3`.
#[test]
fn regexp_constructor_reads_only() {
    let v = verdict("const o: any = new Svc(); new RegExp(o); console.log(g(o));");
    assert!(proven(&v), "{v:?}");
}

/// `flags` is a getter the constructor runs on a regexp-like pattern
/// (it has `@@match`): node stores through `this`.
#[test]
fn regexp_constructor_runs_flags_getter() {
    let v = verdict(
        "class Hook { size = 3; get [Symbol.match](): any { return true; }
           get source(): any { return 'a'; }
           get flags(): any { (globalThis as any).hostTake(this); return ''; } }
         const o: any = new Hook(); new RegExp(o); console.log(g(o));",
    );
    assert!(!proven(&v), "{v:?}");
}
