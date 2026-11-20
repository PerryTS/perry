import { mock } from "node:test";

function codeOf(fn: () => void): string {
  try {
    fn();
    return "NO_THROW";
  } catch (error) {
    return (error as any).code ?? (error as Error).name;
  }
}

console.log("fn original:", codeOf(() => mock.fn(1 as any)));
console.log("method target:", codeOf(() => mock.method(null as any, "x")));
console.log("method missing:", codeOf(() => mock.method({}, "x")));
console.log("getter missing:", codeOf(() => mock.getter({}, "x")));
console.log(
  "times zero:",
  codeOf(() => mock.fn(() => undefined, { times: 0 })),
);
console.log(
  "times fraction:",
  codeOf(() => mock.fn(() => undefined, { times: 1.5 })),
);
