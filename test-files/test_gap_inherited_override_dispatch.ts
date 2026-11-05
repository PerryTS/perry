// A method's `this.m()` must resolve m through the receiver's prototype chain
// even when the receiver's class is declared in a module the method's module
// does not know: a subclass of Mid declared here inherits Mid's override.
// (opentui's BoxRenderable subclasses ScrollBoxRenderable lost their
// background fill: Renderable.setNativeScenePaint's default parameter
// `this.getNativeScenePaint()` took Renderable's body instead of Box's.)
import { Base, Mid, Plain, Solo } from "./fixtures/inherited_override_dispatch/base.ts";
class Leaf extends Mid {}
class LeafField extends Mid { extra = 1; }
class Own extends Mid { paint() { return "own"; } }
class PlainLeaf extends Plain {}
class PlainOwn extends Plain { paint() { return "plain-own"; } }
const Expr = class extends Mid {};
function make(parent: typeof Base) { return class extends parent {}; }
const Made = make(Mid);
for (const C of [Base, Mid, Plain, Leaf, LeafField, Own, PlainLeaf, PlainOwn, Expr, Made]) {
  const o = new (C as any)();
  console.log(o.repaint(), o.repaintBody(), o.paint());
}
const shadow = new Leaf() as any;
shadow.paint = () => "instance";
console.log(shadow.repaint(), shadow.repaintBody());
console.log(Base.prototype.repaintBody.call({ paint: () => "foreign" }));
for (const C of [Base, Plain, Mid]) {
  const own = new (C as any)();
  own.paint = () => "own-" + C.name;
  console.log(own.repaint(), own.repaintBody());
}
class SoloOwn extends Solo { paint() { return "solo-own"; } }
console.log(new Solo().repaintBody(), new SoloOwn().repaintBody());
