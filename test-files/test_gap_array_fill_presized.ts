// fill on a pre-sized array whose length is past what `new Array(n)` backs with
// slots (above 1,000,000 it keeps a small store and a logical length): fill and
// fill(v, start, end) used to write every index in [start, end) over that store,
// past the allocation, and on wasm32 trapped once the write left linear memory.
// Sizes cross the dense threshold and the growth steps; start/end cover a range
// inside the store, one that crosses its end, and ones far past it; the arrays
// are then read, mapped, appended to and filled again through every route.
// The harness kills a test at 10 s, so the whole-array fills here are kept to ~7M slots in
// total; what happens to the array afterwards is test_gap_array_fill_presized_uses.
// The array is digested at a stride (Object.keys skips the dense indices of an
// array above 100000 elements), holes counted through `in`.
function digest(a) {
  let sum = 0;
  let holes = 0;
  let others = 0;
  for (let i = 0; i < a.length; i += 997) {
    if (i in a) {
      if (typeof a[i] === 'number') sum += a[i];
      else others++;
    } else holes++;
  }
  const last = a.length - 1;
  return `len=${a.length} first=${a[0]} last=${a[last]} sum=${sum} holes=${holes} other=${others}`;
}
function t(name, f) {
  let r;
  try {
    r = f();
  } catch (e) {
    r = 'threw ' + (e && e.constructor && e.constructor.name);
  }
  console.log(name + ' ' + r);
}

const sizes = [0, 1, 3, 4, 5, 16, 1000, 65536, 1000000, 1000001, 1500000];
for (const n of sizes) {
  t('fill ' + n, () => {
    const a = new Array(n);
    const r = a.fill(7);
    return (r === a) + ' ' + digest(a) + ' | ' + digest(r);
  });
}
// Literal length: the compiler may lower it differently from a variable.
t('fill literal 3e6', () => {
  const a = new Array(3e6).fill(0);
  return digest(a);
});

// Ranges, each on a fresh 1.2M and 3M array (a range that is not the whole array is cheap).
for (const n of [1200000, 3000000]) {
  const ranges = [
    [0, 10],
    [0, 4],
    [2, 6],
    [3, 5000],
    [-5, undefined],
    [-10, -2],
    [n - 10, n],
    [n - 10, n + 100],
    [n - 1, n],
    [1000000, 1000010],
    [1000000, 1002000],
    [n, n],
    [10, 5],
    [5, 6],
    [-n - 50, 20],
  ];
  for (const [s, e] of ranges) {
    t(`fillRange ${n} ${s} ${e}`, () => {
      const a = new Array(n);
      const r = e === undefined ? a.fill(9, s) : a.fill(9, s, e);
      return (r === a) + ' ' + digest(a);
    });
  }
  t(`fillChain ${n}`, () => {
    const a = new Array(n).fill(1, 0, 10).fill(2, 10, 20).fill(3, -5);
    return digest(a);
  });
  t(`fillTwice ${n}`, () => {
    const a = new Array(n);
    a.fill(1, 5, 50);
    a.fill(2, 5, 50);
    return digest(a);
  });
}
t('fill null range', () => {
  const a = new Array(1200000).fill(null, 1000000, 1004000);
  return digest(a) + ' ' + a[1000000] + ' ' + a[999999];
});
