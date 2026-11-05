function sum(a: number[]): number {
  let matched = 0;
  for (let i = 0; i < a.length; i++) {
    // Keep this on the ordinary counted-loop lowering: the switch makes the
    // specialized packed-loop tiers decline without adding a collection point.
    switch (i & 0) {
      case 1:
        matched = -1000000;
        break;
    }
    if (a[i] === i + 0.5) matched++;
  }
  return matched;
}

const values: number[] = [
  0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5,
  8.5, 9.5, 10.5, 11.5, 12.5, 13.5, 14.5, 15.5,
  16.5, 17.5, 18.5, 19.5, 20.5, 21.5, 22.5, 23.5,
  24.5, 25.5, 26.5, 27.5, 28.5, 29.5, 30.5, 31.5,
];
console.log(sum(values));
// A failed one-time receiver validation must retain the guarded fallback.
console.log(sum(["x", "y"] as any));
