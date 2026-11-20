// #12202: computed class keys retain their exact UTF-16 identity, including
// lone surrogates, through registration, lookup, reflection and function names.
const high = String.fromCharCode(0xd800);
const low = String.fromCharCode(0xdc00);
const replacement = "\ufffd";
const escapedText = "\\uD800";
const staticAccess = high + "s";
const formalKey = high + "d";
const restKey = high + "r";
const getterOnlyKey = high + "ro";
const shadowedSetterKey = high + "shadow";
const symbolKey = Symbol.for("issue-12202-symbol-control");

function units(value) {
  const out = [];
  for (let i = 0; i < value.length; i++) out.push(value.charCodeAt(i));
  return out;
}

let coercions = 0;
const coercible = {
  [Symbol.toPrimitive]() {
    coercions++;
    return high;
  },
};

class Keys {
  [coercible](value) { return `high:${value}`; }
  [low](value) { return `low:${value}`; }
  [replacement](value) { return `replacement:${value}`; }
  [escapedText](value) { return `escaped:${value}`; }
  [""](value) { return `empty:${value}`; }
  ["ascii"](value) { return `ascii:${value}`; }
  ["é"](value) { return `bmp:${value}`; }
  ["𝄞"](value) { return `astral:${value}`; }
  get [high + "g"]() { return 17; }
  set [high + "g"](value) { this.saved = value; }
  [formalKey](value) { return value; }
  [restKey](...values) { return values.length; }
  [symbolKey]() { return "symbol"; }
  static [high](value) { return `static:${value}`; }
  static get [staticAccess]() { return this.savedStatic; }
  static set [staticAccess](value) { this.savedStatic = value; }
}
class GetterOnlyKey {
  get [getterOnlyKey]() { return 41; }
}
class ShadowedSetterKey {
  set [shadowedSetterKey](value) { this.setterRan = true; }
}

const instance = new Keys();
const highDescriptor = Object.getOwnPropertyDescriptor(Keys.prototype, high);
const getterDescriptor = Object.getOwnPropertyDescriptor(Keys.prototype, high + "g");
instance[high + "g"] = 19;
const staticGetterDescriptor = Object.getOwnPropertyDescriptor(Keys, staticAccess);
Keys[staticAccess] = 23;
const getterOnly = new GetterOnlyKey();
let getterOnlyThrows = false;
try {
  getterOnly[getterOnlyKey] = 42;
} catch (error) {
  getterOnlyThrows = error instanceof TypeError;
}
const shadowedSetter = new ShadowedSetterKey();
Object.defineProperty(shadowedSetter, shadowedSetterKey, {
  value: 7,
  writable: true,
  configurable: true,
});
shadowedSetter[shadowedSetterKey] = 8;

function factory(key) {
  return class {
    [key]() { return key; }
    static get [key + "s"]() { return key; }
  };
}
function factoryWithLateStatic(key) {
  function readLateStatic() { return Late[key]; }
  const Late = class {};
  Object.defineProperty(Late, key, { value: "late-static" });
  return { Late, readLateStatic };
}
const First = factory(high);
const firstMethod = First.prototype[high];
const firstStaticDescriptor = Object.getOwnPropertyDescriptor(First, high + "s");
const firstStaticValue = First[high + "s"];
const Second = factory(low);
const secondMethod = Second.prototype[low];
const first = new First();
const second = new Second();
const lateStatic = factoryWithLateStatic(high);

class Parent { [high]() { return "parent"; } }
class Child extends Parent {}
class StaticParent { static [high]() { return "parent-static"; } }
class StaticChild extends StaticParent {}

class Duplicates {
  [high]() { return "earlier"; }
  [high]() { return "last"; }
  get [low + "a"]() { return 31; }
  set [low + "a"](value) { this.saved = value; }
}
const duplicate = new Duplicates();
duplicate[low + "a"] = 37;
let staticPrototypeThrows = false;
try {
  class InvalidStaticPrototype { static ["prototype"]() {} }
  void InvalidStaticPrototype;
} catch (error) {
  staticPrototypeThrows = error instanceof TypeError;
}

const result = {
  ownKeys: Reflect.ownKeys(Keys.prototype).map(units),
  names: [
    units(Object.getOwnPropertyDescriptor(Keys.prototype, high).value.name),
    units(Object.getOwnPropertyDescriptor(Keys, high).value.name),
    units(getterDescriptor.get.name),
    units(getterDescriptor.set.name),
    units(staticGetterDescriptor.get.name),
    units(staticGetterDescriptor.set.name),
    units(firstStaticDescriptor.get.name),
  ],
  values: [
    instance[high]("ok"), instance[low]("ok"), instance[replacement]("ok"),
    instance[escapedText]("ok"), instance.ascii("ok"), instance["é"]("ok"),
    instance["𝄞"]("ok"), instance[""]("ok"), instance[high + "g"], instance.saved,
    Keys[high]("ok"), Keys[staticAccess], first[high](), second[low](),
    firstStaticValue, StaticChild[high](),
    new Child()[high](), duplicate[high](), duplicate[low + "a"], duplicate.saved,
    instance[symbolKey](), lateStatic.readLateStatic(),
  ],
  descriptor: [highDescriptor.enumerable, highDescriptor.configurable, highDescriptor.writable],
  keyKinds: [
    typeof instance[high], typeof instance[low], typeof instance[replacement],
    typeof instance[escapedText], typeof instance[""], typeof instance[symbolKey],
  ],
  // Keep the WTF-8 formal/rest controls here. Default-parameter length is
  // tracked separately because Perry's generic entryless computed-method
  // metadata currently counts the defaulted parameter.
  lengths: [Keys.prototype[formalKey].length, Keys.prototype[restKey].length],
  identities: [
    First.prototype[high] === firstMethod,
    Second.prototype[low] === secondMethod,
    firstMethod !== secondMethod,
  ],
  staticDescriptorOwnership: [
    Object.getOwnPropertyDescriptor(StaticParent, high) !== undefined,
    Object.getOwnPropertyDescriptor(StaticChild, high) === undefined,
  ],
  accessorControls: [
    getterOnly[getterOnlyKey], getterOnlyThrows,
    Object.hasOwn(getterOnly, getterOnlyKey), shadowedSetter[shadowedSetterKey],
    shadowedSetter.setterRan === undefined,
  ],
  staticPrototypeThrows,
  coercions,
};

delete Keys.prototype[high];
Keys.prototype[high] = function readded() { return "readded"; };
result.readded = [
  instance[high](),
  Reflect.ownKeys(Keys.prototype).map(units),
];
console.log(JSON.stringify(result));
