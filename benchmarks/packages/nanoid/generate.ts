// nanoid: default-alphabet ids + a customAlphabet generator. Random, so the
// checksum is structural (length, alphabet membership).
import { nanoid, customAlphabet } from "nanoid";
import { iters, header } from "../_lib/bench.ts";

const it = iters(200000, 5000);
header("nanoid/generate", "nanoid", it);
const hexId = customAlphabet("0123456789abcdef", 16);
const urlAlpha = /^[A-Za-z0-9_-]{21}$/;
const hexAlpha = /^[0-9a-f]{16}$/;
let ok = 0;
function op(): void {
  if (urlAlpha.test(nanoid())) ok++;
  if (hexAlpha.test(hexId())) ok++;
}
for (let i = 0; i < it.warm; i++) op();
ok = 0;
for (let i = 0; i < it.n; i++) op();
console.log("ok " + ok);
