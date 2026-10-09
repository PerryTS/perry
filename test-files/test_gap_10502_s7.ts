// Refs #10502 (S7): with the class-table latches and generation counters
// deleted, every answer below must come from the shapes of the objects on the
// chain. Each block warms a site first, then mutates the chain.

class Root { m() { return "root"; } get g() { return "root-g"; } static s() { return "Root.s"; } }
class Base extends Root { m() { return "base:" + super.m(); } n() { return "base-n"; } }
class Sub extends Base { k = 1; }
class Other { m() { return "other"; } n() { return "other-n"; } }

function callM(o: any) { return o.m(); }
function callN(o: Base) { return o.n(); }
function readG(o: any) { return o.g; }
const sub = new Sub();
const base = new Base();
function warm(rounds: number) {
  let out = "";
  for (let i = 0; i < rounds; i++) out = callM(sub) + "|" + callN(sub) + "|" + readG(sub) + "|" + callM(base);
  return out;
}
console.log("warm", warm(50));

// Method values: an inherited member is the declaring class's own object.
console.log("identity", Sub.prototype.m === Base.prototype.m, Base.prototype.m === Root.prototype.m);
const mv = sub.m;
console.log("method value", mv === Base.prototype.m, mv.call(base), typeof mv, mv.name, mv.length);
const bound = sub.n.bind(base);
console.log("bound", bound());

// Patch after warm-up: a prototype store, then a delete back to the parent.
Base.prototype.n = function () { return "patched-n"; };
console.log("patch", warm(5));
delete (Base.prototype as any).n;
try { console.log("deleted", warm(1)); } catch (e) { console.log("deleted throws", (e as Error).constructor.name); }
(Root.prototype as any).n = function () { return "root-n"; };
console.log("root n", warm(3));

// Accessor installs over a warmed read and a warmed method.
Object.defineProperty(Base.prototype, "g", { get() { return "base-g"; }, configurable: true });
console.log("accessor", warm(3));
Object.defineProperty(Base.prototype, "m", { get() { return () => "getter-m"; }, configurable: true });
console.log("accessor method", warm(3));
delete (Base.prototype as any).m;
delete (Base.prototype as any).g;
console.log("accessors removed", warm(3));

// A setter installed on a prototype after stores were warmed.
class Store { v = 0; }
const st = new Store();
function put(o: any, x: number) { o.w = x; }
for (let i = 0; i < 30; i++) put(new Store(), i);
let seen = 0;
Object.defineProperty(Store.prototype, "w", { set(x: number) { seen = x; }, configurable: true });
put(st, 42);
console.log("setter", seen, Object.prototype.hasOwnProperty.call(st, "w"));

// setPrototypeOf on a class prototype.
Object.setPrototypeOf(Base.prototype, Other.prototype);
console.log("relink class proto", warm(3), sub instanceof Root, sub instanceof Other, sub instanceof Base);
Object.setPrototypeOf(Base.prototype, Root.prototype);
console.log("relink back", warm(3), sub instanceof Root, sub instanceof Other);

// setPrototypeOf on an instance.
const lone = new Sub();
for (let i = 0; i < 20; i++) callM(lone);
Object.setPrototypeOf(lone, Other.prototype);
console.log("relink instance", callM(lone), lone instanceof Sub, lone instanceof Other, callM(sub));

// Statics: inherited through the constructor chain, then relinked.
class Kid extends Base {}
function callS(c: any) { return c.s(); }
for (let i = 0; i < 20; i++) callS(Kid);
console.log("static", callS(Kid));
(Base as any).s = () => "Base.s";
console.log("static patched", callS(Kid));
Object.setPrototypeOf(Kid, { s: () => "detached.s" });
console.log("static relinked", callS(Kid), callS(Base));

// super after the parent prototype changes.
class P { who() { return "P"; } }
class Q extends P { who() { return "Q>" + super.who(); } }
const q = new Q();
for (let i = 0; i < 20; i++) q.who();
P.prototype.who = function () { return "P2"; };
console.log("super", q.who());

// toJSON / then reached through a class expression's computed members.
const mk = (k: string) => class { [k]() { return { k }; } x = 1; };
const plain = new (mk("a"))();
for (let i = 0; i < 20; i++) JSON.stringify(plain);
console.log("toJSON before", JSON.stringify(plain));
const withToJSON = new (mk("toJSON"))();
console.log("toJSON after", JSON.stringify(withToJSON), JSON.stringify(plain));

const mkThen = (k: string) => class { [k](resolve: (v: string) => void) { resolve("thenable:" + k); } };
async function thenables() {
  const a = await Promise.resolve(new (mkThen("other"))());
  console.log("then before", typeof a);
  const b = await Promise.resolve(new (mkThen("then"))());
  console.log("then after", b);
}
thenables().then(() => {
  // for-in after Object.prototype gains an enumerable key.
  const rec = { a: 1, b: 2 };
  const keys = () => { const ks: string[] = []; for (const k in rec) ks.push(k); return ks.join(","); };
  for (let i = 0; i < 20; i++) keys();
  (Object.prototype as any).zz = 1;
  console.log("for-in", keys());
  delete (Object.prototype as any).zz;
  console.log("for-in restored", keys());

  // instanceof Object once a class prototype stands on null.
  class Detached { d() { return 1; } }
  const det = new Detached();
  for (let i = 0; i < 20; i++) det instanceof Object;
  Object.setPrototypeOf(Detached.prototype, null);
  console.log("instanceof Object after", det instanceof Object, det instanceof Detached, sub instanceof Object);
});
