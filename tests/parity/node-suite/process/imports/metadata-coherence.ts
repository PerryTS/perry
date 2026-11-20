import processModule from "node:process";
import * as processNamespace from "node:process";
import {
  allowedNodeEnvironmentFlags,
  config,
  features,
  finalization,
  release,
  report,
} from "node:process";

function kind(value: any): string {
  if (Array.isArray(value)) return "array";
  if (value instanceof Set) return "set";
  return typeof value;
}

const names = [
  "allowedNodeEnvironmentFlags",
  "argv0",
  "config",
  "debugPort",
  "execArgv",
  "execPath",
  "features",
  "finalization",
  "moduleLoadList",
  "release",
  "report",
  "sourceMapsEnabled",
  "title",
];

for (const [label, source] of [
  ["global", process],
  ["default", processModule],
  ["namespace", processNamespace],
  ["captured", process],
] as const) {
  console.log(
    `source ${label}:`,
    names.map((name) => `${name}:${kind((source as any)[name])}`).join(","),
  );
}

console.log(
  "named imports:",
  [
    allowedNodeEnvironmentFlags,
    config,
    features,
    finalization,
    release,
    report,
  ].map(kind).join(","),
);

console.log(
  "flags:",
  process.allowedNodeEnvironmentFlags instanceof Set,
  process.allowedNodeEnvironmentFlags.size > 0,
  process.allowedNodeEnvironmentFlags.has("--no-warnings"),
  process.allowedNodeEnvironmentFlags.has("--allow-fs-read"),
  process.allowedNodeEnvironmentFlags.has("-r"),
);
console.log(
  "default export:",
  processModule === process,
  Object.hasOwn(processModule, "default"),
  Object.keys(processModule).includes("default"),
);
console.log(
  "namespace default:",
  processNamespace.default === process,
  Object.hasOwn(processNamespace, "default"),
  Object.keys(processNamespace).includes("default"),
);
console.log("release:", process.release.name, Object.keys(process.release).sort().join(","));
console.log(
  "features:",
  typeof process.features.ipv6,
  typeof process.features.tls,
  typeof process.features.typescript,
  process.features.uv,
);
console.log(
  "report methods:",
  typeof process.report.getReport,
  typeof process.report.writeReport,
);
console.log(
  "finalization methods:",
  typeof process.finalization.register,
  typeof process.finalization.registerBeforeExit,
  typeof process.finalization.unregister,
);

const variables = process.config.variables;
const targetDefaults = process.config.target_defaults;
console.log("config containers:", typeof variables, typeof targetDefaults);
console.log("config target keys:", Object.keys(targetDefaults).sort().join(","));
console.log(
  "config variable types:",
  [
    "target_arch",
    "host_arch",
    "node_module_version",
    "node_shared_openssl",
    "v8_enable_i18n_support",
  ].map((key) => `${key}:${typeof (variables as any)[key]}`).join(","),
);
