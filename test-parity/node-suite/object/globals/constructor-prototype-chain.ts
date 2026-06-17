const entityKind = Symbol.for("fixture:entityKind");

class Table {
  static [entityKind] = "Table";
}

function entityWalk(value: any, type: any): string {
  if (!value || typeof value !== "object") {
    return "non-object";
  }
  if (value instanceof type) {
    return "instance";
  }
  if (!Object.prototype.hasOwnProperty.call(type, entityKind)) {
    return "bad-type";
  }

  let cls = Object.getPrototypeOf(value).constructor;
  let steps = 0;
  while (cls) {
    steps++;
    if (entityKind in cls && cls[entityKind] === type[entityKind]) {
      return `match:${steps}`;
    }
    cls = Object.getPrototypeOf(cls);
    if (steps > 10) {
      return `loop:${typeof cls}`;
    }
  }
  return `done:${steps}`;
}

console.log("Array ctor parent:", Object.getPrototypeOf(Array) === Function.prototype);
console.log(
  "Function prototype parent:",
  Object.getPrototypeOf(Function.prototype) === Object.prototype,
);
console.log("Object prototype parent:", Object.getPrototypeOf(Object.prototype) === null);
console.log("array entity walk:", entityWalk([], Table));
console.log("object entity walk:", entityWalk({}, Table));

class NamespaceTarget {}
class DirectStatic {}
function RegularFunction() {}

(NamespaceTarget as any).DirectStatic = DirectStatic;
function assignViaAlias(target: any) {
  class ViaAlias {}
  target.ViaAlias = ViaAlias;
}
assignViaAlias(NamespaceTarget);

const selected: any = NamespaceTarget || {};
((target: any) => {
  class ViaIife {}
  target.ViaIife = ViaIife;
})(selected);

(RegularFunction as any).extra = 1;

console.log("class direct static:", typeof (NamespaceTarget as any).DirectStatic);
console.log("class alias static:", typeof (NamespaceTarget as any).ViaAlias);
console.log("class iife static:", typeof (NamespaceTarget as any).ViaIife);
console.log("function dynamic prop:", (RegularFunction as any).extra);
console.log(
  "class iife instanceof:",
  {} instanceof (NamespaceTarget as any).ViaIife,
);
