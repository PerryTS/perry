// parity-node-argv: --no-warnings
// node:vm measureMemory result shape and option validation.
import * as vm from "node:vm";

function errorShape(label: string, fn: () => void) {
  try {
    fn();
    console.log(label + ":", "ok");
  } catch (error: any) {
    console.log(label + ":", error.name, error.code || "-");
  }
}

function entryShape(label: string, entry: any) {
  console.log(
    label + ":",
    typeof entry.jsMemoryEstimate,
    Array.isArray(entry.jsMemoryRange),
    entry.jsMemoryRange.length,
    entry.jsMemoryRange.map((value: unknown) => typeof value).join(","),
  );
}

console.log(
  "measure function:",
  typeof vm.measureMemory,
  vm.measureMemory.length,
);
console.log("measure promise:", typeof vm.measureMemory().then);

const summary: any = await vm.measureMemory({ execution: "eager" });
console.log("summary keys:", Object.keys(summary).join(","));
entryShape("summary total", summary.total);
console.log(
  "summary wasm:",
  Object.keys(summary.WebAssembly).join(","),
  typeof summary.WebAssembly.code,
  typeof summary.WebAssembly.metadata,
);

const detailed: any = await vm.measureMemory({
  mode: "detailed",
  execution: "eager",
});
console.log("detailed keys:", Object.keys(detailed).join(","));
entryShape("detailed total", detailed.total);
entryShape("detailed current", detailed.current);
console.log(
  "detailed other:",
  Array.isArray(detailed.other),
  detailed.other.length,
);

errorShape("mode validation", () => {
  vm.measureMemory({ mode: "bad" as any });
});

errorShape("execution validation", () => {
  vm.measureMemory({ execution: "bad" as any });
});

errorShape("null validation", () => {
  vm.measureMemory(null as any);
});

errorShape("number validation", () => {
  vm.measureMemory(1 as any);
});

errorShape("array validation", () => {
  vm.measureMemory([] as any);
});
