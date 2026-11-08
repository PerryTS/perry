// b2 C7: in a covered loop the slow-arm getter throws; a try/catch (and
// try/finally) around the loop, inside an outer loop that re-enters it,
// reads the array after.
let a: number[] = [5, 6, 7, 8];
let throws = 2;
Object.defineProperty(Array.prototype, "1.5", {
  configurable: true,
  get() { if (throws-- > 0) { a = [50, 60, 70, 80]; throw new Error("g" + throws); } return 1; },
});
for (let round = 0; round < 4; round++) {
  let small = 0, big = 0;
  try {
    for (let k = 0; k < 4; k++) {
      if (a[k] === k + 5) small++;
      if (a[k] === (k + 5) * 10) big++;
      if (k === 2 && a[k - 0.5] === 1) small += 10;
    }
  } catch (e) {
    console.log("caught", (e as Error).message, a[0], a[3]);
  } finally {
    console.log("finally", round, small, big, a[1]);
  }
}
delete (Array.prototype as any)["1.5"];
