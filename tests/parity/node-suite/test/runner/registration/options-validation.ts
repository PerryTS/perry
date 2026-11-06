import test from "node:test";

function codeOf(fn: () => void): string {
  try {
    fn();
    return "NO_THROW";
  } catch (error) {
    return (error as any).code ?? (error as Error).name;
  }
}

console.log("timeout string:", codeOf(() => test({ timeout: "1" as any })));
console.log("timeout negative:", codeOf(() => test({ timeout: -1 })));
console.log("concurrency string:", codeOf(() => test({ concurrency: "1" as any })));
console.log("concurrency zero:", codeOf(() => test({ concurrency: 0 })));
