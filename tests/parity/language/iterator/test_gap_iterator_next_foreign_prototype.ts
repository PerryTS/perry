// Installing a foreign intrinsic next must revoke native-step admission.
// Both for-of and destructuring must then Call it and observe its TypeError.
// DEC 250 locks the intrinsic iterator prototypes: each kind's iterators
// adopt a user prototype UP[kind] below its intrinsic one, and the foreign
// next is installed there.
function raw(kind: number): any {
  if (kind === 0) return [1][Symbol.iterator]();
  if (kind === 1) return new Map([[1, 2]]).values();
  if (kind === 2) return new Set([1]).values();
  return "x"[Symbol.iterator]();
}
const UP: any[] = [0, 1, 2, 3].map((k) => Object.create(Object.getPrototypeOf(raw(k))));
function make(kind: number): any {
  return Object.setPrototypeOf(raw(kind), UP[kind]);
}
for (let target = 0; target < 4; target++) {
  for (let source = 0; source < 4; source++) {
    if (source === target) continue;
    const p: any = Object.getPrototypeOf(make(target));
    const original = p.next;
    const foreign = Object.getPrototypeOf(raw(source)).next;
    p.next = foreign;
    try {
      let count = 0;
      for (const value of make(target)) { count++; }
      console.log("for-of", target, source, "wrong", count);
    } catch (e: any) {
      console.log("for-of", target, source, e.name);
    }
    try {
      const [value] = make(target);
      console.log("binding", target, source, "wrong", value);
    } catch (e: any) {
      console.log("binding", target, source, e.name);
    }
    try {
      let value: any;
      [value] = make(target);
      console.log("assignment", target, source, "wrong", value);
    } catch (e: any) {
      console.log("assignment", target, source, e.name);
    }
    delete p.next;
    if (p.next !== original) console.log("restore failed", target, source);
  }
}
