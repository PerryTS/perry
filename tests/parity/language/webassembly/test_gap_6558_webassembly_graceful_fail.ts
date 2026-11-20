// #6558: graceful-fail baseline for the WebAssembly global. Perry ships no
// WebAssembly engine by default; the namespace must be spec-SHAPED (every
// standard member present with the right type) and fail GRACEFULLY: async
// members reject with a CompileError, `validate` answers false, and
// `new WebAssembly.Module` throws CompileError synchronously — so lazy
// wasm-bindgen loaders (photon-node, @jsquash/webp, undici's llhttp probe)
// land in their own catch/fallback paths instead of crashing the process.
//
// The module bytes below are deliberately INVALID so node degrades through
// the exact same spec shapes and every line prints byte-identical output.
// Aliased namespace access on purpose: the literal `WebAssembly.compile(...)`
// spelling lowers to the wasmi host intrinsics (issue #76) and requires the
// wasm host archive at link time; the namespace VALUE is the path minified
// bundles take.
const WA: any = (globalThis as any).WebAssembly;
const garbage = new Uint8Array([0x01, 0x02, 0x03, 0x04]);

// Feature-detection pattern (presence probe + validate).
console.log("feature detect:", typeof WA !== "undefined");
console.log("validate:", WA.validate(garbage));
console.log(
  "detect pattern:",
  typeof WA !== "undefined" && WA.validate(garbage) ? "wasm" : "fallback",
);

// Member shapes: everything a loader might poke at exists as a function.
for (const name of [
  "compile",
  "instantiate",
  "compileStreaming",
  "instantiateStreaming",
  "validate",
  "Module",
  "Instance",
  "Memory",
  "Table",
  "Global",
  "CompileError",
  "LinkError",
  "RuntimeError",
]) {
  console.log(name + ":", typeof WA[name]);
}

// Synchronous constructor: CompileError, thrown (not returned, not crashed).
try {
  new WA.Module(garbage);
  console.log("Module: constructed");
} catch (e: any) {
  console.log("Module threw:", e instanceof Error, e instanceof WA.CompileError, e.name);
}

// Async members REJECT — observable as clean rejections.
try {
  await WA.compile(garbage);
  console.log("compile: resolved");
} catch (e: any) {
  console.log("compile rejected:", e instanceof Error, e instanceof WA.CompileError, e.name);
}
try {
  await WA.instantiate(garbage);
  console.log("instantiate: resolved");
} catch (e: any) {
  console.log("instantiate rejected:", e instanceof Error, e.name);
}

// Error classes construct branded instances.
const ce = new WA.CompileError("boom");
console.log("CompileError:", ce instanceof Error, ce instanceof WA.CompileError, ce.name, ce.message);

// Memory allocates a real ArrayBuffer (feature-detection code does this).
const mem = new WA.Memory({ initial: 1 });
console.log("Memory buffer:", mem.buffer instanceof ArrayBuffer, mem.buffer.byteLength);

// The photon-node loader pattern: lazy load, resolve null on failure, keep
// running.
async function lazyLoad(bytes: Uint8Array): Promise<unknown | null> {
  try {
    return await WA.compile(bytes);
  } catch {
    return null;
  }
}
const loaded = await lazyLoad(garbage);
console.log("loader result:", loaded === null ? "degraded" : "loaded");
console.log("program continues");
