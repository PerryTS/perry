// Regression: `super(...)` skipped an intermediate user constructor when a
// grandparent in the chain was the native fetch `Request`.
//
// `js_fetch_or_value_super`'s stale-alias fallback walked the instance's
// WHOLE parent chain for a recorded Request/Response parent — so for
// `class Hint extends BaseRequest extends Request` (base in another module),
// the Hint's `super(input, init)` was classified as a DIRECT native-Request
// super: the runtime constructed the native handle and returned without ever
// invoking BaseRequest's constructor body. Its symbol-keyed internal state
// (`this[INTERNALS] = {...}`) never existed, so every getter read
// `undefined.<prop>` and threw.
//
// This is Next.js middleware's exact shape (NextRequestHint extends
// NextRequest extends Request): every request 500'd with "Cannot read
// properties of undefined (reading 'nextUrl')". The fix lets a parent value
// that resolves to a USER constructor (ClassRef / class object / closure)
// take the value-super dispatch — its own `super()` leg attaches the native
// handle when it reaches the builtin — and keeps the chain-walk fallback
// only for a stale/unresolvable alias of the builtin itself.
//
// Output is byte-identical to `node --experimental-strip-types`.

import { BaseRequest } from "./_helpers/cross_module_fetch_super_base.ts";

class RequestHint extends BaseRequest {
  sourcePage: string;
  constructor(params: { page: string; input: any; init: any }) {
    super(params.input, params.init);
    this.sourcePage = params.page;
  }
}

// Construct from a native Request instance (the middleware-adapter shape).
const native = new Request("https://x.test/de");
const hint = new RequestHint({ page: "/", input: native, init: {} });
console.log("sourcePage:", hint.sourcePage);
console.log("nextUrl:", hint.nextUrl);
console.log("srcUrl:", hint.srcUrl);
console.log("native method:", hint.method);

// String input takes the other super arm.
const hint2 = new RequestHint({ page: "/b", input: "https://y.test/en", init: {} });
console.log("hint2:", hint2.nextUrl, hint2.sourcePage);

// The direct subclass still works (its ctor IS the one that extends Request).
const base = new BaseRequest("https://z.test/fr", {});
console.log("base:", base.nextUrl, base.method);
