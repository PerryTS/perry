// An empty literal filled by a later loop is born with a larger append
// reserve. It must stay an ordinary empty array: length, holes past the end,
// growth past the reserve, and the once-pushed per-iteration literal.
function fill(n: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(i * 3);
  return out;
}
for (const n of [0, 1, 4, 5, 16, 17, 40]) {
  const a = fill(n);
  console.log(n, a.length, a[n - 1], a[n], n in a, JSON.stringify(a.slice(-3)));
}
const empty: string[] = [];
for (let i = 0; i < 3; i++) {
  if (i > 5) empty.push("never");
}
console.log(empty.length, empty[0], 0 in empty, JSON.stringify(empty));
let total = 0;
for (let i = 0; i < 4; i++) {
  const fresh: number[] = [];
  fresh.push(i);
  total += fresh.length + fresh[0];
}
console.log(total);
const nested: number[][] = [];
for (let r = 0; r < 3; r++) {
  const row: number[] = [];
  for (let c = 0; c < 20; c++) row.push(r * 100 + c);
  nested.push(row);
}
console.log(nested.map((row) => row.length + ":" + row[19]).join(","));
