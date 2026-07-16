import { Session } from "node:inspector/promises";

const session = new Session();
session.connect();
try {
  const value = await session.post("Schema.getDomains");
  const names = value.domains.map((domain) => domain.name).sort();
  console.log("array:", Array.isArray(value.domains), names.length > 0);
  console.log(
    "required:",
    ["Debugger", "HeapProfiler", "Profiler", "Runtime", "Schema"].every((
      name,
    ) => names.includes(name)),
  );
  console.log(
    "shape:",
    value.domains.every((domain) =>
      typeof domain.name === "string" && typeof domain.version === "string"
    ),
  );
} finally {
  session.disconnect();
}
