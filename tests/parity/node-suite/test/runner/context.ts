import test from "node:test";

test("context helpers", (t) => {
  console.log(
    [
      "name",
      "test",
      "diagnostic",
      "plan",
      "skip",
      "todo",
      "mock",
      "assert",
    ].map((key) => `${key}:${typeof (t as any)[key]}`).join(","),
  );
  t.diagnostic("hello");
});
