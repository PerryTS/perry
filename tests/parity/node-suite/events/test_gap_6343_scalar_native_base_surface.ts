// #6343: escape analysis must not scalar-replace an instance whose class chain
// reaches a base that codegen cannot fully model.
//
// perry's native bases (`EventEmitter`, the `node:stream` classes, …) install
// their method surface as OWN PROPERTIES on the instance at subclass-init time
// — `js_object_set_field_by_name(obj, "emit", <native closure>)` — not on a
// prototype. Scalar replacement promotes a non-escaping instance to one alloca
// per DECLARED field, so a runtime-installed own property has no slot in the
// promoted set and reads back `undefined`.
//
// The trigger is that the instance never ESCAPES: a method call (`x.on(…)`)
// forces the heap path and hides the bug, so every probe below deliberately
// only *reads* properties off its receiver. A declared field alongside the
// read is what makes the instance look worth promoting.
//
// The controls are the other half: a class with no unmodeled base must STILL
// scalar-replace — that is a real perf win (see
// scripts/run_issue_945_scalar_method_ir_guard.sh), so the disqualifier has to
// be precise rather than a blanket disable.

import { EventEmitter } from "node:events";
import { Readable, Writable } from "node:stream";

// ── the issue's exact repro: declared field + bare reads, no method call ──
class X extends EventEmitter {
  a = 1;
}
function probeX(): string {
  const x = new X();
  x.a = 2;
  return `${x.a} ${typeof x.emit} ${typeof x.on}`;
}
console.log("X:", probeX());

// same shape at module scope (the local is not referenced from any function,
// so it stays a plain init-time local — still a promotion candidate)
const moduleX = new X();
console.log("moduleX:", moduleX.a, typeof moduleX.emit, typeof moduleX.on);

// ── no declared field at all: the promoted set is simply empty ──
class NoField extends EventEmitter {}
function probeNoField(): string {
  const n = new NoField();
  return `${typeof n.emit} ${typeof n.on} ${typeof n.once}`;
}
console.log("NoField:", probeNoField());

// ── several fields, a post-construction write, and more of the surface ──
class Multi extends EventEmitter {
  count = 0;
  label = "start";
  flag = true;
}
function probeMulti(): string {
  const m = new Multi();
  m.count = 5;
  m.label = "changed";
  return `${m.count} ${m.label} ${m.flag} ${typeof m.once} ${typeof m.removeAllListeners} ${typeof m.listenerCount}`;
}
console.log("Multi:", probeMulti());

// ── node:stream bases install the same way ──
class R extends Readable {
  pushes = 0;
}
function probeR(): string {
  const r = new R();
  r.pushes = 3;
  return `${r.pushes} ${typeof r.on} ${typeof r.push} ${typeof r.pipe}`;
}
console.log("R:", probeR());

class W extends Writable {
  written = 0;
}
function probeW(): string {
  const w = new W();
  w.written = 4;
  return `${w.written} ${typeof w.on} ${typeof w.write} ${typeof w.end}`;
}
console.log("W:", probeW());

// ── the surface must be LIVE, not merely present ──
class Bus extends EventEmitter {
  seen = 0;
}
const bus = new Bus();
bus.on("ping", (v: number) => {
  bus.seen += v;
  console.log("bus listener got:", v);
});
console.log("bus emit:", bus.emit("ping", 42), "seen:", bus.seen);

// ── explicit constructor + super(): already escaped via `super`, keep working ──
class Ctor extends EventEmitter {
  seen = 0;
  constructor(start: number) {
    super();
    this.seen = start;
  }
}
function probeCtor(): string {
  const c = new Ctor(3);
  c.seen = 9;
  return `${c.seen} ${typeof c.emit}`;
}
console.log("Ctor:", probeCtor());

// ── an Error subclass (#573's sibling disqualifier) still works ──
class MyError extends Error {
  code = 42;
}
function probeErr(): string {
  const e = new MyError("boom");
  return `${e.message} ${e.code}`;
}
console.log("MyError:", probeErr());

// ── an INDIRECT native base: declared fields on both hops must survive ──
// (the emitter SURFACE on an indirect chain is a separate open gap — #6326 —
// so this probes fields only, which is what this issue is about.)
class Mid extends EventEmitter {
  mid = 7;
}
class Leaf extends Mid {
  leaf = 8;
}
function probeLeaf(): string {
  const l = new Leaf();
  l.leaf = 9;
  return `${l.mid} ${l.leaf}`;
}
console.log("Leaf:", probeLeaf());

// ── CONTROL: a plain class with no unmodeled base MUST still scalar-replace ──
class Point {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
  getX(): number {
    return this.x;
  }
}
function hot(n: number): number {
  let sum = 0;
  for (let i = 0; i < n; i++) {
    const p = new Point(i, i * 2);
    sum += p.getX() + p.y;
  }
  return sum;
}
console.log("plain class (scalar-replaced):", hot(5));

// ── CONTROL: a user-class chain is fully modeled — still scalar-replaced ──
class Base {
  base = 10;
}
class Derived extends Base {
  own = 20;
  total(): number {
    return this.base + this.own;
  }
}
function probeDerived(): string {
  const d = new Derived();
  return `${d.base} ${d.own} ${d.total()}`;
}
console.log("user chain:", probeDerived());
