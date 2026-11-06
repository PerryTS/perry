// Run with moving-GC schedule/from-space protection; retain copied references.
function churn(v: number): number {
  const keep: any[] = [];
  for (let j = 0; j < 20; j++) keep.push({ value: v + j, nested: [v, j] });
  return keep[0].value;
}
const proto: any = { inherited: 29 };
const symbol = Symbol("root");
const keep: any[] = [];
let total = 0;
for (let i = 0; i < 100; i++) {
  const source: any = Object.create(proto);
  source.payload = { value: i, text: "payload-" + i };
  source[symbol] = { value: i + 1 };
  Object.defineProperty(source, "accessor", { get() { return churn(31); }, enumerable: true, configurable: true });
  const copy: any = Object.create(proto, Object.getOwnPropertyDescriptors(source));
  const spread: any = { ...copy };
  const assigned: any = Object.assign(Object.create(proto), copy);
  keep.push([source, copy, spread, assigned]);
  total += spread.accessor + assigned.accessor;
}
for (let i = 0; i < keep.length; i++) {
  const row: any = keep[i];
  for (let j = 1; j < 4; j++) {
    if (row[j].payload !== row[0].payload || row[j][symbol] !== row[0][symbol]) throw new Error("lost reference");
    total += row[j].payload.value + row[j][symbol].value;
  }
  if (Object.getPrototypeOf(row[1]) !== proto || Object.getPrototypeOf(row[3]) !== proto) throw new Error("lost prototype");
}
console.log("retained", keep.length, total, keep[99][1].payload.text);


// Two own keys stay at the allocator floor after the first eight births.
// This phase exercises carried zero-live birth identities while objects move.
const narrowProto: any = { inherited: 37 };
const narrow: any[] = [];
for (let i = 0; i < 100; i++) {
  const value: any = Object.create(narrowProto);
  value.payload = { value: i };
  value[symbol] = { value: i + 1 };
  narrow.push(value);
  churn(i);
}
let narrowTotal = 0;
for (let i = 0; i < narrow.length; i++) {
  const value: any = narrow[i];
  if (Object.getPrototypeOf(value) !== narrowProto) throw new Error("lost narrow prototype");
  if (Object.keys(value).join(",") !== "payload") throw new Error("narrow key order");
  narrowTotal += value.payload.value + value[symbol].value + value.inherited;
}
// A later spill teaches a different live bound; later births remain correct.
narrow[99].extra = { value: 41 };
const wider: any = Object.create(narrowProto);
wider.payload = { value: 43 };
wider[symbol] = { value: 47 };
wider.extra = { value: 53 };
churn(59);
if (Object.getPrototypeOf(wider) !== narrowProto) throw new Error("lost wider prototype");
console.log("narrow", narrow.length, narrowTotal, Object.keys(wider).join(","), wider.extra.value);
