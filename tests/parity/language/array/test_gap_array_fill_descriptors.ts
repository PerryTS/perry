// Array.prototype.fill is Set(O, k, v, true) per index. Over an array that
// holds element descriptors an accessor's setter must run and a read-only
// element must make the strict Set throw instead of being overwritten.

const fa: any[] = [1, 2, 3];
let sets = 0;
Object.defineProperty(fa, 1, {
  get() { return "acc"; },
  set(v) { sets += v; },
  configurable: true,
});
fa.fill(7);
console.log("setter sum", sets, fa[0], fa[1], fa[2]);
fa.fill(5, 1, 2);
console.log("setter sum range", sets, fa[0], fa[1], fa[2]);

function attempt(label: string, f: () => void) {
  try {
    f();
    console.log(label, "no throw");
  } catch (e: any) {
    console.log(label, "threw", e instanceof TypeError);
  }
}

const ro: number[] = [1, 2, 3];
Object.defineProperty(ro, 1, { value: 20, writable: false, configurable: true });
attempt("fill read-only", () => ro.fill(7));
console.log(ro[0], ro[1], ro[2]);

const rr: number[] = [1, 2, 3];
Object.defineProperty(rr, 2, { value: 30, writable: false, configurable: true });
attempt("fill range short of read-only", () => rr.fill(9, 0, 2));
console.log(rr[0], rr[1], rr[2]);
attempt("fill range reaching read-only", () => rr.fill(5, 1, 3));
console.log(rr[0], rr[1], rr[2]);

const plain: number[] = [1, 2, 3];
plain.fill(4);
console.log(plain.join(","));
