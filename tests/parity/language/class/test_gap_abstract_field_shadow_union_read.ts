// Abstract class fields are type-only in TypeScript: `node --experimental-strip-types`
// erases them and emits NO runtime slot. Perry previously lowered
// `abstract readonly _tag: string;` into a real field, so a base-class abstract
// field plus a concrete same-named subclass initializer produced TWO physical
// slots (`Object.keys` => `_tag|_tag`). Reading the property through a
// base/union-typed local variable then resolved to the phantom base slot
// (undefined) instead of the concrete subclass value — the switch below would
// fall through. This is the exact shape of @effect/platform's HttpBody
// (`BodyBase` abstract `_tag` + `Uint8ArrayImpl`/`EmptyImpl` concrete `_tag`),
// which made an Effect HTTP server return "Not a valid effect: undefined".
//
// Validated byte-for-byte against `node --experimental-strip-types`.

const TypeId = Symbol.for("BodyTypeId");

abstract class BodyBase {
  readonly [TypeId]: symbol;
  abstract readonly _tag: string;
  constructor() {
    (this as any)[TypeId] = TypeId;
  }
  abstract toJSON(): unknown;
}

class Uint8ArrayImpl extends BodyBase {
  readonly _tag = "Uint8Array";
  readonly body: string;
  readonly contentType: string;
  constructor(body: string, contentType: string) {
    super();
    this.body = body;
    this.contentType = contentType;
  }
  toJSON(): unknown {
    return { _tag: this._tag };
  }
}

class EmptyImpl extends BodyBase {
  readonly _tag = "Empty";
  toJSON(): unknown {
    return { _tag: this._tag };
  }
}

type Body = Uint8ArrayImpl | EmptyImpl;

const x = new Uint8ArrayImpl("hi", "text/html");

// No phantom duplicate slot: abstract `_tag` must not be a runtime field.
console.log("keys:", Object.keys(x).join("|"));

// Every access path must read the concrete subclass value.
console.log("any:", (x as any)._tag);
console.log("base param:", ((b: BodyBase) => b._tag)(x));
console.log("union param:", ((b: Body) => b._tag)(x));

// The failing case: a union-typed local variable.
const b2: Body = x;
console.log("union local:", b2._tag);

// The exact effect discriminated-union dispatch.
switch (b2._tag) {
  case "Uint8Array":
    console.log("switch: matched Uint8Array");
    break;
  case "Empty":
    console.log("switch: matched Empty");
    break;
  default:
    console.log("switch: fell through");
}

// Second subclass through the union, to be sure both discriminants resolve.
const e: Body = new EmptyImpl();
console.log("empty union local:", e._tag);
