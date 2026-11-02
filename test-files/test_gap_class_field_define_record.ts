// A public class field is CreateDataPropertyOrThrow with a Property Descriptor
// record: nothing is read through %Object.prototype%, on any receiver kind.
class Callable {
  constructor(fn: (...a: any[]) => any) {
    return Object.setPrototypeOf(fn.bind(this), new.target.prototype);
  }
}
class Node extends Callable {
  kind = "node";
  count = 0;
  constructor() {
    super((x: any) => x);
  }
}
class Plain {
  a = 1;
  b = "two";
}
function show(label: string, o: any, key: string) {
  const d = Object.getOwnPropertyDescriptor(o, key)!;
  console.log(label, key, JSON.stringify(o[key]), d.writable, d.enumerable, d.configurable, "get" in d);
}
const before = new Node() as any;
show("callable", before, "kind");
show("callable", before, "count");
(Object.prototype as any).get = function () {
  return "polluted";
};
(Object.prototype as any).set = function () {};
try {
  const n = new Node() as any;
  show("callable+pollution", n, "kind");
  console.log(typeof n, n(5), Object.keys(n).join(","));
  const p = new Plain() as any;
  show("plain+pollution", p, "a");
  show("plain+pollution", p, "b");
} catch (e: any) {
  console.log("threw", e.constructor.name);
}
delete (Object.prototype as any).get;
delete (Object.prototype as any).set;
const frozen = Object.freeze(() => 0);
class OnFrozen extends Callable {
  x = 1;
  constructor() {
    super(frozen);
  }
}
try {
  new OnFrozen();
  console.log("no throw");
} catch (e: any) {
  console.log("frozen target", e.constructor.name);
}
