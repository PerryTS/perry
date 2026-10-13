// E10: Object.create(C.prototype) makes an instance without the ctor.
// WPFACTS: fact F3 prefix_stable Made = 0
// WPFACTS: site .m off
class Made {
  m: number = 7;
  read() { return this.m; }
}
const o = Object.create(Made.prototype);
console.log(new Made().read(), o.read());
