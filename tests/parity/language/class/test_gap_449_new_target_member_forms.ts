// #449 follow-up: every member form of `new.target` must behave exactly like the
// same form on any other expression. `new.target?.<prop>` short-circuits when
// new.target is undefined (a function or method invoked without `new`), and
// `new.target.<prop>` throws a TypeError there. A lowering that special-cased
// these spellings dropped the `?.` null guard, so `new.target?.name` in a plain
// call threw "Cannot read properties of undefined (reading name)".
//
// platforms: macos, linux

function probe(label: string, f: () => unknown): void {
  try {
    console.log(label, String(f()));
  } catch (e) {
    console.log(label, "threw", e instanceof TypeError ? "TypeError" : String(e));
  }
}

function Plain(this: unknown) {
  probe("optional name:", () => new.target?.name);
  probe("optional name ??:", () => new.target?.name ?? "none");
  probe("optional computed:", () => new.target?.["name"]);
  probe("optional prototype:", () => new.target?.prototype === (Plain as any).prototype);
  probe("direct name:", () => new.target.name);
  probe("direct computed:", () => new.target["name"]);
  probe("bare:", () => new.target === undefined);
  probe("identity:", () => new.target === Plain);
  probe("typeof:", () => typeof new.target);
  probe("ternary:", () => (new.target ? "constructed" : "called"));
  const arrow = () => new.target?.name ?? "none";
  probe("arrow optional:", arrow);
  const arrowDirect = () => new.target.name;
  probe("arrow direct:", arrowDirect);
}

console.log("-- Plain() --");
Plain.call(undefined);
console.log("-- new Plain() --");
new (Plain as any)();

class Base {
  constructor() {
    console.log("ctor optional:", new.target?.name ?? "none");
    console.log("ctor direct:", new.target.name);
    console.log("ctor arrow:", (() => new.target?.name)());
  }
  method() {
    probe("method optional:", () => new.target?.name ?? "none");
    probe("method direct:", () => new.target.name);
  }
}
class Leaf extends Base {}

console.log("-- new Base() --");
new Base().method();
console.log("-- new Leaf() --");
new Leaf();

class Fields {
  opt = new.target?.name ?? "none";
  arrowOpt = (() => new.target?.name ?? "none")();
}
const fields = new Fields();
console.log("field optional:", fields.opt);
console.log("field arrow optional:", fields.arrowOpt);
