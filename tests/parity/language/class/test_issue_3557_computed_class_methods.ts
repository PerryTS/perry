// Issue #3557: computed class methods must register as real prototype/static
// methods, not per-instance fields, and symbol/generator keys must survive.

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(msg);
}

const events: string[] = [];
const sym = Symbol("issue-3557");

function key(label: string, value: string): string {
  events.push(label);
  return value;
}

class C {
  n: number;

  constructor() {
    this.n = 5;
  }

  [key("inst", "run")](): string {
    return "run:" + this.n;
  }

  static [key("static", "build")](): string {
    return "static:" + this.name;
  }

  [sym](): string {
    return "symbol:" + this.n;
  }

  *[key("gen", "values")](): Generator<number> {
    yield this.n;
    yield this.n + 1;
  }
}

const c = new C();
assert(events.join(",") === "inst,static,gen", "computed class keys evaluated in source order");
assert(c["run"]() === "run:5", "computed instance method dispatch");
assert(C["build"]() === "static:C", "computed static method dispatch");
assert((c as any)[sym]() === "symbol:5", "computed symbol method dispatch");
const values = (c as any)["values"]();
const first = values.next();
const second = values.next();
const third = values.next();
assert(first.value === 5, "computed generator method first yield: " + first.value);
assert(second.value === 6, "computed generator method second yield: " + second.value);
assert(third.done === true, "computed generator method completion");
assert(Object.keys(c).indexOf("run") === -1, "computed methods are not enumerable instance fields");

console.log("issue-3557 ok");
