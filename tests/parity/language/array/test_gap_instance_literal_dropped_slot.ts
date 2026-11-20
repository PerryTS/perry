// An instance-born literal whose local no later expression reads directly
// (wpfacts `summary::compress`): `walk` has an Array parameter, so it is
// solved once per call site, and `other` is one Array per instance. `fill`
// pushes onto it through its parameter. The slot of `other` reached nothing,
// so compress dropped it, the binding had no instance copy, and its facts
// came off the shared node, whose template Array no push reaches: on main
// `other.length` folded to 3 and `walk` printed 53 instead of 68.
// The keep cases: a copy through a call (`o = id(other)`) and a length read
// into a local (`n`) must also see the grown Array.
function fill(out: number[], n: number): void {
  for (let i = 0; i < n; i++) out.push(out.length * 2);
}
function id<T>(x: T): T {
  return x;
}
function walk(xs: number[]): number {
  let s = 0;
  const other: number[] = [5, 6, 7];
  for (const v of xs) {
    s += v + other.length;
    fill(other, 1);
  }
  return s + other.length;
}
function viaId(xs: number[]): number {
  let s = 0;
  const other: number[] = [5, 6, 7];
  const o = id(other);
  for (const v of xs) {
    s += v + o.length;
    fill(o, 1);
  }
  const n = other.length;
  return s + n;
}
console.log(walk([1, 4, 7, 10, 13]), walk([]));
console.log(viaId([1, 4, 7, 10, 13]), viaId([]));
