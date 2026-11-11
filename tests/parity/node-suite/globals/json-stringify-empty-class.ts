// JSON.stringify of a class instance with no own enumerable data fields
// (an empty class, or a class whose only members are prototype methods /
// getters) must serialize as `{}` — methods and getters live on the
// prototype, not as own properties. Such instances fail the `keys_len > 0`
// object probe, and previously fell through to `null` (compact / replacer)
// or were misdetected as an array (`[ 0 ]`, pretty). A prototype `toJSON`
// is still honoured.

function show(label: string, value: any) {
  console.log(label + " = " + value);
}

class Empty {}
class OnlyMethod { m() { return 1; } }
class OnlyGetter { get g() { return 9; } }
class WithField { x = 1; get g() { return 2; } }
class WithToJSON { toJSON() { return { ok: 1 }; } }

// Top level.
show("empty", JSON.stringify(new Empty()));
show("only-method", JSON.stringify(new OnlyMethod()));
show("only-getter", JSON.stringify(new OnlyGetter()));
show("with-field", JSON.stringify(new WithField()));

// Nested (compact / pretty / function-replacer).
show("nested", JSON.stringify({ svc: new OnlyMethod(), x: 1 }));
show("nested-pretty", JSON.stringify({ svc: new OnlyMethod() }, null, 1).replace(/\s+/g, " "));
show("nested-replacer", JSON.stringify({ svc: new OnlyMethod() }, (_k, v) => v));

// Array of empty instances.
show("array", JSON.stringify([new Empty(), new OnlyMethod()]));

// Top-level pretty.
show("pretty-top", JSON.stringify(new Empty(), null, 2).replace(/\s+/g, " "));

// A prototype toJSON is still honoured (compact + pretty).
show("toJSON", JSON.stringify(new WithToJSON()));
show("toJSON-pretty", JSON.stringify(new WithToJSON(), null, 1).replace(/\s+/g, " "));

// Regression: plain objects and arrays are unaffected.
show("plain", JSON.stringify({}));
show("obj", JSON.stringify({ a: 1, b: { c: 2 } }));
