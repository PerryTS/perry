// An arguments object given more own keys than its inline slots hold: index reads, writes and deletes, `length`,
// Object.keys, and the parameter aliases of a mapped (sloppy, simple-parameter) object must keep working. Perry
// placed property slots over the arguments object's private tail and crashed on the next index access.
// Forms: unmapped / mapped / strict; extra keys by computed key, fixed key, delete + re-add; index read, numeric,
// string and computed-key delete.
const KS = "alpha,beta".split(",");
const out: string[] = [];
function keys(a: any): string {
  return Object.keys(a).join("|");
}
(function () {
  const a: any = arguments;
  for (let i = 0; i < 12; i++) a["q" + i] = i;
  out.push("unmapped-read " + a.length + " " + a[0] + " " + a[1] + " " + a.q11);
  out.push("unmapped-del-str " + delete a["0"] + " " + (0 in a) + " " + keys(a));
  out.push("unmapped-del-num " + delete a[1] + " " + a.length + " " + keys(a));
})(10, 20);
(function (x: number, y: number) {
  const a: any = arguments;
  for (let i = 0; i < 12; i++) a["q" + i] = i;
  a[1] = 21;
  out.push("mapped-alias " + a[0] + " " + y + " " + a.q5);
  x = 11;
  out.push("mapped-param-write " + a[0]);
  out.push("mapped-del " + delete a["0"] + " " + (0 in a) + " " + x + " " + keys(a));
  a[0] = 12;
  out.push("mapped-after-del " + x + " " + a[0]);
})(10, 20);
(function () {
  "use strict";
  const a: any = arguments;
  for (let i = 0; i < 12; i++) a["q" + i] = i;
  out.push("strict " + a[0] + " " + delete a["0"] + " " + keys(a));
})(10, 20);
(function () {
  const a: any = arguments;
  a.f0 = 0; a.f1 = 1; a.f2 = 2; a.f3 = 3; a.f4 = 4; a.f5 = 5; a.f6 = 6; a.f7 = 7;
  out.push("fixed-keys " + a[0] + " " + delete a[KS[0].length - 5] + " " + keys(a));
})(10, 20);
(function () {
  const a: any = arguments;
  a[KS[0]] = 1; a.zz = 3; delete a.zz; a.zz = 4;
  out.push("readd " + delete a[KS[0]] + " " + delete a["0"] + " " + keys(a));
})(10, 20);
(function () {
  const a: any = arguments;
  a.zz = 3;
  out.push("one-key " + delete a["0"] + " " + keys(a));
})(10, 20);
for (const line of out) console.log(line);
