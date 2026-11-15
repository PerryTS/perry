// LIFE I3 fixture: work's activation is clean (its accumulator is local and
// dropped at return) although every iteration of its inner loop is dirty.
function work(n: number): number {
  const acc: { v: number }[] = [];
  for (let i = 0; i < n; i++) {
    acc.push({ v: i });
  }
  let t = 0;
  for (let i = 0; i < acc.length; i++) t += acc[i].v;
  return t + acc.length;
}
let total = 0;
for (let k = 0; k < 20; k++) total += work(50);
console.log(total);
