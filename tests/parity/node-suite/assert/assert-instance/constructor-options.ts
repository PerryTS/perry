import assert from "node:assert";
import strictAssert from "node:assert/strict";

class LeftBox {
  value: number;

  constructor(value: number) {
    this.value = value;
  }
}

class RightBox {
  value: number;

  constructor(value: number) {
    this.value = value;
  }
}

function show(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label + ": ok");
  } catch (err: any) {
    console.log(label + ":", err?.name, err?.code, err?.operator);
  }
}

const strictInstance = new assert.Assert();
const looseInstance = new assert.Assert({ strict: false });
const skipPrototypeInstance = new assert.Assert({ skipPrototype: true });

console.log(
  "Assert types:",
  typeof assert.Assert,
  typeof strictAssert.Assert,
  typeof assert.strict.Assert,
);
console.log(
  "Assert same:",
  assert.Assert === strictAssert.Assert,
  assert.strict.Assert === assert.Assert,
);
console.log("instance methods:", typeof strictInstance.equal, typeof looseInstance.deepStrictEqual);

show("Assert call without new", () => (assert.Assert as any)());
show("strict instance equal", () => strictInstance.equal(1, "1"));
show("loose instance equal", () => looseInstance.equal(1, "1"));
show("strict instance deepEqual", () => {
  strictInstance.deepEqual(new LeftBox(1), new RightBox(1));
});
show("loose instance deepStrictEqual", () => {
  looseInstance.deepStrictEqual(new LeftBox(1), new RightBox(1));
});
show("skipPrototype deepStrictEqual", () => {
  skipPrototypeInstance.deepStrictEqual(new LeftBox(1), new RightBox(1));
});
