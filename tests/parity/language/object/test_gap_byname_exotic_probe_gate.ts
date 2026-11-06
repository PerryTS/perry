// Type-erased reads, through named and computed keys, exercise the common
// runtime entry on ordinary shapes and each native receiver kind.
function read(o: any, k: string): any { return o[k]; }
function describe(label: string, o: any): void {
  console.log(label, read(o, "marker"), read(o, "missing"),
    typeof read(o, "constructor"), o.marker);
}

const plain: any = { marker: 11, length: 12, size: 13, _entries: 14 };
describe("plain", plain);
console.log("plain special names", read(plain, "length"), read(plain, "size"),
  read(plain, "_entries"), read(plain, "getTime"), read(plain, "years"));
// A literal that resembles the native structural URLSearchParams probe is
// still an ordinary object. The authoritative shape must rule that probe out.
const mimic: any = { _entries: [] };
console.log("ordinary entries", read(mimic, "size"), typeof read(mimic, "get"),
  typeof read(mimic, "append"));
const inherited: any = Object.create({ inherited: 21 });
inherited.marker = 22;
describe("inherited", inherited);
console.log("inherited key", read(inherited, "inherited"));
Object.defineProperty(plain, "accessor", { get() { return this.marker + 1; } });
console.log("accessor", read(plain, "accessor"));
const dictionary: any = {};
for (let i = 0; i < 150; i++) dictionary["key" + i] = i;
console.log("dictionary", read(dictionary, "key149"), read(dictionary, "missing"));

const array: any = [31, 32];
array.marker = 33;
describe("array", array);
console.log("array keys", read(array, "length"), read(array, "0"), read(array, "1"));
const typed: any = new Uint16Array([41, 42]);
typed.marker = 43;
describe("typed", typed);
console.log("typed keys", read(typed, "length"), read(typed, "byteLength"),
  read(typed, "byteOffset"), read(typed, "BYTES_PER_ELEMENT"),
  read(typed, "0"), typeof read(typed, "subarray"));
const bytes: any = new Uint8Array([44, 45]);
console.log("bytes keys", read(bytes, "length"), read(bytes, "byteLength"),
  read(bytes, "0"), typeof read(bytes, "slice"));

const date: any = new Date(0);
date.marker = 51;
describe("date", date);
console.log("date methods", typeof read(date, "getTime"), typeof read(date, "toISOString"));
Object.defineProperty(date, "getTime", { value: 52 });
console.log("date own shadow", read(date, "getTime"));

const url: any = new URL("https://example.com/a?x=1&x=2");
url.marker = 61;
describe("url", url);
console.log("url keys", read(url, "hostname"), read(url, "pathname"),
  typeof read(url, "toString"));
const params: any = url.searchParams;
console.log("search params", read(params, "size"), typeof read(params, "get"),
  typeof read(params, "append"), read(params, "missing"));

const map: any = new Map([["x", 1]]);
map.marker = 71;
describe("map", map);
console.log("map keys", read(map, "size"), typeof read(map, "get"));
const set: any = new Set([1, 2]);
set.marker = 72;
describe("set", set);
console.log("set keys", read(set, "size"), typeof read(set, "has"));

function fn(a: number, b: number): number { return a + b; }
(fn as any).marker = 81;
describe("function", fn);
console.log("function keys", read(fn, "name"), read(fn, "length"), typeof read(fn, "call"));
class RecordValue {
  marker = 91;
  get accessor() { return this.marker + 1; }
  method() { return this.marker; }
}
const instance: any = new RecordValue();
describe("class", instance);
console.log("class keys", read(instance, "accessor"), typeof read(instance, "method"));

const duration: any = new Temporal.Duration(1, 2, 3, 4);
duration.marker = 101;
describe("temporal duration", duration);
console.log("temporal keys", read(duration, "years"), read(duration, "months"),
  read(duration, "days"), typeof read(duration, "abs"));
const temporalDate: any = new Temporal.PlainDate(2020, 5, 6);
console.log("temporal date", read(temporalDate, "year"), read(temporalDate, "month"),
  read(temporalDate, "day"), typeof read(temporalDate, "add"));
Object.defineProperty(duration, "years", { value: 102 });
console.log("temporal own shadow", read(duration, "years"));
