// Native owners, views, annotation lies and detached owners must keep Node semantics.
function bump(a: Uint32Array, i: number): void { a[i] += 3; }
(globalThis as any).nativeBump = bump;
const u = new Uint32Array(65536);
u[65535] = 4294967295;
for (const i of [0, 65535, 65536, -1, 1.5]) (globalThis as any).nativeBump(u, i);
console.log("native rmw", u[0], u[65535], u.length);
const view = new Uint32Array(u.buffer, 4, 4);
(globalThis as any).nativeBump(view, 0);
console.log("view rmw", view[0], u[1]);
const lie: any = [5];
(globalThis as any).nativeBump(lie, 0);
console.log("array rmw", lie[0]);

function sum(a: any, n: number): number {
  let result = 0;
  for (let i = 0; i < n; i++) result += a[i] + a[i];
  return result;
}
(globalThis as any).nativeRegionSum = sum;
const f = new Float64Array(65536);
f[0] = 2;
f[65535] = 4;
console.log("native region", (globalThis as any).nativeRegionSum(f, 65536));
const fv = new Float64Array(f.buffer, 8, 4);
fv[0] = 3;
console.log("view region", (globalThis as any).nativeRegionSum(fv, 4));
console.log("array region", (globalThis as any).nativeRegionSum([1, 2, 3], 3));
(u.buffer as any).transfer();
(globalThis as any).nativeBump(u, 0);
console.log("detached rmw", u.length, u[0]);
(f.buffer as any).transfer();
console.log("detached region", f.length, (globalThis as any).nativeRegionSum(f, 1));
