import { readFileSync } from "node:fs";
import { WASI } from "node:wasi";

const W: any = WASI;
const wasi = new W({ version: "preview1", returnOnExit: true });
if (typeof wasi.getImportObject !== "function") {
  console.log("getImportObject: unavailable");
} else {
  const result: any = await WebAssembly.instantiate(
    readFileSync("test-parity/node-suite/wasi/fixtures/exit-7-command.wasm"),
    wasi.getImportObject(),
  );
  const instance: any = result?.instance ?? result;
  console.log("instance available:", typeof instance?.exports === "object");
  if (instance?.exports) {
    console.log("exit code:", wasi.start(instance));
  }
}
