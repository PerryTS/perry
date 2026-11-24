//! A captured lexical binding's dead zone on the web target.

use super::run_web_source;

// A closure that reads or assigns a `let` or class binding before its declaration ran
// throws Node's ReferenceError; after the declaration it sees the value, and a `let`
// with no initializer is `undefined`. Expected output is Node's.
#[test]
fn closure_reads_in_the_dead_zone_throw() {
    assert_eq!(
        run_web_source(
            r#"
function probe(f: () => unknown): string {
  try { return "ok " + String(f()); } catch (e: any) { return e.name + ": " + e.message; }
}
function viaLet(): string {
  const read = () => x;
  const write = () => { x = 5; };
  const before = probe(read) + " | " + probe(write);
  let x = 1;
  return before + " | " + probe(read);
}
function viaClass(): string {
  const read = () => C;
  const before = probe(read);
  class C {}
  return before + " | " + (read() === C);
}
function viaDefault(): string {
  const read = (v = y) => v;
  const before = probe(read);
  let y = 2;
  return before + " | " + probe(read);
}
function noInit(): string {
  const read = () => z;
  const before = probe(read);
  let z: number | undefined;
  return before + " | " + probe(read);
}
console.log(viaLet());
console.log(viaClass());
console.log(viaDefault());
console.log(noInit());
"#
        ),
        "ReferenceError: Cannot access 'x' before initialization | ReferenceError: Cannot access 'x' before initialization | ok 1\n\
         ReferenceError: Cannot access 'C' before initialization | true\n\
         ReferenceError: Cannot access 'y' before initialization | ok 2\n\
         ReferenceError: Cannot access 'z' before initialization | ok undefined"
    );
}
