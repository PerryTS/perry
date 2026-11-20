// Negative control for the poll-free bounded loop
// (`stmt::loops::bounded_poll`): a bounded loop whose read can miss into user
// code keeps its polls. `row[f - 1]` reads index -1 on the first iteration,
// which is a property of `Array.prototype` here: a getter that allocates.
// The loop must run the getter, and the collector must still be able to run
// while the outer loop repeats it.

let calls = 0;
let junk: number[][] = [];
Object.defineProperty(Array.prototype, "-1", {
  get() {
    calls++;
    junk.push([calls, calls + 1, calls + 2]);
    if (junk.length > 256) junk = [];
    return calls & 3;
  },
  configurable: true,
});

function sweep(row: number[]): number {
  let s = 0;
  for (let f = 0; f < 8; f++) {
    s += (row[f - 1] ?? 0) + (row[f] ?? 0);
  }
  return s;
}

const RUN = Math.random() < 2 ? 1 : 0;
const row = [5, 1, 4, 1, 5, 9, 2, 6];
let total = 0;
for (let i = 0; i < 40000 * RUN; i++) total = (total + sweep(row)) % 1000003;
console.log(total, calls, junk.length);
