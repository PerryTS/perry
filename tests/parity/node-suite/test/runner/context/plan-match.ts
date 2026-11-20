import test from "node:test";

test("matched assertion plan", (t) => {
  t.plan(1);
  t.assert.fileSnapshot(
    { ok: true },
    "tests/parity/node-suite/test/snapshots/file-object.json",
  );
  console.log("plan:matched-one");
});
