// A key add must ask the WHOLE prototype chain: a setter (or a non-writable
// property) on any hop intercepts it, including hops past the receiver's
// recorded prototype and hops changed after the add path was warmed.

const log: string[] = [];

// 1. A setter declared on a parent class's prototype.
class A {
  set x(v: number) {
    log.push("A.x=" + v);
  }
}
class B extends A {}
class C extends B {}
const c: any = new C();
c.x = 5;
console.log("declared:", log.join(","), Object.keys(c).join(","), Object.prototype.hasOwnProperty.call(c, "x"));

// 2. A setter defined on a parent class's prototype AFTER instances warmed the add.
class P {}
class Q extends P {}
class R extends Q {}
function addY(o: any, v: number) {
  o.y = v;
  return o;
}
for (let i = 0; i < 50; i++) addY(new R(), i);
Object.defineProperty(P.prototype, "y", {
  set(v: number) {
    log.push("P.y=" + v);
  },
  get() {
    return "P.y";
  },
  configurable: true,
});
const r = addY(new R(), 99);
console.log("defined late:", log.join(","), Object.keys(r).join(","), r.y);

// 3. The same through dynamic keys.
const k = ["y", "z"][0];
const r2: any = new R();
r2[k] = 7;
console.log("dynamic key:", log.join(","), Object.keys(r2).join(","));

// 4. A non-writable property on the grandparent prototype.
Object.defineProperty(P.prototype, "ro", { value: 1, writable: false, configurable: true });
const r3: any = new R();
try {
  r3.ro = 2;
  console.log("ro assigned", r3.ro, Object.keys(r3).join(","));
} catch (e) {
  console.log("ro threw", (e as Error).constructor.name, r3.ro, Object.keys(r3).join(","));
}

// 5. Object.create chains deeper than the recorded prototype.
const g: any = {};
const m: any = Object.create(g);
function addW(v: number) {
  const o: any = Object.create(m);
  o.w = v;
  return o;
}
for (let i = 0; i < 50; i++) addW(i);
Object.defineProperty(g, "w", {
  set(v: number) {
    log.push("g.w=" + v);
  },
  configurable: true,
});
const o5 = addW(5);
console.log("create chain:", log.join(","), Object.keys(o5).join(","));

// 6. A setter added to the middle hop of a class chain by plain assignment of an accessor holder.
class S {}
class T extends S {}
const t1: any = new T();
t1.q = 1;
Object.defineProperty(T.prototype, "q", {
  set(v: number) {
    log.push("T.q=" + v);
  },
  configurable: true,
});
const t2: any = new T();
t2.q = 2;
console.log("middle hop:", log.join(","), Object.keys(t1).join(","), Object.keys(t2).join(","));
