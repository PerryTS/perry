// #10890: a class declaration that extends a class OBJECT returned by a
// factory. Instance reads must walk that evaluation's prototype, never the
// parent constructor itself. Perry answered with the parent's statics, and
// `name` came back as the class binding's own name (`out`) instead of the
// inherited prototype property.

// Effect 4's `Schema.ErrorClass`: `makeClass` builds a class expression bound
// to `out`, then writes the tag onto `out.prototype`.
const YieldableError = (function () {
  class YieldableError extends globalThis.Error {}
  Object.assign(YieldableError.prototype, { op: "YieldableError" });
  return YieldableError;
})();
const CoreError = (function () {
  return class Base extends YieldableError {
    constructor(args: any) {
      super(args?.message, args?.cause ? { cause: args.cause } : undefined);
      if (args) Object.assign(this, args);
    }
  };
})();
function makeClass(Inherited: any, identifier: string, proto?: (id: string) => any) {
  const out = class extends Inherited {
    constructor(...[input, options]: any[]) {
      super(input, { ...options });
    }
    static identifier = identifier;
    static make(input: any) {
      return new this(input);
    }
    static get ast() {
      return "ast:" + identifier;
    }
  };
  if (proto !== undefined) {
    Object.assign(out.prototype, proto(identifier));
  }
  return out;
}
const ErrorClass = (identifier: string) =>
  makeClass(CoreError, identifier, (id) => ({ name: id }));
class InitError extends ErrorClass("ProviderInitError") {}
class NoProvidersError extends ErrorClass("ProviderNoProvidersError") {}
const init: any = new InitError({ providerID: "anthropic" });
const none: any = new NoProvidersError({});
console.log("effect-name", init.name, none.name);
console.log("effect-string", String(init), String(none));
console.log("effect-field", init.providerID, init.op);
console.log("effect-instanceof", init instanceof InitError, init instanceof NoProvidersError);
console.log("effect-own-name", Object.prototype.hasOwnProperty.call(init, "name"));
console.log("effect-statics", (InitError as any).identifier, (InitError as any).ast);
console.log("effect-make", (InitError as any).make({}).name);
console.log("effect-instance-statics", init.identifier, typeof init.make, init.ast);

// The same edge without Error.
function makeLabelled(label: string) {
  const out = class {
    static label = label;
    static describe() {
      return "static:" + label;
    }
    own() {
      return "own:" + label;
    }
  };
  Object.assign(out.prototype, {
    tag: label,
    greet() {
      return "greet:" + label;
    },
  });
  return out;
}
const First = makeLabelled("first");
class Labelled extends First {}
class Other extends makeLabelled("second") {}
const labelled: any = new Labelled();
const other: any = new Other();
console.log("plain-proto", labelled.tag, other.tag);
console.log("plain-name", labelled.name, typeof labelled.describe, labelled.label);
console.log("plain-static", (Labelled as any).label, (Other as any).describe());
console.log("call-proto", labelled.greet(), other.greet());
console.log("call-own", labelled.own(), other.own());
try {
  labelled.describe();
  console.log("call-static", "no throw");
} catch (e) {
  console.log("call-static", e instanceof TypeError);
}
console.log(
  "in",
  "tag" in labelled,
  "label" in labelled,
  "describe" in labelled,
  "greet" in labelled,
);
const keys: string[] = [];
for (const k in labelled) keys.push(k);
console.log("for-in", keys.join(","));
console.log("chain", Object.getPrototypeOf(Labelled.prototype) === First.prototype);

// Symbol-keyed statics follow the same rule. Effect's `Schema.isSchema(u)` is
// `TypeId in u`, so a leaked static brands every error instance as a schema.
const TypeId = Symbol.for("test/10890/TypeId");
const ProtoId = Symbol.for("test/10890/ProtoId");
function makeBranded(label: string) {
  const out = class {
    static [TypeId] = TypeId;
    static label = label;
  };
  Object.assign(out.prototype, { [ProtoId]: label });
  return out;
}
class Branded extends makeBranded("branded") {}
const branded: any = new Branded();
console.log("sym-instance", String(branded[TypeId]), TypeId in branded);
console.log("sym-static", String((Branded as any)[TypeId]), TypeId in Branded);
console.log("sym-proto", branded[ProtoId], ProtoId in branded);

// A subclass's own members still shadow the parent evaluation's prototype.
function makeBase(label: string) {
  const out = class {
    static label = label;
    m() {
      return "parent-m:" + label;
    }
    get g() {
      return "parent-g:" + label;
    }
  };
  Object.assign(out.prototype, {
    a() {
      return "parent-a:" + label;
    },
    d: "parent-d",
  });
  return out;
}
class Child extends makeBase("x") {
  m() {
    return "child-m";
  }
  a() {
    return "child-a";
  }
  get g() {
    return "child-g";
  }
  get d() {
    return "child-d";
  }
}
const child: any = new Child();
console.log("override", child.m(), child.a(), child.g, child.d, child.a === Child.prototype.a);
const Base = makeBase("z");
class Plain extends Base {}
const plain: any = new Plain();
console.log(
  "inherit",
  plain.m(),
  plain.a(),
  plain.g,
  plain.d,
  plain.m === Base.prototype.m,
  plain.a === Base.prototype.a,
);
