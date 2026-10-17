// Functions answer through their layout: own name/length/prototype, the rest from Function.prototype (node parity).
export {};
const keys = (o: any) => Reflect.ownKeys(o).map((k) => (typeof k === "symbol" ? k.toString() : k)).join(",");
const FP: any = Function.prototype;
const OP: any = Object.prototype;
console.log("FP names", Object.getOwnPropertyNames(FP).join(","));
console.log("FP keys", keys(FP));
console.log("FP own", FP.hasOwnProperty("valueOf"), FP.hasOwnProperty("toLocaleString"), FP.hasOwnProperty("isPrototypeOf"), FP.hasOwnProperty("call"), FP.hasOwnProperty("name"));
console.log("FP name/length", JSON.stringify(FP.name), FP.length, JSON.stringify(Object.getOwnPropertyDescriptor(FP, "name")), JSON.stringify(Object.getOwnPropertyDescriptor(FP, "length")));
console.log("FP inherits", FP.valueOf === OP.valueOf, FP.toLocaleString === OP.toLocaleString, Object.getPrototypeOf(FP) === OP);
function plain(a: any, b: any) { return a; }
const arrow = (x: any) => x;
class K { constructor(a: any) {} static s() {} }
const bound = plain.bind(null, 1);
async function asy(a: any) {}
function* gen(a: any, b: any, c: any) {}
async function* agen() {}
const anon = [function () {}][0];

const cases: any[] = [["plain", plain], ["arrow", arrow], ["class", K], ["bound", bound], ["async", asy], ["gen", gen], ["agen", agen], ["anon", anon]];
for (const [label, f] of cases) {
  console.log(label, "keys", keys(f), JSON.stringify(f.name), f.length, "name" in f, "length" in f, "call" in f, "valueOf" in f);
  console.log(label, "own", f.hasOwnProperty("name"), f.hasOwnProperty("length"), f.hasOwnProperty("valueOf"), f.hasOwnProperty("toString"), f.hasOwnProperty("call"), f.hasOwnProperty("constructor"), f.hasOwnProperty("prototype"), f.hasOwnProperty("caller"));
  console.log(label, "desc", JSON.stringify(Object.getOwnPropertyDescriptor(f, "name")), JSON.stringify(Object.getOwnPropertyDescriptor(f, "length")));
  const proto = Object.getPrototypeOf(f);
  console.log(label, "inherit", f.call === FP.call, f.apply === FP.apply, f.bind === FP.bind, f.toString === FP.toString, f.constructor === proto.constructor, f.valueOf === OP.valueOf, f.toLocaleString === OP.toLocaleString, f.isPrototypeOf === OP.isPrototypeOf, f.hasOwnProperty === OP.hasOwnProperty, f.valueOf() === f);
}
// The same reads at one site over many receivers, and through computed keys.
const names = ["name", "length", "call", "apply", "bind", "toString", "constructor", "valueOf", "toLocaleString", "isPrototypeOf", "prototype", "missing"];
const out: string[] = [];
for (let round = 0; round < 3; round++) {
  for (const [label, f] of cases) {
    out.push(label + ":" + JSON.stringify(f.name) + "/" + f.length + "/" + typeof f.call + "/" + typeof f.apply + "/" + typeof f.bind + "/" + typeof f.toString + "/" + (f.constructor === Object.getPrototypeOf(f).constructor) + "/" + typeof f.valueOf + "/" + typeof f.toLocaleString + "/" + typeof f.isPrototypeOf + "/" + typeof f.missing);
    for (const k of names) out.push(k + "=" + (typeof f[k]) + (k === "name" || k === "length" ? ":" + f[k] : ""));
  }
}
console.log(out.join(" "));
// Own writes shadow; Function.prototype writes reach every function.
function own(a: any) {}
(own as any).toString = () => "mine";
(own as any).extra = 1;
console.log("own write", (own as any).toString(), keys(own), (own as any).call === FP.call);
Object.defineProperty(own, "name", { value: "renamed" });
Object.defineProperty(own, "length", { value: 7 });
console.log("redefined", own.name, own.length, keys(own), JSON.stringify(Object.getOwnPropertyDescriptor(own, "name")));
function del(a: any, b: any) {}
delete (del as any).name;
delete (del as any).length;
console.log("deleted", JSON.stringify(del.name), del.length, del.hasOwnProperty("name"), "name" in del, keys(del));
FP.fpExtra = 42;
console.log("FP write", (plain as any).fpExtra, (arrow as any).fpExtra, (K as any).fpExtra, (bound as any).fpExtra, keys(FP).includes("fpExtra"));
Object.defineProperty(FP, "valueOf", { value: function () { return "fp-valueOf"; }, configurable: true, writable: true });
console.log("FP valueOf", (plain as any).valueOf(), FP.hasOwnProperty("valueOf"), (arrow as any).valueOf === FP.valueOf);
delete FP.valueOf;
console.log("FP valueOf deleted", (plain as any).valueOf === OP.valueOf, FP.hasOwnProperty("valueOf"));
// Own-key order after the first own write: the intrinsic keys come first.
function late(a: any) {}
(late as any).extra = 1;
console.log("late keys", keys(late), Object.getOwnPropertyNames(late).join(","));
function* lateGen() {}
(lateGen as any).extra = 1;
console.log("late gen keys", keys(lateGen));
