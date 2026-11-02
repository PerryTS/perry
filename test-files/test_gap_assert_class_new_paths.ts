// `new assert.Assert()` constructs through import, require and namespace.
import assert from "node:assert";
import * as ns from "node:assert";
import { createRequire } from "node:module";
const req = createRequire(import.meta.url);
const ra: any = req("assert");

function check(label: string, make: () => any): void {
  try {
    const a = make();
    console.log(label, typeof a, typeof a.strictEqual, a instanceof (assert as any).Assert);
    a.strictEqual(1, 1);
    try {
      a.strictEqual(1, 2);
    } catch (e: any) {
      console.log(label, "thrown", e.code, e instanceof Error);
    }
  } catch (e: any) {
    console.log(label, "throw", e && e.message);
  }
}
check("default", () => new (assert as any).Assert());
check("default-opts", () => new (assert as any).Assert({}));
check("namespace", () => new (ns as any).Assert());
check("require", () => new ra.Assert());
console.log("AssertionError.length", (assert as any).AssertionError.length);
try {
  assert.strictEqual(1, 2);
} catch (e: any) {
  console.log(String(e).split("\n")[0]);
}
