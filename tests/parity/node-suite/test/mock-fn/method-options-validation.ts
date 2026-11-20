import { mock } from "node:test";

function codeOf(fn: () => void): string {
  try {
    fn();
    return "NO_THROW";
  } catch (error) {
    return (error as any).code ?? (error as Error).name;
  }
}

const target = { method() {} };
console.log("options number:", codeOf(() => mock.method(target, "method", () => {}, 5 as any)));
console.log(
  "getter setter:",
  codeOf(() => mock.method(target, "method", { getter: true, setter: true } as any)),
);
console.log("getter false:", codeOf(() => mock.getter(target, "method", { getter: false } as any)));
mock.restoreAll();
