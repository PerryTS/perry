// Method calls on builtin receivers through unproven sites (object model
// step 5, phase 2: the js_bm_ entries). Every receiver arrives as `any`, so
// the site cannot prove its kind; the entry proves kind + pristine prototype
// and otherwise takes the generic call.

function pick(i: number): any {
  const all: any[] = [
    [1, 2, 3],
    "hello world",
    new Map<any, any>([["a", 1]]),
    new Set<any>([1]),
    function (this: any, a: any, b: any) {
      return [this === undefined ? "u" : typeof this, a, b].join(",");
    },
    { push(x: any) { return "obj.push:" + x; }, get(k: any) { return "obj.get:" + k; } },
  ];
  return all[i];
}

const out: string[] = [];
function log(...xs: any[]) {
  out.push(xs.map((x) => (typeof x === "object" ? JSON.stringify(x) : String(x))).join(" "));
}

function round(tag: string) {
  const arr = pick(0);
  log(tag, "push", arr.push(4), arr.length, arr.pop(), arr.join("-"), arr.indexOf(2), arr.includes(3));
  log(tag, "slice", JSON.stringify(arr.slice(1)), pick(1).slice(1, 4), pick(1).indexOf("o"));
  const m = pick(2);
  log(tag, "map", m.get("a"), m.has("a"), m.set("b", 2) === m, m.get("b"), m.delete("a"), m.has("a"));
  const s = pick(3);
  log(tag, "set", s.add(2) === s, s.has(2), s.delete(1), s.has(1));
  const f = pick(4);
  log(tag, "fn", f.call({}, 1, 2), f.call(undefined, 3), f.apply({}, [5, 6]), f.bind({}, 7)(8));
  const o = pick(5);
  log(tag, "obj", o.push(9), o.get("k"));
  const n: any = 12.5;
  log(tag, "num", n.toFixed(1));
}

round("pristine");

// A forwarded pre-growth array alias: the program holds the old head.
const grow: any = [];
const alias: any = grow;
for (let i = 0; i < 100; i++) grow.push(i);
log("alias", alias.length, alias.pop(), alias.push(1000), alias.indexOf(1000), alias.length);

// Own overrides on builtin receivers.
const own: any = [1, 2];
own.push = function (x: any) { return "own.push:" + x; };
log("own-array", own.push(3), own.length);
const fn: any = function () { return "plain"; };
fn.call = function () { return "own.call"; };
log("own-fn", fn.call(null), fn.apply(null, []));
const ownMap: any = new Map();
ownMap.get = function () { return "own.get"; };
log("own-map", ownMap.get("x"));

// A changed [[Prototype]] on a builtin receiver.
const relinked: any = [1, 2, 3];
Object.setPrototypeOf(relinked, { push() { return "proto.push"; }, pop() { return "proto.pop"; } });
log("relinked", relinked.push(1), relinked.pop());

// Patched builtin prototypes: every entry must take the generic call.
const origPush = Array.prototype.push;
const origGet = Map.prototype.get;
const origCall = Function.prototype.call;
const origSlice = String.prototype.slice;
const origAdd = Set.prototype.add;
(Array.prototype as any).push = function (this: any, ...xs: any[]) { return "patched.push:" + xs.length; };
(Map.prototype as any).get = function () { return "patched.get"; };
(String.prototype as any).slice = function () { return "patched.slice"; };
(Set.prototype as any).add = function () { return "patched.add"; };
round("patched");
(Function.prototype as any).call = function () { return "patched.call"; };
log("patched-call", pick(4).call({}, 1, 2));
Function.prototype.call = origCall;
Array.prototype.push = origPush;
Map.prototype.get = origGet;
String.prototype.slice = origSlice;
Set.prototype.add = origAdd;
round("restored");

// A frozen array's push throws.
const frozen: any = Object.freeze([1]);
try { frozen.push(2); log("frozen", "no throw"); } catch (e: any) { log("frozen", e instanceof TypeError); }

// Class constructors are not callable through .call.
class K {}
const kref: any = K;
try { kref.call(null); log("class-call", "no throw"); } catch (e: any) { log("class-call", e instanceof TypeError); }

// An argument that patches the method before the call (split site).
const arr2: any = [1];
const r = arr2.push((Array.prototype.push = function () { return "late"; } as any, 5));
log("late", r);
Array.prototype.push = origPush;

for (const line of out) console.log(line);
