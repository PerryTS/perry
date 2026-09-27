// jsonwebtoken: sign + verify an RS256 token per iteration (fixed key, so
// the PKCS#1 v1.5 signatures are deterministic).
import jwt from "jsonwebtoken";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";
import { RSA_PRIVATE_PEM, RSA_PUBLIC_PEM } from "../_lib/keys.ts";

const it = iters(200, 20);
header("jsonwebtoken/rs256", "jsonwebtoken", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;

function op(i: number, h: number): number {
  const payload = { sub: "svc-" + (i % 50), scope: ["read", "write"].slice(0, 1 + (i % 2)), n: i };
  const tok = jwt.sign(payload, RSA_PRIVATE_PEM, { algorithm: "RS256", noTimestamp: true, keyid: "k1" });
  const back: any = jwt.verify(tok, RSA_PUBLIC_PEM, { algorithms: ["RS256"] });
  h = fnv(h, tok);
  return fnv(h, back.sub + ":" + back.scope.join(",") + ":" + back.n);
}

let h = FNV_SEED;
for (let i = 0; i < WARM; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < N; i++) h = op(i, h);
console.log("checksum " + hex(h));
