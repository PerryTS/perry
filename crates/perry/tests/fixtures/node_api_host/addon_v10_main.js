// Driver for addon_v10.c (#10456, #10461). Its stdout under Node 26 is
// addon_v10_expected.txt; the Perry executable must print the same.
const fixture = require("fixture-v10")
if (process.env.FIXTURE_LOAD_NEWER) {
  // Node 26.5.1 itself crashes on this rejection, so only Perry runs it.
  try {
    fixture.loadNewer()
    console.log("newer loaded")
  } catch (error) {
    console.log("newer rejected", error.code, error.message)
  }
  process.exit(0)
}
const { current, legacy } = fixture

class Plain {}
class Tagged {
  constructor() {
    this.tag = "made"
  }
}
class MyError extends Error {}
const Expr = class {}
function decl() {}

for (const [name, value] of [
  ["class Plain", Plain],
  ["class Tagged", Tagged],
  ["class MyError extends Error", MyError],
  ["class expression", Expr],
  ["function decl", decl],
  ["arrow", () => 1],
  ["bound", decl.bind(null)],
  ["Map", Map],
  ["object", {}],
  ["array", []],
  ["null", null],
  ["undefined", undefined],
  ["number", 1.5],
  ["string", "text"],
  ["symbol", Symbol("s")],
  ["bigint", 10n],
  ["boolean", true],
]) {
  console.log("typeof", name, typeof value, current.typeOf(value))
}
console.log("created int32", [1, 2, 3, 4].map((n) => current.intTypeOf(n)).join(","))
console.log("number status", current.numberStatus(Plain), current.numberStatus(3))
const made = current.construct(Tagged)
console.log("new instance", made instanceof Tagged, made.tag)
console.log("version", current.version(), legacy.version())
console.log("symbol for", current.symbolFor("perry.fixture") === Symbol.for("perry.fixture"))
const syntax = current.syntaxError("ERR_FIXTURE", "made syntax")
console.log("syntax error", syntax instanceof SyntaxError, syntax.name, syntax.code, syntax.message)
try {
  current.throwSyntax()
  console.log("throw syntax did not throw")
} catch (error) {
  console.log("throw syntax", error instanceof SyntaxError, error.code, error.message)
}
const fileName = (url) => (url.startsWith("file:///") ? url.slice(url.lastIndexOf("/") + 1) : "bad " + url)
console.log("module file", fileName(current.moduleFile()), fileName(legacy.moduleFile()))
console.log("external", current.externalLatin1(), current.externalUtf16(), current.externalContract())
console.log("keys", JSON.stringify(current.keysObject()))
const bytes = new ArrayBuffer(8)
new Uint8Array(bytes).set([1, 2, 3, 4, 5, 6, 7, 8])
const view = current.bufferView(bytes, 2, 4)
console.log("buffer", Buffer.isBuffer(view), view.length, Array.from(view).join(","))
view[0] = 42
console.log("shared", new Uint8Array(bytes)[2])
try {
  current.bufferView(bytes, 6, 4)
  console.log("range did not throw")
} catch (error) {
  console.log("range", error.code, error.message)
}

const events = []
process.on("uncaughtException", (error) => events.push("uncaught: " + error.message))
process.on("warning", (warning) => events.push("warning: " + warning.code))
current.throwFromWork()
current.throwFromTsfn()
legacy.throwFromTsfn()
const started = Date.now()
const wait = () => {
  if (events.length >= 3 || Date.now() - started > 10000) {
    console.log("callback exceptions", events.sort().join(" | "))
  } else {
    setTimeout(wait, 5)
  }
}
wait()
