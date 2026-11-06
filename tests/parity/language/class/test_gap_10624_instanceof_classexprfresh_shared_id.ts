// Gap test for #10624: `instanceof` against a `ClassExprFresh` parent must
// not resolve by shared (template) class id. Each per-evaluation class
// object's OWN pinned heritage must be honored even after a LATER
// evaluation of the same factory has overwritten the shared last-write-wins
// slot that `instanceof`'s class-chain walk otherwise reads.

function extend(Base: any, tag: string) {
  return class extends Base {
    getTag() {
      return tag;
    }
  };
}

class Root {
  kind() {
    return "root";
  }
}
class RootAlt {
  kind() {
    return "alt";
  }
}

// Same call site invoked twice (a loop), so both evaluations share Perry's
// internal template class id - the shape #10624 is about.
const bases = [Root, RootAlt];
const tags = ["r1", "r2"];
const evaluations: any[] = [];
for (let i = 0; i < bases.length; i++) {
  evaluations.push(extend(bases[i], tags[i]));
}
const A = evaluations[0]; // extends Root
const B = evaluations[1]; // extends RootAlt (later eval; overwrites the shared dynamic-parent slot)

// Construction order interleaved: build from the EARLIER evaluation (A)
// *after* the LATER evaluation (B) has already run.
const earlyInstance = new A();
console.log("earlyInstance instanceof Root:", earlyInstance instanceof Root);
console.log("earlyInstance instanceof RootAlt:", earlyInstance instanceof RootAlt);
console.log("earlyInstance instanceof A:", earlyInstance instanceof A);
console.log("earlyInstance instanceof B:", earlyInstance instanceof B);
console.log("earlyInstance.getTag():", earlyInstance.getTag());

const lateInstance = new B();
console.log("lateInstance instanceof Root:", lateInstance instanceof Root);
console.log("lateInstance instanceof RootAlt:", lateInstance instanceof RootAlt);
console.log("lateInstance.getTag():", lateInstance.getTag());

// instanceof in both directions, re-checked after more evaluations ran.
console.log("earlyInstance instanceof RootAlt (again):", earlyInstance instanceof RootAlt);
console.log("lateInstance instanceof Root (again):", lateInstance instanceof Root);

// A THIRD evaluation, constructed immediately (control - the "latest"
// evaluation was never the stale case, so this must always have worked).
const C = extend(Root, "r3");
const freshInstance = new C();
console.log("freshInstance instanceof Root:", freshInstance instanceof Root);
console.log("freshInstance instanceof RootAlt:", freshInstance instanceof RootAlt);

// Two-level subclass: a SECOND dynamic factory evaluated against a specific
// evaluation of the FIRST one (A, not B), checked after yet another
// evaluation of the first factory has run and overwritten its shared slot
// again. Checked against Root/RootAlt only (distinct classes, distinct
// class ids) - not against A/D directly, which exercises a separate,
// pre-existing limitation (instanceof against a *specific* sibling
// evaluation of the same template, referenced directly as the RHS, is not
// this issue's mechanism).
function extendAgain(Base: any, mark: string) {
  return class extends Base {
    extra() {
      return mark;
    }
  };
}
const G = extendAgain(A, "grandchild"); // extends A specifically, i.e. transitively Root
const D = extend(RootAlt, "r4"); // yet another eval of `extend` - overwrites its shared slot again
const grandchildInstance = new G();
console.log("grandchildInstance instanceof Root:", grandchildInstance instanceof Root);
console.log("grandchildInstance instanceof RootAlt:", grandchildInstance instanceof RootAlt);
console.log("grandchildInstance.extra():", grandchildInstance.extra());
