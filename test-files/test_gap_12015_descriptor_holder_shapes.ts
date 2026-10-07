// Descriptor installation retires a warmed read, and every exotic holder
// reflects data/accessor facts from its own shape.
function desc(o: any, k: string) {
  const d: any = Object.getOwnPropertyDescriptor(o, k);
  if (!d) return "missing";
  return ["value" in d ? "data:" + (typeof d.value === "function" ? "function" : String(d.value)) : "accessor:" + typeof d.get + ":" + typeof d.set,
    "writable" in d ? d.writable : "na", d.enumerable, d.configurable].join("|");
}
function read(o: any) { return o.lane12015; }
const holder: any = { lane12015: 10 };
const receiver: any = Object.create(holder);
let sum = 0;
for (let i = 0; i < 2000; i++) sum += read(receiver);
console.log("warm", sum);
Object.defineProperty(holder, "lane12015", { get() { return 41; }, configurable: true });
console.log("memo", read(receiver), desc(holder, "lane12015"));
for (const proto of [Object.prototype, Array.prototype]) {
  let writes = 0;
  Object.defineProperty(proto, "lane12015", {
    get() { return 42; }, set(v: any) { writes += v; }, configurable: true, enumerable: false
  });
  const literal: any = {};
  if (proto === Array.prototype) Object.setPrototypeOf(literal, proto);
  console.log("proto-read", literal.lane12015);
  literal.lane12015 = 3;
  console.log("proto-store", writes, Object.prototype.hasOwnProperty.call(literal, "lane12015"));
  const own: any = { lane12015: 5 };
  if (proto === Array.prototype) Object.setPrototypeOf(own, proto);
  own.lane12015 = 6;
  console.log("own-shadow", own.lane12015, writes, desc(proto, "lane12015"));
  delete (proto as any).lane12015;
}
const array: any = [1, 2];
Object.defineProperty(array, "0", { get() { return 7; }, enumerable: false, configurable: true });
Object.defineProperty(array, "named", { value: 8, writable: false, enumerable: true, configurable: true });
console.log("array", array[0], array.named, desc(array, "0"), desc(array, "named"), desc(array, "length"));
console.log("array-desc-keys", Object.keys(Object.getOwnPropertyDescriptors(array)).join(","));
for (let i = 0; i < 100; i++) array.push(i);
console.log("array-grow", array[0], array.named, desc(array, "0"), desc(array, "named"));
Object.defineProperty(array, "0", { value: 9, writable: true, enumerable: true });
console.log("array-data", array[0], desc(array, "0"));
console.log("builtin", desc(Array.prototype, "map"), desc(Object.prototype, "hasOwnProperty"));
const arrayProtoDescriptors: any = Object.getOwnPropertyDescriptors(Array.prototype);
const objectProtoDescriptors: any = Object.getOwnPropertyDescriptors(Object.prototype);
console.log("builtin-descriptors", arrayProtoDescriptors.map.writable, arrayProtoDescriptors.map.enumerable,
  arrayProtoDescriptors.map.configurable, typeof arrayProtoDescriptors.map.value,
  objectProtoDescriptors.hasOwnProperty.writable, objectProtoDescriptors.hasOwnProperty.enumerable,
  objectProtoDescriptors.hasOwnProperty.configurable, typeof objectProtoDescriptors.hasOwnProperty.value);
