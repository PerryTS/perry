import test, { snapshot } from "node:test";

const snapshotPath = "test-parity/node-suite/test/snapshots/basic.snapshot";
const fileSnapshotPath = "test-parity/node-suite/test/snapshots/file-object.json";

function codeOf(fn: () => void): string {
  try {
    fn();
    return "NO_THROW";
  } catch (err) {
    return (err as any).code ?? (err as Error).name;
  }
}

console.log(
  "serializers invalid:",
  codeOf(() => snapshot.setDefaultSnapshotSerializers(1 as any)),
);
console.log(
  "resolver invalid:",
  codeOf(() => snapshot.setResolveSnapshotPath(1 as any)),
);

snapshot.setResolveSnapshotPath(() => snapshotPath);

test("snap", (t) => {
  t.assert.snapshot({ b: 2, a: 1 });
  t.assert.fileSnapshot({ ok: true }, fileSnapshotPath);
  console.log("snapshot assertions: ok");
});
