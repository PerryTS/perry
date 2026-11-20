const methods = ["forEach", "map", "filter", "some", "every", "find", "findIndex", "findLast", "findLastIndex"];
const shapes = ["plain", "bound", "proxy", "rest", "padded"];
for (const method of methods) {
  for (const shape of shapes) {
    const calls: string[] = [];
    const source = [2, 3, 4];
    const context = { label: "receiver" };
    let callback: any = function(this: any, value: number, index: number, receiver: any) {
      calls.push(value + ":" + index + ":" + (receiver === source) + ":" + this.label);
      return value === 3;
    };
    if (shape === "bound") callback = callback.bind(context);
    if (shape === "proxy") callback = new Proxy(callback, {});
    if (shape === "rest") callback = function(this: any, ...args: any[]) {
      calls.push(args[0] + ":" + args[1] + ":" + (args[2] === source) + ":" + this.label);
      return args[0] === 3;
    };
    if (shape === "padded") callback = function(this: any, value: number, index: number, receiver: any, missing: any) {
      calls.push(value + ":" + index + ":" + (receiver === source) + ":" + this.label + ":" + missing);
      return value === 3;
    };
    const result = (Array.prototype as any)[method].call(source, callback, context);
    console.log(method, shape, JSON.stringify(result), calls.join("|"));
  }
}
for (const method of ["reduce", "reduceRight"]) {
  const source = [2, 3, 4];
  const calls: string[] = [];
  const result = (Array.prototype as any)[method].call(source, (a: number, v: number, i: number, receiver: any) => {
    calls.push(i + ":" + (receiver === source));
    return a + v;
  }, 10);
  console.log(method, result, calls.join("|"));
}
let lengthReads = 0;
const empty = { get length() { lengthReads++; return 0; } };
try { Array.prototype.map.call(empty, null as any); } catch (e: any) { console.log(e.name, lengthReads); }
const revoke = Proxy.revocable((v: number) => v, {});
revoke.revoke();
try { Array.prototype.find.call([1], revoke.proxy); } catch (e: any) { console.log("revoked", e.name); }

const changed = [10, 20, 30];
const trace: string[] = [];
const mapped = Array.prototype.map.call(changed, (value: number, index: number) => {
  trace.push("call:" + index + ":" + value);
  if (index === 0) Object.defineProperty(changed, "1", {
    configurable: true,
    get() { trace.push("get:1"); return 7; },
  });
  return value + index;
});
console.log("live accessor", JSON.stringify(mapped), trace.join("|"));
const retargeted = [10, 20];
const inherited = Array.prototype.map.call(retargeted, (value: number, index: number) => {
  if (index === 0) {
    delete retargeted[1];
    Object.setPrototypeOf(retargeted, { 1: 99 });
  }
  return value;
});
console.log("live prototype", JSON.stringify(inherited));
