// Issue #5024: expando properties written to a function's auto-created
// `.prototype` object must be registered in the object's own-property key
// metadata, so Object.keys / getOwnPropertyNames / `in` / hasOwnProperty /
// for-in / Object.assign all see them (React PureComponent setup relies on
// Object.assign(PureComponent.prototype, Component.prototype)).

// --- Part 1: minimal own-key tracking on the auto-created prototype ---
function F(this: any) {}
(F.prototype as any).mark = { tag: 1 };

console.log("direct get:", typeof (F.prototype as any).mark);
console.log("keys:", JSON.stringify(Object.keys(F.prototype)));
console.log("ownNames:", JSON.stringify(Object.getOwnPropertyNames(F.prototype).sort()));
console.log("in:", "mark" in F.prototype);
console.log("hasOwn:", Object.prototype.hasOwnProperty.call(F.prototype, "mark"));

const forInKeys: string[] = [];
for (const k in F.prototype) forInKeys.push(k);
console.log("forIn:", JSON.stringify(forInKeys));

const t: any = {};
Object.assign(t, F.prototype);
console.log("assign keys:", JSON.stringify(Object.keys(t)));
console.log("assign get:", typeof t.mark);

// Read through a plain variable (generic property-get path, not the
// recognised `<funcName>.prototype.<name>` shape).
const FP: any = F.prototype;
console.log("var get:", typeof FP.mark);

// --- Part 2: replaced prototype (control — always worked) ---
function G(this: any) {}
G.prototype = { gmark: 1 };
console.log("replaced keys:", JSON.stringify(Object.keys(G.prototype)));

// --- Part 3: the React PureComponent shape (2-level chain) ---
function Component(this: any, props: any) {
  this.props = props;
}
(Component.prototype as any).isReactComponent = {};
(Component.prototype as any).setState = function (this: any, s: any) {
  return "setState";
};

function ComponentDummy(this: any) {}
ComponentDummy.prototype = Component.prototype;

function PureComponent(this: any, props: any) {
  this.props = props;
}
const pureProto: any = new (ComponentDummy as any)();
PureComponent.prototype = pureProto;
pureProto.constructor = PureComponent;
Object.assign(pureProto, Component.prototype);
pureProto.isPureReactComponent = true;

const PC: any = PureComponent;
console.log("PC proto isReactComponent:", typeof PC.prototype.isReactComponent);

class Y extends (PureComponent as any) {
  render() {
    return null;
  }
}
console.log("Y proto isReactComponent:", typeof (Y.prototype as any).isReactComponent);

// shouldConstruct, exactly as react-reconciler does it
function shouldConstruct(type: any) {
  const prototype = type.prototype;
  return !!(prototype && prototype.isReactComponent);
}
console.log("shouldConstruct(Y):", shouldConstruct(Y));

// --- Part 4: dispatch through instances still works ---
const f: any = new (F as any)();
console.log("inst mark:", typeof f.mark);
const y: any = new (Y as any)();
console.log("y setState:", y.setState ? y.setState() : "MISSING");
