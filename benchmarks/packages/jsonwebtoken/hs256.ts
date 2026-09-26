// jsonwebtoken: sign + verify an HS256 token per iteration.
import jwt from "jsonwebtoken";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

const it = iters(2000, 200);
header("jsonwebtoken/hs256", "jsonwebtoken", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;
const secret = "pkg-bench-hs256-secret-0123456789abcdef";

function op(i: number, h: number): number {
  const payload = { sub: "user-" + (i % 1000), role: i % 3 === 0 ? "admin" : "user", n: i };
  const tok = jwt.sign(payload, secret, { algorithm: "HS256", noTimestamp: true });
  const back: any = jwt.verify(tok, secret, { algorithms: ["HS256"] });
  h = fnv(h, tok);
  return fnv(h, back.sub + ":" + back.role + ":" + back.n);
}

let h = FNV_SEED;
for (let i = 0; i < WARM; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < N; i++) h = op(i, h);
console.log("checksum " + hex(h));
