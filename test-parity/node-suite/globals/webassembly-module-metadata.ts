function show(label: string, value: unknown) {
  console.log(`${label}: ${String(value)}`);
}

const addBytes = new Uint8Array([
  0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,
  0x01, 0x07, 0x01, 0x60, 0x02, 0x7f, 0x7f, 0x01,
  0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x07, 0x01,
  0x03, 0x61, 0x64, 0x64, 0x00, 0x00, 0x0a, 0x09,
  0x01, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6a,
  0x0b,
]);

const importBytes = new Uint8Array([
  0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,
  0x01, 0x06, 0x01, 0x60, 0x01, 0x7f, 0x01, 0x7f,
  0x02, 0x09, 0x01, 0x03, 0x65, 0x6e, 0x76, 0x01,
  0x66, 0x00, 0x00,
]);

const customBytes = new Uint8Array([
  0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,
  0x00, 0x08, 0x04, 0x6d, 0x65, 0x74, 0x61, 0x01,
  0x02, 0x03,
]);

function exportSummary(module: WebAssembly.Module) {
  return WebAssembly.Module.exports(module)
    .map((entry) => `${entry.name}:${entry.kind}`)
    .join(",");
}

function importSummary(module: WebAssembly.Module) {
  return WebAssembly.Module.imports(module)
    .map((entry) => `${entry.module}.${entry.name}:${entry.kind}`)
    .join(",");
}

show("validate add", WebAssembly.validate(addBytes));

const addModule = new WebAssembly.Module(addBytes);
show("exports add", exportSummary(addModule));

const importModule = new WebAssembly.Module(importBytes);
show("imports fn", importSummary(importModule));

const customModule = new WebAssembly.Module(customBytes);
const sections = WebAssembly.Module.customSections(customModule, "meta");
show("custom meta count", sections.length);
show("custom meta bytes", Array.from(new Uint8Array(sections[0])).join(","));
show("custom missing count", WebAssembly.Module.customSections(customModule, "none").length);

const compiled = await WebAssembly.compile(addBytes);
show("compile exports", exportSummary(compiled));

// A constructed / compiled module is recognized by `instanceof
// WebAssembly.Module`, and is NOT a sibling constructor's instance.
show("module instanceof Module", addModule instanceof WebAssembly.Module);
show("compiled instanceof Module", compiled instanceof WebAssembly.Module);
show("module not instanceof Memory", addModule instanceof WebAssembly.Memory);

// A plain object that merely copies the internal brand fields is NOT a
// module — the brand is a live host pointer this runtime handed out, not a
// user-writable string/number property.
const forged = { __wasmKind: "module", __wasmModulePtr: 1 } as unknown;
show("forged not instanceof Module", forged instanceof WebAssembly.Module);

// `WebAssembly.Memory` instances are recognized too (they link
// `Memory.prototype` through the construct path).
const memory = new WebAssembly.Memory({ initial: 1 });
show("memory instanceof Memory", memory instanceof WebAssembly.Memory);
show("memory not instanceof Module", memory instanceof WebAssembly.Module);
