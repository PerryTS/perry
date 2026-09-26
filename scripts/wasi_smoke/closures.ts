// Closures and callbacks: array methods call closure bodies through the
// runtime with more or fewer arguments than they declare.
const xs = [3, 1, 4, 1, 5, 9, 2, 6];
console.log(xs.map((x) => x * 2).join(","));
console.log(xs.filter((x, i) => i % 2 === 0));
console.log(xs.reduce((a, b) => a + b, 0));
console.log([...xs].sort((a, b) => a - b).join(" "));
console.log(xs.findIndex((x) => x > 4), xs.some((x) => x > 8), xs.every((x) => x > 0));
function counter() {
  let c = 0;
  return () => ++c;
}
const next = counter();
next();
next();
console.log(next());
const add = (a: number) => (b: number) => a + b;
console.log(add(2)(3));
xs.forEach(function (x, i, arr) {
  if (i === 0) console.log(x, i, arr.length);
});
