//! A for-in key is a String: EnumerateObjectProperties never returns a
//! Symbol property key (ECMA-262 14.7.5.9; the For-In Iterator, 14.7.5.10,
//! returns only String keys). A store `o[k] = v` under a run's key `k`
//! writes a string-keyed property of `o`, so a read of a well-known-symbol
//! key (`o[Symbol.iterator]`, the iterator protocol of `for-of`) never sees
//! `v`. A store under a key that may be a Symbol, and unknown code holding
//! `o`, may write `@@iterator`.
//!
//! Each fixture copies a function of unknown code into `ext` under a for-in
//! key (the run's keys include one the program does not know), then calls
//! `ext[Symbol.iterator]()`: a read that sees the copy calls unknown code with
//! `ext` as receiver, so `ext` escapes with `obj` in its field `o`, and the
//! call `obj.len()` after it is unproven.

use super::call_shadow::{call_of, proven};
use super::*;

fn fixture(extra: &str) -> String {
    format!(
        r#"
class V {{ k = 1; len() {{ return this.k; }} }}
const obj = new V();
const ext: any = {{ o: obj }};
const src: any = {{ p: new Function(process.argv[3]) }};
src[process.argv[5]] = 2;
for (const name in src) {{
  ext[name] = src[name];
}}
{extra}
try {{ ext[Symbol.iterator](); }} catch (e) {{}}
console.log(obj.len());
"#
    )
}

fn len_proven(program: &str) -> bool {
    let pf = facts(program);
    proven(&call_of(&pf, "len").verdict)
}

#[test]
fn a_for_in_keyed_store_never_writes_the_iterator() {
    assert!(len_proven(&fixture("")));
}

#[test]
fn control_a_symbol_keyed_store_writes_the_iterator() {
    assert!(!len_proven(&fixture(
        "ext[Symbol.iterator] = new Function(process.argv[4]);"
    )));
}

#[test]
fn control_a_key_that_may_be_a_symbol_writes_the_iterator() {
    assert!(!len_proven(&fixture(
        "const k: any = process.argv.length > 5 ? Symbol.iterator : \"p\";\next[k] = new Function(process.argv[4]);"
    )));
}

#[test]
fn control_unknown_code_holding_the_object_may_write_the_iterator() {
    assert!(!len_proven(&fixture(
        "new Function(\"e\", process.argv[2])(ext);"
    )));
}
