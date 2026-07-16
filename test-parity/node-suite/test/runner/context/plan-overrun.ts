import test from "node:test";

test("exceeded assertion plan", (t) => {
  t.plan(0);
  console.log("plan:before-extra");
  t.assert.fileSnapshot(
    { ok: true },
    "test-parity/node-suite/test/snapshots/file-object.json",
  );
  console.log("plan:after-extra");
});
