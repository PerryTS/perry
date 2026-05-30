function close(label: string, actual: number, expected: number) {
  const tolerance = Math.max(Number.EPSILON * Math.abs(expected), 1e-30);
  console.log(label, Math.abs(actual - expected) <= tolerance);
}

close("expm1 tiny positive", Math.expm1(1e-16), 1e-16);
close("expm1 tiny negative", Math.expm1(-1e-16), -1e-16);
close("log1p tiny positive", Math.log1p(1e-16), 1e-16);
close("log1p tiny negative", Math.log1p(-1e-16), -1e-16);

close("expm1 sanity", Math.expm1(1e-9), 1.0000000005000001e-9);
close("log1p sanity", Math.log1p(1e-9), 9.999999995e-10);

console.log("naive expm1 collapses", Math.exp(1e-16) - 1 === 0);
console.log("naive log1p collapses", Math.log(1 + 1e-16) === 0);
