// validator: a batch of isEmail / isURL / isIP / isUUID / isISO8601 checks
// over a fixed corpus of valid and invalid inputs.
import validator from "validator";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

const it = iters(5000, 200);
header("validator/batch", "validator", it);
const corpus = [
  "alice@example.com", "bob.smith+tag@sub.example.co.uk", "not-an-email", "a@b", "x@@y.com",
  "https://example.com/path?q=1#frag", "http://localhost:8080", "ftp://files.example.org/a.txt",
  "javascript:alert(1)", "example", "192.168.1.1", "::1", "2001:db8::ff00:42:8329", "999.1.1.1",
  "550e8400-e29b-41d4-a716-446655440000", "550e8400-e29b-41d4-a716-44665544000Z",
  "2024-03-15T10:20:30Z", "2024-13-45", "Z29vZCBiYXNlNjQ=", "not base64!!",
];

function op(i: number, h: number): number {
  let bits = "";
  for (let k = 0; k < corpus.length; k++) {
    const s = corpus[k];
    bits += (validator.isEmail(s) ? "1" : "0") + (validator.isURL(s) ? "1" : "0") +
      (validator.isIP(s) ? "1" : "0") + (validator.isUUID(s) ? "1" : "0") +
      (validator.isISO8601(s) ? "1" : "0") + (validator.isBase64(s) ? "1" : "0");
  }
  return fnv(h, bits + (i & 1));
}
let h = FNV_SEED;
for (let i = 0; i < it.warm; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < it.n; i++) h = op(i, h);
console.log("checksum " + hex(h));
