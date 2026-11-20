// Issue #3559: computed keys must evaluate in source order and ToPropertyKey
// must use the string hint for non-symbol keys.

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(msg);
}

const events: string[] = [];
const keyObject = {
  [Symbol.toPrimitive](hint: string) {
    events.push("toPrimitive:" + hint);
    return "coerced";
  },
  toString() {
    events.push("toString");
    return "wrong";
  },
};

const obj = {
  [(events.push("first"), "x")]: 1,
  [keyObject as any]: 2,
  get [(events.push("getter-key"), "g")]() {
    events.push("getter-call");
    return 3;
  },
  ...(events.push("spread"), { y: 4 }),
  [(events.push("last"), "x")]: 5,
};

assert(events.join(",") === "first,toPrimitive:string,getter-key,spread,last", "object key evaluation order");
assert(obj["x"] === 5, "duplicate computed key overwrites");
assert((obj as any)["coerced"] === 2, "ToPropertyKey uses string-hint primitive conversion");
assert(obj["y"] === 4, "spread remains in source order");
assert(obj["g"] === 3, "computed getter installed");
assert(events.join(",") === "first,toPrimitive:string,getter-key,spread,last,getter-call", "getter body runs on read");

const classEvents: string[] = [];
class C {
  [(classEvents.push("method"), "m")](): string {
    return "m";
  }

  get [(classEvents.push("getter"), "p")](): string {
    return "p";
  }
}

const c = new C();
assert(classEvents.join(",") === "method,getter", "class computed keys evaluate in source order");
assert(c["m"]() === "m" && (c as any)["p"] === "p", "class computed members installed");

console.log("issue-3559 ok");
