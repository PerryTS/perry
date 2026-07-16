import inspector, * as namespace from "node:inspector";

const keys = Object.keys(inspector).sort();
console.log("keys:", keys.join(","));
console.log("default identity:", namespace.default === inspector);
for (
  const name of ["open", "close", "url", "waitForDebugger", "Session"] as const
) {
  const descriptor = Object.getOwnPropertyDescriptor(inspector, name);
  console.log(
    name,
    typeof inspector[name],
    descriptor?.enumerable,
    descriptor?.writable,
    descriptor?.configurable,
  );
}
console.log(
  "objects:",
  typeof inspector.console,
  typeof inspector.Network,
  typeof inspector.NetworkResources,
  typeof inspector.DOMStorage,
);
