// #6336: an IMMEDIATELY-CONSTRUCTED class expression — `new (class extends
// Base {})()` — never registered its `Subclass → Base` edge when `Base` was a
// builtin, so the instance came out parentless: `instanceof Base` was false and
// the inherited native surface read as `undefined`.
//
// The registry edge for a runtime-value parent is a SIDE EFFECT of evaluating
// the class expression: `lower_class_expr` sequences a
// `RegisterClassParentDynamic` in front of the `ClassRef` it yields, which is
// why the const-bound form (`const K = class extends Event {}`) worked. The
// callee-position form takes a different HIR path that lowered the class
// straight to a `New` — skipping the registration entirely. It now sequences the
// same registration in front of the construction.
//
// The class-chain native-base init (#6325/#6326) rides on the same fix: with the
// parent edge wired and the base reachable through the chain, an anonymous
// `class extends Map` / `class extends EventEmitter` gets its surface too.

import { EventEmitter } from "node:events";

// ── the issue's exact repro: builtin parent, constructed in place ──
const ev = new (class extends Event {})("tick");
console.log("anon Event:", ev instanceof Event, ev.type);

// ── const-bound control: the shape that already worked ──
const E2 = class extends Event {};
const e2 = new E2("bound");
console.log("bound Event:", e2 instanceof Event, e2.type);

// ── USER parent control: this shape always worked, and must keep working ──
class UserBase {
  hello(): string {
    return "hi";
  }
}
const p = new (class extends UserBase {})();
console.log("anon user parent:", p instanceof UserBase, typeof p.hello, p.hello());

// ── an anonymous subclass with its own members on top of the builtin base ──
const tagged = new (class extends Event {
  tag = "T";
  describe(): string {
    return this.tag + ":" + this.type;
  }
})("go");
console.log("anon Event members:", tagged instanceof Event, tagged.describe());

// ── the native-base surface reaches an anonymous subclass too ──
const anonEmitter = new (class extends EventEmitter {})();
console.log("anon emitter:", typeof anonEmitter.on, typeof anonEmitter.emit);
anonEmitter.on("hit", (v: number) => console.log("anon emitter got:", v));
console.log("anon emitter emit:", anonEmitter.emit("hit", 9));

const anonMap = new (class extends Map {})();
anonMap.set("k", 1);
console.log("anon map:", anonMap.get("k"), anonMap.size, anonMap instanceof Map);

// ── a named class expression in callee position behaves the same ──
const named = new (class MyEvent extends Event {})("named");
console.log("named class expr:", named instanceof Event, named.type);

// ── constructed in place inside a FUNCTION body (the mixin-ish shape) ──
function makeEvent(t: string): Event {
  return new (class extends Event {})(t);
}
const fromFn = makeEvent("fn");
console.log("from function:", fromFn instanceof Event, fromFn.type);
