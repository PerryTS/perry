// A factory wrapper adds keys beyond the base's inline width. Receiver
// selection must be checked again after every reassignment and mutation.
function createRecord(value) {
  return {
    first: value, second: value + 1, next: null,
    a: null, b: null, c: null, d: null, e: null, f: null, g: null, h: true,
  };
}
function extendRecord(value) {
  const record = createRecord(value);
  record.observers = [value];
  record.observerSlots = [0];
  record.comparator = undefined;
  return record;
}
function read(record) {
  return [record.first, record.second, record.observers, record.comparator];
}
function walk(node) {
  let total = 0;
  while (node) {
    total += node.first + node.second;
    node = node.next;
  }
  return total;
}
const base = createRecord(2);
const wide = extendRecord(5);
console.log("base", Object.keys(base).join(","), JSON.stringify(read(base)));
console.log("wide", Object.keys(wide).join(","), JSON.stringify(read(wide)));
console.log("proto", Object.getPrototypeOf(base) === Object.prototype,
  Object.getPrototypeOf(wide) === Object.prototype);
base.next = wide;
console.log("walk", walk(base));
delete wide.observers;
console.log("deleted", Object.keys(wide).join(","), JSON.stringify(read(wide)));
wide.observers = [9];
console.log("readded", Object.keys(wide).join(","), JSON.stringify(read(wide)));
const proto = Object.create(Object.prototype);
Object.defineProperty(proto, "first", { get() { return 40; }, configurable: true });
Object.setPrototypeOf(wide, proto);
delete wide.first;
console.log("inherited", JSON.stringify(read(wide)), walk(wide));
Object.defineProperty(base, "first", { get() { return 20; }, configurable: true });
console.log("own-accessor", JSON.stringify(read(base)));
const foreign = Object.create(null);
foreign.first = 11;
foreign.second = 12;
foreign.next = null;
console.log("foreign", walk(foreign), JSON.stringify(read(foreign)));
function switchKinds(record, values) {
  const output = [];
  for (let i = 0; i < values.length; i++) {
    record = values[i];
    output.push([record.first, record.second, record.next, record.observers]);
  }
  return JSON.stringify(output);
}
console.log("reassigned", switchKinds(base, [base, wide, foreign, 7, "text"]));
Object.preventExtensions(foreign);
console.log("nonextensible", Reflect.set(foreign, "observers", [3]));
console.log("refused", Object.keys(foreign).join(","), JSON.stringify(read(foreign)));
const reversed = createRecord(8);
reversed.observerSlots = [4];
reversed.observers = [7];
console.log("reversed", Object.keys(reversed).join(","), JSON.stringify(read(reversed)));
const dynamicKey = ["extra", "unknown"].join("_");
reversed[dynamicKey] = 19;
console.log("computed", Object.keys(reversed).join(","), reversed[dynamicKey]);
const proxied = new Proxy(wide, {
  get(target, key, receiver) {
    if (key === "first") return 30;
    return Reflect.get(target, key, receiver);
  },
});
console.log("proxy", walk(proxied), JSON.stringify(read(proxied)));
let checksum = 0;
for (let i = 0; i < 30000; i++) {
  const item = extendRecord(i);
  if (i % 3000 === 0) checksum += walk(item);
}
console.log("churn", checksum, Object.getPrototypeOf(wide) === proto, JSON.stringify(read(wide)));
