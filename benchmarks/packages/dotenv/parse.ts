// dotenv: parse a ~40-line .env document (quotes, comments, multiline,
// export prefixes) per iteration.
import dotenv from "dotenv";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

const it = iters(10000, 500);
header("dotenv/parse", "dotenv", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;
let doc = "# generated\nexport NODE_ENV=production\nPORT=8080\n";
doc += 'MULTI="line one\nline two\nline three"\n';
doc += "SINGLE='single quoted # not a comment'\n";
doc += 'DOUBLE="double \\n escaped"\n';
doc += "BACKTICK=`back ticked`\n";
doc += "EMPTY=\n  SPACED  =  spaced value  \n";
for (let k = 0; k < 30; k++) doc += "KEY_" + k + "=value_" + k + " # trailing comment " + k + "\n";

function op(i: number, h: number): number {
  const parsed = dotenv.parse(doc);
  const keys = Object.keys(parsed);
  return fnv(h, keys.length + parsed.MULTI + parsed.SINGLE + parsed.DOUBLE + parsed.SPACED +
    parsed["KEY_" + (i % 30)] + parsed.EMPTY + parsed.NODE_ENV);
}
let h = FNV_SEED;
for (let i = 0; i < WARM; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < N; i++) h = op(i, h);
console.log("checksum " + hex(h));
