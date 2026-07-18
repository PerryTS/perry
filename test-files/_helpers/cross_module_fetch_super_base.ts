// Helper for test_gap_cross_module_fetch_subclass_super.ts — the
// next/dist/server/web NextRequest shape: a class extending the NATIVE
// `Request`, storing symbol-keyed internal state in its constructor and
// exposing it through getters. Lives in its own module so the subclass's
// `super(...)` crosses a module boundary (value-super dispatch).
const INTERNALS = Symbol("internal request");

export class BaseRequest extends Request {
  constructor(input: any, init: any = {}) {
    const url = typeof input !== "string" && "url" in input ? input.url : String(input);
    if (input instanceof Request) {
      super(input, init);
    } else {
      super(url, init);
    }
    (this as any)[INTERNALS] = { nextUrl: "NU:" + url, srcUrl: url };
  }
  get nextUrl(): string {
    return (this as any)[INTERNALS].nextUrl;
  }
  get srcUrl(): string {
    return (this as any)[INTERNALS].srcUrl;
  }
}
