// F4: .call / .apply / bind pass a foreign `this` to a method.
// WPFACTS: site .v off
class Own {
  v: number = 1;
  get() { return this.v; }
}
const foreign = { pad: 0, v: "foreign" };
const o = new Own();
console.log(o.get(), Own.prototype.get.call(foreign), Own.prototype.get.apply(foreign, []));
const bound = o.get.bind(foreign);
console.log(bound());
