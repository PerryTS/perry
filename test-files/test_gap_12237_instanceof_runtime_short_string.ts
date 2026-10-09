// #12237: `value instanceof Marker` exited 139 when `value` was a 1-5 byte
// string read from a `JSON.parse` result and passed through an `any`
// parameter. Those strings are inline (SSO) NaN-boxes whose payload is the
// string bytes; `instanceof` decoded that payload as an object address and
// `object_static_prototype` dereferenced it. A literal or a 6+ byte string is
// a heap string with a real pointer, so it never faulted. Fixed by #10479's
// value decoding; this pins the reported shape (class with a constructor and
// a field, operand routed through a function parameter, object-field source).

class Marker {
  readonly width: number;
  constructor(width: number) {
    this.width = width;
  }
}

function describe(value: any): string {
  if (value instanceof Marker) return "marker";
  return typeof value;
}

// --- issue repro ----------------------------------------------------------------
const parsed = JSON.parse('{"surface":"grass"}') as { surface: string };
console.log("literal: " + describe("grass"));
console.log("parsed: " + describe(parsed.surface));

// --- runtime strings of every inline length, plus heap neighbours -----------------
const fields = JSON.parse('{"s0":"","s1":"g","s2":"gr","s3":"gra","s4":"gras","s5":"grass","s6":"grassy","s7":"grasses"}');
const Dynamic: any = JSON.parse("1") ? Marker : Object;
for (const key of Object.keys(fields)) {
  const v = fields[key];
  console.log(
    `${key} ${JSON.stringify(v)}:`,
    describe(v),
    v instanceof Marker,
    v instanceof Dynamic,
    v instanceof Object,
    v instanceof String,
    v.length,
  );
}

// --- other primitives and objects through the same parameter ----------------------
const values = JSON.parse('[0, 1.5, -7, true, false, null, {}, []]');
values.push(undefined, new Marker(2));
for (const v of values) {
  console.log(`${JSON.stringify(v) ?? "undefined"}:`, describe(v), v instanceof Dynamic);
}

// --- a user @@hasInstance still sees the short string itself ------------------------
class ShortText {
  static [Symbol.hasInstance](v: any): boolean {
    return typeof v === "string" && v.length > 0 && v.length < 6;
  }
}
for (const key of ["s0", "s3", "s5", "s6"]) {
  console.log(`${key} instanceof ShortText:`, fields[key] instanceof ShortText);
}
console.log("done");
