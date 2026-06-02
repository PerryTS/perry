// node:vm measureMemory result shape, validation, and warning behavior.
import * as vm from "node:vm";

const warnings: string[] = [];
process.on("warning", (warning: any) => {
  warnings.push(`${warning.name}:${warning.message}`);
});

function showMemoryEntry(label: string, entry: any): void {
  console.log(`${label} keys:`, Object.keys(entry).join(","));
  console.log(`${label} estimate type:`, typeof entry.jsMemoryEstimate);
  console.log(
    `${label} range shape:`,
    Array.isArray(entry.jsMemoryRange),
    entry.jsMemoryRange.length,
    entry.jsMemoryRange.every((value: unknown) => typeof value === "number"),
  );
}

function showWasm(label: string, wasm: any): void {
  console.log(`${label} wasm keys:`, Object.keys(wasm).join(","));
  console.log(`${label} wasm types:`, typeof wasm.code, typeof wasm.metadata);
}

function expectThrow(label: string, expectedCode: string, expectedMessage: string, fn: () => unknown): void {
  try {
    fn();
    console.log(`${label}:`, "no throw");
  } catch (err: any) {
    console.log(
      `${label}:`,
      err?.name,
      err?.code === expectedCode,
      err?.message === expectedMessage,
    );
    console.log(`${label} message:`, err?.message);
  }
}

const promise = vm.measureMemory({ execution: "eager" });
console.log("promise shape:", promise instanceof Promise, typeof (promise as any).then);

const summary = await promise;
console.log("summary keys:", Object.keys(summary).join(","));
showMemoryEntry("summary total", summary.total);
showWasm("summary", (summary as any).WebAssembly);

const detailed = await vm.measureMemory({ mode: "detailed", execution: "eager" });
console.log("detailed keys:", Object.keys(detailed).join(","));
showMemoryEntry("detailed total", detailed.total);
showMemoryEntry("detailed current", (detailed as any).current);
console.log(
  "detailed other:",
  Array.isArray((detailed as any).other),
  (detailed as any).other.length,
);
showWasm("detailed", (detailed as any).WebAssembly);

expectThrow(
  "invalid mode",
  "ERR_INVALID_ARG_VALUE",
  "The property 'options.mode' must be one of: 'summary', 'detailed'. Received 'bad'",
  () => vm.measureMemory({ mode: "bad" } as any),
);
expectThrow(
  "invalid execution",
  "ERR_INVALID_ARG_VALUE",
  "The property 'options.execution' must be one of: 'default', 'eager'. Received 'bad'",
  () => vm.measureMemory({ execution: "bad" } as any),
);
expectThrow(
  "null options",
  "ERR_INVALID_ARG_TYPE",
  'The "options" argument must be of type object. Received null',
  () => vm.measureMemory(null as any),
);

setImmediate(() => {
  console.log("warnings:", warnings.length, warnings.join("|"));
});
