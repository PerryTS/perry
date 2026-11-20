import inspector from "node:inspector";

for (const domain of ["Network", "DOMStorage"] as const) {
  const value = inspector[domain];
  console.log(domain, Object.keys(value).sort().join(","));
  for (const name of Object.keys(value).sort()) {
    const fn = (value as Record<string, Function>)[name];
    const desc = Object.getOwnPropertyDescriptor(value, name);
    console.log(
      domain,
      name,
      fn.name,
      fn.length,
      desc?.enumerable,
      desc?.writable,
      desc?.configurable,
    );
  }
}
console.log(
  "resources:",
  Object.keys(inspector.NetworkResources).sort().join(","),
  typeof inspector.NetworkResources.put,
);
