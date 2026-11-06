// Sloppy twin of test_gap_class_accessors_on_prototype.ts row 10: outside a
// class body, a write to a getter-only class accessor is silently ignored.
class ReadOnly {
  get v() {
    return 1;
  }
  poke() {
    (this as any).v = 2;
  }
}
const r: any = new ReadOnly();
let threw = "no throw";
try {
  r.v = 5;
} catch (e) {
  threw = (e as Error).constructor.name;
}
console.log("sloppy-write:", threw, r.v, Object.prototype.hasOwnProperty.call(r, "v"));
try {
  r.poke();
  console.log("class-body-write: no throw");
} catch (e) {
  console.log("class-body-write:", (e as Error).constructor.name);
}
