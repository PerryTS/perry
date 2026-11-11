// `new Array(4)` is born with four holes. `a` holds only that plain Array,
// whose length stays 4 (every store is at index 0), so the facts give `a`
// `Arr` with floor 4. Under PERRY_FACTS_SABOTAGE=arrdense every
// `new Array(...)` is born hole-free, so the rep also claims `dense`, and
// the witness must report the hole at index 1, below the floor.
function probe(): number {
  const a: number[] = new Array(4);
  a[0] = 1;
  let s = 0;
  for (let i = 0; i < 4; i++) {
    const k = i;
    const v = a[k];
    s = s + (v === undefined ? 9 : v);
  }
  return s;
}
console.log(probe());
