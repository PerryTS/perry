// Non-constant `new Function` source replaces builtin prototype methods. The
// compiler cannot see these writes; the call sites below must still call the
// replacements (the intrinsic prototypes' shadow bits route them).
const arr: number[] = [3, 1, 2];
const m = new Map<string, number>([["a", 1]]);
const s = new Set<number>([1]);
const str = "hello";
function f(x: number) { return x + 1; }
const before = [arr.join("-"), arr.indexOf(1), arr.map((x) => x * 2).join(","), m.get("a"), s.has(1),
    str.slice(1), f.call(null, 1), arr.includes(2)];
console.log(JSON.stringify(before));
arr.push(4);
const body = process.argv[2] || [
    "Array.prototype.join = function () { return 'J' + this.length; };",
    "Array.prototype.indexOf = function () { return -7; };",
    "Array.prototype.map = function () { return ['M']; };",
    "Array.prototype.push = function (v) { return 'P' + v; };",
    "Array.prototype.includes = function () { return 'I'; };",
    "Map.prototype.get = function (k) { return 'G' + k; };",
    "Set.prototype.has = function () { return 'H'; };",
    "String.prototype.slice = function () { return 'S'; };",
    "Function.prototype.call = function () { return 'C'; };",
].join("\n");
new Function(body)();
const after = [arr.join("-"), arr.indexOf(1), arr.map((x) => x * 2).join(","), m.get("a"), s.has(1),
    str.slice(1), f.call(null, 1), arr.includes(2), arr.push(9), arr.length];
console.log(JSON.stringify(after));
function loop(a: number[]) { let out = ""; for (let i = 0; i < 2; i++) out += a.join("+") + a.indexOf(3); return out; }
console.log(loop([3, 4]));
