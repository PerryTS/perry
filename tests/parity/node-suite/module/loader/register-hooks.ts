import * as Module from "node:module";

function errorLine(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(`${label}: no throw`);
  } catch (error: any) {
    console.log(`${label}:`, error.name, error.code ?? "no-code");
  }
}

function summarizeHandle(label: string, handle: any) {
  console.log(`${label} keys:`, Object.keys(handle).sort().join(","));
  console.log(
    `${label} resolve/load:`,
    typeof handle.resolve,
    String(handle.resolve),
    typeof handle.load,
    String(handle.load),
  );
  console.log(
    `${label} deregister:`,
    typeof handle.deregister,
    handle.deregister.length,
    handle.deregister.name,
    String(handle.deregister()),
  );
}

console.log(
  "registerHooks shape:",
  typeof Module.registerHooks,
  Module.registerHooks.length,
  Module.registerHooks.name,
);

summarizeHandle("empty", Module.registerHooks({}));

const full = Module.registerHooks({
  resolve(specifier: string, context: unknown, nextResolve: Function) {
    return nextResolve(specifier, context);
  },
  load(url: string, context: unknown, nextLoad: Function) {
    return nextLoad(url, context);
  },
});
console.log(
  "full types:",
  typeof full.resolve,
  full.resolve.length,
  full.resolve.name,
  typeof full.load,
  full.load.length,
  full.load.name,
);
console.log("full deregister:", String(full.deregister()));

const capturedRegisterHooks = Module.registerHooks;
const nulls = capturedRegisterHooks({ resolve: null, load: null });
console.log(
  "null hooks:",
  Object.keys(nulls).sort().join(","),
  String(nulls.resolve),
  String(nulls.load),
  typeof nulls.deregister,
);

errorLine("missing hooks", () => Module.registerHooks());
errorLine("null hooks arg", () => Module.registerHooks(null as any));
errorLine("bad resolve", () => Module.registerHooks({ resolve: 1 as any }));
errorLine("bad load", () => Module.registerHooks({ load: "x" as any }));
