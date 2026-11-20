// Non-constant `new Function` source replaces builtin prototype methods. The
// compiler cannot see these writes; the call sites below must still call the
// replacements (the intrinsic prototypes' shadow bits route them).
// DEC 250 locks the intrinsic prototypes: the source patches user prototypes
// below them (globalThis.UAP/UMP/USP/UFP) that the receivers adopt. A primitive
// string's slice exists only on String.prototype (intrinsic-only): that
// replace is refused (expected file) and the other patches still land.
const G: any = globalThis;
G.UAP = Object.create(Array.prototype);
G.UMP = Object.create(Map.prototype);
G.USP = Object.create(Set.prototype);
G.UFP = Object.create(Function.prototype);
const arr: number[] = Object.setPrototypeOf([3, 1, 2], G.UAP);
const m: Map<string, number> = Object.setPrototypeOf(new Map<string, number>([["a", 1]]), G.UMP);
const s: Set<number> = Object.setPrototypeOf(new Set<number>([1]), G.USP);
const str = "hello";
function f(x: number) { return x + 1; }
Object.setPrototypeOf(f, G.UFP);
const before = [arr.join("-"), arr.indexOf(1), arr.map((x) => x * 2).join(","), m.get("a"), s.has(1),
    str.slice(1), f.call(null, 1), arr.includes(2)];
console.log(JSON.stringify(before));
arr.push(4);
const body = process.argv[2] || [
    "UAP.join = function () { return 'J' + this.length; };",
    "UAP.indexOf = function () { return -7; };",
    "UAP.map = function () { return ['M']; };",
    "UAP.push = function (v) { return 'P' + v; };",
    "UAP.includes = function () { return 'I'; };",
    "UMP.get = function (k) { return 'G' + k; };",
    "USP.has = function () { return 'H'; };",
    "try { String.prototype.slice = function () { return 'S'; }; } catch (e) { console.log('DEC 250: ' + e.message); }",
    "UFP.call = function () { return 'C'; };",
].join("\n");
new Function(body)();
const after = [arr.join("-"), arr.indexOf(1), arr.map((x) => x * 2).join(","), m.get("a"), s.has(1),
    str.slice(1), f.call(null, 1), arr.includes(2), arr.push(9), arr.length];
console.log(JSON.stringify(after));
function loop(a: number[]) { let out = ""; for (let i = 0; i < 2; i++) out += a.join("+") + a.indexOf(3); return out; }
console.log(loop(Object.setPrototypeOf([3, 4], G.UAP)));
