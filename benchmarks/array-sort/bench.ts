import { readFileSync } from "node:fs";
const distribution = process.argv[2] || "random";
const kind = process.argv[3] || "number";
const n = Number(process.argv[4] || "100000");
const rounds = Number(process.argv[5] || "1");
for (let round = 0; round < rounds; round++) {
  const values: number[] = JSON.parse(readFileSync(process.argv[6], "utf8"));
  const items: any[] = [];
  for (let i = 0; i < n; i++) {
    if (kind === "object") items.push({key: values[i], id: i});
    else if (kind === "string") items.push(("0000000000" + values[i]).slice(-10));
    else items.push(values[i]);
  }
  const start = performance.now();
  if (kind === "object") items.sort((a, b) => a.key - b.key);
  else if (kind === "string") items.sort((a, b) => a < b ? -1 : a > b ? 1 : 0);
  else items.sort((a, b) => a - b);
  const sortMs = performance.now() - start;
  let hash = 1;
  let sum = 0;
  for (let i = 0; i < n; i++) {
    const value = kind === "object" ? items[i].key : Number(items[i]);
    if (i > 0) {
      const previous = kind === "object" ? items[i - 1].key : Number(items[i - 1]);
      if (previous > value) throw new Error("out of order at " + i);
      if (kind === "object" && previous === value && items[i - 1].id > items[i].id) {
        throw new Error("unstable sort at " + i);
      }
    }
    sum += value;
    hash = (hash * 31 + value) % 2147483647;
    if (kind === "object") hash = (hash * 31 + items[i].id) % 2147483647;
  }
  console.log(JSON.stringify({distribution, kind, n, round, sortMs, length: items.length, sum, hash, items}));
}
