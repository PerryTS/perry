// E13/E25: a subclass shadows a base field with an accessor.
// WPFACTS: site .h off
class Hb {
  h: number = 1;
}
class Hs extends Hb {
  get h2() { return 5; }
}
class Ha extends Hb {
  constructor() {
    super();
    Object.defineProperty(this, "h", { get() { return 99; } });
  }
}
function rd(x: Hb) { return x.h; }
console.log(rd(new Hb()), rd(new Hs()), rd(new Ha()));
