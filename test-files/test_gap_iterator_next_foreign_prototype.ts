// Installing a foreign intrinsic next must revoke native-step admission.
// Both for-of and destructuring must then Call it and observe its TypeError.
function make(kind: number): any {
  if (kind === 0) return [1][Symbol.iterator]();
  if (kind === 1) return new Map([[1, 2]]).values();
  if (kind === 2) return new Set([1]).values();
  return "x"[Symbol.iterator]();
}
for (let target = 0; target < 4; target++) {
  for (let source = 0; source < 4; source++) {
    if (source === target) continue;
    const p: any = Object.getPrototypeOf(make(target));
    const original = p.next;
    const foreign = Object.getPrototypeOf(make(source)).next;
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
    p.next = original;
  }
}
