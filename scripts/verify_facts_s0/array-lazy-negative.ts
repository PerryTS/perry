// `JSON.parse` of an array text may return a lazy JSON array header, which
// is no `GC_TYPE_ARRAY`. Its result is no plain Array, so `a` is Boxed.
// Under PERRY_FACTS_SABOTAGE=arrlazy `JSON.parse` returns a plain Array of
// Numbers, so `a` is `Arr`, and the witness must report the header type at
// the first read.
const text = "[1, 2, 3, 4, 5, 6, 7, 8]";
function probe(): number {
  const a = JSON.parse(text);
  let s = 0;
  for (let i = 0; i < 8; i++) {
    const k = i;
    s = s + a[k];
  }
  return s;
}
console.log(probe());
