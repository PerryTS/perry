// Object-literal `super` getters preserve their call-site receiver (#12212).
function check(condition: boolean, label: string): void {
  if (!condition) throw new Error(`object super getter: ${label}`);
}

// The issue's exact literal: the getter must see the object whose method
// invoked `super`, not the prototype where the getter lives.
const p={get v(){return this.t}};
const o={__proto__:p,t:"o",g(){return super.v}};
console.log(o.g());
check(o.g() === "o", "issue literal receiver");
check(o.g.call({ t: "borrowed" }) === "borrowed", "borrowed receiver");

// `super` reads the home object's prototype at call time.
const replacement = { get v() { return this.t + " relinked"; } };
Object.setPrototypeOf(o, replacement);
check(o.g() === "o relinked", "live home prototype");

// Proxy [[Get]] traps receive the original method receiver too.
const proxyHome = new Proxy({}, {
  get(_target, key, receiver) {
    return key === "v" ? receiver.t + " proxy" : undefined;
  },
});
Object.setPrototypeOf(o, proxyHome);
check(o.g() === "o proxy", "proxy receiver");

// Symbol keys follow the same receiver-aware lookup path.
const key = Symbol("v");
const symbolProto = { get [key]() { return this.t + " symbol"; } };
const symbolHome = { __proto__: symbolProto, t: "o", g() { return super[key]; } };
check(symbolHome.g() === "o symbol", "symbol getter receiver");

// An explicit undefined receiver must remain undefined, while ordinary object
// literals without `__proto__` inherit from Object.prototype.
const undefinedProto = { get v() { return this; } };
const undefinedHome = { __proto__: undefinedProto, g() { return super.v; } };
check(undefinedHome.g.call(undefined) === undefined, "undefined receiver");
const defaultHome = { g() { return super.toString; } };
check(typeof defaultHome.g() === "function", "default Object.prototype");

const nullHome = {
  __proto__: null,
  g() { return super.missing_property_name_long_enough_to_check_string_data; },
};
let nullBaseThrew = false;
let nullBaseName = false;
try {
  nullHome.g();
} catch (error) {
  nullBaseThrew = error instanceof TypeError;
  nullBaseName = error.message.includes("missing_property_name_long_enough_to_check_string_data");
}
check(nullBaseThrew && nullBaseName, "null home prototype preserves property name");
