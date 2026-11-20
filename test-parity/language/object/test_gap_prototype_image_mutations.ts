{
const P: any = { x: "p", m() { return this.x; } };
const o: any = Object.create(P); o.own = "child";
P.added = "late";
console.log(P.x, P.added, o.x, o.added, Object.keys(P).join(","));
console.log(JSON.stringify(P), JSON.stringify(o));
console.log(Object.getPrototypeOf(P) === Object.prototype, Object.hasOwn(P, "added"), "added" in P);
console.log(JSON.stringify(Object.getOwnPropertyDescriptors(P)));
}
{
const P: any = { x: "p", pad: "stay" };
const o: any = Object.create(P); o.own = "child";
delete P.x;
console.log(P.x, o.x, P.pad, Object.keys(P).join(","));
P.x = "again";
console.log(P.x, o.x, Object.keys(P).join(","));
}
{
const P: any = { x: 1, m() { return "first:" + this.x; } };
const o: any = Object.create(P); o.own = "child";
P.m = function() { return "second:" + this.x; };
console.log(P.m(), o.m());
console.log(Reflect.get(P, "m").call(o));
console.log(Reflect.ownKeys(P).map(String).join(","));
const symbol = Symbol("s");
P[symbol] = "symbol";
console.log(P[symbol], o[symbol], Object.getOwnPropertySymbols(P).length);
Object.defineProperty(P, "x", { value: 7, writable: false });
console.log(Reflect.set(P, "x", 8), P.x, o.x);
}
{
const P: any = { x: "p" };
const a: any = Object.create(P); a.y = "a";
const b: any = Object.create(P); b.z = "b";
console.log(P.x, a.x, b.x, a.y, b.z);
console.log(JSON.stringify(structuredClone(P)), Object.getPrototypeOf(P) === Object.prototype);
console.log(Object.hasOwn(a, "x"), "x" in a, Object.keys(P).join(","));
}
{
const P: any = { x: "p" };
const o: any = Object.create(P); o.own = "child";
const other: any = { inherited: "other" };
Object.setPrototypeOf(P, other);
console.log(P.x, P.inherited, o.x, o.inherited);
P.inherited = "own";
console.log(P.inherited, o.inherited, Object.getPrototypeOf(P) === other);
}
{
const source: any = { x: "p" };
function add(o: any, key: string) { o[key] = "late"; }
add(source, ["ex", "tra"].join(""));
const P: any = { ...source };
const o: any = Object.create(P); o.own = "child";
console.log(P.x, P.extra, o.x, o.extra, Object.keys(P).join(","));
}
