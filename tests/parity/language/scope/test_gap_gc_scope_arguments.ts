// Mapped (sloppy-mode) `arguments` aliasing next to scope context objects: a
// parameter that a closure captures and mutates is still the same binding as
// its `arguments[i]` entry, and body locals grouped into scope objects beside
// it keep their own values.
declare function gc(): void;
function collect() {
  if (typeof gc === "function") gc();
}

function mapped(a: any, b: any) {
  let local = 1;
  const bump = () => { a = a + 1; local += a; return local; };
  arguments[1] = "B";
  bump();
  collect();
  arguments[0] = 100;
  const r = bump();
  return `${a} ${b} ${arguments[0]} ${arguments[1]} ${local} ${r}`;
}
console.log(mapped(1, "b"));

function captureParam(x: number) {
  let hits = 0;
  const set = (v: number) => { x = v; hits++; };
  const get = () => x + hits;
  set(5);
  collect();
  x += 1;
  return `${get()} ${arguments[0]}`;
}
console.log(captureParam(1));

function spread(...rest: number[]) {
  let sum = 0;
  const add = () => { for (const r of rest) sum += r; rest = [sum]; return sum; };
  add();
  collect();
  return `${add()} ${rest.join(",")}`;
}
console.log(spread(1, 2, 3));
