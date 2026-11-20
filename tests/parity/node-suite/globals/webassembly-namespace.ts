const WA: any = (globalThis as any).WebAssembly;

function showDesc(label: string, obj: any, key: string) {
  const desc = Object.getOwnPropertyDescriptor(obj, key);
  console.log(
    label + ":",
    !!desc,
    desc?.writable,
    desc?.enumerable,
    desc?.configurable,
  );
}

console.log("typeof namespace:", typeof WA);
console.log("global identity:", WA === WebAssembly);
showDesc("global desc", globalThis, "WebAssembly");

console.log("keys:", Object.keys(WA).join(","));

for (const name of [
  "compile",
  "validate",
  "instantiate",
  "compileStreaming",
  "instantiateStreaming",
  "promising",
]) {
  const fn = WA[name];
  console.log(`${name}:`, typeof fn, fn?.name, fn?.length);
  showDesc(`${name} desc`, WA, name);
}

for (const name of [
  "Module",
  "Instance",
  "Memory",
  "Table",
  "Global",
  "CompileError",
  "LinkError",
  "RuntimeError",
]) {
  const ctor = WA[name];
  console.log(`${name}:`, typeof ctor, ctor?.name, ctor?.length);
  showDesc(`${name} desc`, WA, name);
}

console.log(
  "Module statics:",
  typeof WA.Module.exports,
  WA.Module.exports.name,
  WA.Module.exports.length,
  typeof WA.Module.imports,
  WA.Module.imports.name,
  WA.Module.imports.length,
  typeof WA.Module.customSections,
  WA.Module.customSections.name,
  WA.Module.customSections.length,
);

console.log(
  "Module proto:",
  Object.getOwnPropertyNames(WA.Module.prototype).join(","),
);
console.log(
  "Instance proto:",
  Object.getOwnPropertyNames(WA.Instance.prototype).join(","),
);
console.log(
  "CompileError proto:",
  Object.getOwnPropertyNames(WA.CompileError.prototype).join(","),
);
