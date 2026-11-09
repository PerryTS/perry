// `A.shift()` removes A's first slot. The runtime does not move the other
// elements: it advances the front of the storage, so A's elements no longer
// start right behind its header. `sum`'s parameter holds A and B, and B has
// 19 elements, the length A has after the shift.
// Under PERRY_FACTS_SABOTAGE=arrfront `shift` keeps the length interval, so
// the facts keep A's floor (the parameter's is 19) and the element tier
// addresses slot `k` behind the header. Its witness must report the false
// proof at A's moved front.
const A = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19];
const B = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
function sum(a: number[]): number {
  let s = 0;
  for (let i = 0; i < 1000; i++) {
    const k = i & 7;
    const x = a[k];
    s = s + (x === undefined ? 1000 : x);
  }
  return s;
}
console.log(sum(B));
A.shift();
console.log(sum(A), A.length);
