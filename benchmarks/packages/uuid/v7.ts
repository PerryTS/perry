// uuid: v7 generate; checks version and that consecutive ids are strictly
// increasing (uuid guarantees monotonic v7 within a process).
import { v7, validate, version } from "uuid";
import { iters, header } from "../_lib/bench.ts";

const it = iters(200000, 5000);
header("uuid/v7", "uuid", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;
let ok = 0, mono = 0, prev = "";
function op(): void {
  const u = v7();
  if (validate(u) && version(u) === 7) ok++;
  if (u > prev) mono++;
  prev = u;
}
for (let i = 0; i < WARM; i++) op();
ok = 0; mono = 0;
for (let i = 0; i < N; i++) op();
console.log("valid " + ok + " increasing " + mono);
