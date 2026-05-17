export function touchModuleEvalPump() {
  return "loaded";
}

await globalThis.__perryAsyncTick();
Deno.core.ops.op_perry_print("module-pump: ready");
