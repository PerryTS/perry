// Reads of a function's own properties once its property bag has an
// overflow: an entry's position in the overflow is not a field slot, so
// every key must read back its own value, whether it lives in a fixed slot
// or only in the overflow.
function F() {}
const f: any = F;
f.a = "A";
f.b = "B";
f.c = "C";
delete f.b;
f.b = "B2";
const names = ["d", "e", "g", "h", "i", "j"];
for (let n = 0; n < names.length; n++) f[names[n]] = names[n].toUpperCase() + n;
console.log(f.a, f.b, f.c, f.d, f.e, f.g, f.h, f.i, f.j);
for (const n of ["a", "b", "c", ...names]) console.log(n, f[n], n in f, Object.prototype.hasOwnProperty.call(f, n));
console.log(Object.keys(f).join(","));

class K {
  static s1 = 1;
  static s2 = 2;
}
const k: any = K;
delete k.s1;
k.s1 = 11;
for (let n = 0; n < 5; n++) k["dyn" + n] = n * 10;
console.log(k.s1, k.s2, k.dyn0, k.dyn3, k.dyn4, k["dyn" + 2]);
console.log(Object.keys(k).join(","));

// An arrow with dynamic props written before any static one.
const arrow: any = () => 0;
for (const n of ["x", "y", "z"]) arrow[n] = n + "!";
arrow.w = "w!";
console.log(arrow.x, arrow.y, arrow.z, arrow.w, Object.keys(arrow).join(","));
