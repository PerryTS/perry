// b2 C7: the slow-arm getter throws; a try/catch (and try/finally) around
// the loop, inside an outer loop that re-enters it, reads the array after.
let a: number[] = [5, 6, 7, 8];
let throws = 2;
Object.defineProperty(Array.prototype, "1.5", {
  configurable: true,
  get() { if (throws-- > 0) { a = [50, 60, 70, 80]; throw new Error("g" + throws); } return 1; },
});
for (let round = 0; round < 4; round++) {
  let s = 0;
  try {
    for (let k = 0; k < 4; k++) { s += a[k]; if (k === 2) s += a[k - 0.5]; }
  } catch (e) {
    console.log("caught", (e as Error).message, a[0], a[3]);
  } finally {
    console.log("finally", round, s, a[1]);
  }
}
delete (Array.prototype as any)["1.5"];
