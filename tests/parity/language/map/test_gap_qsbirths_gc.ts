// Retain factory closures and branded births across protected moving collections.
function make(value: number): any {
  let state = value;
  return { get: function () { return state; }, set: function (v: number) { state = v; },
    has: function () { return state >= 0; }, reset: function () { state = value; },
    twice: function () { return state * 2; } };
}
const W: any = WeakMap;
const keep: any[] = [];
for (let i = 0; i < 100; i++) {
  const key: any = { value: i };
  const channel = make(i);
  const map = new W([[key, channel]]);
  keep.push([key, channel, map]);
  const churn: any[] = [];
  for (let j = 0; j < 20; j++) churn.push({ pair: [i, j], label: "churn-" + j });
}
let total = 0;
for (let i = 0; i < keep.length; i++) {
  const row: any = keep[i];
  if (row[2].get(row[0]) !== row[1]) throw new Error("lost weak entry");
  if (i && row[1].get === keep[i - 1][1].get) throw new Error("shared closure");
  row[1].set(i + 1);
  total += row[1].get() + row[1].twice();
  if (Object.getPrototypeOf(row[2]) !== WeakMap.prototype) throw new Error("lost prototype");
}
console.log("retained", keep.length, total, keep[99][1].get());
