// #10555: Web/runtime built-ins lack `Symbol.toStringTag`, so
// `Object.prototype.toString.call(x)` falls through to the generic
// `[object Object]` and `x[Symbol.toStringTag]` reads back `undefined`.
// Covers the full set the issue names plus adjacent built-ins that share the
// same fix shape: URL, URLSearchParams, Headers, Request, Response,
// FormData, Blob, AbortController, AbortSignal, TextEncoder, TextDecoder,
// EventTarget, Event. (Map/Promise/ArrayBuffer/DataView are deliberately out
// of scope -- see the PR body.)

function describe(name: string, ctor: any, value: unknown): void {
  const tagString = Object.prototype.toString.call(value);
  const ownTag = String((value as any)[Symbol.toStringTag]);
  const desc = Object.getOwnPropertyDescriptor(ctor.prototype, Symbol.toStringTag);
  console.log(
    name,
    tagString,
    ownTag,
    desc ? desc.value : "MISSING",
    desc ? desc.writable : "MISSING",
    desc ? desc.enumerable : "MISSING",
    desc ? desc.configurable : "MISSING",
  );
}

describe("URL", URL, new URL("http://x/"));
describe("URLSearchParams", URLSearchParams, new URLSearchParams("a=1"));
describe("Headers", Headers, new Headers());
describe("Request", Request, new Request("http://x/"));
describe("Response", Response, new Response("x"));
describe("FormData", FormData, new FormData());
describe("Blob", Blob, new Blob(["a"]));
describe("AbortController", AbortController, new AbortController());
describe("AbortSignal", AbortSignal, new AbortController().signal);
describe("TextEncoder", TextEncoder, new TextEncoder());
describe("TextDecoder", TextDecoder, new TextDecoder());
describe("EventTarget", EventTarget, new EventTarget());
describe("Event", Event, new Event("x"));

// The tag must never leak into JSON serialization (symbol keys never
// serialize -- a true invariant, kept here as a non-regression canary).
const u = new URL("http://x/");
console.log("json:", JSON.stringify({ tag: String(u[Symbol.toStringTag]) }));

// `typeof` must be unaffected by the new property (a pre-existing,
// unrelated `instanceof` gap for the generic-class-id representations this
// fix's own doc comment describes is out of scope for #10555 -- see the PR
// body).
console.log(
  "typeof:",
  typeof u,
  typeof new Headers(),
  typeof new AbortController(),
  typeof new EventTarget(),
);

// The descriptor is non-writable: a direct `Reflect.set` on the prototype
// object itself (no inheritance walk involved) must report failure without
// throwing, and must not change the value.
console.log("reflect-set:", Reflect.set(URL.prototype, Symbol.toStringTag, "Nope"));
console.log("still URL:", String(u[Symbol.toStringTag]));
