// Issue #3558: computed class/object accessors must preserve ToPropertyKey,
// symbol keys, and static accessors.

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(msg);
}

function assertThrowsTypeError(fn: () => void, msg: string): void {
  let threw = false;
  try {
    fn();
  } catch (err) {
    threw = err instanceof TypeError && (err as any).constructor === TypeError;
  }
  assert(threw, msg);
}

const sym = Symbol("issue-3558");

class C {
  value: number;

  constructor() {
    this.value = 1;
  }

  get ["x"](): number {
    return this.value + 1;
  }

  set ["x"](v: number) {
    this.value = v;
  }

  get [sym](): number {
    return this.value + 10;
  }

  set [sym](v: number) {
    this.value = v * 2;
  }

  static get ["label"](): string {
    return (this as any)._label || "unset";
  }

  static set ["label"](v: string) {
    (this as any)._label = v;
  }

  static get [sym](): string {
    return "sym:" + (this as any).label;
  }
}

const c = new C();
assert(c["x"] === 2, "computed string getter");
c["x"] = 4;
assert(c["x"] === 5, "computed string setter");
assert((c as any)[sym] === 14, "computed symbol getter");
(c as any)[sym] = 3;
assert((c as any)[sym] === 16, "computed symbol setter");

assert((C as any)["label"] === "unset", "computed static string getter");
(C as any)["label"] = "ready";
assert((C as any)["label"] === "ready", "computed static string setter");
assert((C as any)[sym] === "sym:ready", "computed static symbol getter");

class DuplicateGetterComputedLast {
  get name(): string {
    throw new Error("named getter should be overwritten");
  }

  get ["name"](): string {
    return "computed";
  }
}
assert(new DuplicateGetterComputedLast().name === "computed", "computed getter overwrites named getter");

class DuplicateGetterNamedLast {
  get ["name"](): string {
    throw new Error("computed getter should be overwritten");
  }

  get name(): string {
    return "named";
  }
}
assert(new DuplicateGetterNamedLast().name === "named", "named getter overwrites computed getter");

let duplicateSetterCalls = 0;
class DuplicateSetterComputedLast {
  set value(_v: number) {
    throw new Error("named setter should be overwritten");
  }

  set ["value"](_v: number) {
    duplicateSetterCalls++;
  }
}
new DuplicateSetterComputedLast().value = 1;
assert(duplicateSetterCalls === 1, "computed setter overwrites named setter");

class DuplicateSetterNamedLast {
  set ["value"](_v: number) {
    throw new Error("computed setter should be overwritten");
  }

  set value(_v: number) {
    duplicateSetterCalls += 10;
  }
}
new DuplicateSetterNamedLast().value = 1;
assert(duplicateSetterCalls === 11, "named setter overwrites computed setter");

assertThrowsTypeError(function () {
  class BadStaticMethod {
    static ["prototype"]() {}
  }
  void BadStaticMethod;
}, "static computed prototype method throws");

assertThrowsTypeError(function () {
  class BadStaticAccessor {
    static get ["prototype"](): number {
      return 1;
    }
  }
  void BadStaticAccessor;
}, "static computed prototype accessor throws");

const obj = {
  hidden: 7,
  get [sym]() {
    return this.hidden + 1;
  },
  set [sym](v: number) {
    this.hidden = v;
  },
};
assert((obj as any)[sym] === 8, "object literal symbol getter");
(obj as any)[sym] = 12;
assert((obj as any)[sym] === 13, "object literal symbol setter");

console.log("issue-3558 ok");
