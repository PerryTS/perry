import test from "node:test";

function codeOf(fn: () => void): string {
  try {
    fn();
    return "NO_THROW";
  } catch (error) {
    return (error as any).code ?? (error as Error).name;
  }
}

test("snapshot assertion validation", (t) => {
  console.log("snapshot options:", codeOf(() => (t.assert.snapshot as any)("value", null)));
  console.log("file path:", codeOf(() => (t.assert.fileSnapshot as any)({}, 5)));
  console.log(
    "file options:",
    codeOf(() => (t.assert.fileSnapshot as any)({}, "missing.snapshot", null)),
  );
});
