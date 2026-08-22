// Resolve the namespace through a computed key so this test deliberately does
// not trigger the compiler's static WebAssembly host auto-linking. It checks
// the default runtime's honest graceful degradation instead.
const WA: any = (globalThis as any)["Web" + "Assembly"];
const validAdd = new Uint8Array([
  0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,
  0x01, 0x07, 0x01, 0x60, 0x02, 0x7f, 0x7f, 0x01,
  0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x07, 0x01,
  0x03, 0x61, 0x64, 0x64, 0x00, 0x00, 0x0a, 0x09,
  0x01, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6a,
  0x0b,
]);
console.log("validate valid:", WA.validate(validAdd));
try {
  await WA.compile(validAdd);
  console.log("compile: resolved");
} catch (e: any) {
  console.log("compile rejected:", e instanceof WA.CompileError, e.name);
  console.log("mentions api:", e.message.includes("WebAssembly.compile"));
  console.log("mentions issue:", e.message.includes("6558"));
}
try {
  new WA.Module(validAdd);
  console.log("Module: constructed");
} catch (e: any) {
  console.log("Module threw:", e instanceof WA.CompileError, e.name);
  console.log("Module mentions issue:", e.message.includes("6558"));
}
async function lazyLoad(bytes: Uint8Array): Promise<unknown | null> {
  try { return await WA.instantiate(bytes); } catch { return null; }
}
const loaded = await lazyLoad(validAdd);
console.log("loader result:", loaded === null ? "degraded" : "loaded");
console.log("program continues");
