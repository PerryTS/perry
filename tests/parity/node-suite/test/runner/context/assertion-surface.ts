import test from "node:test";

test("assertion surface", (t) => {
  const names = [
    "deepEqual",
    "deepStrictEqual",
    "doesNotMatch",
    "doesNotReject",
    "doesNotThrow",
    "equal",
    "fail",
    "ifError",
    "match",
    "notDeepEqual",
    "notStrictEqual",
    "ok",
    "rejects",
    "strictEqual",
    "throws",
    "fileSnapshot",
    "snapshot",
  ];
  console.log(
    "assertion surface:",
    names.map((name) => `${name}:${typeof (t.assert as any)[name]}`).join(","),
  );
});
