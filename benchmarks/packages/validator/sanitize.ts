// validator: sanitizers — escape, normalizeEmail, trim/blacklist, toInt.
import validator from "validator";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

const it = iters(20000, 1000);
header("validator/sanitize", "validator", it);
const inputs = [
  "<script>alert('x')</script> & \"quotes\"", "  Some.User+news@GoogleMail.com ",
  "Foo.Bar@Example.COM", "   padded text\t\n", "12345abc", "a/b\\c'd\"e<f>g&h",
];

function op(i: number, h: number): number {
  const s = inputs[i % inputs.length];
  const e = validator.escape(s);
  const n = validator.normalizeEmail(s.trim()) || "none";
  const t = validator.trim(validator.blacklist(s, "<>&"));
  const num = validator.toInt(s.trim());
  return fnv(h, e + "|" + n + "|" + t + "|" + num);
}
let h = FNV_SEED;
for (let i = 0; i < it.warm; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < it.n; i++) h = op(i, h);
console.log("checksum " + hex(h));
