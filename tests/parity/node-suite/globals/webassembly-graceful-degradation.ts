// #6558 — graceful-fail baseline for the WebAssembly namespace.
//
// Perry ships no WebAssembly engine by default; the namespace is spec-shaped
// and fails GRACEFULLY: async members reject with a CompileError, `validate`
// answers false, `new WebAssembly.Module` throws CompileError synchronously,
// and `Memory` is minimally functional (real ArrayBuffer backing). Every
// line below prints identically under node, the default perry runtime, and
// the wasm-host (wasmi) runtime — which is why the module bytes used here
// are deliberately INVALID: node rejects them through the same spec shapes
// perry uses for "no engine".
//
// Aliased access on purpose: the literal `WebAssembly.compile(...)` spelling
// lowers to the wasmi host intrinsics (issue #76) and auto-links the wasm
// host archive; the namespace VALUE below is the path minified bundles and
// lazy wasm-bindgen loaders actually take.
const WA: any = (globalThis as any).WebAssembly;

// (c) Feature-detection pattern: presence probe + validate as the honest
// "can I run this module here" answer.
const garbage = new Uint8Array([0x01, 0x02, 0x03, 0x04]);
console.log("feature detect:", typeof WA !== "undefined");
console.log("validate garbage:", WA.validate(garbage));
console.log(
  "detect pattern:",
  typeof WA !== "undefined" && WA.validate(garbage) ? "wasm" : "fallback",
);

// `new WebAssembly.Module(bytes)` throws a CompileError synchronously.
try {
  new WA.Module(garbage);
  console.log("Module: constructed");
} catch (e: any) {
  console.log(
    "Module threw:",
    e instanceof Error,
    e instanceof WA.CompileError,
    e.name,
  );
}

// Async members REJECT — never crash, never hang, never throw sync.
try {
  await WA.compile(garbage);
  console.log("compile: resolved");
} catch (e: any) {
  console.log(
    "compile rejected:",
    e instanceof Error,
    e instanceof WA.CompileError,
    e.name,
  );
}

try {
  await WA.instantiate(garbage);
  console.log("instantiate: resolved");
} catch (e: any) {
  console.log(
    "instantiate rejected:",
    e instanceof Error,
    e instanceof WA.CompileError,
    e.name,
  );
}

// Streaming entry points also reject cleanly (reason type differs from
// node's — TypeError there, CompileError here — so only the Error-ness is
// asserted).
try {
  await WA.compileStreaming(garbage);
  console.log("compileStreaming: resolved");
} catch (e: any) {
  console.log("compileStreaming rejected:", e instanceof Error);
}

try {
  await WA.instantiateStreaming(garbage);
  console.log("instantiateStreaming: resolved");
} catch (e: any) {
  console.log("instantiateStreaming rejected:", e instanceof Error);
}

// Error constructors are real constructors: `new` and plain call both
// produce branded Error instances.
const ce = new WA.CompileError("boom");
console.log(
  "CompileError:",
  ce instanceof Error,
  ce instanceof WA.CompileError,
  ce instanceof WA.LinkError,
  ce.name,
  ce.message,
);
const le = new WA.LinkError();
console.log("LinkError:", le instanceof Error, le instanceof WA.LinkError, le.name, le.message === "");
const re = WA.RuntimeError("trap");
console.log("RuntimeError:", re instanceof Error, re instanceof WA.RuntimeError, re.name, re.message);

// Memory is minimally functional: a real zero-filled ArrayBuffer backs it,
// and grow() re-backs it (returning the old page count).
const mem = new WA.Memory({ initial: 1 });
console.log(
  "Memory:",
  mem instanceof WA.Memory,
  mem.buffer instanceof ArrayBuffer,
  mem.buffer.byteLength,
);
const view = new Uint8Array(mem.buffer);
console.log("Memory zeroed:", view[0], view[65535]);
console.log("Memory grow:", mem.grow(1), mem.buffer.byteLength);

// (b) The photon-node loader pattern: a lazy wasm loader that resolves null
// on failure. The program must observe a clean rejection and continue.
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
