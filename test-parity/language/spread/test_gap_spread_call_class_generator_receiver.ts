// three's shape: v4.set(...v3, 1), v3 a class with a *[Symbol.iterator] member; patching the method or its prototype between calls.
class Vector3 { constructor(x, y, z) { this.x = x; this.y = y; this.z = z; } *[Symbol.iterator]() { yield this.x; yield this.y; yield this.z; } }
class Vector4 { constructor() { this.x = 0; this.y = 0; this.z = 0; this.w = 0; } set(x, y, z, w) { this.x = x; this.y = y; this.z = z; this.w = w; return this; } }
const a = new Vector3(1, 2, 3), b = new Vector4();
let s = 0;
for (let i = 0; i < 1000; i++) { a.x = i & 7; b.set(...a, 1); s += b.x + b.y + b.z + b.w; }
console.log(s);
Vector3.prototype[Symbol.iterator] = function* () { yield 10; yield 20; yield 30; };
b.set(...a, 1); console.log(b.x, b.y, b.z, b.w);
a[Symbol.iterator] = function* () { yield -1; };
b.set(...a, 1); console.log(b.x, b.y, b.z, b.w);
Vector4.prototype.set = function (...args) { this.n = args.length; return this; };
b.set(...[1, 2, 3, 4, 5]); console.log(b.n);
