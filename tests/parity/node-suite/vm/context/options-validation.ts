import * as vm from "node:vm";

function shape(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label + ": ok");
  } catch (error: any) {
    console.log(label + ":", error.name, error.code || "-");
  }
}

shape(
  "codeGeneration null",
  () => vm.createContext({}, { codeGeneration: null as any }),
);
shape(
  "strings type",
  () => vm.createContext({}, { codeGeneration: { strings: 1 as any } }),
);
shape(
  "wasm type",
  () => vm.createContext({}, { codeGeneration: { wasm: "no" as any } }),
);
shape(
  "microtask bad",
  () => vm.createContext({}, { microtaskMode: "bad" as any }),
);
shape(
  "microtask type",
  () => vm.createContext({}, { microtaskMode: 1 as any }),
);
shape(
  "name valid",
  () => vm.createContext({}, { name: "fixture" }),
);
shape(
  "origin valid",
  () => vm.createContext({}, { origin: "vm://fixture" }),
);
