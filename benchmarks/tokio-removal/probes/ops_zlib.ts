// N sequential async gzip+gunzip round trips.
import * as zlib from "node:zlib";
const N = Number(process.argv[2] || 100);
const input = Buffer.from("hello tokio-free world ".repeat(20));
async function main() {
  let n = 0;
  for (let i = 0; i < N; i++) {
    const gz = await new Promise<Buffer>((ok, bad) => zlib.gzip(input, (e, x) => (e ? bad(e) : ok(x))));
    const out = await new Promise<Buffer>((ok, bad) => zlib.gunzip(gz, (e, x) => (e ? bad(e) : ok(x))));
    if (out.length === input.length) n++;
  }
  console.log("zlib", n);
}
main();
