// #5586: `RegExp(re)` invoked as a *function* (not `new`) with a RegExp
// argument and no `flags` returns the argument unchanged — object identity,
// per ECMA-262 22.2.4.1. `new RegExp(re)`, or any call that supplies `flags`,
// must construct a fresh copy instead.
// (test262 built-ins/RegExp/S15.10.3.1_A1_T1 .. _A2_T2)

function ok(name: string, value: boolean) {
  if (!value) {
    throw new Error(name + ": FAIL");
  }
  console.log(name + ": ok");
}

const re = /x/i;

// Calling RegExp as a function with a RegExp and undefined flags returns the
// SAME object: a property added afterwards is visible through the result.
const instance = RegExp(re);
(re as any).indicator = 1;
ok("call-identity-same-object", instance === re);
ok("call-identity-property-visible", (instance as any).indicator === 1);

// Supplying flags forces a fresh copy (different object), even via the call form.
const copyWithFlags = RegExp(re, "g");
ok("call-with-flags-is-copy", copyWithFlags !== re);
ok("call-with-flags-applies-flags", copyWithFlags.flags === "g");
ok("call-with-flags-keeps-source", copyWithFlags.source === "x");

// `new RegExp(re)` ALWAYS constructs a new object, never identity.
const constructed = new RegExp(re);
ok("new-is-copy", constructed !== re);
ok("new-copies-source", constructed.source === "x");
ok("new-copies-flags", constructed.flags === "i");

// A non-RegExp argument is never the identity case — it builds a new RegExp.
const fromString = RegExp("y");
ok("call-from-string-source", fromString.source === "y");
ok("call-from-string-not-input", (fromString as any) !== "y");

// Zero-arg `RegExp()` (function-call form) builds an empty-source regex
// `/(?:)/` — NOT null. It is an ordinary object that accepts expando props.
const empty = RegExp();
ok("call-empty-source", empty.source === "(?:)");
ok("call-empty-no-flags", empty.flags === "");
(empty as any).indicator = 1;
ok("call-empty-is-object", (empty as any).indicator === 1);

// The identity shortcut requires `pattern.constructor` to still be the RegExp
// intrinsic. Overriding `constructor` makes SameValue fail → a fresh copy.
const tweaked = /(?:)/;
(tweaked as any).constructor = null;
ok("call-other-constructor-is-copy", RegExp(tweaked) !== tweaked);

// The shortcut is also gated on IsRegExp, which consults `pattern[@@match]`:
// a RegExp whose own `Symbol.match` is falsy is NOT regexp-like → fresh copy.
const unmatched = /(?:)/;
(unmatched as any)[Symbol.match] = false;
ok("call-match-falsy-is-copy", RegExp(unmatched) !== unmatched);
// A truthy `Symbol.match` override keeps it regexp-like → identity holds.
const stillRe = /(?:)/;
(stillRe as any)[Symbol.match] = true;
ok("call-match-truthy-is-identity", RegExp(stillRe) === stillRe);

// The copy is independent: mutating lastIndex on one must not affect the other.
const g = /a/g;
const gCopy = new RegExp(g);
g.lastIndex = 3;
ok("new-copy-independent-lastindex", gCopy.lastIndex === 0);
