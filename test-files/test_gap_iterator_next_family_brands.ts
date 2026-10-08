// Every intrinsic next must reject foreign iterator brands before stepping.
function* values() { yield 1; }
function make(kind: number): any {
  if (kind === 0) return [1][Symbol.iterator]();
  if (kind === 1) return new Map([[1, 2]]).values();
  if (kind === 2) return new Set([1]).values();
  if (kind === 3) return "x"[Symbol.iterator]();
  if (kind === 4) return "x".matchAll(/x/g);
  if (kind === 5) return [1].values().map((x: number) => x);
  return values();
}
function proto(kind: number): any {
  const p = Object.getPrototypeOf(make(kind));
  return kind === 6 ? Object.getPrototypeOf(p) : p;
}
for (let family = 0; family < 7; family++) {
  const next = proto(family).next;
  for (let receiver = 0; receiver < 11; receiver++) {
    const it: any = receiver < 7 ? make(receiver)
      : receiver === 7 ? {} : receiver === 8 ? null : receiver === 9 ? undefined : 1;
    try {
      const result = next.call(it);
      console.log(family, receiver, "ok", result.done);
    } catch (e: any) {
      console.log(family, receiver, e.name);
    }
  }
}
// Buffer and typed arrays carry the ECMAScript Array Iterator brand.
const arrayNext = proto(0).next;
console.log("typed-array", arrayNext.call(new Uint8Array([7]).values()).value);
