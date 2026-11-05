const tag = Symbol("tag");
const other = Symbol.for("shared");
const target: any = { [tag]: "T", [other]: 42, plain: "p" };
const proxy: any = new Proxy(target, { get(t: any, k: any) { return t[k]; } });
if (proxy[tag] !== "T" || proxy[other] !== 42 || proxy.plain !== "p") throw new Error("symbol get trap");
if (new Proxy(target, {})[tag] !== "T") throw new Error("symbol target forwarding");
class C { [tag] = "instTag"; }
const p3: any = new Proxy(new C(), { get(t: any, k: any) { return t[k]; } });
if (p3[tag] !== "instTag") throw new Error("instance symbol proxy");
if (proxy[Symbol("absent")] !== undefined) throw new Error("absent symbol");
console.log("OK");
