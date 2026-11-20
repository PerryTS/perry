// A spread operand stepped through its iterator protocol appends its values
// in place, after the arguments before it and before the ones after it.
class Vec3 {
  x: number; y: number; z: number;
  constructor(x: number, y: number, z: number) { this.x = x; this.y = y; this.z = z; }
  *[Symbol.iterator]() { yield this.x; yield this.y; yield this.z; }
}
function args(...a: unknown[]) { return a; }
const v = new Vec3(1, 2, 3);
console.log(JSON.stringify(args(...v, 4)));
console.log(JSON.stringify(args(0, ...v, ...v, 9)));
console.log(JSON.stringify([..."ab", ...v, ...[7, 8], ...new Set([5])]));

// A hand-written iterator, and one that throws part way: the call never runs.
const counter = {
  [Symbol.iterator]() {
    let i = 0;
    return { next: () => (i < 3 ? { value: i++, done: false } : { value: undefined, done: true }) };
  },
};
console.log(JSON.stringify(args("a", ...counter, "b")));
function* boom() { yield 1; throw new Error("stop"); }
let ran = false;
try {
  args(...boom(), (ran = true));
} catch (e) {
  console.log("threw", (e as Error).message, ran);
}
// Many values: the destination grows while it drains.
function* many() { for (let i = 0; i < 100; i++) yield i; }
const big = args(-1, ...many());
console.log(big.length, big[0], big[1], big[100]);
