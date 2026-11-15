// Mapped, unmapped and strict arguments objects (with extra keys) kept alive
// across many collections: the birth word and the alias array live in the
// object's meta record and must move and survive with it.

const held: any[] = [];
const setters: ((v: any) => void)[] = [];
const getters: (() => any)[] = [];

function mappedKeep(a: any, b: any, c: any): void {
  held.push(arguments);
  (arguments as any).tag = "m" + a;
  setters.push(function (v: any) { a = v; });
  getters.push(function () { return a; });
}

function unmappedKeep(a: any = 0, ...rest: any[]): void {
  held.push(arguments);
}

function strictKeep(a: any, b: any): void {
  "use strict";
  held.push(arguments);
  for (let i = 0; i < 10; i++) (arguments as any)["s" + i] = i;
}

for (let i = 0; i < 20; i++) {
  mappedKeep({ v: i }, "b" + i, [i]);
  unmappedKeep(i, i + 1, i + 2);
  strictKeep({ w: i }, i);
}

function churn(n: number): number {
  let sum = 0;
  let keep: any[] = [];
  for (let i = 0; i < n; i++) {
    const o = { a: i, b: [i, i + 1], c: "s" + (i % 97) };
    sum += o.b.length;
    if (i % 1000 === 0) keep.push(o);
  }
  return sum + keep.length;
}

console.log("churn1", churn(200000));
let lines: string[] = [];
for (let i = 0; i < held.length; i += 3) {
  const m = held[i];
  lines.push([m[0].v, m[1], m[2][0], m.tag, m.length, getters[i / 3]().v].join(","));
}
console.log("after1", lines.join(" "));

for (let j = 0; j < 20; j++) {
  setters[j]({ v: 100 + j });
  held[j * 3][1] = "nb" + j;
}
console.log("churn2", churn(200000));
lines = [];
for (let j = 0; j < 20; j++) {
  const m = held[j * 3];
  const u = held[j * 3 + 1];
  const s = held[j * 3 + 2];
  lines.push([m[0].v, m[1], u[0], u.length, s[0].w, s.s9, s.length].join(","));
}
console.log("after2", lines.join(" "));
console.log("keys", Object.keys(held[0]).join(","), Object.keys(held[2]).length);
