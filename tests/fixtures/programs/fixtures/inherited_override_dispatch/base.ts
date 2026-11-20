// Base class whose methods call `this.paint()`. The computed field keeps the
// call site off the guarded direct path, so it lowers to the class-id tower
// over the overrides this module's class table knows (Mid).
const K = Symbol.for("gap.inherited_override_dispatch");
export class Base {
  [K] = 1;
  paint() { return "base"; }
  repaint(p = this.paint()) { return p; }
  repaintBody() { return this.paint(); }
}
export class Mid extends Base {
  paint() { return "mid"; }
}
export class Plain extends Base {}
// No subclass in this module overrides `paint`: the site has no tower.
export class Solo {
  [K] = 1;
  paint() { return "solo"; }
  repaintBody() { return this.paint(); }
}
