function logImul(a: number, b: number): void {
  console.log(Math.imul(a, b));
}

logImul(NaN, 5);
logImul(Infinity, 5);
logImul(4294967295, 5);
logImul(2147483647, 2);
