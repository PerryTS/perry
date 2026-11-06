// A constructor statement that never names the receiver can still run code
// that changes the chain a later store meets.
let log: string[] = [];
let armed = false;
function hook(): void {
  if (armed) {
    Object.defineProperty(C.prototype, "b", {
      configurable: true,
      set(v: number) {
        log.push("set b " + v);
      },
      get() {
        return 99;
      },
    });
  }
}
class C {


  constructor(x: number) {
    (this as any).a = x;
    hook();
    (this as any).b = x + 1;
  }
}
const first = new C(1);
log.push(Object.keys(first).join(",") + " " + (first as any).b);
armed = true;
const second = new C(5);
log.push(Object.keys(second).join(",") + " " + (second as any).b);
console.log(log.join("\n"));
