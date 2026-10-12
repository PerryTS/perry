import { Schema } from "effect"
const t0 = performance.now()
const out: any[] = []
for (let i = 0; i < 2000; i++) {
  const S = Schema.Struct({
    id: Schema.String, name: Schema.optional(Schema.String), count: Schema.Finite,
    tags: Schema.Array(Schema.String), kind: Schema.Literals(["a", "b", "c"]),
    nested: Schema.Struct({ x: Schema.Number, y: Schema.optionalKey(Schema.Boolean) }),
  }).annotate({ identifier: "S" + i, description: "d" + i })
  out.push(S)
}
const t1 = performance.now()
const D = out[0]; let ok = 0
for (let i = 0; i < 20000; i++) {
  const v = Schema.decodeUnknownSync(D)({ id: "x" + i, count: i, tags: ["a"], kind: "b", nested: { x: 1 } })
  if (v.id) ok++
}
const t2 = performance.now()
console.log("construct2000=2000 decode20000=20000 ok=" + ok)
