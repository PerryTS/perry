// parity-env: PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_SCHEDULE_SEED=11789 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=4 PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_VERIFY_EVACUATION=1
// #11789 sweep: calls the HIR lowers to a runtime helper or a native-module
// dispatch must not hold an earlier heap argument in a register while a later
// argument collects. This is the "native call" family of the sweep
// (`lower_call/native/*`, `expr/calls/fs.rs`, `lower_call/property_get/
// number_string.rs`, `expr/os_uri_dates.rs`, `expr/string_regex_proc.rs`).
//
// Each shape passes a freshly built string / bigint / object ahead of an
// argument that runs a loop (whose back-edge polls collect). Output must be
// byte-identical to node. Without the fix every shape below faulted under the
// from-space quarantine or printed a corrupted value.

import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import * as util from "util";

function churn(rounds: number): number {
  let n = 0;
  for (let r = 0; r < rounds; r++) {
    const a: any[] = new Array(16);
    for (let j = 0; j < 16; j++) a[j] = { j, r, s: "v" + j };
    n += a.length;
  }
  return n;
}
function none(rounds: number): string {
  churn(rounds);
  return "";
}
const big: any = 4294967301;

// fs.renameSync(from, to) and fs.copyFileSync(from, to): the source path is a
// fresh concatenation, the destination path collects while it is held.
function fsShapes() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "w11789-"));
  fs.writeFileSync(dir + "/a" + String(big), "payload");
  fs.copyFileSync(dir + "/a" + String(big), dir + "/c" + String(big) + none(1500));
  console.log("copy", fs.readFileSync(dir + "/c" + String(big), "utf8"));
  fs.renameSync(dir + "/a" + String(big), dir + "/b" + String(big) + none(1500));
  console.log("rename", fs.existsSync(dir + "/b" + String(big)), fs.existsSync(dir + "/a" + String(big)));
  fs.rmSync(dir, { recursive: true });
}
fsShapes();

// util.format: the half-built argument array and the format string.
console.log(util.format("%s-%s", "k" + String(big), String(churn(1500))));

// JSON.stringify(value, replacer, indent): the value is held across the indent.
{
  const ind = () => {
    churn(1500);
    return 2;
  };
  console.log(JSON.stringify({ a: "k" + String(big), b: [1, 2] }, null, ind()));
}

// bigint.toString(radix): the receiver is held across the radix expression.
{
  const mkbi = (): bigint => 2n ** 80n + BigInt(Math.floor(churn(1)));
  const rad = () => {
    churn(1500);
    return 16;
  };
  console.log(mkbi().toString(rad()));
}

// process.emitWarning(warning, type): the message is held across the type.
{
  const w = (): string => "warn " + String(big);
  const t = (): string => {
    churn(1500);
    return "MyWarning";
  };
  process.on("warning", (e: any) => console.log("W", e.message, e.name));
  process.emitWarning(w(), t());
}
