import { readFileSync } from "node:fs";
import { collect } from "perry/gc";
import { runComponentCommand } from "perry/component";
import { load } from "./generated/bindings";

const command = readFileSync(process.argv[2]);
const library = readFileSync(process.argv[3]);
let calls = 0;
const instance = load(library, {
  "perry:transform/logging@1.0.0": {
    log(message: string): void {
      collect();
      if (message !== "héllo 🌍") throw new Error("library callback value lost");
      const status = runComponentCommand(command, {
        wasi: { args: ["probe", "exit", String(17)] }
      });
      if (status !== 17) throw new Error("nested command exit lost");
      calls++;
      collect();
    }
  }
});
for (let i = 0; i < 5; i++) {
  const result = instance.exports["perry:transform/transform@1.0.0"].transform({
    text: "héllo 🌍", bytes: new Uint8Array([0, 127, 255])
  });
  if (result.tag !== "ok" || result.value.text !== "héllo 🌍 / Rust" || result.value.bytes[0] !== 255) {
    throw new Error("command disturbed the active library call");
  }
  let trapped = false;
  try {
    runComponentCommand(command, { wasi: { args: ["probe", "trap"] } });
  } catch (_) { trapped = true; }
  if (!trapped) throw new Error("command trap was ignored");
  collect();
}
instance.dispose();
if (calls !== 5 || runComponentCommand(command) !== 0) {
  throw new Error("library disposal disturbed fresh command execution");
}
console.log("PASS command/library integration: nested command exits, traps, callback GC, library survival");
