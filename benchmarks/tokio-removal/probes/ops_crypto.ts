// N sequential async crypto ops (randomBytes + 1-iteration pbkdf2) on the worker pool.
import * as crypto from "node:crypto";
const N = Number(process.argv[2] || 100);
async function main() {
  let n = 0;
  for (let i = 0; i < N; i++) {
    const b = await new Promise<Buffer>((ok, bad) => crypto.randomBytes(16, (e, x) => (e ? bad(e) : ok(x))));
    const k = await new Promise<Buffer>((ok, bad) => crypto.pbkdf2(b, "s", 1, 16, "sha256", (e, x) => (e ? bad(e) : ok(x))));
    if (k.length === 16) n++;
  }
  console.log("crypto", n);
}
main();
