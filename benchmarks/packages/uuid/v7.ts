// uuid: v7 generate; checks version and that consecutive ids are strictly
// increasing (uuid guarantees monotonic v7 within a process).
import { v7, validate, version } from "uuid";
import { iters, header } from "../_lib/bench.ts";

const it = iters(200000, 5000);
header("uuid/v7", "uuid", it);
let ok = 0, mono = 0, prev = "";
function op(): void {
  const u = v7();
  if (validate(u) && version(u) === 7) ok++;
  if (u > prev) mono++;
  prev = u;
}
for (let i = 0; i < it.warm; i++) op();
ok = 0; mono = 0;
for (let i = 0; i < it.n; i++) op();
console.log("valid " + ok + " increasing " + mono);
