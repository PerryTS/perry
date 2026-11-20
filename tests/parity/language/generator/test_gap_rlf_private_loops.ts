// RLF private frames: loops, try/finally, break, in-generator catch.
let total = 0;
function* range(a: number, b: number): Generator<number> {
  for (let i = a; i < b; i++) yield i;
}

function* withFinally(n: number): Generator<number> {
  for (let i = 0; i < n; i++) {
    try {
      yield i;
    } finally {
      total += 100;
    }
  }
}
function* catchInside(n: number): Generator<number> {
  for (let i = 0; i < n; i++) {
    try {
      if (i % 2 === 1) throw i;
      yield i;
    } catch (e) {
      yield -i;
    }
  }
}
function* throwsLate(n: number): Generator<number> {
  for (let i = 0; i < n; i++) {
    if (i === 3) throw i + 1000;
    yield i;
  }
}
let s = 0;
for (const v of range(0, 10)) s += v;
console.log("range " + s);
s = 0;
for (const v of withFinally(5)) s += v;
console.log("finally " + s + " " + total);
s = 0;
for (const v of withFinally(10)) {
  if (v === 4) break;
  s += v;
}
console.log("break " + s + " " + total);
let line = "";
for (const v of catchInside(6)) line += v + ",";
console.log("catch " + line);
try {
  for (const v of throwsLate(10)) total += v;
} catch (e) {
  console.log("threw " + e + " " + total);
}
