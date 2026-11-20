import * as Module from "node:module";

function errorLine(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(`${label}: no throw`);
  } catch (error: any) {
    console.log(`${label}:`, error.name, error.code ?? "no-code");
  }
}

console.log("register shape:", typeof Module.register, Module.register.length, Module.register.name);

const loader =
  "data:text/javascript,export async function resolve(specifier, context, nextResolve) { return nextResolve(specifier, context); }";
console.log("register data:", String(Module.register(loader, import.meta.url)));

const capturedRegister = Module.register;
console.log("captured register data:", String(capturedRegister(loader, import.meta.url)));

errorLine("invalid specifier", () => Module.register("not a real url %", import.meta.url));
