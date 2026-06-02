// #806 — systematic harness for `class X extends Fn(...)<...>` patterns.
//
// Effect's TaggedError, Schema.Class, and similar factory-shaped bases
// exercise a corner of TS+JS that historically broke perry in subtle
// ways (e.g. #740 captured-factory + #809 cross-module spread). This
// file covers the family with small, independent assertions, each
// printing a stable line so the parity runner can diff vs Node.
//
// All assertions print a "section: result" line so a single grep tells
// you which sub-case regressed.

// ── 1. Bare factory: `class X extends Fn()` ────────────────────────────────
// The simplest dynamic-extends shape: a 0-arg factory returns a class,
// the user subclasses the result. Caught the #740 base-shape regression.
function makeBare() {
  return class {
    kind = "bare";
    hello(): string {
      return "Hi from Bare";
    }
  };
}
class Bare extends makeBare() {
  extra = 7;
}
const bare = new Bare();
console.log("bare.kind:", bare.kind);
console.log("bare.hello:", bare.hello());
console.log("bare.extra:", bare.extra);

// ── 2. Parameterized factory: `class X extends Fn("tag", schema)` ──────────
// Effect's `Schema.Class` / `TaggedError` shape. The factory takes
// runtime args that get baked into the produced class's prototype.
function tagged(tag: string, fields: Record<string, string>) {
  return class {
    readonly _tag = tag;
    readonly _fields = fields;
  };
}
class MyTagged extends tagged("MyTag", { a: "string", b: "number" }) {
  // Subclasses freely add fields/methods. The parameter-baked
  // properties survive the subclass step.
}
const t = new MyTagged();
console.log("tagged._tag:", t._tag);
console.log("tagged._fields.a:", t._fields.a);
console.log("tagged._fields.b:", t._fields.b);

// ── 3. Captured-factory: `class X extends F<T>()` ──────────────────────────
// Effect's TaggedError closes around a Constructable. The factory is
// captured-then-called — `F<T>()` evaluates the call AFTER reading the
// generic, and Perry must preserve that. #740 root cause was the captured
// form silently aliasing to the un-captured base.
function makeNamed<T>(name: T) {
  return class {
    name: T = name;
    greet(): string {
      // Cast the name to a primitive for printing. The test fixes T = string
      // so this is a no-op cast in the type system but keeps the lowering
      // honest about T's runtime shape.
      return `Hello, ${String(this.name)}`;
    }
  };
}
class Greeter extends makeNamed<string>("world") {
  emphasize(): string {
    return this.greet() + "!";
  }
}
const g = new Greeter();
console.log("captured.name:", g.name);
console.log("captured.greet:", g.greet());
console.log("captured.emphasize:", g.emphasize());

// ── 4. Chained mixins: `class X extends M1(M2(M3(Base)))` ──────────────────
// Layered mixin pattern from older TypeScript codebases (sequelize-typescript,
// older Nest, etc.). Each layer wraps the prior class and adds a method.
class CoreBase {
  core(): string {
    return "core";
  }
}
type Ctor<T = {}> = new (...args: any[]) => T;
function WithA<TBase extends Ctor>(B: TBase) {
  return class extends B {
    a(): string {
      return "a";
    }
  };
}
function WithB<TBase extends Ctor>(B: TBase) {
  return class extends B {
    b(): string {
      return "b";
    }
  };
}
function WithC<TBase extends Ctor>(B: TBase) {
  return class extends B {
    c(): string {
      return "c";
    }
  };
}
class Chained extends WithA(WithB(WithC(CoreBase))) {}
const chained = new Chained();
console.log("chained.core:", chained.core());
console.log("chained.a:", chained.a());
console.log("chained.b:", chained.b());
console.log("chained.c:", chained.c());

// ── 5. Mixin that calls super() with args ──────────────────────────────────
// Constructor-arg forwarding through a mixin layer. Both Perry and Node
// must propagate the super-constructor's side effects.
class Logged {
  log: string;
  constructor(seed: string) {
    this.log = "seed=" + seed;
  }
}
function WithSuffix<TBase extends Ctor<Logged>>(B: TBase) {
  return class extends B {
    constructor(seed: string) {
      super(seed);
      this.log += ":wrapped";
    }
  };
}
class WrappedLogged extends WithSuffix(Logged) {}
const wl = new WrappedLogged("alpha");
console.log("super-args.log:", wl.log);

// ── 6. Factory returning class expression with static method ──────────────
// Statics on factory-produced classes must survive the subclass step
// without losing their `this`-binding.
function makeWithStatic() {
  return class {
    static prefix(s: string): string {
      return "STATIC:" + s;
    }
    instance(): string {
      return "instance";
    }
  };
}
class StaticHost extends makeWithStatic() {}
console.log("static.prefix:", StaticHost.prefix("x"));
console.log("static.instance:", new StaticHost().instance());

// ── 7. Effect-shape: `class X extends TaggedError<X>()("name", schema)` ────
// Hot path: Effect's TaggedError is a double-call factory — the outer
// `TaggedError<X>` is a curried generic, and calling it with no args
// returns the inner factory that *then* takes the tag + schema. This
// is the precise shape #740 fixed (class-extends-factory(captured)).
function TaggedError<_Self>() {
  return <Tag extends string>(tag: Tag, schema: Record<string, string>) =>
    class {
      readonly _tag: Tag = tag;
      readonly _schema = schema;
      readonly cause?: unknown;
    };
}
class MyError extends TaggedError<MyError>()("MyError", { code: "string" }) {
  describe(): string {
    return `${this._tag}(code:${this._schema.code})`;
  }
}
const err = new MyError();
console.log("effect._tag:", err._tag);
console.log("effect._schema.code:", err._schema.code);
console.log("effect.describe:", err.describe());

// ── 8. Mixin + parameterized generic combo ─────────────────────────────────
// The combination that #321 tracks for Effect end-to-end: a mixin that
// is itself a parameterized factory.
function ParamMixin<T>(seed: T) {
  return <TBase extends Ctor>(B: TBase) =>
    class extends B {
      seed: T = seed;
      describeSeed(): string {
        return "seed=" + String(this.seed);
      }
    };
}
class Combined extends ParamMixin<number>(42)(CoreBase) {}
const combined = new Combined();
console.log("combo.core:", combined.core());
console.log("combo.seed:", combined.seed);
console.log("combo.describeSeed:", combined.describeSeed());
