// Per-unit facts (decision 151): a shared prototype is one object for the
// whole program, and no contract describes it.
// Another unit installs an accessor on it that writes its receiver (E23):
// the literal of this unit must not claim `p` a readonly number.
// DEC 250 locks Object.prototype, so the shared prototype is a user object
// that the literal names as its [[Prototype]].
const shared: any = {};
function install(): void {
  Object.defineProperty(shared, "x", {
    get(this: any) {
      this.p = "t";
      return 1;
    },
    configurable: true,
  });
}
install();
var box = { __proto__: shared, p: 3, q: 4 };
function sum(): number {
  return box.p * 2 + box.q;
}
console.log(sum(), (box as any).x, sum(), typeof box.p);
