// #6326: an INDIRECT subclass of a native base inherited nothing.
//
// Perry models `EventEmitter` (and the `node:stream` classes) by stamping the
// base's method surface onto the INSTANCE at construction time. That init only
// fired when the class named the base LITERALLY in its own `extends` clause, so
// `class B extends EventEmitter {}` + `class D extends B {}` lost the emitter
// surface entirely one hop away — `typeof d.on` was `undefined`.
//
// The trigger is now "the class CHAIN reaches the native base", walking through
// ctor-less user classes exactly the way `node_stream_parent_kind` already did
// for the stream bases. A user class WITH its own constructor stops the walk:
// its `super()` performs the init itself, and doing it twice would re-stamp the
// surface over a live emitter.

import { EventEmitter } from "node:events";

// ── the issue's exact repro: one hop away, no constructors anywhere ──
class B extends EventEmitter {}
class D extends B {}
const d = new D();
console.log("typeof d.on:", typeof d.on, "typeof d.emit:", typeof d.emit);
d.on("ping", (v: number) => console.log("d listener got:", v));
console.log("d emit returned:", d.emit("ping", 42));

// ── deep chain: D2 → C2 → B → EventEmitter ──
class C2 extends B {}
class D2 extends C2 {}
const d2 = new D2();
console.log("deep typeof:", typeof d2.on, typeof d2.emit);
d2.on("deep", (v: string) => console.log("d2 listener got:", v));
console.log("deep emit returned:", d2.emit("deep", "hello"));

// ── indirect subclass WITH its own constructor calling super() ──
class Counter extends B {
  seen: number;
  constructor(start: number) {
    super();
    this.seen = start;
  }
  bump(): void {
    this.seen++;
    this.emit("bump", this.seen);
  }
}
const counter = new Counter(10);
console.log("counter typeof:", typeof counter.on, "seen:", counter.seen);
counter.on("bump", (n: number) => console.log("counter bumped to:", n));
counter.bump();

// ── an intermediate class WITH a constructor: its super() does the init, and
//    the leaf must NOT double-install over it (listeners registered by the
//    intermediate's ctor survive) ──
class Seeded extends EventEmitter {
  constructor() {
    super();
    this.on("seeded", (v: string) => console.log("seeded listener got:", v));
  }
}
class SeededLeaf extends Seeded {}
const sl = new SeededLeaf();
console.log("seeded leaf typeof:", typeof sl.emit);
console.log("seeded leaf emit returned:", sl.emit("seeded", "kept"));

// ── instance fields on both levels still initialize, and the emitter surface
//    installed underneath them works ──
//
// (Deliberately EXERCISED, not just `typeof`-probed: a bare `typeof lf.on`
// value-read next to a declared-field read lets scalar replacement promote the
// instance to scalars — it never escapes — and a runtime-INSTALLED own property
// is invisible to that promotion, so the read comes back `undefined`. That is a
// pre-existing gap independent of the native base, and it reproduces on `main`
// for a DIRECT `class X extends EventEmitter { a = 1 }` too.)
class WithField extends EventEmitter {
  base = "base";
}
class LeafField extends WithField {
  leaf = "leaf";
}
const lf = new LeafField();
lf.on("field", (v: string) => console.log("field listener got:", v));
console.log("fields:", lf.base, lf.leaf, typeof lf.on);
console.log("fields emit returned:", lf.emit("field", "ok"));

// ── an indirect subclass OVERRIDE still wins over the native base, and
//    `super.<m>()` reaches the base (#6316 / #6322 ordering discipline) ──
class Loud extends B {
  emit(ev: string, ...args: any[]): boolean {
    console.log("Loud.emit override ran:", ev);
    return super.emit(ev, ...args);
  }
}
const loud = new Loud();
loud.on("x", (v: number) => console.log("loud listener got:", v));
console.log("loud emit returned:", loud.emit("x", 7));

// ── direct-subclass control: unchanged behavior ──
class Plain extends EventEmitter {}
const plain = new Plain();
plain.on("go", (v: number) => console.log("plain listener got:", v));
console.log("plain emit returned:", plain.emit("go", 3));

// ── listenerCount / removeAllListeners reach the base through the chain ──
console.log("d listenerCount:", d.listenerCount("ping"));
d.removeAllListeners("ping");
console.log("d after removeAll:", d.listenerCount("ping"));

// ── ctor-less INTERMEDIATE classes must still run their field initializers ──
// The native base is the chain root and has no TS fields, so every class after
// it needs initializing — including intermediates that write no `super()` of
// their own. A middle class (`FieldB` below) was left uninitialized when the
// post-super() pass only covered the leaf. Two intermediates are required to
// expose it: with one, the leaf and the root-adjacent class both happen to be
// covered. (Reported by review on #6342.)
class FieldA extends EventEmitter {
  aField = "a-ok";
}
class FieldB extends FieldA {
  bField = "b-ok";
}
class FieldC extends FieldB {
  cField = "c-ok";
  constructor() {
    super();
  }
}
const fc = new FieldC();
console.log("chain fields:", fc.aField, fc.bField, fc.cField);
console.log("chain emit:", typeof fc.emit);
