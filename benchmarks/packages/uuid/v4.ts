// uuid: v4 generate + validate + version. Random output, so the checksum
// is over structure only (validity, version, length), never the bytes.
import { v4, validate, version } from "uuid";
import { iters, header } from "../_lib/bench.ts";

const it = iters(200000, 5000);
header("uuid/v4", "uuid", it);
let ok = 0, len = 0;
function op(): void {
  const u = v4();
  if (validate(u) && version(u) === 4 && u.charAt(14) === "4") ok++;
  len += u.length;
}
for (let i = 0; i < it.warm; i++) op();
ok = 0; len = 0;
for (let i = 0; i < it.n; i++) op();
console.log("valid " + ok + " totalLen " + len);
