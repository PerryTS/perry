// #11910: `recv.m(args)` reads `recv.m` (getter, Proxy trap, nullish TypeError)
// before the arguments run (ECMA-262 13.3.6.1), on every call path; an
// optional call reads the method once, and an optional chain evaluates its
// upstream links once.
const log: string[] = [];
function arg(tag: string = "arg"): number { log.push(tag); return 1; }
function flush(name: string): void { console.log(name + ": " + log.join(",")); log.length = 0; }
{
  // 1. accessor method
  const o1: any = { get m() { log.push("get"); return (x: number) => { log.push("call"); return x; }; } };
  o1.m(arg());
  flush("getter");
  // 2. Proxy get trap
  const p: any = new Proxy({ m(x: number) { log.push("call"); return x; } }, {
    get(t: any, k: any, r: any) { log.push("trap:" + String(k)); return Reflect.get(t, k, r); },
  });
  p.m(arg());
  flush("proxy");
  // 3. throwing getter: arg must NOT run
  const o3: any = { get m(): any { log.push("get"); throw new Error("boom"); } };
  try { o3.m(arg()); } catch (e: any) { log.push("caught:" + e.message); }
  flush("throwing getter");
  // 4. non-callable member, args still must not run after the TypeError? (spec: args evaluate first, then IsCallable check)
  const o4: any = { m: 5 };
  try { o4.m(arg()); } catch (e: any) { log.push("caught:" + (e instanceof TypeError)); }
  flush("non-callable");
  // 5. optional call
  const o5: any = { get m() { log.push("get"); return (x: number) => x; } };
  o5.m?.(arg());
  flush("optional call");
  const o5b: any = { get m() { log.push("get"); return undefined; } };
  o5b.m?.(arg());
  flush("optional call undefined");
  // 6. computed key
  const o6: any = { get m() { log.push("get"); return (x: number) => x; } };
  function key(): string { log.push("key"); return "m"; }
  o6[key()](arg());
  flush("computed");
  // 7. getter on the prototype chain / class getter
  class C { get m() { log.push("get"); return (x: number) => x; } }
  new C().m(arg());
  flush("class getter");
  class D extends C { run() { return (super.m as any)(arg()); } runCall() { return super.m(arg()); } }
  new D().runCall();
  flush("super call");
  // 8. this binding of the getter result
  const o8: any = { v: 7, get m() { log.push("get"); return function (this: any, x: number) { log.push("this=" + this.v); return x; }; } };
  o8.m(arg());
  flush("this binding");
  // 9. receiver is a primitive with a getter on its prototype
  Object.defineProperty(String.prototype, "gm", { configurable: true, get() { log.push("get"); return function (x: number) { return x; }; } });
  ("s" as any).gm(arg());
  flush("string proto getter");
  delete (String.prototype as any).gm;
  // 10. several effectful args
  const o10: any = { get m() { log.push("get"); return (a: number, b: number) => a + b; } };
  o10.m(arg("a"), arg("b"));
  flush("two args");
  // 11. spread
  const o11: any = { get m() { log.push("get"); return (...a: number[]) => a.length; } };
  o11.m(...[arg("s")]);
  flush("spread");
  // 12. property replaced by the argument expression: the OLD value is called
  const o12: any = { m(x: any) { log.push("old"); }, };
  o12.m((o12.m = () => log.push("new"), 1));
  flush("replaced by arg");
  // 13. receiver itself reassigned by an argument: receiver value is read first
  let r13: any = { m() { log.push("first"); } };
  r13.m((r13 = { m() { log.push("second"); } }, 1));
  flush("receiver reassigned");
  // 14. getter on a class instance reached via a typed receiver
  class E { get f(): (x: number) => number { log.push("get"); return (x) => x; } }
  const e: E = new E();
  e.f(arg());
  flush("typed class getter");
  // 15. defineProperty accessor added later to a plain object that had a data property
  const o15: any = { m: (x: number) => x };
  Object.defineProperty(o15, "m", { get() { log.push("get"); return (x: number) => x; }, configurable: true });
  o15.m(arg());
  flush("redefined accessor");
  // 16. Array / builtin receiver with getter method name installed on Array.prototype
  Object.defineProperty(Array.prototype, "gq", { configurable: true, get() { log.push("get"); return function (x: number) { return x; }; } });
  ([1, 2] as any).gq(arg());
  flush("array proto getter");
  delete (Array.prototype as any).gq;
}
{
  // computed key, property replaced by the argument
  const c1: any = { m(x: any) { log.push("old"); } };
  const k1 = "m";
  c1[k1]((c1[k1] = () => log.push("new"), 1));
  flush("computed replaced by arg");
  // computed key on a Proxy
  const p2: any = new Proxy({ m(x: number) { log.push("call"); return x; } }, {
    get(t: any, k: any, r: any) { log.push("trap:" + String(k)); return Reflect.get(t, k, r); },
  });
  function key(): string { log.push("key"); return "m"; }
  p2[key()](arg());
  flush("proxy computed");
  // spread on a Proxy
  p2.m(...[arg("s")]);
  flush("proxy spread");
  // computed spread with a getter
  const c3: any = { get m() { log.push("get"); return (...a: number[]) => a.length; } };
  c3[key()](...[arg("s")]);
  flush("computed spread");
  // nullish receiver: TypeError before the argument
  const n4: any = null;
  try { n4.m(arg()); } catch (e: any) { log.push("caught:" + (e instanceof TypeError)); }
  flush("null receiver");
  try { n4[key()](arg()); } catch (e: any) { log.push("caught:" + (e instanceof TypeError)); }
  flush("null receiver computed");
  // optional call, pure arguments: one read
  const o5: any = { get m() { log.push("get"); return (x: number) => { log.push("call"); return x; }; } };
  o5.m?.(1);
  flush("optional pure");
  o5.m?.(arg());
  flush("optional effectful");
  // optional call on a missing method: no argument runs
  const o6: any = {};
  const r6 = o6.nope?.(arg());
  log.push(String(r6));
  flush("optional missing");
  // optional call on a string builtin (#4814) and a plain method
  const s7: any = "a/b";
  log.push(String(s7.split?.("/").length));
  log.push(String(s7.split?.(String.fromCharCode(47 + arg("x") - 1)).length));
  flush("optional string builtin");
  const o7: any = { v: 3, m(x: number) { return this.v + x; } };
  log.push(String(o7.m?.(arg())));
  flush("optional this");
  // optional call where the getter returns undefined
  const o8: any = { get m() { log.push("get"); return undefined; } };
  log.push(String(o8.m?.(arg())));
  flush("optional getter undefined");
  // super spread and super with getter, dynamic base
  class B { get g() { log.push("get"); return (...a: number[]) => a.length; } m(x: number) { log.push("m"); return x; } }
  class D extends B {
    spread() { return super.g(...[arg("s")]); }
    plain() { return super.m(arg()); }
  }
  new D().spread();
  flush("super spread");
  new D().plain();
  flush("super method");
  // forced collections inside the argument: the early method survives
  const o9: any = { get m() { return function (this: any, a: any) { return this.tag + a.length; }; }, tag: "t" };
  function churn(): number[] { let a: any[] = []; for (let i = 0; i < 200000; i++) a.push({ i }); return a.slice(0, 3) as any; }
  let total = "";
  for (let i = 0; i < 5; i++) total += o9.m(churn());
  console.log("gc in arg: " + total);
  // receiver is a class instance with a method, effectful args (site hit path)
  class K { n = 0; inc(x: number) { this.n += x; return this.n; } }
  const kk = new K();
  let acc = 0;
  for (let i = 0; i < 1000; i++) acc = kk.inc(i % 3 === 0 ? arg("i").valueOf() : 1);
  log.length = 0;
  console.log("site hit: " + acc);
}
{
  const o: any = { get m() { log.push("get"); return (x: any) => { log.push("call"); return "ab"; }; }, n: null as any };
  log.push(String(o.m?.(arg()).length)); flush("call then member");
  log.push(String(o.m?.(arg()).toString())); flush("call then method");
  log.push(String(o.m?.(arg())[0])); flush("call then index");
  const a: any = { get b() { log.push("getb"); return { c: { d: 4 } }; } };
  log.push(String(a?.b.c.d)); flush("member chain");
  const u: any = undefined;
  log.push(String(u?.b.c.d)); flush("nullish member chain");
  log.push(String(o.nope?.(arg()).length)); flush("missing call then member");
  try { log.push(String(o.n?.x.y)); } catch (e: any) { log.push("caught:" + (e instanceof TypeError)); } flush("null then member");
  const z: any = { b: null };
  try { log.push(String(z?.b.c)); } catch (e: any) { log.push("caught:" + (e instanceof TypeError)); } flush("non-optional link on null");
  log.push(String(z?.b?.c)); flush("optional link on null");
}
{
  // An array receiver's OWN accessor runs before the arguments, however it
  // was installed, after the array grew, and on the spread form. The
  // receivers come out of an `any[]` so no literal refines their type.
  const getPush = function (this: any) { log.push("get"); return function (...x: number[]) { return x.length; }; };
  const rs: any[] = [[1, 2], [1], [], [0], [3]];
  Object.defineProperty(rs[0], "push", { configurable: true, get: getPush });
  rs[0].push(arg());
  flush("array own getter");
  rs[1].__defineGetter__("push", getPush);
  rs[1].push(arg());
  flush("array __defineGetter__");
  Object.defineProperty(rs[2], "push", { configurable: true, get: getPush });
  for (let i = 0; i < 100; i++) Array.prototype.push.call(rs[2], i);
  log.length = 0;
  rs[2].push(arg());
  flush("array own getter after growth");
  Object.defineProperty(rs[3], "push", { configurable: true, get: getPush });
  function mk(): number[] { log.push("mk"); return [1, 2]; }
  log.push(String(rs[3].push(...mk(), arg())));
  flush("array own getter spread");
  log.push(String(rs[4].push(...mk(), arg())));
  flush("plain array spread");
  log.push(String(rs[4].length));
  flush("plain array pushed");
  // Map / Set receivers: an own accessor runs first; a bare one by name.
  const ms: any[] = [new Map(), new Set(), new Map()];
  Object.defineProperty(ms[0], "set", { configurable: true, get: getPush });
  ms[0].set(arg());
  flush("map own getter");
  Object.defineProperty(ms[1], "add", { configurable: true, get: getPush });
  ms[1].add(arg());
  flush("set own getter");
  ms[2].set(arg(), 2);
  log.push(String(ms[2].size));
  flush("plain map");
}
