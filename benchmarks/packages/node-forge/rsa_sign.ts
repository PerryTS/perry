// node-forge: RSA-2048 PKCS#1 v1.5 sign (SHA-256) + verify with a fixed key.
import forge from "node-forge";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";
import { RSA_PRIVATE_PEM, RSA_PUBLIC_PEM } from "../_lib/keys.ts";

const it = iters(50, 5);
header("node-forge/rsa_sign", "node-forge", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;
const priv = forge.pki.privateKeyFromPem(RSA_PRIVATE_PEM);
const pub = forge.pki.publicKeyFromPem(RSA_PUBLIC_PEM);

function op(i: number, h: number): number {
  const md = forge.md.sha256.create();
  md.update("payload-" + i, "utf8");
  const sig = priv.sign(md);
  const md2 = forge.md.sha256.create();
  md2.update("payload-" + i, "utf8");
  const ok = pub.verify(md2.digest().bytes(), sig);
  return fnv(h, forge.util.bytesToHex(sig) + ok);
}
let h = FNV_SEED;
for (let i = 0; i < WARM; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < N; i++) h = op(i, h);
console.log("checksum " + hex(h));
