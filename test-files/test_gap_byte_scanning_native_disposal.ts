// parity-node-argv: --expose-gc --import ./test-files/fixtures/byte_scanning_native_oracle.mjs
import { NativeArena } from "perry/native";
declare function gc(): void;
function read(b: Uint8Array, cb: (i: number) => void, count: number): string {
  let out = "";
  for (let i = 0; i < count; i++) {
    cb(i);
    try { out += String(b[i]) + ","; }
    catch { out += "throws,"; }
  }
  return out;
}
const owner = NativeArena.alloc(32);
const b = owner.view(Uint8Array, 16, 4);
b[0] = 3; b[1] = 5; b[2] = 7; b[3] = 11;
console.log("native-disposal", read(b, (i) => { if (i === 1) { owner.dispose(); gc(); } }, 4));
