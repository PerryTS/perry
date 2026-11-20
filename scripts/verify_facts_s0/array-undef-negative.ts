// `A[k]` with `k = i & 127` reads A[127], one past the end of A's 127 elements, at
// every 128th step. Under PERRY_FACTS_SABOTAGE=arrundef the solve's
// `ElemUndef` takes a key that reaches the length's lower bound as below
// it: the facts drop the read's `undefined` and claim the key is below the
// floor, and the witness must report the false proof when the key is 127.
const A = [
  0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
  21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39,
  40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58,
  59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77,
  78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96,
  97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111,
  112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 124, 125, 126,
];
function sum(): number {
  let s = 0;
  for (let i = 0; i < 1000; i++) {
    const k = i & 127;
    const x = A[k];
    s = s + (x === undefined ? 1000 : x);
  }
  return s;
}
console.log(sum());
