// Method calls on builtin receivers through unproven sites (object model
// step 5, phase 2: the js_bm_ entries). Every receiver arrives as `any`, so
// the site cannot prove its kind; the entry proves kind + pristine prototype
// and otherwise takes the generic call.

// DEC 250 locks the intrinsic prototypes: the patched round's receivers adopt
// user prototypes below them (UAP/UMP/USP/UFP) and the patches go there. A
// primitive string's slice exists only on String.prototype (intrinsic-only):
// that replace is refused (expected file).
const UAP: any = Object.create(Array.prototype);
const UMP: any = Object.create(Map.prototype);
const USP: any = Object.create(Set.prototype);
const UFP: any = Object.create(Function.prototype);
let ADOPT = false;
function pick(i: number): any {
  const v = pickRaw(i);
  const user = [UAP, null, UMP, USP, UFP, null][i];
  if (ADOPT && user) Object.setPrototypeOf(v, user);
  return v;
}
function pickRaw(i: number): any {
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
ADOPT = true;
UAP.push = function (this: any, ...xs: any[]) { return "patched.push:" + xs.length; };
UMP.get = function () { return "patched.get"; };
try { (String.prototype as any).slice = function () { return "patched.slice"; }; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
USP.add = function () { return "patched.add"; };
round("patched");
UFP.call = function () { return "patched.call"; };
log("patched-call", pick(4).call({}, 1, 2));
ADOPT = false;
delete UFP.call;
delete UAP.push;
delete UMP.get;
delete USP.add;
try { String.prototype.slice = origSlice; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
void origCall, origPush, origGet, origAdd;
round("restored");

// A frozen array's push throws.
const frozen: any = Object.freeze([1]);
try { frozen.push(2); log("frozen", "no throw"); } catch (e: any) { log("frozen", e instanceof TypeError); }

// Class constructors are not callable through .call.
class K {}
const kref: any = K;
try { kref.call(null); log("class-call", "no throw"); } catch (e: any) { log("class-call", e instanceof TypeError); }

// An argument that writes a method the receiver does not read (split site):
// arr2's chain is Array.prototype, so its push stays the builtin.
const arr2: any = [1];
const r = arr2.push((UAP.push = function () { return "late"; } as any, 5));
log("late", r);
delete UAP.push;

for (const line of out) console.log(line);
