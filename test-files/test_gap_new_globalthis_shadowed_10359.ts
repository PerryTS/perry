// #10359 — `new globalThis.X(...)` must construct the GLOBAL `X` even when a
// module binding shadows the bare name. `globalThis.X` is the idiom for
// escaping exactly that shadow, but the qualified `new` lowered to a by-name
// construct that bound to the shadowing import/class/function/local — so it
// built the user class, while the aliased `const E = globalThis.Event;
// new E()` form was correct.
// @ts-nocheck
import {
  Event,
  Request,
  MessageChannel,
  Map,
  Int32Array,
  ReadableStream,
} from "./_helpers/new_globalthis_shadowed_10359.ts";

// The exact reproduction from the issue: an explicit import shadows `Event`.
const e: any = new globalThis.Event("ping");
console.log("tag:", e.tag);
console.log("type:", e.type);
console.log("ctor-name:", e.constructor?.name);
console.log("is-user-class:", e instanceof Event);

// The bare name is still legitimately the imported class, and the aliased
// form (always correct) stays correct.
console.log("bare:", (new Event() as any).tag);
const E = globalThis.Event;
console.log("aliased:", new E("pong").type);

// Fetch constructors took a separate by-name arm.
const r: any = new globalThis.Request("http://example.test/a");
console.log("request:", r.tag, r.url, r instanceof Request);

// So did MessageChannel.
const mc: any = new globalThis.MessageChannel();
console.log("channel:", mc.tag, typeof mc.port1, mc instanceof MessageChannel);
mc.port1.close();
mc.port2.close();

// A stream's methods come from codegen's builtin table, not from the
// runtime value of `globalThis.ReadableStream`.
const rs: any = new globalThis.ReadableStream();
console.log("stream:", rs.tag, typeof rs.getReader, rs instanceof ReadableStream);

// A constructor with a dedicated intrinsic node keeps constructing it.
const m: any = new globalThis.Map([[1, 2]]);
console.log("map:", m.tag, m.get(1), m.size);

// The multi-argument typed-array form falls past its dedicated node.
const ia: any = new globalThis.Int32Array(new ArrayBuffer(16), 4, 2);
console.log("int32:", ia.tag, ia.length, ia.byteOffset);

// A module-scope class and a function declaration shadow just like an import.
class Headers {
  tag = "user-Headers";
}
const h: any = new globalThis.Headers({ a: "1" });
console.log("headers:", h.tag, h.get("a"), h instanceof Headers);

function URLSearchParams(this: any) {
  this.tag = "user-URLSearchParams";
}
const usp: any = new globalThis.URLSearchParams("a=1&b=2");
console.log("usp:", usp.tag, usp.get("b"));

// A function-local binding shadows too.
function local() {
  const CustomEvent = function (this: any) {
    this.tag = "local-CustomEvent";
  };
  const ce: any = new globalThis.CustomEvent("x", { detail: 7 });
  console.log("local:", ce.tag, ce.detail, ce.type);
}
local();

// The same through a `globalThis` alias.
const g = globalThis;
const h2: any = new g.Headers({ b: "2" });
console.log("alias-headers:", h2.tag, h2.get("b"), h2 instanceof Headers);

// A global the program installs itself, shadowed by a module class of the same
// name: codegen folded the `globalThis.Widget` callee back onto the class.
class Widget {
  kind = "module-class";
}
globalThis.Widget = class {
  kind = "global-property";
};
console.log("widget:", new globalThis.Widget().kind, new Widget().kind);

// …and with no such global, the qualified construct must throw rather than
// quietly build the module class.
class Gadget {
  kind = "module-class";
}
try {
  const gadget: any = new globalThis.Gadget();
  console.log("gadget: constructed", gadget.kind);
} catch (err) {
  console.log("gadget: threw", err instanceof TypeError);
}

// Unshadowed forms are unaffected.
console.log("unshadowed:", new globalThis.CustomEvent("y", { detail: 9 }).detail);
