// #11559: a property stored into an object's overflow ("spill") buffer PAST
// the buffer's high-water mark must be traced by the collector.
//
// An object keeps two properties inline and the rest in a spill array. The
// first spill buffer requests 8 slots, but the allocator rounds it up to 16 and
// writes TAG_HOLE only into the requested 8; the other 8 hold whatever the
// memory held before. A store into that headroom read the leftover word as the
// value it was overwriting, and when that word looked like a pointer the
// layout note was skipped as a "pointer over pointer" overwrite. The slot never
// entered the buffer's pointer mask, so the collector neither marked nor
// rewrote it, and the property later read back as some other object.
//
// claude-code 2.1.112 builds every tool record as
// `Object.defineProperties({...defaults}, Object.getOwnPropertyDescriptors(q))`
// and `-p` died with "prompt is not a function": the `prompt` method sat in the
// headroom of such a record's spill buffer and read back as a plain object.
//
// The bug needs recycled, pointer-dense memory under the headroom, so the test
// churns arrays of fresh objects between batches, and it needs the children to
// move or die, so every record is checked only after more churn. The helpers
// are assigned by a lazy initializer to outer `var`s, as in the bundle.
//
// Output must be byte-identical to node.

var methodNames: string[];
var makeRecord: (names: string[]) => any;
const init = () => {
  methodNames = [];
  for (let i = 0; i < 14; i++) methodNames.push("m" + i);
  makeRecord = (names) => {
    const src: any = {};
    for (const n of names) {
      src[n] = function () {
        return "call:" + n;
      };
    }
    const defaults = { isEnabled: () => true, userFacingName: () => "" };
    return Object.defineProperties({ ...defaults }, Object.getOwnPropertyDescriptors(src));
  };
};
init();

// Pointer-dense garbage: arrays of fresh objects, dropped immediately. When the
// nursery recycles this memory the leftover words are tagged object pointers.
function churn(rounds: number): number {
  let n = 0;
  for (let r = 0; r < rounds; r++) {
    const a: any[] = new Array(16);
    for (let j = 0; j < 16; j++) a[j] = { j, r };
    n += a.length;
  }
  return n;
}

const records: any[] = [];
for (let batch = 0; batch < 40; batch++) {
  churn(2000);
  for (let k = 0; k < 8; k++) records.push(makeRecord(methodNames));
}
churn(20000);

let bad = 0;
let firstBad = "none";
for (const r of records) {
  for (const n of methodNames) {
    const f = r[n];
    const v = typeof f === "function" ? f() : n + " is " + typeof f;
    if (v !== "call:" + n) {
      if (bad === 0) firstBad = v;
      bad++;
    }
  }
}
console.log("records: " + records.length);
console.log("bad reads: " + bad);
console.log("first bad: " + firstBad);
console.log("isEnabled: " + records[0].isEnabled());
