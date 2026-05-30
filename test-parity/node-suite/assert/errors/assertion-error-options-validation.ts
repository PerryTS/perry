import assert from "node:assert";

// #3034 — `new assert.AssertionError(options)` requires an options object and
// generates a Node-compatible default message from `actual`/`operator`/
// `expected` when `message` is omitted.
const cases: unknown[] = [
  undefined,
  null,
  1,
  "x",
  {},
  { actual: 1, expected: 2, operator: "===", message: "bad" },
  { actual: 1, expected: 2, operator: "===" },
];

for (const opt of cases) {
  try {
    const e = new assert.AssertionError(opt as object) as Error & {
      actual?: unknown;
      expected?: unknown;
      operator?: unknown;
      generatedMessage?: boolean;
      code?: string;
    };
    console.log(
      String(opt),
      "OK",
      e.message,
      e.actual,
      e.expected,
      e.operator,
      e.generatedMessage,
      e.name,
      e.code,
      e instanceof Error,
    );
  } catch (err) {
    const e = err as Error & { code?: string };
    console.log(String(opt), "THROW", e.name, e.code, e.message.split("\n")[0]);
  }
}
