import test from "node:test";

test("context assertions", (t) => {
  console.log(
    "assertions:",
    ["ok", "strictEqual", "deepStrictEqual", "throws"]
      .map((name) => `${name}:${typeof (t.assert as any)[name]}`)
      .join(","),
  );
  t.assert.strictEqual(2 + 2, 4);
  console.log("assertions:passed");
});
