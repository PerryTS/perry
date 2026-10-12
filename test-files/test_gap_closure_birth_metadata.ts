function pair(a: number, b: number) {
  let x = a;
  const add = (delta: number = 1) => { x += delta; return x + b; };
  const read = () => x;
  return [add, read];
}
const a = pair(3, 5);
const b = pair(3, 5);
console.log(a[0].name, b[0].name, a[1].name);
console.log(a[0] === b[0], a[0].length, a[1].length);
console.log(a[0](2), a[1](), b[1]());
console.log(Object.getPrototypeOf(a[0]) === Function.prototype);
Object.defineProperty(a[0], "name", { value: "changed", configurable: true });
Object.defineProperty(a[0], "length", { value: 7, configurable: true });
console.log(a[0].name, a[0].length, b[0].length);
(a[0] as any).tag = "left";
console.log((a[0] as any).tag, (b[0] as any).tag);
delete (a[0] as any).length;
console.log(a[0].length, b[0].length);
let sum = 0;
for (let i = 0; i < 5000; i++) {
  const p = pair(i, 2);
  sum += p[0](1) + p[1]();
}
console.log(sum, a[1](), b[1]());
