// A number is a Number whatever its value. Web Streams handles used to travel
// as plain numbers in [0x100000, 0x200000), so while stream 0x100000 was live
// the NUMBER 1048576 answered typeof "object", passed `instanceof
// ReadableStream`, and failed every `typeof x === "number"` check. OpenCode's
// catalog Schema rejected a model record's `limit.context: 1048576` that way.

const streams: ReadableStream<number>[] = []
for (let i = 0; i < 4; i++) {
  streams.push(new ReadableStream<number>({ start(c) { c.enqueue(i); c.close() } }))
}
const writable = new WritableStream<number>({ write() {} })
const transform = new TransformStream<number, number>()

function isNumber(u: unknown): u is number {
  return typeof u === "number"
}

function describe(label: string, v: unknown) {
  const o: any = v
  // Own keys of a primitive: Reflect.ownKeys must throw for a number.
  let ownKeys = "-"
  try {
    if (typeof v !== "object") ownKeys = String(Reflect.ownKeys(o).length)
  } catch (e) {
    ownKeys = "threw " + (e as Error).constructor.name
  }
  let inOp: string
  try {
    inOp = String("locked" in o)
  } catch (e) {
    inOp = "threw " + (e as Error).constructor.name
  }
  let weak: string
  try {
    new WeakMap<any, number>().set(o, 1)
    weak = "ok"
  } catch (e) {
    weak = "threw " + (e as Error).constructor.name
  }
  console.log(
    label,
    typeof v,
    isNumber(v),
    v instanceof ReadableStream,
    v instanceof WritableStream,
    Object.prototype.toString.call(v),
    ownKeys,
    inOp,
    weak,
    o.locked,
    typeof o.getReader,
  )
}

// The values that used to collide with live stream ids, in every spelling.
const record = JSON.parse('{"limit":{"context":1048576,"output":1048577},"ids":[1048578,1048579,2097151]}')
describe("json.context", record.limit.context)
describe("json.output", record.limit.output)
for (const id of record.ids) describe("json.id", id)
describe("literal", 1048576)
describe("computed", 2 ** 20)
describe("sum", 1048575 + 1)
describe("below", 1048575)
describe("above", 2097152)
const typed: number = record.limit.context
console.log("typed", typeof typed === "number", typed + 1, typed.toFixed(1), String(typed))
console.log("schema-like", [1048576, 1048577, 1048579].every(isNumber))
console.log("format", JSON.stringify({ context: record.limit.context }))

// The streams themselves are still streams.
describe("readable", streams[0])
describe("writable", writable)
describe("transform.readable", transform.readable)
const reader = streams[1].getReader()
console.log("locked", streams[1].locked, typeof reader, typeof reader.read)
reader.read().then((r) => {
  console.log("read", JSON.stringify(r))
  const [a, b] = streams[2].tee()
  console.log("tee", a instanceof ReadableStream, b instanceof ReadableStream, typeof a)
  const piped = streams[3].pipeThrough(new TransformStream<number, number>())
  console.log("pipeThrough", piped instanceof ReadableStream)
  const s: any = streams[0]
  s.tag = "expando"
  console.log("expando", s.tag, "tag" in s)
})
