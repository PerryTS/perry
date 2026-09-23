// #11003: a WebIDL interface prototype carries BOTH kinds of own property at
// once -- accessor properties for the interface's *attributes* (#10555) and
// data properties whose value is a function for its *operations* (#10808).
// They are complementary, not alternatives, so this pins the descriptor SHAPE
// of both on the same object. It also pins the enumerability split, which is
// the part a uniform install helper would silently get wrong: WebIDL
// operations are enumerable (unlike ECMAScript builtin methods), except that
// Node makes `AbortSignal.prototype.throwIfAborted` and its `reason` accessor
// non-enumerable.

function shape(label: string, proto: any, name: string): void {
  const d = Object.getOwnPropertyDescriptor(proto, name);
  if (!d) {
    console.log(label + "." + name, "ABSENT");
    return;
  }
  const accessor = typeof d.get === "function" || typeof d.set === "function";
  const kind = accessor ? "accessor" : typeof d.value;
  const writable = accessor ? "-" : String(d.writable);
  console.log(
    label + "." + name,
    kind,
    "enumerable=" + d.enumerable,
    "configurable=" + d.configurable,
    "writable=" + writable,
  );
}

// A second, independent observation path for the same bit: the property has to
// show up in `Object.keys` exactly when its descriptor says enumerable.
function inKeys(label: string, proto: any, name: string): void {
  console.log(label + "." + name, "inKeys=" + Object.keys(proto).includes(name));
}

console.log("-- URL: operations and attributes coexist --");
shape("URL", URL.prototype, "toString");
shape("URL", URL.prototype, "toJSON");
shape("URL", URL.prototype, "href");
shape("URL", URL.prototype, "pathname");
inKeys("URL", URL.prototype, "toString");
inKeys("URL", URL.prototype, "pathname");

console.log("-- EventTarget / Event / AbortController operations --");
shape("EventTarget", EventTarget.prototype, "addEventListener");
shape("Event", Event.prototype, "preventDefault");
shape("AbortController", AbortController.prototype, "abort");
inKeys("EventTarget", EventTarget.prototype, "addEventListener");

console.log("-- AbortSignal: Node deviates, throwIfAborted is NOT enumerable --");
shape("AbortSignal", AbortSignal.prototype, "throwIfAborted");
inKeys("AbortSignal", AbortSignal.prototype, "throwIfAborted");

console.log("-- contrast: ECMAScript builtins are non-enumerable --");
shape("Array", Array.prototype, "map");
shape("Object", Object.prototype, "hasOwnProperty");
inKeys("Array", Array.prototype, "map");

console.log("-- the operations are real values, not call-position dispatch --");
const url = new URL("https://example.com/a?b=c#d");
console.log("in", "toString" in url, "toJSON" in url, "pathname" in url);
const toStringValue = URL.prototype.toString;
console.log("detached", typeof toStringValue, toStringValue.call(url));
const pathnameGetter = Object.getOwnPropertyDescriptor(URL.prototype, "pathname")!.get!;
console.log("getter", pathnameGetter.call(url));
const { abort } = AbortController.prototype;
const controller = new AbortController();
abort.call(controller);
console.log("destructured abort", controller.signal.aborted);
