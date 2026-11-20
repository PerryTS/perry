// Dynamic Error subclasses, like Effect's Schema.TaggedError, set the
// inherited name on a factory-created base prototype.
class Plain extends Error {}
Plain.prototype.name = "PlainTag";
console.log("plain", new Plain("message").name);

function makeBase(tag: string) {
  class Base extends Error {}
  Base.prototype.name = tag;
  return class Tagged extends Base {};
}
const Factory = makeBase("FactoryTag");
console.log("factory", new Factory("message").name);

function makeObjectBase(tag: string) {
  const O = { Base: class extends Error {} };
  O.Base.prototype.name = tag;
  return class Tagged extends O.Base {};
}
const ObjectBase = makeObjectBase("ObjectTag");
console.log("object-base", new ObjectBase("message").name);

function makeDeepBase(tag: string) {
  class Base extends Error {}
  Base.prototype.name = tag;
  const makeClass = (Ctor: typeof Base) => class Mid extends Ctor {};
  return class Tagged extends makeClass(Base) {};
}
const DeepBase = makeDeepBase("DeepTag");
console.log("deep-base", new DeepBase("message").name);

function makeTagged(tag: string) {
  class Base extends Error {}
  Base.prototype.name = tag;
  return class Tagged extends Base {
    static _tag = tag;
  };
}
class First extends makeTagged("FirstTag") {}
class Second extends makeTagged("SecondTag") {}
const first = new First("message");
const second = new Second("message");
console.log("prototypes", First.prototype.name, Second.prototype.name);
console.log("two-tags", first.name, second.name, first instanceof Second);
console.log("to-string", String(first), String(second));
console.log("own-name", Object.prototype.hasOwnProperty.call(first, "name"));

// Effect's Data.Error adds factory-created ancestors beyond the tagged Base.
// Constructor replay must retain the nearest class evaluation: pinning the
// deepest ancestor makes this instance read Error.prototype.name instead.
const YieldableError = (function () {
  class YieldableError extends Error {
    toJSON() {
      return { ...this };
    }
  }
  return YieldableError;
})();
const DataError = (function () {
  const classes = {
    BaseEffectError: class extends YieldableError {
      constructor(args: any) {
        super(args?.message);
        if (args) Object.assign(this, args);
      }
    },
  };
  return classes.BaseEffectError;
})();
function makeSchemaClass(Base: any) {
  const klass = class extends Base {
    constructor(props: any = {}) {
      super(props);
    }
    static get ast() {
      return "ast";
    }
  };
  return klass;
}
function makeEffectTagged(tag: string) {
  class Base extends DataError {}
  Base.prototype.name = tag;
  class TaggedErrorClass extends makeSchemaClass(Base) {
    static _tag = tag;
  }
  return TaggedErrorClass;
}
class NestedFirst extends makeEffectTagged("NestedFirstTag") {}
class NestedSecond extends makeEffectTagged("NestedSecondTag") {}
const nestedFirst = new NestedFirst({ message: "message" });
const nestedSecond = new NestedSecond({ message: "message" });
console.log(
  "nested-tags",
  nestedFirst.name,
  nestedSecond.name,
  nestedFirst instanceof NestedSecond,
);
