// Module B: names `qzonly`; inits after module A.
export function fill(xs: any[]): void {
  for (let i = 0; i < xs.length; i++) xs[i].qzonly = i + 1;
}
export function rd(o: any): number {
  return o.qzonly;
}
export function run(n: number, xs: any[]): number {
  let h = 0;
  for (let k = 0; k < n; k++) h += rd(xs[k & 15]);
  return h;
}
