// S0 `restcap` (I32 at rest, the capacity reservation): the birth of an
// at-rest Array whose length may move reserves nothing, so a store past its
// birth length lands at or past the capacity. The cold store's capacity
// witness must report it ("never past capacity").
const a = new Array<number>(8).fill(0);
function put(i: number, v: number): void {
  a[i & 15] = v & 1023;
}
for (let i = 0; i < 8; i++) put(i, i * 3);
put(12, 77);
let s = 0;
for (let i = 0; i < 8; i++) s = (s + (a[i] | 0)) | 0;
console.log(s, a.length, a[12] | 0, a[9]);
