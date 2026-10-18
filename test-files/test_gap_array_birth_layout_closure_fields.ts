// Array birth layouts survive closure collection and remain reservations:
// aliases, computed writes, deletion and re-addition use ordinary own-key order.
const make = () => {
  const left: any = [1, 2];
  const right: any = Array.from([3, 4]);
  const sliced: any = [5, 6, 7].slice(1);
  const one: any = { items: left };
  const two: any = { items: right };
  one.items.pos = 10;
  two.items.end = 20;
  sliced.extra = 30;
  const alias = one;
  alias.items.late = 40;
  const update = () => {
    one.items.end = 50;
    return one.items;
  };
  const updated = update();
  delete updated.pos;
  updated.pos = 60;
  const key = process.argv[2] || "dynamic";
  updated[key] = 70;
  for (const a of [left, right, sliced]) {
    console.log(JSON.stringify(a), Object.keys(a).join("|"),
      a.pos, a.end, a.extra, a.late, a[key], a.length);
  }
  console.log("separate", Object.hasOwn(right, "pos"), Object.hasOwn(left, "extra"));
};
make();
