// jsonwebtoken: decode (complete, no verification) pre-signed tokens.
import jwt from "jsonwebtoken";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

const it = iters(20000, 1000);
header("jsonwebtoken/decode", "jsonwebtoken", it);
const tokens: string[] = [];
for (let k = 0; k < 64; k++) {
  tokens.push(jwt.sign({ sub: "u" + k, tags: ["a", "b", "c" + k], nested: { k: k, s: "x".repeat(k) } },
    "decode-secret", { algorithm: "HS256", noTimestamp: true }));
}

function op(i: number, h: number): number {
  const d: any = jwt.decode(tokens[i & 63], { complete: true });
  return fnv(h, d.header.alg + d.payload.sub + d.payload.nested.k + d.signature.length);
}

let h = FNV_SEED;
for (let i = 0; i < it.warm; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < it.n; i++) h = op(i, h);
console.log("checksum " + hex(h));
