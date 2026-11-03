// Every super() form into node:assert AssertionError keeps the subclass
// instance: implicit constructor, explicit super(o), explicit super(...a),
// and the assert/strict export. No zlib import: the in-place init must work
// in a program that links no native-module hook.
import { AssertionError } from "assert";
import strictAssert from "assert/strict";
const H = (strictAssert as any).AssertionError;

class E extends AssertionError {}
class F extends AssertionError {
  tag: number;
  constructor(o: any) {
    super(o);
    this.tag = 1;
  }
}
class G extends AssertionError {
  constructor(...a: any[]) {
    super(...a);
  }
}
class I extends H {}

const opts = { message: "m", actual: 1, expected: 2, operator: "fail" };
function show(name: string, x: any, Sub: any) {
  console.log(name, x instanceof Sub, x instanceof AssertionError, x instanceof Error,
    Object.getPrototypeOf(x) === Sub.prototype, x.message, x.code, x.name, String(x), x.tag);
}
show("E", new E(opts), E);
show("F", new F(opts), F);
show("G", new (G as any)(opts), G);
show("I", new I(opts), I);
