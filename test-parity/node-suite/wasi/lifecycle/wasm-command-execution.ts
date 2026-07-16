import { readFileSync } from "node:fs";
import { WASI } from "node:wasi";

const W: any = WASI;
const result: any = await WebAssembly.instantiate(
  readFileSync("test-parity/node-suite/wasi/fixtures/counter-command.wasm"),
);
const instance: any = result?.instance ?? result;
const exportsObject: any = instance?.exports;
console.log("exports object:", typeof exportsObject === "object");
if (exportsObject && typeof exportsObject === "object") {
  const memory = new Uint8Array(exportsObject.memory.buffer);
  const wasi = new W({ version: "preview1" });
  console.log("before:", memory[0]);
  console.log("return:", wasi.start(instance));
  console.log("after:", new Uint8Array(exportsObject.memory.buffer)[0]);
}
