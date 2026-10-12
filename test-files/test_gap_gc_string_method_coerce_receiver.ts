// parity-env: PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=4 PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_VERIFY_EVACUATION=1
// A string method coerces its arguments while its receiver is held. Every
// receiver below is a fresh (young) heap string and every argument an object
// whose valueOf/toString runs a polling loop, so a collection happens INSIDE
// the coercion:
//
// * padStart/padEnd: ToLength(maxLength) / ToString(fill) ran with the
//   receiver unrooted -- the rooting decision only asked whether LOWERING an
//   argument could collect, and loading a local cannot. zod's
//   `b.toString(16).padStart(2, "0")` is the same window with inert
//   arguments, which the native root-dominance gate reported.
// * charAt, at, codePointAt, charCodeAt, slice, substring, substr, repeat,
//   indexOf, lastIndexOf, includes, startsWith, endsWith, anchor,
//   localeCompare: the receiver was unboxed to a raw StringHeader address
//   BEFORE the coercion ran, so the address named from-space afterwards.
//
// Under the from-space quarantine every one of these faulted on its first
// iteration. The fix coerces first, then reads the rooted receiver.

function churn(rounds: number): number {
  let n = 0;
  for (let r = 0; r < rounds; r++) {
    const a: any[] = new Array(16);
    for (let j = 0; j < 16; j++) a[j] = { j, r, s: "v" + j };
    n += a.length;
  }
  return n;
}

// Longer than an inline short string: a young heap string.
const fresh = (i: number): string => "receiver-string-" + String(i) + "-abcdefghijklmnop";
const lenObj: any = { valueOf() { churn(400); return 40; } };
const fillObj: any = { toString() { churn(400); return "*"; } };
const idxObj: any = { valueOf() { churn(400); return 3; } };
const cntObj: any = { valueOf() { churn(400); return 2; } };
const needleObj: any = { toString() { churn(400); return "string"; } };
const tailObj: any = { toString() { churn(400); return "p"; } };

for (let i = 0; i < 3; i++) {
  console.log("padStart", fresh(i).padStart(lenObj, "0"));
  console.log("padEnd", fresh(i).padEnd(40, fillObj));
  console.log("padNumFill", fresh(i).padStart(44, 7 as any));
  console.log("charAt", fresh(i).charAt(idxObj));
  console.log("at", fresh(i).at(idxObj));
  console.log("codePointAt", fresh(i).codePointAt(idxObj));
  console.log("charCodeAt", fresh(i).charCodeAt(idxObj));
  console.log("slice", fresh(i).slice(idxObj));
  console.log("substring", fresh(i).substring(idxObj, cntObj));
  console.log("substr", fresh(i).substr(idxObj, cntObj));
  console.log("repeat", fresh(i).repeat(cntObj));
  console.log("indexOf", fresh(i).indexOf(needleObj));
  console.log("indexOfNum", fresh(i).indexOf(5 as any));
  console.log("lastIndexOf", fresh(i).lastIndexOf("-", idxObj));
  console.log("includes", fresh(i).includes(needleObj));
  console.log("startsWith", fresh(i).startsWith(needleObj, idxObj));
  console.log("endsWith", fresh(i).endsWith(tailObj));
  console.log("anchor", fresh(i).anchor(needleObj));
  console.log("localeCompare", fresh(i).localeCompare(needleObj));
}
