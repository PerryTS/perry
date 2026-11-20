// Object.defineProperty / Reflect.defineProperty on native fetch handle
// receivers (Headers, Request, Response). They are ordinary extensible objects:
// a new string or symbol key is accepted (the Next.js PATCHED_SET_HEADER shape),
// and only a non-configurable own property can reject a redefinition.
function attempt(label: string, f: () => unknown): void {
  try {
    console.log(label, String(f()));
  } catch (e: any) {
    console.log(label, "threw", e instanceof TypeError ? "TypeError" : String(e));
  }
}

const PATCHED = Symbol("patched-set-header");
const receivers: Array<[string, () => any]> = [
  ["headers", () => new Headers()],
  ["request", () => new Request("http://localhost/x")],
  ["response", () => new Response("body")],
];

for (const [name, make] of receivers) {
  const h = make();
  console.log(name, "extensible", Object.isExtensible(h), "sealed", Object.isSealed(h), "frozen", Object.isFrozen(h));

  attempt(name + " define string", () => {
    Object.defineProperty(h, "lane", { value: 43, configurable: true });
    return h.lane;
  });
  attempt(name + " define symbol", () => {
    Object.defineProperty(h, PATCHED, { value: true });
    return h[PATCHED];
  });
  attempt(name + " define getter", () => {
    Object.defineProperty(h, "computed", { get() { return 44; }, enumerable: true, configurable: true });
    return h.computed;
  });
  attempt(name + " reflect string", () => Reflect.defineProperty(make(), "k", { value: 2 }));
  attempt(name + " reflect symbol", () => Reflect.defineProperty(make(), PATCHED, { value: 2 }));

  // A non-configurable, non-writable expando keeps its spec invariants.
  attempt(name + " same value", () => Reflect.defineProperty(h, PATCHED, { value: true }));
  attempt(name + " reflect changed value", () => Reflect.defineProperty(h, PATCHED, { value: false }));
  attempt(name + " object changed value", () => {
    Object.defineProperty(h, PATCHED, { value: false });
    return "no throw";
  });
  attempt(name + " fixed string", () => {
    Object.defineProperty(h, "fixed", { value: 1 });
    return Reflect.defineProperty(h, "fixed", { value: 1 }) + "," + Reflect.defineProperty(h, "fixed", { value: 2 }) + "," + Reflect.defineProperty(h, "fixed", { configurable: true });
  });
  attempt(name + " object fixed string", () => {
    Object.defineProperty(h, "fixed", { enumerable: true });
    return "no throw";
  });
  attempt(name + " after", () => h.lane + "," + h[PATCHED] + "," + h.fixed);
}
