// #10060: shifted storage must agree across generated indexing, runtime
// mutators, JSON, aliases, forwarding, and moving collection.
declare function gc(): void;

function collect(): void {
  if (typeof gc === "function") gc();
}

function shiftedNumbers(): number[] {
  const q: number[] = [];
  for (let i = 0; i < 20; i++) q.push(i);
  for (let i = 0; i < 7; i++) q.shift();
  return q;
}

const q = shiftedNumbers();
const alias = q;
q[0] = 40;
q.push(20);
console.log("index", q.length, alias[0], q[3], q.pop());
console.log("slice", JSON.stringify(q.slice(0, 4)));
q.reverse();
q.copyWithin(1, 3, 5);
q.fill(60, 2, 4);
q.unshift(50, 51);
console.log("mutate", JSON.stringify(q), JSON.stringify(q.splice(2, 3, 70)));
console.log("map", q.map((n: number) => n + 1).join(","));
console.log("concat", JSON.stringify(q.concat([80, 81])));
q.length = 2;
q.length = 5;
console.log("holes", JSON.stringify(q), 2 in q);
while (q.length) q.shift();
q.push(90);
console.log("reuse", alias[0], q.shift(), q.length, q.shift());

function mixedQueue(size: number): void {
  const values: any[] = [];
  const other = values;
  let checksum = 0;
  for (let cycle = 0; cycle < 3; cycle++) {
    for (let i = 0; i < size; i++) {
      values.push(i % 2 === 0 ? { id: i, text: "item-" + i } : i);
    }
    for (let i = 0; i < size / 2; i++) {
      const v = values.shift();
      checksum += typeof v === "number" ? v : v.id;
    }
    collect();
    for (let i = 0; i < size; i++) values.push({ id: i, text: "new-" + i });
    collect();
    while (other.length) {
      const v = other.shift();
      checksum += typeof v === "number" ? v : v.id;
    }
  }
  console.log("mixed", size, checksum, values.length, other.length);
}
mixedQueue(32);
mixedQueue(4096);
