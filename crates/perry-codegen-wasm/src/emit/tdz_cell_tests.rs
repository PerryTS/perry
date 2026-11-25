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

// A class evaluated, first-evaluation-checked or constructed while a binding its methods
// capture is in its dead zone reads that binding for its environment, not for the
// program: nothing throws. Expected output is Node's.
#[test]
fn class_captures_in_the_dead_zone_do_not_throw() {
    assert_eq!(
        run_web_source(
            r#"
function declared(): number {
  class A { m() { return 5; } n() { return x; } }
  const read = () => x;
  const early = new A().m();
  const x = 1;
  return early + read();
}
function fresh(): number {
  const C = class { static s = 1; m() { return x; } };
  const read = () => x;
  const x = 4;
  return C.s + new C().m() + read();
}
function refreshed(): number {
  const read = () => [x, new A().n()];
  let x = 1;
  x = 2;
  class A { n() { return x; } }
  return read()[1] as number;
}
function unreached(): string {
  class A { n() { return x; } }
  const read = () => x;
  return typeof A + typeof read;
  const x = 4;
}
console.log(declared(), fresh(), refreshed(), unreached());
"#
        ),
        "6 9 2 functionfunction"
    );
}

// Reading or assigning a captured binding never inspects the value it holds, so a Proxy's
// traps stay silent.
#[test]
fn captured_proxy_is_not_inspected() {
    assert_eq!(
        run_web_source(
            r#"
const HostProxy: any = (globalThis as any).Proxy;
let traps = 0;
function held(): unknown {
  let p: any = new HostProxy({}, { getPrototypeOf() { traps++; return null; } });
  const read = () => p;
  const write = (v: any) => { p = v; };
  read();
  write(1);
  return read();
}
console.log(held(), traps);
"#
        ),
        "1 0"
    );
}

// The mark of a dead zone is no string key, so a program's Object.prototype cannot supply it
// to an initialized cell. Expected output is Node's.
#[test]
fn object_prototype_does_not_mark_a_cell() {
    assert_eq!(
        run_web_source(
            r#"
(Object.prototype as any).tdz = "poison";
function bump(a: number): number {
  const add = () => { a += 1; };
  add();
  return a;
}
console.log(bump(1));
"#
        ),
        "2"
    );
}
