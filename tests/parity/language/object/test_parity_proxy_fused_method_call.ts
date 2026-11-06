function makeForwardingProxy(): any {
  const target: any = { kind: "col" };
  target.run = function (v: any) { return v; };
  return new Proxy(target, { get(t: any, k: any) { return t[k]; } });
}
const p: any = makeForwardingProxy();
if (p.run("hi") !== "hi") throw new Error("fused get-trap proxy call");
const f = p.run;
if (f("hi") !== "hi") throw new Error("decomposed proxy call");

const target2: any = {};
target2.echo = function (v: any) { return v; };
const p2: any = new Proxy(target2, {});
if (p2.echo("yo") !== "yo") throw new Error("fused no-trap proxy call");

const target3: any = { tag: "T" };
target3.readTag = function (this: any) { return this.tag; };
const p3: any = new Proxy(target3, { get(t: any, k: any) { return t[k]; } });
if (p3.readTag() !== "T") throw new Error("proxy this binding");

class Base { mapFromDriverValue(v: any) { return v; } }
class Mid extends Base { x = 1; }
class Leaf extends Mid { y = 2; }
const p4: any = new Proxy(new Leaf(), { get(t: any, k: any) { return t[k]; } });
if (p4.mapFromDriverValue("hi") !== "hi") throw new Error("inherited proxy method");

const target5: any = {};
target5.add3 = (a: number, b: number, c: number) => a + b + c;
const p5: any = new Proxy(target5, { get(t: any, k: any) { return t[k]; } });
if (p5.add3(1, 2, 3) !== 6) throw new Error("proxy multi-arg call");
console.log("OK");
