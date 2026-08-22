class Base { static tag = "B"; }
class Mid extends Base {}
class Leaf extends Mid {
  n: string;
  constructor(n: string) { super(); this.n = n; }
}

const handler = { get(t: any, k: any) { return t[k]; } };
const proxy: any = new Proxy(new Leaf("x"), handler);
const proto = Object.getPrototypeOf(proxy);
if (proto?.constructor?.name !== "Leaf") throw new Error("proto.constructor");
if (!(proxy instanceof Leaf) || !(proxy instanceof Mid) || !(proxy instanceof Base) || !(proxy instanceof Object)) {
  throw new Error("proxy instanceof chain");
}
if (proxy.n !== "x") throw new Error("proxy member forwarding");

const proxy2: any = new Proxy(proxy, handler);
if (!(proxy2 instanceof Leaf) || Object.getPrototypeOf(proxy2)?.constructor?.name !== "Leaf") {
  throw new Error("nested proxy prototype");
}
console.log("OK");
