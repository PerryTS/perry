// An ordinary source's first value must not select a native module's own keys.
// A non-enumerable property forces Object.assign's descriptor-driven path.
const TypeId = Symbol("endpoint");
const Proto = { [TypeId]: TypeId, pipe() { return this; } };
function make(name: string, error: Set<any>) {
  const options = {
    name,
    path: "/k",
    method: "GET",
    params: undefined,
    query: { directory: "x" },
    headers: undefined,
    payload: new Map(),
    success: new Set([{}]),
    error,
    annotations: { mapUnsafe: new Map() },
    middlewares: new Set()
  };
  Object.defineProperty(options, "hidden", { value: 0 });
  const endpoint = Object.assign(Object.create(Proto), options);
  console.log(Object.keys(endpoint).length + ":" + endpoint.name);
  console.log(Object.keys(endpoint).join(","));
}
make("capabilities", new Set());
make("console", new Set([{}]));
make("third", new Set());
make("fourth", new Set([{}]));
// Set contents are irrelevant: the first data value was the false brand.
make("console", new Set());

// The same enumeration must work directly, including runtime-built descriptors.
const record = { name: "console", error: 1 };
console.log("record", Object.getOwnPropertyNames(record).join(","));
console.log("reflect", Reflect.ownKeys(record).join(","));
console.log("other module", Object.getOwnPropertyNames({ name: "fs", tail: 1 }).join(","));
console.log("reordered", Reflect.ownKeys({ error: 1, name: "console" }).join(","));
const descriptor = Object.getOwnPropertyDescriptor({ field: "console" }, "field");
console.log("descriptor", Object.getOwnPropertyNames(descriptor).join(","));

// Real namespaces still enumerate their virtual exports.
const consoleNames = Object.getOwnPropertyNames(console);
console.log("namespace", consoleNames.includes("log"), consoleNames.includes("__module__"));
