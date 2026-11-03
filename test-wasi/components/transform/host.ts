import { readFileSync } from "node:fs";
import { collect } from "perry/gc";
import { load } from "./generated/bindings";

const bytes = readFileSync(process.argv[2]);
let calls = 0;
const component = load(bytes, {
  "perry:transform/logging@1.0.0": {
    log(message: string): void {
      calls++;
      collect();
      if (message !== "héllo 🌍" && message !== "") throw new Error("wrong callback text");
    }
  }
});

for (let i = 0; i < 50; i++) {
  const result = component.exports["perry:transform/transform@1.0.0"].transform({
    text: "héllo 🌍", bytes: new Uint8Array([0, 127, 255])
  });
  if (result.tag !== "ok") throw new Error("unexpected WIT error");
  if (result.value.text !== "héllo 🌍 / Rust") throw new Error("string mismatch");
  if (!(result.value.bytes instanceof Uint8Array)) throw new Error("byte list lost its type");
  if (result.value.bytes.length !== 3 || result.value.bytes[0] !== 255 || result.value.bytes[2] !== 0) {
    throw new Error("byte list mismatch");
  }
  collect();
}
const error = component.exports["perry:transform/transform@1.0.0"].transform({text: "", bytes: new Uint8Array()});
if (error.tag !== "err" || error.value.tag !== "invalid-input" || error.value.value !== "text must not be empty") {
  throw new Error("typed error mismatch");
}
component.dispose();
component.dispose();
let disposed = false;
try {
  component.exports["perry:transform/transform@1.0.0"].transform({text: "héllo 🌍", bytes: new Uint8Array()});
} catch (_) { disposed = true; }
if (!disposed || calls !== 51) throw new Error("lifetime mismatch");
console.log("PASS Rust component: 51 calls, typed results, callback GC, disposal");
