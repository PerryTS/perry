// b2 C8: a covered loop starts on an array with an element accessor (the
// fast form fails its validation); a slow-arm getter replaces it with a
// plain numeric array, and the later reads are exact on the new one.
let a: number[] = [1, 2, 3, 4];
Object.defineProperty(a, 2, { get() { a = [10, 20, 30, 40]; return 33; }, configurable: true });
function run(): string {
  let small = 0, big = 0, acc = 0;
  for (let r = 0; r < 3; r++) {
    for (let k = 0; k < 4; k++) {
      const j = (k * 5) & 3;
      if (a[j] === j + 1) small++;
      if (a[j] === (j + 1) * 10) big++;
      if (a[j] === 33) acc++;
    }
  }
  return small + " " + big + " " + acc;
}
console.log(run());
