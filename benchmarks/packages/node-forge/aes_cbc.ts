// node-forge: AES-256-CBC encrypt + decrypt of a 1 KiB buffer (fixed key/IV).
import forge from "node-forge";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

const it = iters(5000, 200);
header("node-forge/aes_cbc", "node-forge", it);
const key = "0123456789abcdef0123456789abcdef"; // 32 bytes
const iv = "fedcba9876543210";
const plain = "Lorem ipsum dolor sit amet, consectetur adipiscing. ".repeat(20);

function op(i: number, h: number): number {
  const c = forge.cipher.createCipher("AES-CBC", key);
  c.start({ iv: iv });
  c.update(forge.util.createBuffer(plain + (i % 100), "utf8"));
  c.finish();
  const enc = c.output.getBytes();
  const d = forge.cipher.createDecipher("AES-CBC", key);
  d.start({ iv: iv });
  d.update(forge.util.createBuffer(enc));
  d.finish();
  return fnv(h, forge.util.bytesToHex(enc.substring(0, 32)) + enc.length + d.output.toString().length);
}
let h = FNV_SEED;
for (let i = 0; i < it.warm; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < it.n; i++) h = op(i, h);
console.log("checksum " + hex(h));
