// parity-env: PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_SCHEDULE_SEED=11830 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=4 PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_VERIFY_EVACUATION=1
// #11830: a native-table call with two string arguments turned each one into a
// raw `*const StringHeader` in order. Coercing the SECOND argument can run user
// code (an object argument goes through `JSON.stringify`, so its `toJSON`
// runs), and a collection there left the FIRST argument's raw pointer aimed at
// the retired from-space copy of a freshly built string.
//
// `ClientRequest#setHeader(name, value)` takes two `StrPtr` arguments. The name
// is a string built at run time and the value an object whose `toJSON` /
// `toString` run a loop that polls. Node accepts the object and stores it;
// perry coerces it. Either way the header must be present under the name that
// was passed, which is what `hasHeader` reports: a stale name pointer stored a
// different name (or faulted under the from-space quarantine).

import * as http from "node:http";

function churn(rounds: number): number {
  let n = 0;
  for (let r = 0; r < rounds; r++) {
    const a: any[] = new Array(16);
    for (let j = 0; j < 16; j++) a[j] = { j, r, s: "v" + j };
    n += a.length;
  }
  return n;
}

const req = http.request({ host: "127.0.0.1", port: 9, path: "/", method: "GET" });
req.on("error", () => {});
// Longer than an inline short string: a young heap string.
const nm = (i: number): string => "x-witness-" + String(i);

for (let i = 0; i < 6; i++) {
  const value: any = {
    toJSON() {
      churn(1500);
      return "v" + i;
    },
    toString() {
      churn(1500);
      return "v" + i;
    },
  };
  req.setHeader(nm(i), value);
  console.log(i, req.hasHeader(nm(i)), req.hasHeader("x-nothere"));
}
req.destroy();
