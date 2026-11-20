// Issue #6003 — a user-defined `class Headers` was conflated with the native
// fetch Headers: `const h = new Headers()` tagged `h` as a native fetch
// instance by name alone, so `h.set(...)` dispatched through the Headers FFI
// (silently skipping the user's method) and every property the method stored
// vanished. The same lexical-shadowing gap covered the sibling reserved
// names (`Request`, `Response`, ...) and the `instanceof` reserved-id map.

class Headers {
  set(o: Record<string, string>): void {
    const self: any = this;
    for (const k of Object.keys(o)) self[k] = String(o[k]);
  }
  get(name: string): string {
    return (this as any)[name] ?? "<missing>";
  }
}

const h = new Headers();
h.set({ "x-one": "1", two: "2", "x-three": "3" });
console.log("KEYS", Object.keys(h).join("|"));
console.log("GET", h.get("two"));

// Inline chained call on the construction — no intermediate binding — must
// also resolve to the user class, not the fetch FFI chain dispatch.
console.log("CHAIN", new Headers().get("absent"));

// `instanceof` with the shadowed bare name must test against the USER class
// id, not the reserved native-fetch-Headers class id.
console.log("INSTANCEOF", h instanceof Headers);

// A typed parameter (`(h: Headers)`) previously re-tagged the param as a
// native fetch instance inside the callee, re-breaking method dispatch.
function fill(target: Headers): string {
  target.set({ filled: "yes" });
  return target.get("filled");
}
console.log("PARAM", fill(new Headers()));

// Same shape through an ARROW function — the arrow-param registration path
// (expr_function.rs) is separate from the fn-decl one exercised above.
const fillArrow = (target: Headers): string => {
  target.set({ arrow: "also-yes" });
  return target.get("arrow");
};
console.log("ARROW", fillArrow(new Headers()));

// Sibling reserved name: a user `class Response` collides identically.
// `text` is a reserved NATIVE Response method name — calling it through a
// let-bound instance checks that the construction-site native tagging (not
// just inline chaining) backs off for the shadowed name.
class Response {
  private code = 0;
  set(c: number): void {
    this.code = c;
  }
  get status(): number {
    return this.code;
  }
  text(): string {
    return `user-text:${this.code}`;
  }
}
const r = new Response();
r.set(204);
console.log("RESPONSE", r.status, r instanceof Response);
console.log("TEXT", r.text());
