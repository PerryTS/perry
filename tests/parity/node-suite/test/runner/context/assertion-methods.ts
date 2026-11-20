import test from "node:test";

test("assertion methods", (t) => {
  t.assert.ok(true);
  t.assert.deepStrictEqual({ value: [1, 2] }, { value: [1, 2] });
  t.assert.match("node:test", /^node:/);
  t.assert.throws(() => {
    throw new TypeError("controlled assertion error");
  }, TypeError);
  console.log("assertion methods:passed");
});
