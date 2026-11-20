// Integer keys on a plain object: reads, writes, enumeration order, deletes,
// and the string spellings that name the same key.
const caps: any = {};
const ids = [2929, 2884, 3042, 2960, 32823, 1, 0, 7];
for (const id of ids) caps[id] = false;
caps[2929] = true;
let c = 0;
for (let i = 0; i < 100; i++) if (caps[ids[i & 7]] !== true) c++;
console.log("count", c, caps["2929"], caps[2884], caps[-0], caps["0"], caps[99]);
console.log("keys", Object.keys(caps).join(","));
delete caps[3042];
console.log("deleted", caps[3042], 3042 in caps, Object.keys(caps).length);
caps["3042"] = 5;
console.log("string add", caps[3042]);
const m: any = { a: 1 };
m[1.5] = "f"; m[-1] = "n"; m[2 ** 32] = "big"; m[1e21] = "e";
console.log("odd", m["1.5"], m["-1"], m["4294967296"], m["1e+21"], Object.keys(m).join(","));
const proto: any = { 5: "inherited" };
const child = Object.create(proto);
console.log("inherited", child[5], child[6]);
Object.defineProperty(child, 6, { get() { return "getter"; } });
console.log("getter", child[6]);
const big: any = {};
for (let i = 0; i < 300; i++) big[i * 7 + 1] = i;
let s = 0;
for (let i = 0; i < 300; i++) s += big[i * 7 + 1];
console.log("big", s, big[8], big[9]);
function args(..._r: unknown[]) { return arguments; }
const a: any = args(10, 20);
console.log("arguments", a[0], a[1], a[2]);
