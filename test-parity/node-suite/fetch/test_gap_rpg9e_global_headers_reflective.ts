// A global surface installs its implementation: a web-fetch class reached by
// a runtime string constructs the real class.
const rows: [string, any[]][] = [
  ["Headers", []],
  ["Request", ["https://example.test/x"]],
  ["Response", ["body", { status: 201 }]],
];
for (const [name, args] of rows) {
  const C: any = (globalThis as any)[name];
  const holder = { C };
  const v: any = new holder.C(...args);
  console.log(name, typeof v);
}
const H: any = (globalThis as any)[rows[0][0]];
const h: any = new H({ "X-A": "1" });
h.append("x-b", "2");
console.log(h.get("x-a"), h.get("X-B"), h.has("x-c"));
