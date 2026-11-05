// #321 / #64 / #65: legacy `arguments` inside an object-literal shorthand
// method body (`{ pipe() { return f(this, arguments) } }`).
//
// This is the EXACT shape effect's Pipeable trait uses on its
// `TypeMatcherProto` / `ValueMatcherProto` (and on every other Pipeable
// instance — Effect, Match, Console, Logger, Either, Option, Cause, Exit,
// …). Without synthesis, `arguments` inside the object-literal method body
// was unbound, so `pipeArguments(this, arguments)` saw nothing → `.pipe(fn)`
// silently returned `this`, dropping every operand.
//
// `class_members.rs` / `fn_decl.rs` / `expr_function.rs` already synthesize a
// trailing `...arguments` rest param when the body references the identifier
// (#677, #1069, #1816). `expr_object.rs::lower_method_prop` was the missing
// site — added in the same shape so the synthetic param is in scope before
// the body lowers.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

// (1) capture-free object-literal method using `arguments`.
const a = {
  fn() {
    return arguments.length;
  },
};
console.log("(1) a.fn():", a.fn());
console.log("(1) a.fn(1,2,3):", (a as any).fn(1, 2, 3));

// (2) `this`-using object-literal method with `arguments` — the Pipeable
// shape. Falls through the Closure path (captures_this=true).
const proto = {
  pipe() {
    const args = arguments;
    return args.length > 0 ? args[0](this) : this;
  },
};
const o = Object.create(proto);
(o as any).x = 10;
const bump = (self: any) => ({ ...self, x: self.x + 1 });
const o2 = (o as any).pipe(bump);
console.log("(2) o2.x:", o2.x);

// (3) named params + `arguments` together. `arguments` reflects ALL passed
// values, not just the trailing ones.
const m = {
  go(first: number) {
    return `first=${first} count=${arguments.length}`;
  },
};
console.log("(3) m.go(7):", m.go(7));
console.log("(3) m.go(7,8,9):", (m as any).go(7, 8, 9));

// (4) the actual Pipeable.pipe shape that effect ships — variadic
// transformations chained off arguments.
function applyAll(self: any, args: IArguments): any {
  let acc = self;
  for (let i = 0; i < args.length; i++) acc = args[i](acc);
  return acc;
}
const pipeable = {
  v: 1,
  pipe() {
    return applyAll(this, arguments);
  },
};
const inc = (s: any) => ({ ...s, v: s.v + 1 });
const dbl = (s: any) => ({ ...s, v: s.v * 2 });
const result = (pipeable as any).pipe(inc, dbl, inc);
console.log("(4) result.v:", result.v); // ((1+1)*2)+1 = 5
