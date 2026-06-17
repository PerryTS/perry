// Regression for #4699 (part 2): `new (A ?? B)(args)` — constructing via a
// short-circuit operator callee — must run the constructor body, not return
// an empty object. zod v4's `safeParse` builds its error with
// `new (_Err ?? errors.$ZodError)(issues)`; pre-fix the `??` callee fell
// through to codegen's empty-object placeholder so the `ZodError` constructor
// never ran and `r.error.issues` was `undefined`.

function makeCtor(tag: string) {
  return function (this: any, def: any) {
    this.def = def;
    this.tag = tag;
  } as any;
}
const Primary = makeCtor("primary");
const Fallback = makeCtor("fallback");

// `??` — left operand non-null, so Primary is used.
const a = new (Primary ?? Fallback)([1, 2]);
console.log("a:", a.tag, a.def);

// `??` — left operand nullish, so Fallback is used.
const nullish: any = undefined;
const b = new (nullish ?? Fallback)([3, 4]);
console.log("b:", b.tag, b.def);

// `||` — falsy left operand falls to the right.
const falsy: any = null;
const c = new (falsy || Primary)([5]);
console.log("c:", c.tag, c.def);

// `&&` — truthy left operand yields the right operand.
const d = new (Primary && Fallback)([6]);
console.log("d:", d.tag, d.def);

console.log("new (logical) callee tests passed!");
